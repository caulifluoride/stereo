//! Stereo: a music player in the spirit of iTunes 4 to 7, built with Rune. A library of
//! songs read from your music folders (and watched for new ones), a column browser by genre,
//! artist and album, playlists and smart playlists, star ratings and play counts, gapless
//! playback, crossfades, volume levelling and an equaliser, media keys, scrobbling, and Party
//! Shuffle: a list that keeps itself topped up with songs picked at random as each one
//! finishes.
//!
//! `cargo run` opens the window (and `cargo run -- ~/Music` adds a folder). `cargo run --
//! --snap out.png` renders it headless, with a sample library, unless `STEREO_LIBRARY` names
//! a library file to open.

#![deny(unsafe_code)]
#![cfg_attr(windows, windows_subsystem = "windows")]

mod art;
mod audio;
#[cfg(windows)]
#[allow(unsafe_code)]
mod console;
mod control;
mod desktop;
mod dialogs;
mod dsp;
mod icons;
mod info;
mod keys;
mod library;
mod lists;
mod lcd;
mod m3u;
mod menus;
mod mpris;
mod net;
mod party;
mod partyview;
mod prefs;
mod queue;
mod rows;
mod sample;
mod scrobble;
mod settings;
mod sidebar;
mod smart;
mod smartedit;
mod store;
mod watch;
mod widgets;

use std::cell::Cell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use rand::SeedableRng;
use rand::rngs::StdRng;
use rune::{App, AppSpec, Cx, El, Live, LiveText, TextEdit, Tx, Waker, col, oklch, row};
use rune_kit::{Kit, TableState};

use audio::{Engine, Event};
use control::{Now, Sleep};
use library::{Found, Library, TrackId};
use party::Place;
use queue::Queue;
use rows::{Browser, View};
use settings::{Launch, Settings};
use store::Saver;

/// What a background job has to report, through [`State::jobs`].
pub enum Job {
    /// Which songs' files are gone, from a check of every file.
    Missing(HashSet<TrackId>),
    /// Tags were written: the songs whose files couldn't be, and why.
    TagsWritten(Vec<String>),
    /// A Last.fm sign-in: the session key, or why not.
    LastFm(Result<String, String>),
    ScrobbleFailed(String),
}

pub struct State {
    pub lib: Library,
    pub view: View,
    /// The track list's rows, in order, worked out by [`State::refresh`].
    pub rows: Vec<TrackId>,
    pub party_rows: Vec<(party::Entry, Place)>,
    pub table: TableState,
    pub table_party: TableState,
    pub table_genre: TableState,
    pub table_artist: TableState,
    pub table_album: TableState,
    pub browser: Browser,
    pub search: TextEdit,
    pub settings: Settings,
    pub now: Option<Now>,
    pub queue: Queue,
    /// Seconds into the song playing, as last read from the player.
    pub pos: f32,
    /// A drag on the scrubber: where it would seek to.
    pub scrubbing: Option<f32>,
    pub engine: Engine,
    pub token: u64,
    pub failures: u32,
    pub rng: StdRng,
    pub time_in: LiveText,
    pub time_left: LiveText,
    pub progress: Live<[f32; 4]>,
    /// The scrubber's width as last laid out, for placing its fill between frames.
    pub scrub_w: Cell<f32>,
    pub art: art::Art,
    pub menu: Option<menus::Menu>,
    pub dialog: Option<dialogs::Dialog>,
    pub renaming: Option<u64>,
    pub rename: TextEdit,
    /// The source section headers folded closed.
    pub closed: HashSet<&'static str>,
    pub notice: Option<String>,
    pub scan: Option<Receiver<Found>>,
    pub scanned: usize,
    pub waker: Option<Waker>,
    /// Songs to play before the rest of the list, outside Party Shuffle.
    pub up_next: Vec<TrackId>,
    /// The song the audio thread was told comes next, and its load token.
    pub preload: Option<(u64, TrackId)>,
    pub sleep: Option<Sleep>,
    /// Songs whose files are gone.
    pub missing: HashSet<TrackId>,
    /// A check for missing files is under way.
    pub checking: bool,
    /// Files being read in to join a playlist once they're in the library.
    pub pending_adds: Vec<(u64, PathBuf)>,
    /// Folders and files waiting for the scan running now to finish.
    pub scan_next: Vec<PathBuf>,
    /// Changes on disk the folder watcher saw, held until things go quiet.
    pub watched: (Vec<PathBuf>, Vec<PathBuf>, Option<Instant>),
    pub watch: Option<watch::Watch>,
    pub mpris: Option<mpris::Mpris>,
    pub scrobbler: scrobble::Scrobbler,
    pub jobs: Sender<Job>,
    jobs_rx: Receiver<Job>,
    /// The settings window's text fields.
    pub prefs: prefs::Fields,
    /// Get Info, while open.
    pub info: Option<info::Draft>,
    /// The smart playlist editor, while open.
    pub smart_edit: Option<smartedit::Draft>,
    /// A field for edits that arrive after the form they were for has closed.
    pub scratch: TextEdit,
    /// The last time the poll counted plays and checked the sleep timer.
    ticked: Option<Instant>,
    saver: Option<Saver>,
    dirty: Option<Instant>,
    /// Whether the window is ours to play sound in (not a headless run).
    live: bool,
}

