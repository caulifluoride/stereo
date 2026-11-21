//! Album artwork for the artwork pane: read off the song's file (or a cover image beside it)
//! on a worker, decoded by Rune's image cache. The sample library gets artwork painted from
//! each album's name instead, so headless snapshots have some to show.

use std::path::PathBuf;
use std::time::Duration;

use rune::{Image, ImageCache, Latest, Waker};

use crate::library::Track;

#[derive(Default)]
pub struct Art {
    cache: Option<ImageCache>,
    bytes: Latest<(PathBuf, Option<Vec<u8>>)>,
    /// The file the artwork showing (or on its way) belongs to.
    for_path: Option<PathBuf>,
    pub image: Option<Image>,
    /// Paint artwork rather than reading files (the sample library's songs have none).
    painted: bool,
}

impl Art {
    pub fn start(&mut self, waker: Waker, live: bool) {
        self.cache = Some(ImageCache::new(waker.clone()));
        self.bytes = Latest::new(waker).debounce(Duration::from_millis(40));
        self.painted = !live;
    }

    /// Shows `t`'s artwork, or none.
    pub fn show(&mut self, t: Option<&Track>) {
        let path = t.map(|t| t.path.clone());
        // Before `start` there's no cache to decode with; ask again once there is.
        if path == self.for_path || self.cache.is_none() {
            return;
        }
        self.for_path = path.clone();
        let (Some(cache), Some(t)) = (&self.cache, t) else {
            self.image = None;
            return;
        };
        if self.painted {
            self.image = Some(paint(cache, &t.album));
            return;
        }
        let path = path.expect("a track has a path");
        self.bytes.request(path, |p| {
            let b = crate::library::read_art(&p);
            (p, b)
        });
    }

    /// Takes artwork that arrived. Returns whether what shows changed.
    pub fn poll(&mut self) -> bool {
        let Some((path, bytes)) = self.bytes.take() else { return false };
        if Some(&path) != self.for_path.as_ref() {
            return false;
        }
        self.image = match (bytes, &self.cache) {
            (Some(b), Some(c)) => Some(c.load(b)),
            _ => None,
        };
        true
    }
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
