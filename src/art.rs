//! Album artwork for the artwork pane: read off the song's file (or a cover image beside it)
//! on a worker, decoded by Rune's image cache. With online artwork on, an album that has
//! none is looked up on MusicBrainz and its front cover fetched from the Cover Art Archive,
//! once: the cover, or the finding that there's none, is kept in the cache folder. The sample
//! library gets artwork painted from each album's name instead, so headless snapshots have
//! some to show.
//!
//! The cover showing is also written to the cache folder, so the desktop's media controls
//! can show it by its path.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use rune::{Image, ImageCache, Latest, Waker};

use crate::library::Track;
use crate::net;

/// What a lookup answers: the song's file, the cover's bytes, and the cover on disk.
type Found = (PathBuf, Option<Vec<u8>>, Option<PathBuf>);

#[derive(Default)]
pub struct Art {
    cache: Option<ImageCache>,
    bytes: Latest<Found>,
    /// The file the artwork showing (or on its way) belongs to.
    for_path: Option<PathBuf>,
    pub image: Option<Image>,
    /// The cover showing, as a file (for the media controls).
    pub file: Option<PathBuf>,
    /// Paint artwork rather than reading files (the sample library's songs have none).
    painted: bool,
}

/// What to look up for a song: its file, and its album to search for online.
struct Ask {
    path: PathBuf,
    artist: String,
    album: String,
    online: bool,
}

impl Art {
    pub fn start(&mut self, waker: Waker, live: bool) {
        self.cache = Some(ImageCache::new(waker.clone()));
        self.bytes = Latest::new(waker).debounce(Duration::from_millis(40));
        self.painted = !live;
    }

    /// Shows `t`'s artwork, or none, looking it up online when `online` and it has none.
    pub fn show(&mut self, t: Option<&Track>, online: bool) {
        let path = t.map(|t| t.path.clone());
        // Before `start` there's no cache to decode with; ask again once there is.
        if path == self.for_path || self.cache.is_none() {
            return;
        }
        self.for_path = path.clone();
        let (Some(cache), Some(t)) = (&self.cache, t) else {
            self.image = None;
            self.file = None;
            return;
        };
        if self.painted {
            self.image = Some(paint(cache, &t.album));
            return;
        }
        let ask = Ask { path: t.path.clone(), artist: t.album_artist_or_artist().to_string(), album: t.album.clone(), online };
        self.bytes.request(ask, |a| {
            let b = crate::library::read_art(&a.path).or_else(|| if a.online { online_cover(&a.artist, &a.album) } else { cached_cover(&a.artist, &a.album) });
            let file = b.as_deref().and_then(keep_showing);
            (a.path, b, file)
        });
    }

    /// Looks again at the song showing, as after its artwork changed.
    pub fn forget(&mut self) {
        self.for_path = None;
    }

    /// Takes artwork that arrived. Returns whether what shows changed.
    pub fn poll(&mut self) -> bool {
        let Some((path, bytes, file)) = self.bytes.take() else { return false };
        if Some(&path) != self.for_path.as_ref() {
            return false;
        }
        self.file = file;
        self.image = match (bytes, &self.cache) {
            (Some(b), Some(c)) => Some(c.load(b)),
            _ => None,
        };
        true
    }

    /// The file the artwork showing is for.
    pub fn for_path(&self) -> Option<&Path> {
        self.for_path.as_deref()
    }
}

fn dir() -> Option<PathBuf> {
    rune::dirs::AppDirs::new("stereo").map(|d| d.cache().join("artwork"))
}

fn hash(s: &str) -> u64 {
    s.bytes().fold(14695981039346656037u64, |h, b| (h ^ u64::from(b)).wrapping_mul(1099511628211))
}

/// Writes the cover showing to `now-<hash>.img` in the cache, removing the last one, and
/// returns its path.
fn keep_showing(bytes: &[u8]) -> Option<PathBuf> {
    static LAST: Mutex<Option<PathBuf>> = Mutex::new(None);
    let dir = dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let key = bytes.iter().step_by(97).fold(bytes.len() as u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(1099511628211));
    let path = dir.join(format!("now-{key:016x}.img"));
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if last.as_ref() != Some(&path) {
        std::fs::write(&path, bytes).ok()?;
        if let Some(old) = last.replace(path.clone()) {
            let _ = std::fs::remove_file(old);
        }
    }
    Some(path)
}