impl State {
    /// The library changed: save it a moment from now.
    pub fn changed(&mut self) {
        self.dirty.get_or_insert_with(Instant::now);
    }

    pub fn settings_changed(&self) {
        if self.live {
            self.settings.save();
        }
    }

    /// Works out the rows again after the library, source, search, browser or sort changed.
    pub fn refresh(&mut self) {
        let sort = self.table.sort().map(|(c, d)| (c.to_string(), d));
        let browse = self.view == View::Music && self.settings.browser;
        let extra = rows::Extra { sizes: self.settings.smart, up_next: &self.up_next, missing: Some(&self.missing) };
        self.rows = rows::compute(&self.lib, self.view, &self.search.text, sort.as_ref().map(|(c, d)| (c.as_str(), *d)), &mut self.browser, browse, &extra);
        // The browser lists show their choice, All when a choice fell away.
        let pick = |c: &Option<String>| c.as_ref().map_or_else(|| "all".to_string(), |v| format!("v{v}"));
        for (table, choice) in [(&mut self.table_genre, &self.browser.genre), (&mut self.table_artist, &self.browser.artist), (&mut self.table_album, &self.browser.album)] {
            let id = pick(choice);
            if table.cursor() != Some(id.as_str()) {
                table.select(id);
            }
        }
        self.refresh_party();
    }

    pub fn refresh_party(&mut self) {
        self.party_rows = self.lib.party.rows();
    }

    pub fn select_view(&mut self, v: View) {
        if self.view == v {
            return;
        }
        self.view = v;
        self.table.clear_sort();
        self.table.clear_selection();
        self.renaming = None;
        self.menu = None;
        if v == View::Party && self.lib.party.upcoming.len() < self.lib.party.upcoming_len {
            self.party_fill();
        }
        self.refresh();
        self.request_art();
    }

    /// The tracks selected in the list showing, in list order.
    pub fn selected_tracks(&self) -> Vec<TrackId> {
        if self.view == View::Party {
            let sel: HashSet<&str> = self.table_party.selected().collect();
            return self.party_rows.iter().filter(|(e, _)| sel.contains(e.serial.to_string().as_str())).map(|(e, _)| e.track).collect();
        }
        self.rows.iter().copied().filter(|id| self.table.is_selected(&id.to_string())).collect()
    }

    /// The song whose artwork shows: the one selected, else the one playing.
    pub fn art_track(&self) -> Option<TrackId> {
        let selected = if self.view == View::Party {
            self.table_party.cursor().and_then(|c| c.parse().ok()).and_then(|s: u64| self.party_rows.iter().find(|(e, _)| e.serial == s)).map(|(e, _)| e.track)
        } else {
            self.table.cursor().and_then(|c| c.parse().ok()).filter(|id| self.rows.contains(id))
        };
        selected.or(self.now.map(|n| n.track))
    }

