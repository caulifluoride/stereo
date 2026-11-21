//! Media keys and the desktop's media controls on Linux: Stereo serves MPRIS
//! (`org.mpris.MediaPlayer2.stereo` on the session bus), so play/pause/next on the keyboard,
//! headphones, the panel's media widget and `playerctl` reach it. Requests arrive on zbus's
//! thread and wait in a channel for the UI's poll; the UI publishes what's playing back with
//! [`Mpris::update`], which signals only what changed.
//!
//! Elsewhere this is a stand-in that does nothing.

/// A request from the desktop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaCmd {
    PlayPause,
    Play,
    Pause,
    Stop,
    Next,
    Prev,
    /// Seconds forward (negative: back).
    Seek(f32),
    /// To this many seconds in.
    SetPosition(f32),
    Volume(f32),
    Shuffle(bool),
    /// "None", "Track" or "Playlist".
    Loop(&'static str),
}

/// What the desktop is shown.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// "Playing", "Paused" or "Stopped".
    pub status: &'static str,
    pub track: u64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track_no: Option<u32>,
    /// Seconds.
    pub length: f32,
    /// A `file://` link to the cover, when there's one on disk.
    pub art: Option<String>,
    pub volume: f32,
    pub shuffle: bool,
    pub loop_status: &'static str,
    pub can_next: bool,
    pub can_prev: bool,
}

#[cfg(target_os = "linux")]
pub use linux::Mpris;

#[cfg(not(target_os = "linux"))]
pub struct Mpris;

#[cfg(not(target_os = "linux"))]
impl Mpris {
    pub fn start(_: rune::Waker) -> Option<Mpris> {
        None
    }
    pub fn update(&mut self, _: Snapshot, _: f32) {}
    pub fn seeked(&self, _: f32) {}
    pub fn take(&self) -> Vec<MediaCmd> {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::sync::{Arc, Mutex};

    use zbus::blocking::Connection;
    use zbus::interface;
    use zbus::zvariant::{ObjectPath, OwnedValue, Value};

    use super::{MediaCmd, Snapshot};

    const PATH: &str = "/org/mpris/MediaPlayer2";

    struct Shared {
        snap: Mutex<Snapshot>,
        /// Microseconds into the song, as MPRIS counts.
        pos: AtomicI64,
        tx: Mutex<Sender<MediaCmd>>,
        wake: rune::Waker,
    }

    impl Shared {
        fn send(&self, c: MediaCmd) {
            let _ = self.tx.lock().unwrap_or_else(|e| e.into_inner()).send(c);
            (self.wake)();
        }
        fn snap(&self) -> Snapshot {
            self.snap.lock().unwrap_or_else(|e| e.into_inner()).clone()
        }
    }

    struct Root;

    #[interface(name = "org.mpris.MediaPlayer2")]
    impl Root {
        fn raise(&self) {}
        fn quit(&self) {}
        #[zbus(property)]
        fn can_quit(&self) -> bool {
            false
        }
        #[zbus(property)]
        fn can_raise(&self) -> bool {
            false
        }
        #[zbus(property)]
        fn has_track_list(&self) -> bool {
            false
        }
        #[zbus(property)]
        fn identity(&self) -> String {
            "Stereo".into()
        }
        #[zbus(property)]
        fn desktop_entry(&self) -> String {
            "stereo".into()
        }
        #[zbus(property)]
        fn supported_uri_schemes(&self) -> Vec<String> {
            Vec::new()
        }
        #[zbus(property)]
        fn supported_mime_types(&self) -> Vec<String> {
            Vec::new()
        }
    }

    struct Player(Arc<Shared>);

    fn track_path(id: u64) -> String {
        format!("/io/github/caulifluoride/Stereo/track/{id}")
    }

    fn value<'a>(v: impl Into<Value<'a>>) -> Option<OwnedValue> {
        OwnedValue::try_from(v.into()).ok()
    }

