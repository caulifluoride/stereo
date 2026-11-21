//! Party Shuffle, as iTunes 4 to 7 had it: a running list of songs picked at random from a
//! source (the library or a playlist), with a few recently played songs kept above the one
//! playing and a set number of upcoming ones below it. As each song finishes the list moves up
//! and a new pick joins the bottom, so it never runs out. Songs can be added, played next,
//! taken out or moved, and "play higher rated songs more often" weights the picks by rating.
//! Picks can also lean toward songs played less, and keep the same artist from playing twice
//! in a row.
//!
//! Each entry has its own serial, so the same song can be in the list twice and a selection
//! follows an entry as the list moves.

use std::collections::HashSet;

use rand::{Rng, RngExt};
use serde::{Deserialize, Serialize};

use crate::library::TrackId;
use crate::rows::Smart;

/// How many played entries are kept, whatever is shown.
const HISTORY: usize = 100;
pub const UPCOMING: [usize; 6] = [5, 10, 15, 25, 50, 100];
pub const RECENT: [usize; 6] = [0, 5, 10, 15, 25, 50];

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Source {
    #[default]
    Library,
    Playlist(u64),
    Smart(Smart),
    /// A smart playlist the user made.
    UserSmart(u64),
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    pub serial: u64,
    pub track: TrackId,
}

/// A song the source could give, with its rating (0 to 5 stars, 0 unrated), its play count
/// and its artist (as a hash of the folded name, 0 for none).
#[derive(Clone, Copy, Debug, Default)]
pub struct Candidate {
    pub id: TrackId,
    pub rating: u8,
    pub plays: u32,
    pub artist: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Played,
    Current,
    Upcoming,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Party {
    pub source: Source,
    pub upcoming_len: usize,
    pub recent_len: usize,
    pub prefer_rated: bool,
    /// Lean toward songs played fewer times.
    pub prefer_unplayed: bool,
    /// Keep the same artist from coming up twice in a row.
    pub vary_artists: bool,
    pub history: Vec<Entry>,
    pub current: Option<Entry>,
    pub upcoming: Vec<Entry>,
    serial: u64,
}

impl Default for Party {
    fn default() -> Self {
        Party {
            source: Source::Library,
            upcoming_len: 15,
            recent_len: 5,
            prefer_rated: false,
            prefer_unplayed: false,
            vary_artists: false,
            history: Vec::new(),
            current: None,
            upcoming: Vec::new(),
            serial: 0,
        }
    }
}

/// How the picks lean.
#[derive(Clone, Copy, Debug, Default)]
struct Lean {
    rated: bool,
    unplayed: bool,
}

/// How much more often a song plays. Preferring higher-rated songs, unrated ones count as
/// about three stars, so rating a song one star plays it less than leaving it alone;
/// preferring songs played less, one never played comes up about four times as often as one
/// played fifteen times.
fn weight(c: &Candidate, lean: Lean) -> f64 {
    let mut w = 1.0;
    if lean.rated {
        w *= [4.0, 1.0, 2.0, 4.0, 7.0, 11.0][usize::from(c.rating.min(5))];
    }
    if lean.unplayed {
        w /= (1.0 + f64::from(c.plays)).sqrt();
    }
    w
}

impl Party {
    fn entry(&mut self, track: TrackId) -> Entry {
        self.serial += 1;
        Entry { serial: self.serial, track }
    }

    /// What the list shows, top to bottom: the last `recent_len` played, the one playing, then
    /// the upcoming ones.
    pub fn rows(&self) -> Vec<(Entry, Place)> {
        let from = self.history.len().saturating_sub(self.recent_len);
        let played = self.history[from..].iter().map(|&e| (e, Place::Played));
        let now = self.current.iter().map(|&e| (e, Place::Current));
        let next = self.upcoming.iter().map(|&e| (e, Place::Upcoming));
        played.chain(now).chain(next).collect()
    }

    pub fn find(&self, serial: u64) -> Option<(Entry, Place)> {
        self.rows().into_iter().find(|(e, _)| e.serial == serial)
    }

