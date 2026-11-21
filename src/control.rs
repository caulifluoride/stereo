//! Playing songs: starting one from a list or from Party Shuffle, moving on when it ends,
//! next and previous, Up Next, pause, seeking, volume and the sleep timer. A song plays its
//! count when it ends (or halfway, or at four minutes, as set) and counts a skip when moved
//! past early, as iTunes kept them.
//!
//! The audio thread is always told which song follows the one playing, so it can start it
//! without a gap or fade into it; when it does, [`State::advanced`] catches the lists up.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use rune::Tx;

use crate::State;
use crate::library::{TrackId, now};
use crate::party::{Candidate, Source};
use crate::queue::Queue;
use crate::rows::{Extra, View};

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
    /// It has counted its play already.
    pub counted: bool,
    /// It has been scrobbled already.
    pub scrobbled: bool,
    /// When it started, in Unix seconds, as a scrobble records it.
    pub started: u64,
}

impl Now {
    pub fn new(track: TrackId, token: u64, playing: bool, party: bool, duration: f32) -> Now {
        Now { track, token, playing, party, duration, counted: false, scrobbled: false, started: now() }
    }
}

/// When the sleep timer stops the music.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sleep {
    At(Instant),
    EndOfSong,
    EndOfAlbum,
}

/// How far into a song Previous restarts it rather than going back.
const RESTART_AFTER: f32 = 3.0;

/// The folded name a song's artist goes by, hashed, for telling artists apart quickly.
fn artist_key(name: &str) -> u64 {
    let name = name.trim().to_lowercase();
    if name.is_empty() {
        return 0;
    }
    name.bytes().fold(14695981039346656037u64, |h, b| (h ^ u64::from(b)).wrapping_mul(1099511628211)).max(1)
}