    pub fn request_art(&mut self) {
        let t = self.art_track().and_then(|id| self.lib.get(id));
        self.art.show(t, self.settings.fetch_art);
    }

    /// Puts the time and the scrubber's fill on screen without rebuilding the view.
    pub fn publish_time(&self) {
        let (at, len) = match self.now {
            Some(n) => (self.scrubbing.unwrap_or(self.pos), n.duration),
            None => (0.0, 0.0),
        };
        self.time_in.set(lcd::clock(at));
        self.time_left.set(format!("-{}", lcd::clock((len - at).max(0.0))));
        let w = self.scrub_w.get();
        let frac = if len > 0.0 { (at / len).clamp(0.0, 1.0) } else { 0.0 };
        self.progress.set([0.0, 0.0, (w * frac).round(), lcd::SCRUB_H]);
    }

    /// Adds folders or files to the library, scanning them on a worker. A scan already
    /// running finishes first; these follow it.
    pub fn import(&mut self, paths: Vec<PathBuf>) {
        let Some(wake) = self.waker.clone() else { return };
        let mut new_folder = false;
        for p in &paths {
            if p.is_dir() && !self.lib.folders.iter().any(|f| p.starts_with(f)) {
                self.lib.remember_folder(p);
                new_folder = true;
            }
        }
        if new_folder {
            self.restart_watch();
        }
        if self.scan.is_some() {
            self.scan_next.extend(paths);
            return;
        }
        let known = self.lib.tracks.iter().map(|t| t.path.clone()).collect();
        self.scanned = 0;
        self.scan = Some(library::scan(paths, known, wake));
        self.changed();
    }

    /// Reads `files` in and adds them to playlist `id` once they're in the library; those it
    /// has already join now.
    pub fn import_into(&mut self, id: u64, files: Vec<PathBuf>) {
        let known: Vec<TrackId> = files.iter().filter_map(|f| self.lib.by_path(f)).collect();
        menus::add_to_playlist(self, id, &known);
        let new: Vec<PathBuf> = files.into_iter().filter(|f| !self.lib.contains_path(f)).collect();
        if new.is_empty() {
            return;
        }
        self.pending_adds.extend(new.iter().map(|p| (id, p.clone())));
        self.import(new);
    }

    /// Watches the library's folders, or stops, as the setting and the folders say.
    pub fn restart_watch(&mut self) {
        self.watch = None;
        if !self.live || !self.settings.watch || self.lib.folders.is_empty() {
            return;
        }
        if let Some(w) = self.waker.clone() {
            self.watch = watch::Watch::start(&self.lib.folders, w);
        }
    }

    /// Checks every song's file on a worker; [`Job::Missing`] answers.
    pub fn find_missing(&mut self) {
        let (Some(wake), false) = (self.waker.clone(), self.checking) else { return };
        self.checking = true;
        let files: Vec<(TrackId, PathBuf)> = self.lib.tracks.iter().map(|t| (t.id, t.path.clone())).collect();
        let jobs = self.jobs.clone();
        let _ = rune::task::spawn("stereo-missing", move || {
            let gone = files.into_iter().filter(|(_, p)| !p.exists()).map(|(id, _)| id).collect();
            let _ = jobs.send(Job::Missing(gone));
            wake();
        });
    }

    /// Takes in what the folder watcher saw, once nothing has changed for a moment.
    fn take_watched(&mut self, changes: watch::Changes) {
        if !changes.is_empty() {
            self.watched.0.extend(changes.added);
            self.watched.1.extend(changes.removed);
            self.watched.2 = Some(Instant::now());
        }
    }

