//! Writing the library file. The UI thread serializes the library when it changes (a second
//! after the last change, so a burst of ratings or an import writes once) and a worker writes
//! the bytes to a temporary file and renames it over the old one.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::library::Library;

pub struct Saver {
    path: PathBuf,
}

static LATEST: Mutex<u64> = Mutex::new(0);

fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

impl Saver {
    pub fn new(path: PathBuf) -> Saver {
        Saver { path }
    }

    pub fn save(&self, lib: &Library) {
        let bytes = match serde_json::to_vec(lib) {
            Ok(b) => b,
            Err(e) => return eprintln!("stereo: couldn't save the library: {e}"),
        };
        let ticket = {
            let mut n = LATEST.lock().unwrap_or_else(|e| e.into_inner());
            *n += 1;
            *n
        };
        let path = self.path.clone();
        let _ = rune::task::spawn("stereo-save", move || {
            let latest = LATEST.lock().unwrap_or_else(|e| e.into_inner());
            if *latest == ticket
                && let Err(e) = write(&path, &bytes)
            {
                eprintln!("stereo: couldn't save {}: {e}", path.display());
            }
        });
    }

    /// Saves on the calling thread, for the window closing.
    pub fn save_now(&self, lib: &Library) {
        let mut latest = LATEST.lock().unwrap_or_else(|e| e.into_inner());
        *latest += 1;
        match serde_json::to_vec(lib) {
            Ok(b) => {
                if let Err(e) = write(&self.path, &b) {
                    eprintln!("stereo: couldn't save {}: {e}", self.path.display());
                }
            }
            Err(e) => eprintln!("stereo: couldn't save the library: {e}"),
        }
    }
}
