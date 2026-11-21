//! Playlists in and out as M3U files: the extended kind (`#EXTM3U`, with each song's length
//! and name), as UTF-8 (`.m3u8`, or `.m3u` read the same way). Paths written are absolute;
//! paths read may be relative to the playlist file's folder, or `file://` links.

use std::path::{Path, PathBuf};

use crate::library::Library;

/// The playlist file's text.
pub fn write(lib: &Library, ids: &[u64]) -> String {
    let mut out = String::from("#EXTM3U\n");
    for t in ids.iter().filter_map(|&id| lib.get(id)) {
        let name = if t.artist.is_empty() { t.title.clone() } else { format!("{} - {}", t.artist, t.title) };
        out.push_str(&format!("#EXTINF:{},{}\n{}\n", t.duration.round() as i64, name.replace('\n', " "), t.path.display()));
    }
    out
}

/// The songs a playlist file lists, as paths, in order.
pub fn read(text: &str, dir: &Path) -> Vec<PathBuf> {
    text.lines()
        .map(|l| l.trim().trim_start_matches('\u{feff}'))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let l = l.strip_prefix("file://").map(unescape).unwrap_or_else(|| l.to_string());
            let p = PathBuf::from(l);
            if p.is_absolute() { p } else { dir.join(p) }
        })
        .collect()
}

/// `%20` and friends back to bytes.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Track;

    #[test]
    fn playlists_go_out_and_come_back() {
        let mut lib = Library::default();
        let a = lib.add(Track { path: "/m/a song.mp3".into(), title: "A".into(), artist: "Abba".into(), duration: 61.4, ..Track::default() }).unwrap();
        let b = lib.add(Track { path: "/m/b.flac".into(), title: "B".into(), duration: 5.0, ..Track::default() }).unwrap();
        let text = write(&lib, &[b, a]);
        assert_eq!(text, "#EXTM3U\n#EXTINF:5,B\n/m/b.flac\n#EXTINF:61,Abba - A\n/m/a song.mp3\n");
        assert_eq!(read(&text, Path::new("/x")), [PathBuf::from("/m/b.flac"), PathBuf::from("/m/a song.mp3")]);
    }

    #[test]
    fn relative_paths_and_links_resolve() {
        let text = "\u{feff}#EXTM3U\r\nsub/one.mp3\r\n\r\nfile:///m/two%20songs.mp3\r\n";
        assert_eq!(read(text, Path::new("/lists")), [PathBuf::from("/lists/sub/one.mp3"), PathBuf::from("/m/two songs.mp3")]);
    }
}
