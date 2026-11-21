//! Stereo: a music player in the spirit of iTunes 4 to 7, built with Rune. A library of
//! songs read from your music folders, a column browser by genre, artist and album,
//! playlists and smart playlists, star ratings and play counts, and Party Shuffle: a list
//! that keeps itself topped up with songs picked at random as each one finishes.
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
mod dialogs;
mod icons;
mod keys;
mod library;
mod lists;
mod lcd;
mod menus;
mod party;
mod partyview;
mod queue;
mod rows;
mod sample;
mod settings;
mod sidebar;
mod store;

use std::cell::Cell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use rand::SeedableRng;
use rand::rngs::StdRng;
use rune::{App, AppSpec, Cx, El, Live, LiveText, TextEdit, Tx, Waker, col, row};
use rune_kit::{Kit, TableState};

use audio::Engine;
use control::Now;
use library::{Found, Library, TrackId};
use party::Place;
use queue::Queue;
use rows::{Browser, View};
use settings::Settings;
use store::Saver;

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
        self.rows = rows::compute(&self.lib, self.view, &self.search.text, sort.as_ref().map(|(c, d)| (c.as_str(), *d)), &mut self.browser, browse);
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
        self.art.show(t);
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

    /// Adds folders or files to the library, scanning them on a worker.
    pub fn import(&mut self, paths: Vec<PathBuf>) {
        let Some(wake) = self.waker.clone() else { return };
        for p in &paths {
            if p.is_dir() {
                self.lib.remember_folder(p);
            }
        }
        let known = self.lib.tracks.iter().map(|t| t.path.clone()).collect();
        self.scanned = 0;
        self.scan = Some(library::scan(paths, known, wake));
        self.changed();
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

fn init() -> State {
    let headless = rune::tooling::headless_run().is_some();
    let file = library::path().filter(|_| !headless || std::env::var_os("STEREO_LIBRARY").is_some());
    let mut lib = file.as_deref().map(library::load).unwrap_or_default();
    let sample = headless && file.is_none();
    if sample {
        lib = sample::library();
    }
    let settings = if headless { Settings::default() } else { Settings::load() };
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
        saver: file.map(Saver::new),
        dirty: None,
        live: !headless,
    };
    s.refresh();
    if sample {
        sample::play(&mut s);
    } else if let Some(r) = s.settings.resume
        && s.lib.get(r.track).is_some()
    {
        // Back where it was, paused.
        s.queue = Queue::start(s.rows.clone(), s.rows.iter().position(|&t| t == r.track).unwrap_or(0), s.settings.shuffle, &mut s.rng);
        s.now = Some(Now { track: r.track, token: 0, playing: false, party: r.party, duration: s.lib.get(r.track).map_or(0.0, |t| t.duration) });
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
    s.engine = Engine::start(waker);
    s.engine.volume(s.settings.volume);
    if let Some(n) = s.now {
        // The resumed song, loaded paused where it left off.
        s.load(n.track, n.party, false, s.pos);
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
    let news = s.engine.take_news();
    if let Some(why) = news.no_device {
        s.notice = Some(format!("No sound device: {why}"));
        changed = true;
    }
    if let Some(t) = news.ended {
        s.failures = 0;
        s.ended(t);
        changed = true;
    }
    if let Some((t, why)) = news.failed {
        s.failed(t, why);
        changed = true;
    }
    if let Some(rx) = &s.scan {
        let mut added = false;
        let mut done = false;
        while let Ok(found) = rx.try_recv() {
            match found {
                Found::Tracks(ts) => {
                    s.scanned += ts.len();
                    for t in ts {
                        added |= s.lib.add(t).is_some();
                    }
                }
                Found::Done(_) => done = true,
            }
        }
        if done {
            s.scan = None;
        }
        if added {
            s.changed();
            s.refresh();
        }
        changed |= added || done;
    }
    if s.live && s.now.is_some_and(|n| n.playing) {
        s.pos = s.engine.position();
        s.publish_time();
        tx.wake_after(Duration::from_millis(250));
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

fn view(s: &State, cx: &mut Cx<State>) -> El {
    let dark = s.settings.dark(cx.system_dark());
    let k = &Kit::neutral(dark);
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
