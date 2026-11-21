//! What Stereo remembers between runs besides the library: the theme, volume, shuffle and
//! repeat, which panes and columns show, playback (crossfade, volume levelling, the
//! equaliser), how plays and skips count, the online services, and the song that was
//! playing. Kept in Rune's prefs store (`prefs.json` in the platform's config folder) and
//! saved off the UI thread, the latest change winning.

use std::sync::Mutex;

use rune::prefs::Prefs;
use serde::{Deserialize, Serialize};

use crate::library::TrackId;
use crate::queue::Repeat;

const APP: &str = "stereo";
const KEY: &str = "settings";

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// The song playing when Stereo last closed, to come back to paused.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct Resume {
    pub track: TrackId,
    pub at: f32,
    pub party: bool,
}

/// What happens to the song that was playing when Stereo opens.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Launch {
    /// Back where it was, paused.
    #[default]
    Paused,
    /// Back where it was, playing.
    Playing,
    /// Nothing loaded.
    Nothing,
}

/// When a song counts a play.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum PlayCount {
    /// Played to its end, as iTunes counted.
    #[default]
    End,
    /// Played halfway.
    Half,
    /// Played halfway or four minutes, whichever comes first (Last.fm's rule).
    Scrobble,
}

impl PlayCount {
    /// Whether `pos` seconds into a song `len` long counts a play before it ends.
    pub fn reached(self, pos: f32, len: f32) -> bool {
        match self {
            PlayCount::End => false,
            PlayCount::Half => len > 0.0 && pos >= len / 2.0,
            PlayCount::Scrobble => len > 0.0 && (pos >= len / 2.0 || pos >= 240.0),
        }
    }
}

/// When moving on from a song counts a skip.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum SkipCount {
    Never,
    /// Skipped in its first half (after the first two seconds), as iTunes counted.
    #[default]
    Half,
    /// Skipped in its first 30 seconds (after the first two).
    Early,
}

impl SkipCount {
    pub fn counts(self, pos: f32, len: f32) -> bool {
        pos > 2.0
            && match self {
                SkipCount::Never => false,
                SkipCount::Half => pos < len * 0.5,
                SkipCount::Early => pos < 30.0,
            }
    }
}

/// Volume levelling from ReplayGain tags.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Gain {
    #[default]
    Off,
    /// Each song at the same loudness.
    Track,
    /// Each album at the same loudness, its songs as loud as each other as mastered.
    Album,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    Compact,
    #[default]
    Normal,
    Large,
}

impl Density {
    pub fn row_height(self) -> f32 {
        match self {
            Density::Compact => 22.0,
            Density::Normal => 28.0,
            Density::Large => 34.0,
        }
    }
}

/// The highlight colour: the kit's own blue, or one of these.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    #[default]
    Blue,
    Purple,
    Pink,
    Red,
    Orange,
    Green,
    Graphite,
}

impl Accent {
    pub const ALL: [Accent; 7] = [Accent::Blue, Accent::Purple, Accent::Pink, Accent::Red, Accent::Orange, Accent::Green, Accent::Graphite];

    pub fn name(self) -> &'static str {
        match self {
            Accent::Blue => "Blue",
            Accent::Purple => "Purple",
            Accent::Pink => "Pink",
            Accent::Red => "Red",
            Accent::Orange => "Orange",
            Accent::Green => "Green",
            Accent::Graphite => "Graphite",
        }
    }

    /// Its hue in OKLCH, or None for the kit's own accent.
    pub fn hue(self) -> Option<f32> {
        match self {
            Accent::Blue => None,
            Accent::Purple => Some(300.0),
            Accent::Pink => Some(350.0),
            Accent::Red => Some(25.0),
            Accent::Orange => Some(55.0),
            Accent::Green => Some(150.0),
            Accent::Graphite => Some(250.0),
        }
    }
}

