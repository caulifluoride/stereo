//! Playing songs: starting one from a list or from Party Shuffle, moving on when it ends,
//! next and previous, pause, seeking and volume. Play counts and skips are kept as iTunes
//! kept them: a song that plays to its end counts a play, one skipped past early a skip.

use std::collections::HashSet;

use rune::Tx;

use crate::State;
use crate::library::{TrackId, now};
use crate::party::{Candidate, Source};
use crate::queue::Queue;
use crate::rows::View;

/// The song loaded in the player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Now {
    pub track: TrackId,
    /// Which load this is, so news about an earlier song is ignored.
    pub token: u64,
    pub playing: bool,
    /// Started from Party Shuffle, so the party list decides what's next.
    pub party: bool,
    pub duration: f32,
}

/// How far into a song Previous restarts it rather than going back.
const RESTART_AFTER: f32 = 3.0;

impl State {
    /// The songs Party Shuffle picks from.
    pub fn party_pool(&self) -> Vec<Candidate> {
        let lib = &self.lib;
        let ids: Vec<TrackId> = match lib.party.source {
            Source::Library => lib.tracks.iter().map(|t| t.id).collect(),
            Source::Playlist(p) => lib.playlist(p).map(|p| p.tracks.clone()).unwrap_or_default(),
        };
        ids.into_iter().filter_map(|id| lib.get(id)).map(|t| Candidate { id: t.id, rating: t.rating }).collect()
    }

    /// Fills Party Shuffle's upcoming songs if it has room, as when it's first shown.
    pub fn party_fill(&mut self) {
        let pool = self.party_pool();
        self.lib.party.fill(&pool, &mut self.rng);
        self.party_changed();
    }

    pub fn party_changed(&mut self) {
        self.changed();
        self.refresh_party();
    }

    /// Loads `id` and plays it (or leaves it paused).
    pub fn load(&mut self, id: TrackId, party: bool, play: bool, at: f32) {
        let Some(t) = self.lib.get(id) else { return };
        self.token += 1;
        self.engine.load(t.path.clone(), self.token, at, !play);
        self.now = Some(Now { track: id, token: self.token, playing: play, party, duration: t.duration });
        self.pos = at;
        self.request_art();
        self.publish_time();
    }

    /// Starts row `i` of the track list, the rest of the list queued after it.
    pub fn play_row(&mut self, i: usize) {
        let Some(&id) = self.rows.get(i) else { return };
        self.queue = Queue::start(self.rows.clone(), i, self.settings.shuffle, &mut self.rng);
        self.load(id, false, true, 0.0);
    }

    /// Starts the Party Shuffle entry `serial`.
    pub fn play_party(&mut self, serial: u64) {
        let pool = self.party_pool();
        if let Some(id) = self.lib.party.jump(serial, &pool, &mut self.rng) {
            self.load(id, true, true, 0.0);
        } else if self.lib.party.current.is_some_and(|e| e.serial == serial) {
            let id = self.lib.party.current.map(|e| e.track).expect("checked");
            self.load(id, true, true, 0.0);
        }
        self.party_changed();
    }

    /// Plays from the top of Party Shuffle (or carries on with what it has playing).
    pub fn start_party(&mut self) {
        let pool = self.party_pool();
        let id = match self.lib.party.current {
            Some(e) => Some(e.track),
            None => self.lib.party.advance(&pool, &mut self.rng),
        };
        if let Some(id) = id {
            self.load(id, true, true, 0.0);
        }
        self.party_changed();
    }

    pub fn play_pause(&mut self) {
        match self.now {
            Some(ref mut n) if n.playing => {
                n.playing = false;
                self.engine.pause();
            }
            Some(ref mut n) => {
                n.playing = true;
                self.engine.play();
            }
            // Nothing loaded: play what's selected, else Party Shuffle or the list's top.
            None => {
                if self.view == View::Party {
                    let first = self.table_party.selected().next().and_then(|s| s.parse().ok());
                    match first {
                        Some(serial) => self.play_party(serial),
                        None => self.start_party(),
                    }
                } else {
                    let sel = self.selected_tracks();
                    let i = sel.first().and_then(|id| self.rows.iter().position(|r| r == id)).unwrap_or(0);
                    self.play_row(i);
                }
            }
        }
    }

    fn count_skip(&mut self) {
        if let Some(n) = self.now
            && self.pos > 2.0
            && self.pos < n.duration * 0.5
            && let Some(t) = self.lib.get_mut(n.track)
        {
            t.skips += 1;
            self.changed();
        }
    }

