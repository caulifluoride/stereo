//! What the audio thread does to each song's samples on their way out: volume levelling from
//! its ReplayGain tags and the ten-band equaliser. Both read [`Tone`], which the UI changes
//! while a song plays; the next samples pick a change up.

use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use lofty::prelude::*;
use rodio::source::SeekError;
use rodio::{ChannelCount, Sample, SampleRate, Source};

use crate::settings::{EQ_BANDS, Gain};

/// The levelling and equaliser settings, as the UI last set them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ToneSettings {
    pub gain: Gain,
    /// Decibels added to the ReplayGain level.
    pub preamp: f32,
    pub eq_on: bool,
    pub eq: [f32; 10],
}

#[derive(Default)]
pub struct Tone {
    version: AtomicU64,
    settings: Mutex<ToneSettings>,
}

impl Tone {
    pub fn set(&self, t: ToneSettings) {
        *self.settings.lock().unwrap_or_else(|e| e.into_inner()) = t;
        self.version.fetch_add(1, Ordering::Release);
    }

    fn get(&self) -> ToneSettings {
        *self.settings.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// A song's ReplayGain tags: gains in decibels, peaks as sample amplitudes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Levels {
    pub track_gain: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain: Option<f32>,
    pub album_peak: Option<f32>,
}

/// "-6.54 dB" or "0.98765" to a number.
fn number(s: &str) -> Option<f32> {
    s.trim().trim_end_matches(|c: char| c.is_alphabetic() || c.is_whitespace()).trim().parse().ok()
}

pub fn read_levels(path: &Path) -> Levels {
    let Ok(tagged) = lofty::read_from_path(path) else { return Levels::default() };
    let mut l = Levels::default();
    for tag in tagged.tags() {
        let get = |k: ItemKey| tag.get_string(k).and_then(number);
        l.track_gain = l.track_gain.or(get(ItemKey::ReplayGainTrackGain));
        l.track_peak = l.track_peak.or(get(ItemKey::ReplayGainTrackPeak));
        l.album_gain = l.album_gain.or(get(ItemKey::ReplayGainAlbumGain));
        l.album_peak = l.album_peak.or(get(ItemKey::ReplayGainAlbumPeak));
    }
    l
}

impl Levels {
    /// The gain to play at, as a factor: the tag's (album, falling back to track) plus the
    /// preamp, held down so the song's peak doesn't clip.
    pub fn factor(&self, mode: Gain, preamp: f32) -> f32 {
        let (gain, peak) = match mode {
            Gain::Off => return 1.0,
            Gain::Track => (self.track_gain.or(self.album_gain), self.track_peak.or(self.album_peak)),
            Gain::Album => (self.album_gain.or(self.track_gain), self.album_peak.or(self.track_peak)),
        };
        let Some(db) = gain else { return 1.0 };
        let f = 10f32.powf((db + preamp) / 20.0);
        match peak {
            Some(p) if p > 0.0 => f.min(1.0 / p),
            _ => f,
        }
    }
}

/// One peaking filter (the Audio EQ Cookbook's), transposed direct form II, per channel.
#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

/// An octave band's width.
const Q: f32 = 1.41;

impl Biquad {
    fn peaking(freq: f32, gain_db: f32, rate: f32) -> Biquad {
        let a = 10f32.powf(gain_db / 40.0);
        let w = std::f32::consts::TAU * freq / rate;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * Q);
        let a0 = 1.0 + alpha / a;
        Biquad {
            b0: (1.0 + alpha * a) / a0,
            b1: -2.0 * cos / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha / a) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// A song's samples, levelled and equalised.
pub struct Dsp<S> {
    inner: S,
    tone: Arc<Tone>,
    levels: Levels,
    seen: u64,
    rate: u32,
    channels: u16,
    gain: f32,
    /// One filter chain per channel; empty with the equaliser off or flat.
    filters: Vec<Vec<Biquad>>,
    channel: usize,
    /// Samples until the sample rate and channel count are looked at again.
    check: u32,
}

impl<S: Source> Dsp<S> {
    pub fn new(inner: S, tone: Arc<Tone>, levels: Levels) -> Dsp<S> {
        let mut d = Dsp { inner, tone, levels, seen: u64::MAX, rate: 0, channels: 0, gain: 1.0, filters: Vec::new(), channel: 0, check: 0 };
        d.update();
        d
    }

