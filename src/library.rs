//! The music library: every track Stereo knows, the playlists made from them, and reading
//! tags off audio files. A scan walks folders on a worker thread and hands back tracks in
//! batches; the UI thread gives them ids and folds them in, skipping files it already has.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::{SystemTime, UNIX_EPOCH};

use lofty::prelude::*;
use serde::{Deserialize, Serialize};

use crate::party::Party;
use crate::smart::SmartList;

pub type TrackId = u64;

/// The extensions a scan picks up: what symphonia decodes.
pub const AUDIO: &[&str] = &["mp3", "flac", "ogg", "oga", "opus", "m4a", "mp4", "aac", "wav", "aif", "aiff", "caf", "mka", "webm"];

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Track {
    pub id: TrackId,
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub genre: String,
    pub year: Option<u32>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    /// Seconds.
    pub duration: f32,
    /// Bytes on disk.
    pub size: u64,
    /// 0 to 5 stars.
    pub rating: u8,
    pub plays: u32,
    pub skips: u32,
    /// Unix seconds.
    pub last_played: Option<u64>,
    pub added: u64,
}

impl Track {
    /// The artist an album files under: its album artist, else the track's own.
    pub fn album_artist_or_artist(&self) -> &str {
        if self.album_artist.is_empty() { &self.artist } else { &self.album_artist }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Playlist {
    pub id: u64,
    pub name: String,
    pub tracks: Vec<TrackId>,
}

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(default)]
pub struct Library {
    pub tracks: Vec<Track>,
    pub playlists: Vec<Playlist>,
    /// Smart playlists the user made.
    pub smart: Vec<SmartList>,
    /// Folders scanned before, rescanned on launch for new files.
    pub folders: Vec<PathBuf>,
    pub party: Party,
    next_id: u64,
    #[serde(skip)]
    index: HashMap<TrackId, usize>,
    #[serde(skip)]
    paths: HashSet<PathBuf>,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Library {
    /// Rebuilds the lookups after loading or removing tracks.
    pub fn reindex(&mut self) {
        self.index = self.tracks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
        self.paths = self.tracks.iter().map(|t| t.path.clone()).collect();
        let top = self.tracks.iter().map(|t| t.id).chain(self.playlists.iter().map(|p| p.id)).chain(self.smart.iter().map(|p| p.id)).max().unwrap_or(0);
        self.next_id = self.next_id.max(top + 1);
    }

    pub fn get(&self, id: TrackId) -> Option<&Track> {
        self.index.get(&id).map(|&i| &self.tracks[i])
    }

    pub fn get_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.index.get(&id).map(|&i| &mut self.tracks[i])
    }

    pub fn contains_path(&self, p: &Path) -> bool {
        self.paths.contains(p)
    }