    /// The song after this one. `ended`: it played to its end rather than being skipped.
    pub fn next(&mut self, ended: bool) {
        let Some(n) = self.now else { return };
        if !ended {
            self.count_skip();
        }
        let next = if n.party {
            let pool = self.party_pool();
            let id = self.lib.party.advance(&pool, &mut self.rng);
            self.party_changed();
            id
        } else {
            self.queue.next(self.settings.repeat, ended)
        };
        match next {
            Some(id) => self.load(id, n.party, ended || n.playing, 0.0),
            None => self.stop(),
        }
    }

    pub fn prev(&mut self) {
        let Some(n) = self.now else { return };
        if self.pos > RESTART_AFTER {
            self.seek(0.0);
            return;
        }
        let prev = if n.party {
            let p = self.lib.party.back();
            self.party_changed();
            p
        } else {
            self.queue.prev(self.settings.repeat)
        };
        match prev {
            Some(id) => self.load(id, n.party, n.playing, 0.0),
            None => self.seek(0.0),
        }
    }

    pub fn stop(&mut self) {
        self.engine.stop();
        self.now = None;
        self.pos = 0.0;
        self.publish_time();
    }

    pub fn seek(&mut self, secs: f32) {
        if let Some(n) = self.now {
            self.pos = secs.clamp(0.0, n.duration.max(0.0));
            self.engine.seek(self.pos);
            self.publish_time();
        }
    }

    pub fn set_volume(&mut self, v: f32) {
        self.settings.volume = v.clamp(0.0, 1.0);
        self.engine.volume(self.settings.volume);
        self.settings_changed();
    }

    pub fn toggle_shuffle(&mut self) {
        self.settings.shuffle = !self.settings.shuffle;
        if self.settings.shuffle {
            self.queue.shuffle(&mut self.rng);
        } else {
            self.queue.unshuffle();
        }
        self.settings_changed();
    }

    /// A song played to its end: count it, then move on.
    pub fn ended(&mut self, token: u64) {
        let Some(n) = self.now.filter(|n| n.token == token) else { return };
        if let Some(t) = self.lib.get_mut(n.track) {
            t.plays += 1;
            t.last_played = Some(now());
        }
        self.changed();
        self.next(true);
    }

    /// A song wouldn't play: say so and try the next, unless nothing in a row plays.
    pub fn failed(&mut self, token: u64, why: String) {
        let Some(n) = self.now.filter(|n| n.token == token) else { return };
        let name = self.lib.get(n.track).map(|t| t.title.clone()).unwrap_or_default();
        self.notice = Some(format!("Couldn't play “{name}”: {why}"));
        self.failures += 1;
        if self.failures < 5 {
            self.next(true);
        } else {
            self.failures = 0;
            self.stop();
        }
    }

    /// Puts songs right after the one playing in Party Shuffle.
    pub fn party_play_next(&mut self, ids: &[TrackId]) {
        self.lib.party.play_next(ids);
        self.party_changed();
    }

    pub fn party_add(&mut self, ids: &[TrackId]) {
        self.lib.party.add(ids);
        self.party_changed();
    }

    pub fn party_remove(&mut self, serials: &HashSet<u64>) {
        let pool = self.party_pool();
        self.lib.party.remove(serials, &pool, &mut self.rng);
        self.party_changed();
    }

    pub fn party_refresh(&mut self) {
        let pool = self.party_pool();
        self.lib.party.refresh(&pool, &mut self.rng);
        self.party_changed();
    }

    pub fn party_source(&mut self, src: Source) {
        if self.lib.party.source == src {
            return;
        }
        self.lib.party.source = src;
        self.party_refresh();
    }

    /// Jumps the list to the song playing.
    pub fn reveal_now(&mut self, tx: &mut Tx) {
        let Some(n) = self.now else { return };
        if n.party {
            self.select_view(View::Party);
            if let Some(e) = self.lib.party.current {
                self.table_party.reveal(e.serial.to_string(), tx);
            }
            return;
        }
        if !self.rows.contains(&n.track) {
            self.search = rune::TextEdit::new("");
            self.browser.genre = None;
            self.browser.artist = None;
            self.browser.album = None;
            self.select_view(View::Music);
        }
        self.table.reveal(n.track.to_string(), tx);
    }
}