    fn update(&mut self) {
        let t = self.tone.get();
        self.seen = self.tone.version.load(Ordering::Acquire);
        self.rate = self.inner.sample_rate().get();
        self.channels = self.inner.channels().get();
        let mut gain = self.levels.factor(t.gain, t.preamp);
        self.filters.clear();
        if t.eq_on && t.eq.iter().any(|&g| g != 0.0) {
            let nyquist = self.rate as f32 / 2.0;
            let chain: Vec<Biquad> = EQ_BANDS.iter().zip(t.eq).filter(|&(&f, g)| g != 0.0 && f < nyquist * 0.9).map(|(&f, g)| Biquad::peaking(f, g, self.rate as f32)).collect();
            // Boosts would clip at full scale; bring the whole song down by the biggest one.
            let boost = t.eq.iter().copied().fold(0.0f32, f32::max);
            gain *= 10f32.powf(-boost / 20.0);
            self.filters = vec![chain; usize::from(self.channels)];
        }
        self.gain = gain;
        self.channel = 0;
    }
}

impl<S: Source> Iterator for Dsp<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        if self.check == 0 {
            self.check = 4096;
            if self.tone.version.load(Ordering::Acquire) != self.seen || self.inner.sample_rate().get() != self.rate || self.inner.channels().get() != self.channels {
                self.update();
            }
        }
        self.check -= 1;
        let mut x = self.inner.next()?;
        if let Some(chain) = self.filters.get_mut(self.channel) {
            for f in chain {
                x = f.run(x);
            }
        }
        self.channel = (self.channel + 1) % usize::from(self.channels.max(1));
        Some((x * self.gain).clamp(-1.0, 1.0))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S: Source> Source for Dsp<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }
    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.inner.try_seek(pos)?;
        // Ringing from before the seek would click; the filters start again from silence.
        for chain in &mut self.filters {
            for f in chain {
                f.z1 = 0.0;
                f.z2 = 0.0;
            }
        }
        self.channel = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaygain_levels_follow_the_mode_and_hold_the_peak_down() {
        let l = Levels { track_gain: Some(-6.0), track_peak: Some(0.5), album_gain: Some(-3.0), album_peak: None };
        assert_eq!(l.factor(Gain::Off, 0.0), 1.0);
        assert!((l.factor(Gain::Track, 0.0) - 0.501).abs() < 0.01);
        assert!((l.factor(Gain::Album, 0.0) - 0.708).abs() < 0.01);
        // +12 dB would push a 0.5 peak past full scale: held at 2x.
        assert!((l.factor(Gain::Track, 12.0) - 2.0).abs() < 0.01);
        assert_eq!(Levels::default().factor(Gain::Track, 6.0), 1.0);
        assert_eq!(number("-6.54 dB"), Some(-6.54));
        assert_eq!(number("0.98"), Some(0.98));
    }

    #[test]
    fn a_flat_band_passes_a_tone_through_and_a_boost_raises_it() {
        let rate = 44_100.0;
        let tone = |f: &mut Biquad| -> f32 {
            (0..44_100).map(|i| f.run((i as f32 * 1000.0 * std::f32::consts::TAU / rate).sin())).skip(4410).fold(0.0f32, |m, x| m.max(x.abs()))
        };
        assert!((tone(&mut Biquad::peaking(1000.0, 0.0, rate)) - 1.0).abs() < 0.01);
        let boosted = tone(&mut Biquad::peaking(1000.0, 6.0, rate));
        assert!((boosted - 2.0).abs() < 0.05, "{boosted}");
        // A band far away leaves it nearly alone.
        assert!((tone(&mut Biquad::peaking(32.0, 6.0, rate)) - 1.0).abs() < 0.05);
    }
}