    fn fresh_id(&mut self) -> u64 {
        self.next_id = self.next_id.max(1);
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Adds a scanned track unless its file is already in the library. Returns its new id.
    pub fn add(&mut self, mut t: Track) -> Option<TrackId> {
        if self.paths.contains(&t.path) {
            return None;
        }
        t.id = self.fresh_id();
        if t.added == 0 {
            t.added = now();
        }
        self.paths.insert(t.path.clone());
        self.index.insert(t.id, self.tracks.len());
        let id = t.id;
        self.tracks.push(t);
        Some(id)
    }

    /// Takes tracks out of the library and out of every playlist.
    pub fn remove(&mut self, ids: &HashSet<TrackId>) {
        self.tracks.retain(|t| !ids.contains(&t.id));
        for p in &mut self.playlists {
            p.tracks.retain(|t| !ids.contains(t));
        }
        self.party.retain(|t| !ids.contains(&t));
        self.reindex();
    }

    pub fn new_playlist(&mut self, name: &str, tracks: Vec<TrackId>) -> u64 {
        let id = self.fresh_id();
        self.playlists.push(Playlist { id, name: name.into(), tracks });
        id
    }

    pub fn playlist(&self, id: u64) -> Option<&Playlist> {
        self.playlists.iter().find(|p| p.id == id)
    }

    pub fn playlist_mut(&mut self, id: u64) -> Option<&mut Playlist> {
        self.playlists.iter_mut().find(|p| p.id == id)
    }

    pub fn new_smart(&mut self, mut list: SmartList) -> u64 {
        list.id = self.fresh_id();
        let id = list.id;
        self.smart.push(list);
        id
    }

    pub fn smart_list(&self, id: u64) -> Option<&SmartList> {
        self.smart.iter().find(|p| p.id == id)
    }

    pub fn smart_mut(&mut self, id: u64) -> Option<&mut SmartList> {
        self.smart.iter_mut().find(|p| p.id == id)
    }

    /// A name no playlist has yet: "untitled playlist", then "untitled playlist 2" and on.
    pub fn untitled(&self) -> String {
        self.untitled_from("untitled playlist")
    }

    pub fn untitled_from(&self, base: &str) -> String {
        let taken = |n: &str| self.playlists.iter().any(|p| p.name == n) || self.smart.iter().any(|p| p.name == n);
        if !taken(base) {
            return base.into();
        }
        (2..).map(|i| format!("{base} {i}")).find(|n| !taken(n)).expect("some number is free")
    }

    pub fn by_path(&self, p: &Path) -> Option<TrackId> {
        if !self.paths.contains(p) {
            return None;
        }
        self.tracks.iter().find(|t| t.path == p).map(|t| t.id)
    }

    pub fn remember_folder(&mut self, dir: &Path) {
        if !self.folders.iter().any(|f| dir.starts_with(f)) {
            self.folders.retain(|f| !f.starts_with(dir));
            self.folders.push(dir.to_path_buf());
        }
    }
}

/// Reads a file's tags and length. Files without tags take their name as the title and their
/// folders as the album and artist, as iTunes did.
pub fn read_track(path: &Path) -> Option<Track> {
    let tagged = lofty::read_from_path(path).ok()?;
    let mut t = Track { path: path.to_path_buf(), duration: tagged.properties().duration().as_secs_f32(), ..Track::default() };
    t.size = std::fs::metadata(path).map_or(0, |m| m.len());
    if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
        let s = |v: Option<std::borrow::Cow<'_, str>>| v.map(|c| c.trim().to_string()).unwrap_or_default();
        t.title = s(tag.title());
        t.artist = s(tag.artist());
        t.album = s(tag.album());
        t.genre = s(tag.genre());
        t.album_artist = tag.get_string(ItemKey::AlbumArtist).map(|a| a.trim().to_string()).unwrap_or_default();
        t.year = tag.date().map(|d| u32::from(d.year)).filter(|&y| y > 0);
        t.track_no = tag.track();
        t.disc_no = tag.disk();
    }
    if t.title.is_empty() {
        t.title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    }
    let parent = |n: usize| path.ancestors().nth(n).and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned());
    if t.album.is_empty() {
        t.album = parent(1).unwrap_or_default();
    }
    if t.artist.is_empty() {
        t.artist = parent(2).unwrap_or_default();
    }
    Some(t)
}

/// The first picture embedded in a file: its front cover when it names one.
pub fn read_art(path: &Path) -> Option<Vec<u8>> {
    let tagged = lofty::read_from_path(path).ok()?;
    let tags = tagged.tags();
    let front = tags.iter().flat_map(|t| t.pictures()).find(|p| p.pic_type() == lofty::picture::PictureType::CoverFront);
    let any = || tags.iter().flat_map(|t| t.pictures()).next();
    if let Some(p) = front.or_else(any) {
        return Some(p.data().to_vec());
    }
    // Else a cover image beside it in the folder.
    let dir = path.parent()?;
    for name in ["cover.jpg", "cover.png", "folder.jpg", "folder.png", "front.jpg", "Cover.jpg", "Folder.jpg", "AlbumArt.jpg"] {
        if let Ok(b) = std::fs::read(dir.join(name)) {
            return Some(b);
        }
    }
    None
}

/// Tags to write to a file: each `Some` replaces what's there (an empty string or a zero
/// clears it), each `None` leaves it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TagEdit {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u32>,
    pub track_no: Option<u32>,
    pub disc_no: Option<u32>,
    /// New front cover image bytes, or `Some(None)` to take the artwork out.
    pub cover: Option<Option<Vec<u8>>>,
}