/// The equaliser's bands, in hertz.
pub const EQ_BANDS: [f32; 10] = [32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

/// The equaliser's presets, as iTunes named them: gains in decibels per band.
pub const EQ_PRESETS: &[(&str, [f32; 10])] = &[
    ("Flat", [0.0; 10]),
    ("Acoustic", [5.0, 4.5, 3.5, 1.0, 1.5, 1.5, 3.0, 3.5, 3.0, 1.5]),
    ("Bass Booster", [5.5, 4.5, 4.0, 2.5, 1.5, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Bass Reducer", [-5.5, -4.5, -4.0, -2.5, -1.5, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Classical", [4.5, 4.0, 3.0, 2.5, -1.5, -1.5, 0.0, 2.0, 3.0, 3.5]),
    ("Dance", [3.5, 6.5, 5.0, 0.0, 2.0, 3.5, 5.0, 4.5, 3.5, 0.0]),
    ("Electronic", [4.0, 3.5, 1.0, 0.0, -2.0, 2.0, 1.0, 1.5, 4.0, 4.5]),
    ("Hip-Hop", [5.0, 4.0, 1.5, 3.0, -1.0, -1.0, 1.5, -0.5, 2.0, 3.0]),
    ("Jazz", [4.0, 3.0, 1.5, 2.0, -1.5, -1.5, 0.0, 1.5, 3.0, 3.5]),
    ("Loudness", [6.0, 4.0, 0.0, 0.0, -2.0, 0.0, -1.0, -5.0, 5.0, 1.0]),
    ("Lounge", [-3.0, -1.5, -0.5, 1.5, 4.0, 2.5, 0.0, -1.5, 2.0, 1.0]),
    ("Pop", [-1.5, -1.0, 0.0, 2.0, 4.0, 4.0, 2.0, 0.0, -1.0, -1.5]),
    ("R&B", [2.5, 7.0, 5.5, 1.5, -2.5, -1.5, 2.5, 3.0, 3.0, 4.0]),
    ("Rock", [5.0, 4.0, 3.0, 1.5, -0.5, -1.0, 0.5, 2.5, 3.5, 4.5]),
    ("Small Speakers", [5.5, 4.0, 3.5, 2.5, 1.0, 0.0, -1.0, -2.0, -3.5, -4.0]),
    ("Spoken Word", [-3.5, -0.5, 0.0, 0.5, 3.5, 4.5, 5.0, 4.5, 2.5, 0.0]),
    ("Treble Booster", [0.0, 0.0, 0.0, 0.0, 0.0, 1.5, 2.5, 4.0, 4.5, 5.5]),
    ("Treble Reducer", [0.0, 0.0, 0.0, 0.0, 0.0, -1.5, -2.5, -4.0, -4.5, -5.5]),
    ("Vocal Booster", [-1.5, -3.0, -3.0, 1.5, 4.0, 4.0, 3.0, 1.5, 0.0, -1.5]),
];

/// The columns the song list can show, in order: id, title. `now` always shows.
pub const COLUMNS: &[(&str, &str)] = &[
    ("pos", "# (playlists)"),
    ("name", "Name"),
    ("time", "Time"),
    ("artist", "Artist"),
    ("albumartist", "Album Artist"),
    ("album", "Album"),
    ("disc", "Disc"),
    ("track", "Track"),
    ("genre", "Genre"),
    ("year", "Year"),
    ("rating", "Rating"),
    ("plays", "Plays"),
    ("skips", "Skips"),
    ("last", "Last Played"),
    ("added", "Date Added"),
];

/// How many songs the built-in smart playlists hold.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct SmartSizes {
    pub most_played: usize,
    pub recently_added: usize,
    pub recently_played: usize,
}

impl Default for SmartSizes {
    fn default() -> Self {
        SmartSizes { most_played: 25, recently_added: 100, recently_played: 50 }
    }
}

/// Last.fm: the API account Stereo signs in with (Last.fm gives each app its own) and the
/// session it got. The password is never kept; only the session key.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
#[serde(default)]
pub struct LastFm {
    pub api_key: String,
    pub secret: String,
    pub user: String,
    pub session: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub browser: bool,
    pub artwork: bool,
    pub sidebar: f32,
    pub resume: Option<Resume>,
    pub on_launch: Launch,
    /// A desktop notification when the song changes.
    pub notify: bool,
    pub play_count: PlayCount,
    pub skip_count: SkipCount,
    /// Seconds one song fades into the next; 0 for none.
    pub crossfade: f32,
    /// The next song starts the moment one ends, with no gap.
    pub gapless: bool,
    pub gain: Gain,
    /// Decibels added to the ReplayGain level.
    pub preamp: f32,
    pub eq_on: bool,
    pub eq: [f32; 10],
    pub eq_preset: String,
    /// Watch the library's folders, adding new songs as they appear.
    pub watch: bool,
    /// The song list's columns turned off, by id.
    pub hidden_columns: Vec<String>,
    pub smart: SmartSizes,
    pub accent: Accent,
    pub density: Density,
    /// The interface's zoom, 1 at its normal size.
    pub text_size: f32,
    /// Look up artwork online (MusicBrainz and the Cover Art Archive) for albums without any.
    pub fetch_art: bool,
    pub lastfm: LastFm,
    /// A ListenBrainz user token: listens are submitted while it's set.
    pub listenbrainz: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::System,
            volume: 0.8,
            shuffle: false,
            repeat: Repeat::Off,
            browser: true,
            artwork: false,
            sidebar: 210.0,
            resume: None,
            on_launch: Launch::Paused,
            notify: false,
            play_count: PlayCount::End,
            skip_count: SkipCount::Half,
            crossfade: 0.0,
            gapless: true,
            gain: Gain::Off,
            preamp: 0.0,
            eq_on: false,
            eq: [0.0; 10],
            eq_preset: "Flat".into(),
            watch: true,
            hidden_columns: ["albumartist", "disc", "track", "skips", "last", "added"].map(String::from).to_vec(),
            smart: SmartSizes::default(),
            accent: Accent::Blue,
            density: Density::Normal,
            text_size: 1.0,
            fetch_art: false,
            lastfm: LastFm::default(),
            listenbrainz: String::new(),
        }
    }
}

impl Settings {
    pub fn load() -> Settings {
        Prefs::open(APP).get(KEY).unwrap_or_default()
    }

    pub fn shows(&self, column: &str) -> bool {
        !self.hidden_columns.iter().any(|c| c == column)
    }

    pub fn dark(&self, system_dark: bool) -> bool {
        match self.theme {
            Theme::System => system_dark,
            Theme::Light => false,
            Theme::Dark => true,
        }
    }

    pub fn save(&self) {
        static LATEST: Mutex<u64> = Mutex::new(0);
        let ticket = {
            let mut n = LATEST.lock().unwrap_or_else(|e| e.into_inner());
            *n += 1;
            *n
        };
        let me = self.clone();
        let _ = rune::task::spawn("stereo-settings", move || {
            // Held while writing, so an older change can't land after a newer one.
            let latest = LATEST.lock().unwrap_or_else(|e| e.into_inner());
            if *latest != ticket {
                return;
            }
            if let Err(e) = Prefs::open(APP).set(KEY, &me) {
                eprintln!("stereo: {e}");
            }
        });
    }

    /// Saves on the calling thread, for the window closing.
    pub fn save_now(&self) {
        if let Err(e) = Prefs::open(APP).set(KEY, self) {
            eprintln!("stereo: {e}");
        }
    }
}