impl State {
    pub fn extra(&self) -> Extra<'_> {
        Extra { sizes: self.settings.smart, up_next: &self.up_next, missing: Some(&self.missing) }
    }

    /// The songs Party Shuffle picks from.
    pub fn party_pool(&self) -> Vec<Candidate> {
        let lib = &self.lib;
        let ids: Vec<TrackId> = match lib.party.source {
            Source::Library => lib.tracks.iter().map(|t| t.id).collect(),
            Source::Playlist(p) => lib.playlist(p).map(|p| p.tracks.clone()).unwrap_or_default(),
            Source::Smart(sm) => crate::rows::source(lib, View::Smart(sm), &self.extra()),
            Source::UserSmart(id) => lib.smart_list(id).map(|p| p.pick(lib)).unwrap_or_default(),
        };
        ids.into_iter()
            .filter_map(|id| lib.get(id))
            .map(|t| Candidate { id: t.id, rating: t.rating, plays: t.plays, artist: artist_key(t.album_artist_or_artist()) })
            .collect()
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
        self.sync_preload();
    }

    /// Loads `id` and plays it (or leaves it paused).
    pub fn load(&mut self, id: TrackId, party: bool, play: bool, at: f32) {
        let Some(t) = self.lib.get(id) else { return };
        self.token += 1;
        self.engine.load(t.path.clone(), self.token, t.duration, at, !play);
        self.now = Some(Now::new(id, self.token, play, party, t.duration));
        self.preload = None;
        self.pos = at;
        self.request_art();
        self.publish_time();
        if play {
            self.song_started();
        }
        self.sync_preload();
    }

    /// A song began playing: say so on the desktop and to the scrobblers.
    fn song_started(&mut self) {
        let Some(t) = self.now.and_then(|n| self.lib.get(n.track)) else { return };
        if self.live && self.settings.notify {
            let by = [t.artist.as_str(), t.album.as_str()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" — ");
            crate::desktop::notify(&t.title, &by);
        }
        if self.live {
            self.scrobbler.now_playing(&self.settings, t);
        }
    }

    /// The song that follows the one playing if it plays to its end, unless the sleep timer
    /// stops the music there.
    pub fn peek_next(&self) -> Option<TrackId> {
        let n = self.now?;
        if self.sleep == Some(Sleep::EndOfSong) {
            return None;
        }
        let next = if n.party {
            self.lib.party.upcoming.first().map(|e| e.track)
        } else if let Some(&id) = self.up_next.first() {
            Some(id)
        } else {
            self.queue.peek(self.settings.repeat)
        };
        if self.sleep == Some(Sleep::EndOfAlbum) && !self.same_album(n.track, next) {
            return None;
        }
        next
    }

    fn same_album(&self, a: TrackId, b: Option<TrackId>) -> bool {
        let (Some(a), Some(b)) = (self.lib.get(a), b.and_then(|b| self.lib.get(b))) else { return false };
        a.album == b.album && a.album_artist_or_artist() == b.album_artist_or_artist()
    }

    /// Tells the audio thread which song comes next, when that changed.
    pub fn sync_preload(&mut self) {
        let want = self.peek_next();
        if self.preload.map(|(_, id)| id) == want {
            return;
        }
        match want.and_then(|id| self.lib.get(id)) {
            Some(t) => {
                self.token += 1;
                self.engine.preload(Some((t.path.clone(), self.token, t.duration)));
                self.preload = Some((self.token, t.id));
            }
            None => {
                self.engine.preload(None);
                self.preload = None;
            }
        }
    }

    /// Sends the crossfade, gapless, levelling and equaliser settings to the audio thread.
    pub fn apply_playback(&self) {
        let st = &self.settings;
        self.engine.transition(st.crossfade, st.gapless);
        self.engine.tone(crate::dsp::ToneSettings { gain: st.gain, preamp: st.preamp, eq_on: st.eq_on, eq: st.eq });
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
            && !n.counted
            && self.settings.skip_count.counts(self.pos, n.duration)
            && let Some(t) = self.lib.get_mut(n.track)
        {
            t.skips += 1;
            self.changed();
        }
    }

    fn count_play(&mut self, id: TrackId) {
        if let Some(t) = self.lib.get_mut(id) {
            t.plays += 1;
            t.last_played = Some(now());
        }
        self.changed();
    }

    /// Counts the play and the scrobble a song had coming when it ends.
    fn finish(&mut self, n: Now) {
        if !n.counted {
            self.count_play(n.track);
        }
        if !n.scrobbled
            && self.live
            && crate::scrobble::due(n.duration, n.duration)
            && let Some(t) = self.lib.get(n.track)
        {
            self.scrobbler.scrobble(&self.settings, t, n.started);
        }
    }

    /// Moves the lists on from the song `n` and returns the one after it.
    fn step(&mut self, n: Now, ended: bool) -> Option<TrackId> {
        if n.party {
            let pool = self.party_pool();
            let id = self.lib.party.advance(&pool, &mut self.rng);
            self.party_changed();
            id
        } else if !self.up_next.is_empty() {
            let id = self.up_next.remove(0);
            if self.view == View::UpNext {
                self.refresh();
            }
            Some(id)
        } else {
            self.queue.next(self.settings.repeat, ended)
        }
    }

    /// The song after this one. `ended`: it played to its end rather than being skipped.
    pub fn next(&mut self, ended: bool) {
        let Some(n) = self.now else { return };
        if !ended {
            self.count_skip();
        }
        match self.step(n, ended) {
            Some(id) => self.load(id, n.party, ended || n.playing, 0.0),
            None => self.stop(),
        }
    }

    /// Once a second or so while a song plays: count its play or scrobble it when it gets far
    /// enough, and stop for the sleep timer.
    pub fn tick(&mut self) {
        if let Some(Sleep::At(at)) = self.sleep
            && Instant::now() >= at
        {
            self.sleep = None;
            if self.now.is_some_and(|n| n.playing) {
                self.play_pause();
            }
        }
        let Some(n) = self.now.filter(|n| n.playing) else { return };
        if !n.counted && self.settings.play_count.reached(self.pos, n.duration) {
            self.count_play(n.track);
            if let Some(m) = &mut self.now {
                m.counted = true;
            }
        }
        if !n.scrobbled && crate::scrobble::due(self.pos, n.duration) {
            if self.live
                && let Some(t) = self.lib.get(n.track)
            {
                self.scrobbler.scrobble(&self.settings, t, n.started);
            }
            if let Some(m) = &mut self.now {
                m.scrobbled = true;
            }
        }
    }

    /// Sets the sleep timer: minutes from now, the end of this song or of this album, or off.
    pub fn set_sleep(&mut self, s: Option<Sleep>) {
        self.sleep = s;
        self.sync_preload();
    }

    /// Minutes left on a timed sleep timer.
    pub fn sleep_left(&self) -> Option<Duration> {
        match self.sleep {
            Some(Sleep::At(at)) => Some(at.saturating_duration_since(Instant::now())),
            _ => None,
        }
    }

    /// Puts songs at the front of Up Next, in order.
    pub fn play_next(&mut self, ids: &[TrackId]) {
        for (i, &id) in ids.iter().enumerate() {
            self.up_next.insert(i, id);
        }
        self.up_next_changed();
    }

    pub fn add_up_next(&mut self, ids: &[TrackId]) {
        self.up_next.extend_from_slice(ids);
        self.up_next_changed();
    }

    pub fn up_next_changed(&mut self) {
        if self.view == View::UpNext {
            self.refresh();
        }
        self.sync_preload();
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
        self.preload = None;
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
        self.sync_preload();
    }

    /// A song played to its end with nothing queued after it: count it, then move on, unless
    /// the sleep timer stops here.
    pub fn ended(&mut self, token: u64) {
        let Some(n) = self.now.filter(|n| n.token == token) else { return };
        self.finish(n);
        if let Some(m) = &mut self.now {
            m.counted = true;
            m.scrobbled = true;
        }
        let stop = match self.sleep {
            Some(Sleep::EndOfSong) => true,
            Some(Sleep::EndOfAlbum) => {
                let next = if n.party { self.lib.party.upcoming.first().map(|e| e.track) } else { self.up_next.first().copied().or_else(|| self.queue.peek(self.settings.repeat)) };
                !self.same_album(n.track, next)
            }
            _ => false,
        };
        if stop {
            self.sleep = None;
            // Ready to carry on from the next song, paused.
            match self.step(n, true) {
                Some(id) => self.load(id, n.party, false, 0.0),
                None => self.stop(),
            }
            return;
        }
        self.next(true);
    }

    /// The audio thread moved on from song `from` to the preloaded `to` by itself (gapless, or
    /// fading across): count the one that ended and catch the lists up.
    pub fn advanced(&mut self, from: u64, to: u64) {
        let Some(n) = self.now.filter(|n| n.token == from) else { return };
        self.finish(n);
        let expected = self.preload.filter(|&(t, _)| t == to).map(|(_, id)| id);
        self.preload = None;
        match self.step(n, true) {
            Some(id) if Some(id) == expected => {
                let duration = self.lib.get(id).map_or(0.0, |t| t.duration);
                self.now = Some(Now::new(id, to, true, n.party, duration));
                self.pos = 0.0;
                self.request_art();
                self.publish_time();
                self.song_started();
                self.sync_preload();
            }
            // The lists changed after the song went to the audio thread: play the right one.
            Some(id) => self.load(id, n.party, true, 0.0),
            None => self.stop(),
        }
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
