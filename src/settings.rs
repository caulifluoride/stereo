//! What Stereo remembers between runs besides the library: the theme, volume, shuffle and
//! repeat, which panes show, and the song that was playing. Kept in Rune's prefs store
//! (`prefs.json` in the platform's config folder) and saved off the UI thread, the latest
//! change winning.

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
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: Theme::System,
            volume: 0.8,
            shuffle: false,
            repeat: Repeat::Off,
            browser: true,
            artwork: true,
            sidebar: 210.0,
            resume: None,
        }
    }
}

impl Settings {
    pub fn load() -> Settings {
        Prefs::open(APP).get(KEY).unwrap_or_default()
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