    fn settle_watched(&mut self) -> bool {
        let Some(at) = self.watched.2 else { return false };
        if at.elapsed() < Duration::from_secs(2) {
            return false;
        }
        let added = std::mem::take(&mut self.watched.0);
        let removed = std::mem::take(&mut self.watched.1);
        self.watched.2 = None;
        let mut changed = false;
        // Songs whose files went away are missing until they come back.
        for p in &removed {
            for t in self.lib.tracks.iter().filter(|t| t.path.starts_with(p) && !t.path.exists()) {
                changed |= self.missing.insert(t.id);
            }
        }
        let back: Vec<TrackId> = self.missing.iter().copied().filter(|&id| self.lib.get(id).is_some_and(|t| t.path.exists())).collect();
        for id in back {
            changed |= self.missing.remove(&id);
        }
        let mut new: Vec<PathBuf> = added.into_iter().filter(|p| p.exists() && (p.is_dir() || library::is_audio(p)) && !self.lib.contains_path(p)).collect();
        new.sort();
        new.dedup();
        if !new.is_empty() {
            self.import(new);
        }
        if changed {
            self.refresh();
        }
        changed
    }

    /// What the desktop's media controls show.
    fn media_snapshot(&self) -> mpris::Snapshot {
        let t = self.now.and_then(|n| self.lib.get(n.track));
        let repeat = match self.settings.repeat {
            queue::Repeat::Off => "None",
            queue::Repeat::All => "Playlist",
            queue::Repeat::One => "Track",
        };
        let status = match self.now {
            Some(n) if n.playing => "Playing",
            Some(_) => "Paused",
            None => "Stopped",
        };
        let art = t.filter(|t| self.art.for_path() == Some(t.path.as_path())).and(self.art.file.as_ref()).map(|p| desktop_uri(p));
        mpris::Snapshot {
            status,
            track: t.map_or(0, |t| t.id),
            title: t.map(|t| t.title.clone()).unwrap_or_default(),
            artist: t.map(|t| t.artist.clone()).unwrap_or_default(),
            album: t.map(|t| t.album.clone()).unwrap_or_default(),
            album_artist: t.map(|t| t.album_artist.clone()).unwrap_or_default(),
            track_no: t.and_then(|t| t.track_no),
            length: t.map_or(0.0, |t| t.duration),
            art,
            volume: self.settings.volume,
            shuffle: self.settings.shuffle,
            loop_status: repeat,
            can_next: self.now.is_some(),
            can_prev: self.now.is_some(),
        }
    }

    fn media_command(&mut self, c: mpris::MediaCmd) {
        use mpris::MediaCmd as M;
        match c {
            M::PlayPause => self.play_pause(),
            M::Play if !self.now.is_some_and(|n| n.playing) => self.play_pause(),
            M::Pause if self.now.is_some_and(|n| n.playing) => self.play_pause(),
            M::Play | M::Pause => {}
            M::Stop => self.stop(),
            M::Next => self.next(false),
            M::Prev => self.prev(),
            M::Seek(d) => self.seek(self.pos + d),
            M::SetPosition(at) => self.seek(at),
            M::Volume(v) => self.set_volume(v),
            M::Shuffle(on) => {
                if on != self.settings.shuffle {
                    self.toggle_shuffle();
                }
            }
            M::Loop(l) => {
                self.settings.repeat = match l {
                    "Track" => queue::Repeat::One,
                    "Playlist" => queue::Repeat::All,
                    _ => queue::Repeat::Off,
                };
                self.settings_changed();
                self.sync_preload();
            }
        }
        if matches!(c, M::Seek(_) | M::SetPosition(_))
            && let Some(m) = &self.mpris
        {
            m.seeked(self.pos);
        }
    }

    fn job(&mut self, j: Job) {
        match j {
            Job::Missing(gone) => {
                self.checking = false;
                self.missing = gone;
                if self.missing.is_empty() {
                    self.notice = Some("No missing files: every song's file is where it was.".into());
                    if self.view == View::Missing {
                        self.select_view(View::Music);
                    }
                } else {
                    self.select_view(View::Missing);
                }
                self.refresh();
            }
            Job::TagsWritten(failed) => {
                if let Some(first) = failed.first() {
                    let more = if failed.len() > 1 { format!(" (and {} more)", failed.len() - 1) } else { String::new() };
                    self.notice = Some(format!("Couldn't save tags: {first}{more}"));
                }
                self.art.forget();
                self.request_art();
            }
            Job::LastFm(answer) => match answer {
                Ok(session) => {
                    self.settings.lastfm.session = Some(session);
                    self.settings.lastfm.user = self.prefs.lastfm_user.text.trim().to_string();
                    self.prefs.lastfm_password = TextEdit::new("");
                    self.prefs.status = Some("Signed in to Last.fm.".into());
                    self.settings_changed();
                }
                Err(e) => self.prefs.status = Some(format!("Couldn't sign in to Last.fm: {e}")),
            },
            Job::ScrobbleFailed(e) => self.prefs.status = Some(format!("Scrobbles are waiting to be sent ({e}).")),
        }
    }

