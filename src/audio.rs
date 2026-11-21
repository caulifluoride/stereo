//! Playback, on a thread of its own that owns the sound device. The UI sends it commands and
//! reads back where the song is; it wakes the UI when a song ends, moves on to the next by
//! itself, or won't play.
//!
//! The UI tells it ahead of time which song comes next ([`Engine::preload`]). With gapless
//! playback that song is queued behind the one playing a moment before it ends, so it starts
//! on the next sample; with a crossfade it starts that many seconds early on a player of its
//! own, the two fading across. Either way the thread reports [`Event::Advanced`] and the UI
//! catches up without loading anything.
//!
//! Rune plays audio through AVFoundation and Media Foundation but has no Linux backend, so
//! Stereo decodes with symphonia and plays through cpal, by way of rodio, everywhere.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rodio::{Decoder, DeviceSinkBuilder, Player};

use crate::dsp::{Dsp, Tone, ToneSettings, read_levels};

/// A song to play: its file, the load it belongs to, and its length in seconds.
struct Song {
    path: PathBuf,
    token: u64,
    duration: f32,
}

enum Cmd {
    /// Starts a song `at` seconds in, paused or not.
    Load { song: Song, at: f32, paused: bool },
    /// The song after the one playing, or none.
    Preload(Option<Song>),
    /// How one song moves into the next: seconds of crossfade (0 for none), and gapless.
    Transition { crossfade: f32, gapless: bool },
    Play,
    Pause,
    Seek(f32),
    Volume(f32),
    Stop,
}

/// What the audio thread has to say, taken by the UI in `poll`, in order.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// Song `token` played to its end and nothing followed.
    Ended(u64),
    /// Song `from` ended (or began fading out) and the preloaded `to` is playing.
    Advanced { from: u64, to: u64 },
    /// Song `token` couldn't be played, and why.
    Failed(u64, String),
    /// The sound device couldn't be opened.
    NoDevice(String),
}

struct Shared {
    /// Milliseconds into the song playing.
    pos_ms: AtomicU64,
    news: Mutex<Vec<Event>>,
}

pub struct Engine {
    tx: Option<Sender<Cmd>>,
    shared: Arc<Shared>,
    tone: Arc<Tone>,
}

impl Engine {
    /// An engine that plays nothing, for headless runs and tests.
    pub fn silent() -> Engine {
        Engine { tx: None, shared: Arc::new(Shared { pos_ms: AtomicU64::new(0), news: Mutex::new(Vec::new()) }), tone: Arc::new(Tone::default()) }
    }

    pub fn start(wake: rune::Waker) -> Engine {
        let mut e = Engine::silent();
        let (tx, rx) = channel();
        let shared = e.shared.clone();
        let tone = e.tone.clone();
        if std::thread::Builder::new().name("stereo-audio".into()).spawn(move || run(rx, shared, tone, wake)).is_ok() {
            e.tx = Some(tx);
        }
        e
    }