    /// Tops the upcoming songs up to `upcoming_len` with random picks from `pool`. A pick
    /// avoids what's already upcoming or playing and what played recently; a pool too small
    /// for that only avoids repeating the song just before.
    pub fn fill(&mut self, pool: &[Candidate], rng: &mut impl Rng) {
        if pool.is_empty() {
            return;
        }
        let lean = Lean { rated: self.prefer_rated, unplayed: self.prefer_unplayed };
        let artist_of = |id: TrackId| pool.iter().find(|c| c.id == id).map_or(0, |c| c.artist);
        while self.upcoming.len() < self.upcoming_len {
            let recent = (pool.len() / 2).min(self.history.len());
            let mut blocked: HashSet<TrackId> = self.upcoming.iter().chain(self.current.iter()).map(|e| e.track).collect();
            blocked.extend(self.history[self.history.len() - recent..].iter().map(|e| e.track));
            let last = self.upcoming.last().or(self.current.as_ref()).map(|e| e.track);
            let last_artist = if self.vary_artists { last.map_or(0, artist_of) } else { 0 };
            let other_artist = |c: &Candidate| last_artist == 0 || c.artist != last_artist;
            let pick = pick(pool, |c| !blocked.contains(&c.id) && other_artist(c), lean, rng)
                .or_else(|| pick(pool, |c| !blocked.contains(&c.id), lean, rng))
                .or_else(|| pick(pool, |c| Some(c.id) != last, lean, rng))
                .or_else(|| pick(pool, |_| true, lean, rng));
            let Some(id) = pick else { break };
            let e = self.entry(id);
            self.upcoming.push(e);
        }
    }

    /// Moves on to the next song: the one playing joins the played ones, the first upcoming
    /// one plays, and a new pick joins the bottom. Returns the song now playing.
    pub fn advance(&mut self, pool: &[Candidate], rng: &mut impl Rng) -> Option<TrackId> {
        if self.upcoming.is_empty() {
            self.fill(pool, rng);
        }
        if self.upcoming.is_empty() {
            return None;
        }
        if let Some(c) = self.current.take() {
            self.push_history(c);
        }
        self.current = Some(self.upcoming.remove(0));
        self.fill(pool, rng);
        self.current.map(|e| e.track)
    }

    /// Back to the song played before; the one playing goes back to the top of upcoming.
    pub fn back(&mut self) -> Option<TrackId> {
        let prev = self.history.pop()?;
        if let Some(c) = self.current.replace(prev) {
            self.upcoming.insert(0, c);
        }
        Some(prev.track)
    }

    /// Plays the entry `serial`. An upcoming one plays at once, the ones above it counting as
    /// played; a played one plays again, the ones after it going back to upcoming.
    pub fn jump(&mut self, serial: u64, pool: &[Candidate], rng: &mut impl Rng) -> Option<TrackId> {
        if let Some(i) = self.upcoming.iter().position(|e| e.serial == serial) {
            if let Some(c) = self.current.take() {
                self.push_history(c);
            }
            let skipped: Vec<Entry> = self.upcoming.drain(..i).collect();
            for e in skipped {
                self.push_history(e);
            }
            self.current = Some(self.upcoming.remove(0));
            self.fill(pool, rng);
        } else if let Some(i) = self.history.iter().position(|e| e.serial == serial) {
            let mut back: Vec<Entry> = self.history.drain(i + 1..).collect();
            back.extend(self.current.take());
            back.append(&mut self.upcoming);
            self.upcoming = back;
            self.current = self.history.pop();
        }
        self.current.filter(|e| e.serial == serial).map(|e| e.track)
    }

    fn push_history(&mut self, e: Entry) {
        self.history.push(e);
        if self.history.len() > HISTORY {
            self.history.drain(..self.history.len() - HISTORY);
        }
    }

    /// Adds songs to the end of the list.
    pub fn add(&mut self, tracks: &[TrackId]) {
        for &t in tracks {
            let e = self.entry(t);
            self.upcoming.push(e);
        }
    }

    /// Puts songs right after the one playing, in order.
    pub fn play_next(&mut self, tracks: &[TrackId]) {
        for (i, &t) in tracks.iter().enumerate() {
            let e = self.entry(t);
            self.upcoming.insert(i, e);
        }
    }

    /// Takes entries out of the list (the one playing stays) and tops the list up again.
    pub fn remove(&mut self, serials: &HashSet<u64>, pool: &[Candidate], rng: &mut impl Rng) {
        self.upcoming.retain(|e| !serials.contains(&e.serial));
        self.history.retain(|e| !serials.contains(&e.serial));
        self.fill(pool, rng);
    }

    /// Moves an upcoming entry `delta` places up (negative) or down. Returns whether it moved.
    pub fn shift(&mut self, serial: u64, delta: isize) -> bool {
        let Some(i) = self.upcoming.iter().position(|e| e.serial == serial) else { return false };
        let to = (i as isize + delta).clamp(0, self.upcoming.len() as isize - 1) as usize;
        if to == i {
            return false;
        }
        let e = self.upcoming.remove(i);
        self.upcoming.insert(to, e);
        true
    }

    /// Picks the upcoming songs afresh, as iTunes' Refresh button did.
    pub fn refresh(&mut self, pool: &[Candidate], rng: &mut impl Rng) {
        self.upcoming.clear();
        self.fill(pool, rng);
    }