    pub fn remove_tracks(&mut self, ids: &HashSet<TrackId>) {
        if self.now.is_some_and(|n| ids.contains(&n.track)) {
            self.stop();
        }
        self.lib.remove(ids);
        self.queue.retain(|t| !ids.contains(&t));
        self.table.clear_selection();
        self.changed();
        self.refresh();
    }

    fn save_now(&mut self) {
        self.settings.resume = self.now.map(|n| settings::Resume { track: n.track, at: self.pos, party: n.party });
        if let Some(s) = &self.saver {
            s.save_now(&self.lib);
        }
        if self.live {
            self.settings.save_now();
        }
        self.dirty = None;
    }
}

/// A file's `file://` link for the media controls.
fn desktop_uri(p: &std::path::Path) -> String {
    #[cfg(target_os = "linux")]
    return desktop::file_uri(p);
    #[cfg(not(target_os = "linux"))]
    format!("file://{}", p.display())
}

fn init() -> State {
    let headless = rune::tooling::headless_run().is_some();
    let file = library::path().filter(|_| !headless || std::env::var_os("STEREO_LIBRARY").is_some());
    let mut lib = file.as_deref().map(library::load).unwrap_or_default();
    let sample = headless && file.is_none();
    if sample {
        lib = sample::library();
    }
    let settings = if headless { Settings::default() } else { Settings::load() };
    let (jobs, jobs_rx) = channel();
    let mut s = State {
        lib,
        view: View::Music,
        rows: Vec::new(),
        party_rows: Vec::new(),
        table: TableState::new().key("tracks").multi_select(true),
        table_party: TableState::new().key("party").multi_select(true),
        table_genre: TableState::new().key("genres"),
        table_artist: TableState::new().key("artists"),
        table_album: TableState::new().key("albums"),
        browser: Browser::default(),
        search: TextEdit::new(""),
        settings,
        now: None,
        queue: Queue::default(),
        pos: 0.0,
        scrubbing: None,
        engine: Engine::silent(),
        token: 0,
        failures: 0,
        rng: if headless { StdRng::seed_from_u64(7) } else { rand::make_rng() },
        time_in: LiveText::new("0:00"),
        time_left: LiveText::new("-0:00"),
        progress: Live::new([0.0, 0.0, 0.0, lcd::SCRUB_H]),
        scrub_w: Cell::new(0.0),
        art: art::Art::default(),
        menu: None,
        dialog: None,
        renaming: None,
        rename: TextEdit::new(""),
        closed: HashSet::new(),
        notice: None,
        scan: None,
        scanned: 0,
        waker: None,
        up_next: Vec::new(),
        preload: None,
        sleep: None,
        missing: HashSet::new(),
        checking: false,
        pending_adds: Vec::new(),
        scan_next: Vec::new(),
        watched: (Vec::new(), Vec::new(), None),
        watch: None,
        mpris: None,
        scrobbler: scrobble::Scrobbler::none(),
        jobs,
        jobs_rx,
        prefs: prefs::Fields::default(),
        info: None,
        smart_edit: None,
        scratch: TextEdit::new(""),
        ticked: None,
        saver: file.map(Saver::new),
        dirty: None,
        live: !headless,
    };
    s.refresh();
    if sample {
        sample::play(&mut s);
    } else if let Some(r) = s.settings.resume
        && s.settings.on_launch != Launch::Nothing
        && s.lib.get(r.track).is_some()
    {
        // Back where it was, paused (and set playing in `setup`, if asked).
        s.queue = Queue::start(s.rows.clone(), s.rows.iter().position(|&t| t == r.track).unwrap_or(0), s.settings.shuffle, &mut s.rng);
        s.now = Some(Now::new(r.track, 0, false, r.party, s.lib.get(r.track).map_or(0.0, |t| t.duration)));
        s.pos = r.at;
    }
    s
}

