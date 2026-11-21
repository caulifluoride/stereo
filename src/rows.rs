//! What the track list shows: the source picked in the sidebar, narrowed by the column
//! browser (genre, artist, album) and the search field, in the order of the column clicked.
//! It's worked out when something it depends on changes, not on every frame.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use rune_kit::SortDir;

use crate::library::{Library, Track, TrackId};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Smart {
    RecentlyAdded,
    RecentlyPlayed,
    TopRated,
    MostPlayed,
}

impl Smart {
    pub const ALL: [Smart; 4] = [Smart::MostPlayed, Smart::TopRated, Smart::RecentlyAdded, Smart::RecentlyPlayed];

    pub fn name(self) -> &'static str {
        match self {
            Smart::RecentlyAdded => "Recently Added",
            Smart::RecentlyPlayed => "Recently Played",
            Smart::TopRated => "My Top Rated",
            Smart::MostPlayed => "Top 25 Most Played",
        }
    }

    fn pick(self, lib: &Library) -> Vec<TrackId> {
        let mut ts: Vec<&Track> = lib.tracks.iter().collect();
        let (keep, limit): (fn(&Track) -> bool, usize) = match self {
            Smart::RecentlyAdded => {
                ts.sort_by(|a, b| b.added.cmp(&a.added).then(b.id.cmp(&a.id)));
                (|_| true, 100)
            }
            Smart::RecentlyPlayed => {
                ts.sort_by_key(|t| std::cmp::Reverse(t.last_played));
                (|t| t.last_played.is_some(), 50)
            }
            Smart::TopRated => {
                ts.sort_by(|a, b| b.rating.cmp(&a.rating).then_with(|| default_order(a, b)));
                (|t| t.rating >= 4, usize::MAX)
            }
            Smart::MostPlayed => {
                ts.sort_by(|a, b| b.plays.cmp(&a.plays).then(b.last_played.cmp(&a.last_played)));
                (|t| t.plays > 0, 25)
            }
        };
        ts.into_iter().filter(|t| keep(t)).take(limit).map(|t| t.id).collect()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Music,
    Party,
    Smart(Smart),
    Playlist(u64),
}

impl View {
    pub fn key(self) -> String {
        match self {
            View::Music => "src-music".into(),
            View::Party => "src-party".into(),
            View::Smart(s) => format!("src-smart-{}", Smart::ALL.iter().position(|&x| x == s).unwrap_or(0)),
            View::Playlist(id) => format!("src-pl-{id}"),
        }
    }

    pub fn title(self, lib: &Library) -> String {
        match self {
            View::Music => "Music".into(),
            View::Party => "Party Shuffle".into(),
            View::Smart(s) => s.name().into(),
            View::Playlist(id) => lib.playlist(id).map_or_else(String::new, |p| p.name.clone()),
        }
    }

    /// Whether the list keeps an order of its own (a playlist's, or a smart list's ranking)
    /// until a column is clicked.
    pub fn ordered(self) -> bool {
        !matches!(self, View::Music)
    }
}