    #[interface(name = "org.mpris.MediaPlayer2.Player")]
    impl Player {
        fn next(&self) {
            self.0.send(MediaCmd::Next);
        }
        fn previous(&self) {
            self.0.send(MediaCmd::Prev);
        }
        fn pause(&self) {
            self.0.send(MediaCmd::Pause);
        }
        fn play_pause(&self) {
            self.0.send(MediaCmd::PlayPause);
        }
        fn stop(&self) {
            self.0.send(MediaCmd::Stop);
        }
        fn play(&self) {
            self.0.send(MediaCmd::Play);
        }
        fn seek(&self, offset: i64) {
            self.0.send(MediaCmd::Seek(offset as f32 / 1e6));
        }
        fn set_position(&self, track: ObjectPath<'_>, position: i64) {
            // Only for the song still playing, as the spec asks.
            if track.as_str() == track_path(self.0.snap().track) {
                self.0.send(MediaCmd::SetPosition(position as f32 / 1e6));
            }
        }
        fn open_uri(&self, _uri: String) {}

        #[zbus(property)]
        fn playback_status(&self) -> String {
            let s = self.0.snap().status;
            if s.is_empty() { "Stopped".into() } else { s.into() }
        }
        #[zbus(property)]
        fn loop_status(&self) -> String {
            let s = self.0.snap().loop_status;
            if s.is_empty() { "None".into() } else { s.into() }
        }
        #[zbus(property)]
        fn set_loop_status(&mut self, v: String) {
            let l = match v.as_str() {
                "Track" => "Track",
                "Playlist" => "Playlist",
                _ => "None",
            };
            self.0.send(MediaCmd::Loop(l));
        }
        #[zbus(property)]
        fn rate(&self) -> f64 {
            1.0
        }
        #[zbus(property)]
        fn set_rate(&mut self, rate: f64) {
            let _ = rate;
        }
        #[zbus(property)]
        fn minimum_rate(&self) -> f64 {
            1.0
        }
        #[zbus(property)]
        fn maximum_rate(&self) -> f64 {
            1.0
        }
        #[zbus(property)]
        fn shuffle(&self) -> bool {
            self.0.snap().shuffle
        }
        #[zbus(property)]
        fn set_shuffle(&mut self, on: bool) {
            self.0.send(MediaCmd::Shuffle(on));
        }
        #[zbus(property)]
        fn metadata(&self) -> HashMap<String, OwnedValue> {
            let s = self.0.snap();
            let mut m = HashMap::new();
            if s.status.is_empty() || s.status == "Stopped" {
                if let Some(v) = ObjectPath::try_from("/org/mpris/MediaPlayer2/TrackList/NoTrack").ok().and_then(value) {
                    m.insert("mpris:trackid".into(), v);
                }
                return m;
            }
            let mut put = |k: &str, v: Option<OwnedValue>| {
                if let Some(v) = v {
                    m.insert(k.to_string(), v);
                }
            };
            put("mpris:trackid", ObjectPath::try_from(track_path(s.track)).ok().and_then(value));
            put("mpris:length", value((s.length * 1e6) as i64));
            put("xesam:title", value(s.title.as_str()));
            put("xesam:artist", value(vec![s.artist.as_str()]));
            put("xesam:album", value(s.album.as_str()));
            if !s.album_artist.is_empty() {
                put("xesam:albumArtist", value(vec![s.album_artist.as_str()]));
            }
            if let Some(n) = s.track_no {
                put("xesam:trackNumber", value(n as i32));
            }
            if let Some(a) = &s.art {
                put("mpris:artUrl", value(a.as_str()));
            }
            m
        }
        #[zbus(property)]
        fn volume(&self) -> f64 {
            f64::from(self.0.snap().volume)
        }
        #[zbus(property)]
        fn set_volume(&mut self, v: f64) {
            self.0.send(MediaCmd::Volume(v.clamp(0.0, 1.0) as f32));
        }
        // Read when asked, never signalled, as the spec has it.
        #[zbus(property(emits_changed_signal = "false"))]
        fn position(&self) -> i64 {
            self.0.pos.load(Ordering::Relaxed)
        }
        #[zbus(property)]
        fn can_go_next(&self) -> bool {
            self.0.snap().can_next
        }
        #[zbus(property)]
        fn can_go_previous(&self) -> bool {
            self.0.snap().can_prev
        }
        #[zbus(property)]
        fn can_play(&self) -> bool {
            true
        }
        #[zbus(property)]
        fn can_pause(&self) -> bool {
            true
        }
        #[zbus(property)]
        fn can_seek(&self) -> bool {
            true
        }
        #[zbus(property)]
        fn can_control(&self) -> bool {
            true
        }