fn setup(s: &mut State, waker: Waker) {
    s.waker = Some(waker.clone());
    s.art.start(waker.clone(), s.live);
    s.request_art();
    if !s.live {
        return;
    }
    s.engine = Engine::start(waker.clone());
    s.engine.volume(s.settings.volume);
    s.apply_playback();
    s.scrobbler = scrobble::Scrobbler::start(s.jobs.clone(), waker.clone());
    s.mpris = mpris::Mpris::start(waker.clone());
    s.restart_watch();
    if let Some(n) = s.now {
        // The resumed song, loaded where it left off.
        s.load(n.track, n.party, s.settings.on_launch == Launch::Playing, s.pos);
    }
    // Folders named on the command line, else new files in the folders scanned before, else
    // the music folder on a first run.
    let args: Vec<PathBuf> = rune::extra_args().iter().map(PathBuf::from).filter(|p| p.exists()).collect();
    if !args.is_empty() {
        s.import(args);
    } else if !s.lib.folders.is_empty() {
        let folders = s.lib.folders.clone();
        s.import(folders);
    } else if s.lib.tracks.is_empty()
        && let Some(m) = library::music_dir()
    {
        s.import(vec![m]);
    }
}

fn poll(s: &mut State, _: f64, tx: &mut Tx) -> bool {
    let mut changed = false;
    for e in s.engine.take_news() {
        changed = true;
        match e {
            Event::NoDevice(why) => s.notice = Some(format!("No sound device: {why}")),
            Event::Ended(t) => {
                s.failures = 0;
                s.ended(t);
            }
            Event::Advanced { from, to } => {
                s.failures = 0;
                s.advanced(from, to);
            }
            Event::Failed(t, why) => s.failed(t, why),
        }
    }
    let cmds = s.mpris.as_ref().map(|m| m.take()).unwrap_or_default();
    for c in cmds {
        s.media_command(c);
        changed = true;
    }
    while let Ok(j) = s.jobs_rx.try_recv() {
        s.job(j);
        changed = true;
    }
    if let Some(rx) = &s.scan {
        let mut added = Vec::new();
        let mut done = false;
        while let Ok(found) = rx.try_recv() {
            match found {
                Found::Tracks(ts) => {
                    s.scanned += ts.len();
                    for t in ts {
                        let path = t.path.clone();
                        if let Some(id) = s.lib.add(t) {
                            added.push((path, id));
                        }
                    }
                }
                Found::Done(_) => done = true,
            }
        }
        // Files read in for a playlist join it.
        if !s.pending_adds.is_empty() {
            for (path, id) in &added {
                let lists: Vec<u64> = s.pending_adds.iter().filter(|(_, p)| p == path).map(|(l, _)| *l).collect();
                for l in lists {
                    if let Some(p) = s.lib.playlist_mut(l) {
                        p.tracks.push(*id);
                    }
                }
            }
        }
        if done {
            s.scan = None;
            s.pending_adds.retain(|(_, p)| !p.exists() || !s.lib.contains_path(p));
            if !s.scan_next.is_empty() {
                let next = std::mem::take(&mut s.scan_next);
                s.import(next);
            }
        }
        if !added.is_empty() {
            s.changed();
            s.refresh();
        }
        changed |= !added.is_empty() || done;
    }
    if let Some(w) = &s.watch {
        let c = w.take();
        s.take_watched(c);
    }
    if s.watched.2.is_some() {
        changed |= s.settle_watched();
        tx.wake_after(Duration::from_millis(500));
    }
    if s.live && s.now.is_some_and(|n| n.playing) {
        s.pos = s.engine.position();
        s.publish_time();
        tx.wake_after(Duration::from_millis(250));
    }
    if s.ticked.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
        s.ticked = Some(Instant::now());
        let before = s.now;
        s.tick();
        changed |= s.now != before;
    }
    if let Some(left) = s.sleep_left() {
        tx.wake_after(left.min(Duration::from_secs(30)));
    }
    if s.mpris.is_some() {
        let snap = s.media_snapshot();
        let pos = s.pos;
        if let Some(m) = &mut s.mpris {
            m.update(snap, pos);
        }
    }
    if changed {
        s.request_art();
    }
    if let Some(at) = s.dirty {
        let wait = Duration::from_secs(1).saturating_sub(at.elapsed());
        if wait.is_zero() {
            s.dirty = None;
            if let Some(saver) = &s.saver {
                saver.save(&s.lib);
            }
        } else {
            tx.wake_after(wait);
        }
    }
    changed | s.art.poll()
}

