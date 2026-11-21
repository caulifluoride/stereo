//! Watching the library's folders: files that appear are read in, and songs whose files go
//! away are marked missing rather than dropped (a drive unplugged comes back). The system's
//! watcher reports on a thread of its own; changes wait here until the UI's poll takes them,
//! and the UI holds them a couple of seconds so a big copy is read in once.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

pub struct Watch {
    _watcher: RecommendedWatcher,
    rx: Receiver<Event>,
    pub folders: Vec<PathBuf>,
}

/// What changed on disk since the last look.
#[derive(Default, Debug)]
pub struct Changes {
    /// Files and folders that appeared or changed.
    pub added: Vec<PathBuf>,
    /// Paths that went away.
    pub removed: Vec<PathBuf>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty()
    }
}

impl Watch {
    pub fn start(folders: &[PathBuf], wake: rune::Waker) -> Option<Watch> {
        let (tx, rx) = channel();
        let mut w = notify::recommended_watcher(move |e: notify::Result<Event>| {
            if let Ok(e) = e
                && tx.send(e).is_ok()
            {
                wake();
            }
        })
        .ok()?;
        let mut watching = Vec::new();
        for f in folders {
            match w.watch(f, RecursiveMode::Recursive) {
                Ok(()) => watching.push(f.clone()),
                Err(e) => eprintln!("stereo: can't watch {}: {e}", f.display()),
            }
        }
        Some(Watch { _watcher: w, rx, folders: watching })
    }

    pub fn take(&self) -> Changes {
        let mut c = Changes::default();
        for e in self.rx.try_iter() {
            match e.kind {
                EventKind::Create(_) => c.added.extend(e.paths),
                EventKind::Remove(_) => c.removed.extend(e.paths),
                EventKind::Modify(notify::event::ModifyKind::Name(_)) => {
                    // A rename names the old path, the new one, or both; which is which shows
                    // on disk.
                    for p in e.paths {
                        if p.exists() { c.added.push(p) } else { c.removed.push(p) }
                    }
                }
                // A file finishing its copy is a write.
                EventKind::Modify(notify::event::ModifyKind::Data(_)) | EventKind::Access(notify::event::AccessKind::Close(notify::event::AccessMode::Write)) => c.added.extend(e.paths),
                _ => {}
            }
        }
        c
    }
}