        #[zbus(signal)]
        async fn seeked(emitter: &zbus::object_server::SignalEmitter<'_>, position: i64) -> zbus::Result<()>;
    }

    pub struct Mpris {
        conn: Connection,
        shared: Arc<Shared>,
        rx: Receiver<MediaCmd>,
        last: Snapshot,
    }

    impl Mpris {
        /// Serves MPRIS on the session bus, or None without one.
        pub fn start(wake: rune::Waker) -> Option<Mpris> {
            let (tx, rx) = channel();
            let shared = Arc::new(Shared { snap: Mutex::new(Snapshot::default()), pos: AtomicI64::new(0), tx: Mutex::new(tx), wake });
            let conn = zbus::blocking::connection::Builder::session()
                .and_then(|b| b.name("org.mpris.MediaPlayer2.stereo"))
                .and_then(|b| b.serve_at(PATH, Root))
                .and_then(|b| b.serve_at(PATH, Player(shared.clone())))
                .and_then(|b| b.build());
            match conn {
                Ok(conn) => Some(Mpris { conn, shared, rx, last: Snapshot::default() }),
                Err(e) => {
                    eprintln!("stereo: no media keys: {e}");
                    None
                }
            }
        }

        /// Publishes what's playing; signals the properties that changed.
        pub fn update(&mut self, snap: Snapshot, pos: f32) {
            self.shared.pos.store((pos * 1e6) as i64, Ordering::Relaxed);
            if snap == self.last {
                return;
            }
            *self.shared.snap.lock().unwrap_or_else(|e| e.into_inner()) = snap.clone();
            let Ok(iface) = self.conn.object_server().interface::<_, Player>(PATH) else { return };
            let e = iface.signal_emitter();
            let p = iface.get();
            let (a, b) = (&self.last, &snap);
            zbus::block_on(async {
                if a.status != b.status {
                    let _ = p.playback_status_changed(e).await;
                }
                if (a.track, &a.title, &a.artist, &a.album, &a.art, a.length) != (b.track, &b.title, &b.artist, &b.album, &b.art, b.length) || a.status != b.status {
                    let _ = p.metadata_changed(e).await;
                }
                if a.volume != b.volume {
                    let _ = p.volume_changed(e).await;
                }
                if a.shuffle != b.shuffle {
                    let _ = p.shuffle_changed(e).await;
                }
                if a.loop_status != b.loop_status {
                    let _ = p.loop_status_changed(e).await;
                }
                if a.can_next != b.can_next {
                    let _ = p.can_go_next_changed(e).await;
                }
                if a.can_prev != b.can_prev {
                    let _ = p.can_go_previous_changed(e).await;
                }
            });
            drop(p);
            self.last = snap;
        }

        /// The song jumped to `pos` seconds (a seek, not playing on).
        pub fn seeked(&self, pos: f32) {
            self.shared.pos.store((pos * 1e6) as i64, Ordering::Relaxed);
            if let Ok(iface) = self.conn.object_server().interface::<_, Player>(PATH) {
                let _ = zbus::block_on(Player::seeked(iface.signal_emitter(), (pos * 1e6) as i64));
            }
        }

        pub fn take(&self) -> Vec<MediaCmd> {
            self.rx.try_iter().collect()
        }
    }
}