fn on_close(s: &mut State, _: &mut Tx) {
    s.save_now();
}

/// The kit, in the theme and accent colour chosen.
pub fn kit(s: &Settings, dark: bool) -> Kit {
    let mut k = Kit::neutral(dark);
    if let Some(h) = s.accent.hue() {
        let chroma = if s.accent == settings::Accent::Graphite { 0.02 } else { 0.16 };
        let c = oklch(if dark { 0.70 } else { 0.56 }, chroma, h);
        k.c.bg_accent = c;
        k.c.text_accent = c;
        k.c.icon_accent = c;
        k.c.border_focus = c;
        k.c.border_active = c;
    }
    k
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    let dark = s.settings.dark(cx.system_dark());
    cx.zoom(s.settings.text_size);
    let k = &kit(&s.settings, dark);
    let mut layer = Vec::new();
    let top = lcd::bar(s, k, cx, &mut layer);
    let main = match s.view {
        View::Party => partyview::view(s, k, &mut layer),
        _ => lists::view(s, k),
    };
    let main = menus::wrap(s, k, main, &mut layer);
    let body = row().key("body").w_full().grow(1.0).min_h(0.0)
        .child(sidebar::view(s, k, &mut layer))
        .child(col().key("main").grow(1.0).min_w(0.0).h_full().fill(k.c.bg_base).child(main));
    dialogs::layer(s, k, &mut layer);
    let base = col().key("root").w_full().h_full().fill(k.c.page)
        .child(top)
        .child(body)
        .child(lists::status_bar(s, k))
        .on_drop_files(|s: &mut State, files, _| s.import(files.to_vec()));
    rune_kit::root(base, layer)
}

fn main() {
    #[cfg(windows)]
    console::attach();
    let spec = AppSpec {
        scenes: &[
            ("party", |s: &mut State| s.select_view(View::Party)),
            ("playlist", |s: &mut State| {
                let id = s.lib.playlists[0].id;
                s.select_view(View::Playlist(id));
            }),
            ("menu", |s: &mut State| s.menu = Some(menus::Menu::sample(s))),
            ("shortcuts", |s: &mut State| s.dialog = Some(dialogs::Dialog::Shortcuts)),
            ("settings", |s: &mut State| prefs::open(s, prefs::Tab::Playback)),
            ("settings-library", |s: &mut State| prefs::open(s, prefs::Tab::Library)),
            ("settings-online", |s: &mut State| prefs::open(s, prefs::Tab::Online)),
            ("smart-editor", |s: &mut State| smartedit::open_new(s)),
            ("info", |s: &mut State| {
                if let Some(&id) = s.rows.get(3) {
                    s.table.select(id.to_string());
                }
                info::open(s);
            }),
            ("columns", |s: &mut State| {
                s.settings.hidden_columns.clear();
                s.settings.density = settings::Density::Compact;
                s.settings.accent = settings::Accent::Green;
            }),
            ("dark", |s: &mut State| s.settings.theme = settings::Theme::Dark),
            ("light", |s: &mut State| s.settings.theme = settings::Theme::Light),
            ("no-browser", |s: &mut State| {
                s.settings.browser = false;
                s.refresh();
            }),
        ],
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(keys::window_key),
        min_size: Some((760.0, 460.0)),
        ..AppSpec::new("Stereo", (1180.0, 760.0), init, view)
    };
    App::new(spec).on_close(on_close).on_quit(on_close).run();
}