/// Where an album's fetched cover is kept, and where the finding that it has none is.
fn album_files(artist: &str, album: &str) -> Option<(PathBuf, PathBuf)> {
    let key = hash(&format!("{}\u{1f}{}", artist.trim().to_lowercase(), album.trim().to_lowercase()));
    let dir = dir()?;
    Some((dir.join(format!("album-{key:016x}.img")), dir.join(format!("album-{key:016x}.none"))))
}

fn cached_cover(artist: &str, album: &str) -> Option<Vec<u8>> {
    let (img, _) = album_files(artist, album)?;
    std::fs::read(img).ok()
}

/// The album's front cover from the Cover Art Archive, found through MusicBrainz, or from the
/// cache when it was fetched before.
fn online_cover(artist: &str, album: &str) -> Option<Vec<u8>> {
    if album.trim().is_empty() {
        return None;
    }
    let (img, none) = album_files(artist, album)?;
    if let Ok(b) = std::fs::read(&img) {
        return Some(b);
    }
    // Looked for before and not found; look again after a month.
    if let Ok(m) = std::fs::metadata(&none)
        && m.modified().ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < Duration::from_secs(30 * 86_400))
    {
        return None;
    }
    let found = lookup(artist, album);
    if let Some(dir) = img.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match &found {
        Some(b) => {
            let _ = std::fs::write(&img, b);
        }
        None => {
            let _ = std::fs::write(&none, b"");
        }
    }
    found
}

/// MusicBrainz asks for a request a second at most.
fn wait_turn() {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = *last {
        let since = t.elapsed();
        if since < Duration::from_millis(1100) {
            std::thread::sleep(Duration::from_millis(1100) - since);
        }
    }
    *last = Some(Instant::now());
}

/// A phrase for a Lucene query, quoted.
fn phrase(s: &str) -> String {
    format!("\"{}\"", s.trim().replace('\\', "\\\\").replace('"', "\\\""))
}

fn lookup(artist: &str, album: &str) -> Option<Vec<u8>> {
    let mut q = format!("releasegroup:{}", phrase(album));
    if !artist.trim().is_empty() {
        q.push_str(&format!(" AND artist:{}", phrase(artist)));
    }
    wait_turn();
    let mut resp = net::agent().get("https://musicbrainz.org/ws/2/release-group/").query("query", &q).query("fmt", "json").query("limit", "3").call().ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.body_mut().read_json().ok()?;
    let groups = body["release-groups"].as_array()?;
    // The best match that the archive has a cover for.
    for g in groups.iter().filter(|g| g["score"].as_u64().unwrap_or(0) >= 90) {
        let Some(id) = g["id"].as_str() else { continue };
        let Ok(mut r) = net::agent().get(&format!("https://coverartarchive.org/release-group/{id}/front-500")).call() else { continue };
        if !r.status().is_success() {
            continue;
        }
        if let Ok(b) = r.body_mut().with_config().limit(15 * 1024 * 1024).read_to_vec() {
            return Some(b);
        }
    }
    None
}

/// A cover for `album`: a soft two-colour gradient with rings, its hues from the name.
fn paint(cache: &ImageCache, album: &str) -> Image {
    const N: usize = 128;
    let h = album.bytes().fold(2166136261u32, |h, b| (h ^ u32::from(b)).wrapping_mul(16777619));
    let hue = |n: u32| f32::from((n % 360) as u16) / 360.0;
    let (a, b) = (rgb(hue(h), 0.55, 0.62), rgb(hue(h >> 9), 0.6, 0.32));
    let mut px = vec![0u8; N * N * 4];
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32 / N as f32, y as f32 / N as f32);
            let d = ((fx - 0.5).powi(2) + (fy - 0.5).powi(2)).sqrt();
            let ring = if ((d * 18.0).fract() < 0.12) && d < 0.42 { 0.08 } else { 0.0 };
            let t = (fx * 0.4 + fy * 0.6).clamp(0.0, 1.0);
            let i = (y * N + x) * 4;
            for c in 0..3 {
                let v = a[c] * (1.0 - t) + b[c] * t + ring;
                px[i + c] = (v.clamp(0.0, 1.0) * 255.0) as u8;
            }
            px[i + 3] = 255;
        }
    }
    cache.rgba(N as u32, N as u32, px)
}

fn rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h * 6.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [r + m, g + m, b + m]
}
