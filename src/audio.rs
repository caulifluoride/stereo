//! Playback, on a thread of its own that owns the sound device. The UI sends it commands and
//! reads back where the song is; it wakes the UI when a song ends or won't play.
//!
//! Rune plays audio through AVFoundation and Media Foundation but has no Linux backend, so
//! Stereo decodes with symphonia and plays through cpal, by way of rodio, everywhere.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::{Decoder, DeviceSinkBuilder, Player};

enum Cmd {
    /// Starts `path` as song `token`, `at` seconds in, paused or not.
    Load { path: PathBuf, token: u64, at: f32, paused: bool },
    Play,
    Pause,
    Seek(f32),
    Volume(f32),
    Stop,
}

/// What the audio thread has to say, taken by the UI in `poll`.
#[derive(Default)]
pub struct News {
    /// Song `token` played to its end.
    pub ended: Option<u64>,
    /// Song `token` couldn't be played, and why.
    pub failed: Option<(u64, String)>,
    /// The sound device couldn't be opened.
    pub no_device: Option<String>,
}

struct Shared {
    /// Milliseconds into the song playing.
    pos_ms: AtomicU64,
    news: Mutex<News>,
}

pub struct Engine {
    tx: Option<Sender<Cmd>>,
    shared: Arc<Shared>,
}

impl Engine {
    /// An engine that plays nothing, for headless runs and tests.
    pub fn silent() -> Engine {
        Engine { tx: None, shared: Arc::new(Shared { pos_ms: AtomicU64::new(0), news: Mutex::new(News::default()) }) }
    }

    pub fn start(wake: rune::Waker) -> Engine {
        let mut e = Engine::silent();
        let (tx, rx) = channel();
        let shared = e.shared.clone();
        if std::thread::Builder::new().name("stereo-audio".into()).spawn(move || run(rx, shared, wake)).is_ok() {
            e.tx = Some(tx);
        }
        e
    }

    fn send(&self, c: Cmd) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(c);
        }
    }

    pub fn load(&self, path: PathBuf, token: u64, at: f32, paused: bool) {
        self.shared.pos_ms.store((at * 1000.0) as u64, Ordering::Relaxed);
        self.send(Cmd::Load { path, token, at, paused });
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

    pub fn take_news(&self) -> News {
        std::mem::take(&mut *self.shared.news.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// Volume as a gain: squared, so the slider's middle sounds like half as loud.
fn gain(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    v * v
}

fn run(rx: Receiver<Cmd>, shared: Arc<Shared>, wake: rune::Waker) {
    let news = |f: &dyn Fn(&mut News)| {
        f(&mut shared.news.lock().unwrap_or_else(|e| e.into_inner()));
        wake();
    };
    let mut sink = match DeviceSinkBuilder::open_default_sink() {
        Ok(s) => s,
        Err(e) => {
            let msg = e.to_string();
            news(&|n| n.no_device = Some(msg.clone()));
            // Keep answering so loads report failure instead of hanging the queue.
            while let Ok(c) = rx.recv() {
                if let Cmd::Load { token, .. } = c {
                    news(&|n| n.failed = Some((token, "no sound device".into())));
                }
            }
            return;
        }
    };
    sink.log_on_drop(false);
    let mut volume = 0.8;
    // The song playing, and the player for it: a new one per song, so a change of song never
    // waits on the old one to drain.
    let mut current: Option<(u64, Player)> = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(Cmd::Load { path, token, at, paused }) => {
                current = None;
                let opened = std::fs::File::open(&path).map_err(|e| e.to_string()).and_then(|f| Decoder::try_from(f).map_err(|e| e.to_string()));
                match opened {
                    Ok(source) => {
                        let p = Player::connect_new(sink.mixer());
                        p.pause();
                        p.set_volume(gain(volume));
                        p.append(source);
                        if at > 0.0 {
                            let _ = p.try_seek(Duration::from_secs_f32(at));
                        }
                        if !paused {
                            p.play();
                        }
                        current = Some((token, p));
                    }
                    Err(e) => news(&|n| n.failed = Some((token, e.clone()))),
                }
            }
            Ok(Cmd::Play) => {
                if let Some((_, p)) = &current {
                    p.play();
                }
            }
            Ok(Cmd::Pause) => {
                if let Some((_, p)) = &current {
                    p.pause();
                }
            }
            Ok(Cmd::Seek(s)) => {
                if let Some((_, p)) = &current {
                    let _ = p.try_seek(Duration::from_secs_f32(s.max(0.0)));
                }
            }
            Ok(Cmd::Volume(v)) => {
                volume = v;
                if let Some((_, p)) = &current {
                    p.set_volume(gain(v));
                }
            }
            Ok(Cmd::Stop) => current = None,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if let Some((token, p)) = &current {
            if p.empty() {
                let t = *token;
                current = None;
                news(&|n| n.ended = Some(t));
            } else {
                shared.pos_ms.store(p.get_pos().as_millis() as u64, Ordering::Relaxed);
            }
        }
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
        e.load(path, 7, 0.0, false);
        let start = std::time::Instant::now();
        loop {
            let n = e.take_news();
            if let Some(why) = n.no_device { panic!("no device: {why}") }
            if let Some((_, why)) = n.failed { panic!("failed: {why}") }
            if n.ended == Some(7) { break }
            assert!(start.elapsed().as_secs() < 5, "never ended");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(start.elapsed().as_millis() >= 400, "ended too soon: {:?}", start.elapsed());
        let _ = std::fs::remove_dir_all(dir);
    }
}