    /// Shows `n` upcoming songs: cuts the list or picks more.
    pub fn set_upcoming_len(&mut self, n: usize, pool: &[Candidate], rng: &mut impl Rng) {
        self.upcoming_len = n;
        self.upcoming.truncate(n);
        self.fill(pool, rng);
    }

    /// Drops entries whose songs are gone (deleted, or out of a changed source).
    pub fn retain(&mut self, keep: impl Fn(TrackId) -> bool) {
        self.history.retain(|e| keep(e.track));
        self.upcoming.retain(|e| keep(e.track));
        if self.current.is_some_and(|e| !keep(e.track)) {
            self.current = None;
        }
    }
}

/// One weighted random pick among the candidates `ok` lets through.
fn pick(pool: &[Candidate], ok: impl Fn(&Candidate) -> bool, lean: Lean, rng: &mut impl Rng) -> Option<TrackId> {
    let total: f64 = pool.iter().filter(|c| ok(c)).map(|c| weight(c, lean)).sum();
    if total <= 0.0 {
        return None;
    }
    let mut at = rng.random_range(0.0..total);
    let mut last = None;
    for c in pool.iter().filter(|c| ok(c)) {
        last = Some(c.id);
        at -= weight(c, lean);
        if at < 0.0 {
            return Some(c.id);
        }
    }
    last
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;

    fn pool(n: u64) -> Vec<Candidate> {
        (1..=n).map(|id| Candidate { id, ..Candidate::default() }).collect()
    }

    fn tracks(es: &[Entry]) -> Vec<TrackId> {
        es.iter().map(|e| e.track).collect()
    }

    #[test]
    fn fills_upcoming_without_repeats() {
        let mut rng = StdRng::seed_from_u64(1);
        let mut p = Party::default();
        p.fill(&pool(100), &mut rng);
        assert_eq!(p.upcoming.len(), 15);
        let unique: HashSet<_> = tracks(&p.upcoming).into_iter().collect();
        assert_eq!(unique.len(), 15);
    }

    #[test]
    fn advancing_moves_the_list_up_and_keeps_it_full() {
        let mut rng = StdRng::seed_from_u64(2);
        let pool = pool(100);
        let mut p = Party::default();
        p.fill(&pool, &mut rng);
        let first = p.upcoming[0];
        let second = p.upcoming[1];
        assert_eq!(p.advance(&pool, &mut rng), Some(first.track));
        assert_eq!(p.current, Some(first));
        assert_eq!(p.advance(&pool, &mut rng), Some(second.track));
        assert_eq!(p.history, vec![first]);
        assert_eq!(p.upcoming.len(), 15);
        // Rows show the played one, the one playing, then what's next.
        let rows = p.rows();
        assert_eq!(rows[0], (first, Place::Played));
        assert_eq!(rows[1], (second, Place::Current));
        assert_eq!(rows.len(), 17);
    }

    #[test]
    fn recently_played_songs_dont_come_straight_back() {
        let mut rng = StdRng::seed_from_u64(3);
        let pool = pool(40);
        let mut p = Party { upcoming_len: 5, ..Party::default() };
        for _ in 0..200 {
            p.advance(&pool, &mut rng);
            let now = p.current.unwrap().track;
            let recent: Vec<_> = p.history.iter().rev().take(10).map(|e| e.track).collect();
            assert!(!recent.contains(&now), "{now} played again within ten songs");
        }
    }

    #[test]
    fn a_tiny_source_still_fills_without_back_to_back_repeats() {
        let mut rng = StdRng::seed_from_u64(4);
        let pool = pool(2);
        let mut p = Party::default();
        p.advance(&pool, &mut rng);
        assert_eq!(p.upcoming.len(), 15);
        let mut all = vec![p.current.unwrap().track];
        all.extend(tracks(&p.upcoming));
        assert!(all.windows(2).all(|w| w[0] != w[1]), "{all:?}");
        // An empty source picks nothing.
        let mut q = Party::default();
        assert_eq!(q.advance(&[], &mut rng), None);
    }

    #[test]
    fn higher_rated_songs_play_more_often_when_asked() {
        let mut rng = StdRng::seed_from_u64(5);
        let pool = vec![Candidate { id: 1, rating: 1, ..Candidate::default() }, Candidate { id: 5, rating: 5, ..Candidate::default() }];
        let rated = Lean { rated: true, unplayed: false };
        let mut count: HashMap<TrackId, u32> = HashMap::new();
        for _ in 0..5000 {
            *count.entry(pick(&pool, |_| true, rated, &mut rng).unwrap()).or_default() += 1;
        }
        assert!(count[&5] > count[&1] * 6, "{count:?}");
        let mut even: HashMap<TrackId, u32> = HashMap::new();
        for _ in 0..5000 {
            *even.entry(pick(&pool, |_| true, Lean::default(), &mut rng).unwrap()).or_default() += 1;
        }
        assert!(even[&1].abs_diff(even[&5]) < 400, "{even:?}");
    }

    #[test]
    fn songs_played_less_come_up_more_when_asked() {
        let mut rng = StdRng::seed_from_u64(10);
        let pool = vec![Candidate { id: 1, plays: 0, ..Candidate::default() }, Candidate { id: 2, plays: 15, ..Candidate::default() }];
        let lean = Lean { rated: false, unplayed: true };
        let mut count: HashMap<TrackId, u32> = HashMap::new();
        for _ in 0..5000 {
            *count.entry(pick(&pool, |_| true, lean, &mut rng).unwrap()).or_default() += 1;
        }
        assert!(count[&1] > count[&2] * 3, "{count:?}");
    }

    #[test]
    fn varied_artists_dont_play_back_to_back() {
        let mut rng = StdRng::seed_from_u64(11);
        // Ten artists, ten songs each.
        let pool: Vec<Candidate> = (1..=100).map(|id| Candidate { id, artist: id % 10 + 1, ..Candidate::default() }).collect();
        let mut p = Party { vary_artists: true, upcoming_len: 30, ..Party::default() };
        p.advance(&pool, &mut rng);
        let mut all = vec![p.current.unwrap().track];
        all.extend(tracks(&p.upcoming));
        assert!(all.windows(2).all(|w| w[0] % 10 != w[1] % 10), "{all:?}");
    }

    #[test]
    fn jumping_ahead_counts_the_skipped_as_played_and_back_returns() {
        let mut rng = StdRng::seed_from_u64(6);
        let pool = pool(100);
        let mut p = Party::default();
        p.advance(&pool, &mut rng);
        let playing = p.current.unwrap();
        let (a, b, c) = (p.upcoming[0], p.upcoming[1], p.upcoming[2]);
        assert_eq!(p.jump(c.serial, &pool, &mut rng), Some(c.track));
        assert_eq!(p.history, vec![playing, a, b]);
        assert_eq!(p.upcoming.len(), 15);
        assert_eq!(p.back(), Some(b.track));
        assert_eq!(p.upcoming[0], c);
        // Playing a played one again puts the ones after it back in upcoming.
        assert_eq!(p.jump(playing.serial, &pool, &mut rng), Some(playing.track));
        assert!(p.history.is_empty());
        assert_eq!(&p.upcoming[..3], &[a, b, c]);
    }

    #[test]
    fn songs_can_be_added_played_next_moved_and_removed() {
        let mut rng = StdRng::seed_from_u64(7);
        let pool = pool(100);
        let mut p = Party { upcoming_len: 5, ..Party::default() };
        p.advance(&pool, &mut rng);
        p.play_next(&[500, 501]);
        assert_eq!(tracks(&p.upcoming[..2]), vec![500, 501]);
        p.add(&[502]);
        assert_eq!(p.upcoming.last().unwrap().track, 502);
        let moving = p.upcoming[0].serial;
        assert!(p.shift(moving, 1));
        assert_eq!(tracks(&p.upcoming[..2]), vec![501, 500]);
        assert!(!p.shift(p.upcoming[0].serial, -1));
        let gone: HashSet<u64> = p.upcoming.iter().filter(|e| e.track >= 500).map(|e| e.serial).collect();
        p.remove(&gone, &pool, &mut rng);
        assert!(p.upcoming.iter().all(|e| e.track < 500));
        assert_eq!(p.upcoming.len(), 5);
    }

    #[test]
    fn the_same_song_twice_is_two_entries() {
        let mut p = Party::default();
        p.add(&[9, 9]);
        assert_ne!(p.upcoming[0].serial, p.upcoming[1].serial);
    }

    #[test]
    fn refresh_and_resizing_repick_the_upcoming_songs() {
        let mut rng = StdRng::seed_from_u64(8);
        let pool = pool(500);
        let mut p = Party::default();
        p.advance(&pool, &mut rng);
        let before = p.upcoming.clone();
        p.refresh(&pool, &mut rng);
        assert_ne!(before, p.upcoming);
        assert_eq!(p.upcoming.len(), 15);
        p.set_upcoming_len(5, &pool, &mut rng);
        assert_eq!(p.upcoming.len(), 5);
        p.set_upcoming_len(25, &pool, &mut rng);
        assert_eq!(p.upcoming.len(), 25);
    }

    #[test]
    fn deleted_songs_leave_the_list() {
        let mut rng = StdRng::seed_from_u64(9);
        let pool = pool(30);
        let mut p = Party::default();
        p.advance(&pool, &mut rng);
        p.advance(&pool, &mut rng);
        p.retain(|t| t % 2 == 0);
        assert!(p.rows().iter().all(|(e, _)| e.track % 2 == 0));
    }
}