/// The column browser's three lists and what's picked in each (`None`: All).
#[derive(Default, Debug, PartialEq)]
pub struct Browser {
    pub genres: Vec<String>,
    pub artists: Vec<String>,
    pub albums: Vec<String>,
    pub genre: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

fn fold(s: &str) -> String {
    s.to_lowercase()
}

/// The library's own order: by artist, then album, disc and track number.
pub fn default_order(a: &Track, b: &Track) -> Ordering {
    let key = |t: &Track| (fold(t.album_artist_or_artist()), fold(&t.album), t.disc_no.unwrap_or(0), t.track_no.unwrap_or(0));
    key(a).cmp(&key(b)).then_with(|| rune_kit::natural_cmp(&a.title, &b.title)).then(a.id.cmp(&b.id))
}

fn by_column(col: &str, a: &Track, b: &Track) -> Ordering {
    let text = |x: &str, y: &str| rune_kit::natural_cmp(&fold(x), &fold(y));
    let o = match col {
        "name" => text(&a.title, &b.title),
        "time" => a.duration.total_cmp(&b.duration),
        "artist" => text(&a.artist, &b.artist),
        "album" => text(&a.album, &b.album),
        "genre" => text(&a.genre, &b.genre),
        "year" => a.year.cmp(&b.year),
        "rating" => a.rating.cmp(&b.rating),
        "plays" => a.plays.cmp(&b.plays),
        "last" => a.last_played.cmp(&b.last_played),
        "added" => a.added.cmp(&b.added),
        _ => Ordering::Equal,
    };
    o.then_with(|| default_order(a, b))
}

pub fn matches(t: &Track, words: &[String]) -> bool {
    words.iter().all(|w| [&t.title, &t.artist, &t.album, &t.album_artist, &t.genre].iter().any(|f| fold(f).contains(w.as_str())))
}

/// The source's tracks, before the browser and search narrow them.
pub fn source(lib: &Library, view: View) -> Vec<TrackId> {
    match view {
        View::Music | View::Party => lib.tracks.iter().map(|t| t.id).collect(),
        View::Smart(s) => s.pick(lib),
        View::Playlist(id) => lib.playlist(id).map(|p| p.tracks.clone()).unwrap_or_default(),
    }
}

/// Works out the browser's lists and the rows. `browse` is whether the browser narrows the
/// rows (it shows in Music only).
pub fn compute(lib: &Library, view: View, search: &str, sort: Option<(&str, SortDir)>, browser: &mut Browser, browse: bool) -> Vec<TrackId> {
    let words: Vec<String> = search.split_whitespace().map(fold).collect();
    let mut ids: Vec<TrackId> = source(lib, view).into_iter().filter(|&id| lib.get(id).is_some_and(|t| matches(t, &words))).collect();
    if browse {
        let distinct = |ids: &[TrackId], f: fn(&Track) -> &str| -> Vec<String> {
            let mut seen = BTreeSet::new();
            let mut out = Vec::new();
            for t in ids.iter().filter_map(|&id| lib.get(id)) {
                let v = f(t);
                if !v.is_empty() && seen.insert(fold(v)) {
                    out.push(v.to_string());
                }
            }
            out.sort_by(|a, b| rune_kit::natural_cmp(&fold(a), &fold(b)));
            out
        };
        let pick = |ids: &mut Vec<TrackId>, chosen: &Option<String>, f: fn(&Track) -> &str| {
            if let Some(c) = chosen {
                let c = fold(c);
                ids.retain(|&id| lib.get(id).is_some_and(|t| fold(f(t)) == c));
            }
        };
        browser.genres = distinct(&ids, |t| &t.genre);
        keep_choice(&mut browser.genre, &browser.genres);
        pick(&mut ids, &browser.genre, |t| &t.genre);
        browser.artists = distinct(&ids, |t| t.album_artist_or_artist());
        keep_choice(&mut browser.artist, &browser.artists);
        pick(&mut ids, &browser.artist, |t| t.album_artist_or_artist());
        browser.albums = distinct(&ids, |t| &t.album);
        keep_choice(&mut browser.album, &browser.albums);
        pick(&mut ids, &browser.album, |t| &t.album);
    }
    let get = |id: &TrackId| lib.get(*id).expect("filtered to tracks in the library");
    match sort {
        Some((col, dir)) => ids.sort_by(|a, b| dir.apply(by_column(col, get(a), get(b)))),
        None if !view.ordered() => ids.sort_by(|a, b| default_order(get(a), get(b))),
        None => {}
    }
    ids
}

/// A choice that's no longer in its list goes back to All.
fn keep_choice(choice: &mut Option<String>, list: &[String]) {
    if choice.as_ref().is_some_and(|c| !list.iter().any(|x| fold(x) == fold(c))) {
        *choice = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> Library {
        let mut lib = Library::default();
        let t = |title: &str, artist: &str, album: &str, genre: &str, no: u32| Track {
            path: format!("/m/{title}").into(),
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            genre: genre.into(),
            track_no: Some(no),
            ..Track::default()
        };
        lib.add(t("Song B", "Zed", "Second", "Rock", 2));
        lib.add(t("Song A", "Zed", "Second", "Rock", 1));
        lib.add(t("Tune", "Abba", "First", "Pop", 1));
        lib.add(t("Other", "Abba", "Gold", "Pop", 1));
        lib
    }

    fn titles(lib: &Library, ids: &[TrackId]) -> Vec<String> {
        ids.iter().map(|&id| lib.get(id).unwrap().title.clone()).collect()
    }

    #[test]
    fn music_sorts_by_artist_album_and_track() {
        let lib = lib();
        let ids = compute(&lib, View::Music, "", None, &mut Browser::default(), false);
        assert_eq!(titles(&lib, &ids), ["Tune", "Other", "Song A", "Song B"]);
        let ids = compute(&lib, View::Music, "", Some(("name", SortDir::Descending)), &mut Browser::default(), false);
        assert_eq!(titles(&lib, &ids), ["Tune", "Song B", "Song A", "Other"]);
    }

    #[test]
    fn search_matches_every_word_anywhere() {
        let lib = lib();
        let ids = compute(&lib, View::Music, "abba gold", None, &mut Browser::default(), false);
        assert_eq!(titles(&lib, &ids), ["Other"]);
        assert!(compute(&lib, View::Music, "nothing", None, &mut Browser::default(), false).is_empty());
    }

    #[test]
    fn the_browser_narrows_genre_then_artist_then_album() {
        let lib = lib();
        let mut b = Browser { genre: Some("pop".into()), ..Browser::default() };
        let ids = compute(&lib, View::Music, "", None, &mut b, true);
        assert_eq!(b.genres, ["Pop", "Rock"]);
        assert_eq!(b.artists, ["Abba"]);
        assert_eq!(b.albums, ["First", "Gold"]);
        assert_eq!(ids.len(), 2);
        b.album = Some("Gold".into());
        let ids = compute(&lib, View::Music, "", None, &mut b, true);
        assert_eq!(titles(&lib, &ids), ["Other"]);
        // A choice the search hides goes back to All.
        let ids = compute(&lib, View::Music, "zed", None, &mut b, true);
        assert_eq!(b.genre, None);
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn playlists_keep_their_order_until_sorted() {
        let mut lib = lib();
        let order: Vec<TrackId> = lib.tracks.iter().rev().map(|t| t.id).collect();
        let p = lib.new_playlist("mix", order.clone());
        assert_eq!(compute(&lib, View::Playlist(p), "", None, &mut Browser::default(), false), order);
    }

    #[test]
    fn smart_lists_pick_from_play_counts_and_ratings() {
        let mut lib = lib();
        let ids: Vec<TrackId> = lib.tracks.iter().map(|t| t.id).collect();
        lib.get_mut(ids[0]).unwrap().plays = 3;
        lib.get_mut(ids[2]).unwrap().plays = 9;
        lib.get_mut(ids[1]).unwrap().rating = 5;
        assert_eq!(source(&lib, View::Smart(Smart::MostPlayed)), vec![ids[2], ids[0]]);
        assert_eq!(source(&lib, View::Smart(Smart::TopRated)), vec![ids[1]]);
        assert!(source(&lib, View::Smart(Smart::RecentlyPlayed)).is_empty());
    }
}