impl TagEdit {
    pub fn is_empty(&self) -> bool {
        *self == TagEdit::default()
    }

    /// The same changes in the library's copy of a song.
    pub fn apply(&self, t: &mut Track) {
        let set = |to: &mut String, v: &Option<String>| {
            if let Some(v) = v {
                *to = v.clone();
            }
        };
        set(&mut t.title, &self.title);
        set(&mut t.artist, &self.artist);
        set(&mut t.album, &self.album);
        set(&mut t.album_artist, &self.album_artist);
        set(&mut t.genre, &self.genre);
        if let Some(y) = self.year {
            t.year = (y > 0).then_some(y);
        }
        if let Some(n) = self.track_no {
            t.track_no = (n > 0).then_some(n);
        }
        if let Some(n) = self.disc_no {
            t.disc_no = (n > 0).then_some(n);
        }
    }
}

/// Writes `edit` into the file's main tag, making one if it has none.
pub fn write_tags(path: &Path, edit: &TagEdit) -> Result<(), String> {
    use lofty::config::WriteOptions;
    use lofty::picture::{Picture, PictureType};
    use lofty::tag::Tag;
    let mut tagged = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    if tagged.primary_tag().is_none() {
        let kind = tagged.primary_tag_type();
        tagged.insert_tag(Tag::new(kind));
    }
    let tag = tagged.primary_tag_mut().ok_or("the file has no tag")?;
    let text = |tag: &mut Tag, v: &Option<String>, set: fn(&mut Tag, String), clear: fn(&mut Tag)| {
        match v.as_deref().map(str::trim) {
            Some("") => clear(tag),
            Some(v) => set(tag, v.to_string()),
            None => {}
        }
    };
    text(tag, &edit.title, |t, v| t.set_title(v), |t| t.remove_title());
    text(tag, &edit.artist, |t, v| t.set_artist(v), |t| t.remove_artist());
    text(tag, &edit.album, |t, v| t.set_album(v), |t| t.remove_album());
    text(tag, &edit.genre, |t, v| t.set_genre(v), |t| t.remove_genre());
    text(tag, &edit.album_artist, |t, v| { t.insert_text(ItemKey::AlbumArtist, v); }, |t| t.remove_key(ItemKey::AlbumArtist));
    match edit.year {
        Some(0) => tag.remove_date(),
        Some(y) => tag.set_date(lofty::tag::items::Timestamp { year: y.min(u32::from(u16::MAX)) as u16, ..Default::default() }),
        None => {}
    }
    match edit.track_no {
        Some(0) => tag.remove_track(),
        Some(n) => tag.set_track(n),
        None => {}
    }
    match edit.disc_no {
        Some(0) => tag.remove_disk(),
        Some(n) => tag.set_disk(n),
        None => {}
    }
    if let Some(cover) = &edit.cover {
        tag.remove_picture_type(PictureType::CoverFront);
        if let Some(bytes) = cover {
            tag.push_picture(Picture::unchecked(bytes.clone()).pic_type(PictureType::CoverFront).build());
        }
    }
    tagged.save_to_path(path, WriteOptions::default()).map_err(|e| e.to_string())
}

pub fn is_audio(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| AUDIO.iter().any(|a| a.eq_ignore_ascii_case(e)))
}

/// What a running scan reports.
pub enum Found {
    Tracks(Vec<Track>),
    /// The scan finished: how many files it looked at.
    Done(usize),
}

/// Walks `roots` (folders or single files) on a worker, sending tracks in batches and calling
/// `wake` after each. Files in `known` are skipped without being opened.
pub fn scan(roots: Vec<PathBuf>, known: HashSet<PathBuf>, wake: rune::Waker) -> Receiver<Found> {
    let (tx, rx) = channel();
    let _ = std::thread::Builder::new().name("stereo-scan".into()).spawn(move || {
        let mut batch = Vec::new();
        let mut seen = 0;
        let files = roots.into_iter().flat_map(|r| walkdir::WalkDir::new(r).follow_links(true).into_iter().filter_map(Result::ok));
        for entry in files {
            let p = entry.path();
            if !entry.file_type().is_file() || !is_audio(p) || known.contains(p) {
                continue;
            }
            seen += 1;
            if let Some(t) = read_track(p) {
                batch.push(t);
            }
            if batch.len() >= 200 {
                if tx.send(Found::Tracks(std::mem::take(&mut batch))).is_err() {
                    return;
                }
                wake();
            }
        }
        if !batch.is_empty() {
            let _ = tx.send(Found::Tracks(batch));
        }
        let _ = tx.send(Found::Done(seen));
        wake();
    });
    rx
}