    fn send(&self, c: Cmd) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(c);
        }
    }

    pub fn load(&self, path: PathBuf, token: u64, duration: f32, at: f32, paused: bool) {
        self.shared.pos_ms.store((at * 1000.0) as u64, Ordering::Relaxed);
        self.send(Cmd::Load { song: Song { path, token, duration }, at, paused });
    }
    /// The song to follow the one playing (`None`: nothing follows).
    pub fn preload(&self, next: Option<(PathBuf, u64, f32)>) {
        self.send(Cmd::Preload(next.map(|(path, token, duration)| Song { path, token, duration })));
    }
    pub fn transition(&self, crossfade: f32, gapless: bool) {
        self.send(Cmd::Transition { crossfade, gapless });
    }
    pub fn tone(&self, t: ToneSettings) {
        self.tone.set(t);
    }
    pub fn play(&self) {
        self.send(Cmd::Play);
    }
    pub fn pause(&self) {
        self.send(Cmd::Pause);
    }
    pub fn seek(&self, secs: f32) {
        self.shared.pos_ms.store((secs.max(0.0) * 1000.0) as u64, Ordering::Relaxed);
        self.send(Cmd::Seek(secs));
    }
    /// 0 to 1, on a perceptual curve.
    pub fn volume(&self, v: f32) {
        self.send(Cmd::Volume(v));
    }
    pub fn stop(&self) {
        self.shared.pos_ms.store(0, Ordering::Relaxed);
        self.send(Cmd::Stop);
    }

    /// Seconds into the song playing.
    pub fn position(&self) -> f32 {
        self.shared.pos_ms.load(Ordering::Relaxed) as f32 / 1000.0
    }

    pub fn take_news(&self) -> Vec<Event> {
        std::mem::take(&mut *self.shared.news.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// Volume as a gain: squared, so the slider's middle sounds like half as loud.
fn gain(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    v * v
}

/// How long before a song's end the next one is queued behind it for a gapless start: long
/// enough that the decoder has opened, short enough that a change of mind rarely comes after.
const QUEUE_AHEAD: f32 = 3.0;

/// A player and the song it's playing.
struct Slot {
    token: u64,
    player: Player,
    duration: f32,
}

/// The song fading out under a crossfade.
struct Fade {
    old: Slot,
    started: Instant,
    secs: f32,
}

fn open(song: &Song, tone: &Arc<Tone>) -> Result<Dsp<Decoder<std::io::BufReader<std::fs::File>>>, String> {
    let f = std::fs::File::open(&song.path).map_err(|e| e.to_string())?;
    let d = Decoder::try_from(f).map_err(|e| e.to_string())?;
    Ok(Dsp::new(d, tone.clone(), read_levels(&song.path)))
}

fn run(rx: Receiver<Cmd>, shared: Arc<Shared>, tone: Arc<Tone>, wake: rune::Waker) {
    let news = |e: Event| {
        shared.news.lock().unwrap_or_else(|e| e.into_inner()).push(e);
        wake();
    };
    let mut sink = match DeviceSinkBuilder::open_default_sink() {
        Ok(s) => s,
        Err(e) => {
            news(Event::NoDevice(e.to_string()));
            // Keep answering so loads report failure instead of hanging the queue.
            while let Ok(c) = rx.recv() {
                if let Cmd::Load { song, .. } = c {
                    news(Event::Failed(song.token, "no sound device".into()));
                }
            }
            return;
        }
    };
    sink.log_on_drop(false);
    let mut volume = 0.8;
    let (mut crossfade, mut gapless) = (0.0f32, true);
    // The song playing, and the player for it: a new one per load, so a change of song never
    // waits on the old one to drain.
    let mut current: Option<Slot> = None;
    let mut paused = true;
    // The song to follow, not yet started.
    let mut next: Option<Song> = None;
    // The song queued behind the one playing on its player, for a gapless start.
    let mut queued: Option<(u64, f32)> = None;
    let mut fade: Option<Fade> = None;
    let start = |song: &Song, at: f32, play: bool, volume: f32| -> Result<Slot, String> {
        let source = open(song, &tone)?;
        let p = Player::connect_new(sink.mixer());
        p.pause();
        p.set_volume(gain(volume));
        p.append(source);
        if at > 0.0 {
            let _ = p.try_seek(Duration::from_secs_f32(at));
        }
        if play {
            p.play();
        }
        Ok(Slot { token: song.token, player: p, duration: song.duration })
    };
    loop {
        let busy = fade.is_some() || current.as_ref().is_some_and(|c| c.duration - c.player.get_pos().as_secs_f32() < crossfade.max(QUEUE_AHEAD) + 1.0);
        let wait = Duration::from_millis(if busy { 15 } else { 50 });
        match rx.recv_timeout(wait) {
            Ok(Cmd::Load { song, at, paused: p }) => {
                current = None;
                fade = None;
                queued = None;
                // What follows is the UI's to say again for the new song.
                next = None;
                paused = p;
                match start(&song, at, !p, volume) {
                    Ok(slot) => current = Some(slot),
                    Err(e) => news(Event::Failed(song.token, e)),
                }
            }
            Ok(Cmd::Preload(song)) => {
                // A song already queued on the player can't be taken back off it; one that
                // replaces it plays after the UI hears it advanced to the queued one.
                if queued.is_none() {
                    next = song;
                }
            }
            Ok(Cmd::Transition { crossfade: c, gapless: g }) => {
                crossfade = c.max(0.0);
                gapless = g;
            }
            Ok(Cmd::Play) => {
                paused = false;
                if let Some(c) = &current {
                    c.player.play();
                }
            }
            Ok(Cmd::Pause) => {
                paused = true;
                // A pause mid-crossfade lands on the new song.
                fade = None;
                if let Some(c) = &current {
                    c.player.set_volume(gain(volume));
                    c.player.pause();
                }
            }
            Ok(Cmd::Seek(s)) => {
                fade = None;
                if let Some(c) = &current {
                    c.player.set_volume(gain(volume));
                    let _ = c.player.try_seek(Duration::from_secs_f32(s.max(0.0)));
                }
            }
            Ok(Cmd::Volume(v)) => {
                volume = v;
                if fade.is_none()
                    && let Some(c) = &current
                {
                    c.player.set_volume(gain(v));
                }
            }
            Ok(Cmd::Stop) => {
                current = None;
                fade = None;
                queued = None;
                next = None;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // The crossfade under way: equal power, so the middle doesn't dip.
        if let Some(f) = &fade {
            let t = (f.started.elapsed().as_secs_f32() / f.secs).clamp(0.0, 1.0);
            let half = std::f32::consts::FRAC_PI_2;
            f.old.player.set_volume(gain(volume) * (t * half).cos());
            if let Some(c) = &current {
                c.player.set_volume(gain(volume) * (t * half).sin());
            }
            if t >= 1.0 || f.old.player.empty() {
                fade = None;
                if let Some(c) = &current {
                    c.player.set_volume(gain(volume));
                }
            }
        }
        let Some(c) = &mut current else { continue };
        let pos = c.player.get_pos().as_secs_f32();
        let left = c.duration - pos;
        // The queued song took over the player.
        if let Some((token, duration)) = queued
            && c.player.len() <= 1
        {
            news(Event::Advanced { from: c.token, to: token });
            c.token = token;
            c.duration = duration;
            queued = None;
            continue;
        }
        if c.player.empty() {
            let t = c.token;
            current = None;
            news(Event::Ended(t));
            continue;
        }
        if paused || fade.is_some() || queued.is_some() {
            shared.pos_ms.store((pos * 1000.0) as u64, Ordering::Relaxed);
            continue;
        }
        let fade_secs = crossfade.min(c.duration / 3.0).min(next.as_ref().map_or(0.0, |n| n.duration / 3.0));
        if fade_secs >= 0.5 && left <= fade_secs && c.duration > 0.0 {
            // One that won't open fails again, and is reported, when the UI loads it after
            // this song ends.
            if let Some(song) = next.take()
                && let Ok(slot) = start(&song, 0.0, true, 0.0)
            {
                let old = std::mem::replace(c, slot);
                news(Event::Advanced { from: old.token, to: song.token });
                fade = Some(Fade { old, started: Instant::now(), secs: fade_secs });
                shared.pos_ms.store(0, Ordering::Relaxed);
                continue;
            }
        } else if gapless && crossfade < 0.5 && left <= QUEUE_AHEAD
            && let Some(song) = next.take()
            && let Ok(source) = open(&song, &tone)
        {
            c.player.append(source);
            queued = Some((song.token, song.duration));
        }
        shared.pos_ms.store((c.player.get_pos().as_secs_f32() * 1000.0) as u64, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Half a second of a 440 Hz tone as a 16-bit mono WAV.
    pub fn tone(path: &std::path::Path) {
        let rate = 44_100u32;
        let samples: Vec<i16> = (0..rate / 2).map(|i| ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 8000.0) as i16).collect();
        let data = samples.len() as u32 * 2;
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        for v in [1u16, 1] { b.extend_from_slice(&v.to_le_bytes()); }
        for v in [rate, rate * 2] { b.extend_from_slice(&v.to_le_bytes()); }
        for v in [2u16, 16] { b.extend_from_slice(&v.to_le_bytes()); }
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data.to_le_bytes());
        for s in samples { b.extend_from_slice(&s.to_le_bytes()); }
        std::fs::write(path, b).unwrap();
    }

    /// Plays a real file through the sound device: `cargo test -- --ignored plays`.
    #[test]
    #[ignore = "needs a sound device"]
    fn plays_a_file_to_its_end() {
        let dir = std::env::temp_dir().join(format!("stereo-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tone.wav");
        tone(&path);
        let t = crate::library::read_track(&path).expect("tags read");
        assert!((t.duration - 0.5).abs() < 0.05, "{}", t.duration);
        assert_eq!(t.title, "tone");
        let e = Engine::start(Arc::new(|| {}));
        e.load(path, 7, t.duration, 0.0, false);
        let start = std::time::Instant::now();
        'wait: loop {
            for n in e.take_news() {
                match n {
                    Event::NoDevice(why) => panic!("no device: {why}"),
                    Event::Failed(_, why) => panic!("failed: {why}"),
                    Event::Ended(7) => break 'wait,
                    _ => {}
                }
            }
            assert!(start.elapsed().as_secs() < 5, "never ended");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(start.elapsed().as_millis() >= 400, "ended too soon: {:?}", start.elapsed());
        let _ = std::fs::remove_dir_all(dir);
    }
}