/// `library.json` in the platform's data folder, or `$STEREO_LIBRARY`.
pub fn path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("STEREO_LIBRARY") {
        return Some(PathBuf::from(p));
    }
    rune::dirs::AppDirs::new("stereo").map(|d| d.data().join("library.json"))
}

pub fn load(path: &Path) -> Library {
    let mut lib: Library = match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            eprintln!("stereo: couldn't read {}: {e}", path.display());
            // Keep the unreadable file rather than overwrite it.
            let _ = std::fs::rename(path, path.with_extension("json.bad"));
            Library::default()
        }),
        Err(_) => Library::default(),
    };
    lib.reindex();
    lib
}

/// The user's music folder: `XDG_MUSIC_DIR` from `user-dirs.dirs`, else `~/Music`.
pub fn music_dir() -> Option<PathBuf> {
    let home = rune::dirs::home()?;
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    if let Ok(dirs) = std::fs::read_to_string(config.join("user-dirs.dirs")) {
        for line in dirs.lines() {
            if let Some(v) = line.strip_prefix("XDG_MUSIC_DIR=") {
                let v = v.trim().trim_matches('"').replace("$HOME", &home.to_string_lossy());
                let p = PathBuf::from(v);
                if p.is_dir() {
                    return Some(p);
                }
            }
        }
    }
    Some(home.join("Music")).filter(|p| p.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(path: &str) -> Track {
        Track { path: path.into(), title: path.into(), ..Track::default() }
    }

    #[test]
    fn adding_skips_files_already_there_and_ids_stay_unique() {
        let mut lib = Library::default();
        let a = lib.add(track("/m/a.mp3")).unwrap();
        let b = lib.add(track("/m/b.mp3")).unwrap();
        assert_ne!(a, b);
        assert!(lib.add(track("/m/a.mp3")).is_none());
        let p = lib.new_playlist("mix", vec![a, b]);
        assert!(p != a && p != b);
        assert_eq!(lib.get(b).unwrap().title, "/m/b.mp3");
    }

    #[test]
    fn removing_a_track_takes_it_out_of_playlists() {
        let mut lib = Library::default();
        let a = lib.add(track("/m/a.mp3")).unwrap();
        let b = lib.add(track("/m/b.mp3")).unwrap();
        let p = lib.new_playlist("mix", vec![a, b, a]);
        lib.remove(&HashSet::from([a]));
        assert!(lib.get(a).is_none());
        assert_eq!(lib.get(b).map(|t| t.id), Some(b));
        assert_eq!(lib.playlist(p).unwrap().tracks, vec![b]);
        assert!(!lib.contains_path(Path::new("/m/a.mp3")));
    }

    #[test]
    fn untitled_playlists_number_themselves() {
        let mut lib = Library::default();
        assert_eq!(lib.untitled(), "untitled playlist");
        lib.new_playlist("untitled playlist", vec![]);
        assert_eq!(lib.untitled(), "untitled playlist 2");
    }

    #[test]
    fn a_saved_library_loads_back_with_its_lookups() {
        let mut lib = Library::default();
        let a = lib.add(track("/m/a.mp3")).unwrap();
        let json = serde_json::to_vec(&lib).unwrap();
        let mut back: Library = serde_json::from_slice(&json).unwrap();
        back.reindex();
        assert!(back.get(a).is_some());
        assert!(back.add(track("/m/a.mp3")).is_none());
        assert!(back.add(track("/m/c.mp3")).unwrap() > a);
    }

    #[test]
    fn folders_nest_into_the_outermost() {
        let mut lib = Library::default();
        lib.remember_folder(Path::new("/m/rock"));
        lib.remember_folder(Path::new("/m"));
        lib.remember_folder(Path::new("/m/jazz"));
        assert_eq!(lib.folders, vec![PathBuf::from("/m")]);
    }
}
