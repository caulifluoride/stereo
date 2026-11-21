//! The desktop around the window: a notification when the song changes, and showing a song's
//! file in the file manager. Linux goes through D-Bus (the freedesktop notification and file
//! manager services); macOS and Windows show files with `open -R` and Explorer, and post no
//! notifications.

use std::path::Path;

/// Posts (or replaces) the "now playing" notification, off the UI thread.
pub fn notify(title: &str, body: &str) {
    #[cfg(target_os = "linux")]
    {
        let (title, body) = (title.to_string(), body.to_string());
        let _ = rune::task::spawn("stereo-notify", move || linux::notify(&title, &body));
    }
    #[cfg(not(target_os = "linux"))]
    let _ = (title, body);
}

/// Opens the folder holding `path` with the file selected, where the platform can.
pub fn show_in_folder(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    return linux::show(path);
    #[cfg(target_os = "macos")]
    return std::process::Command::new("open").arg("-R").arg(path).spawn().map(drop).map_err(|e| e.to_string());
    #[cfg(target_os = "windows")]
    {
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path);
        return std::process::Command::new("explorer").arg(arg).spawn().map(drop).map_err(|e| e.to_string());
    }
    #[allow(unreachable_code)]
    Err(format!("can't show {} here", path.display()))
}

/// A path as a `file://` URI, every byte outside the unreserved set percent-encoded.
#[cfg(target_os = "linux")]
pub fn file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut out = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU32, Ordering};

    use zbus::blocking::Connection;
    use zbus::zvariant::Value;

    fn session() -> Option<&'static Connection> {
        static BUS: OnceLock<Option<Connection>> = OnceLock::new();
        BUS.get_or_init(|| Connection::session().ok()).as_ref()
    }

    /// The last notification's id, so the next replaces it rather than piling up.
    static LAST: AtomicU32 = AtomicU32::new(0);

    pub fn notify(title: &str, body: &str) {
        let Some(bus) = session() else { return };
        let hints: HashMap<&str, Value> = HashMap::from([("category", Value::from("x-gnome.music")), ("desktop-entry", Value::from("stereo"))]);
        let reply = bus.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &("Stereo", LAST.load(Ordering::Relaxed), "stereo", title, body, Vec::<&str>::new(), hints, 5000i32),
        );
        if let Ok(m) = reply
            && let Ok(id) = m.body().deserialize::<u32>()
        {
            LAST.store(id, Ordering::Relaxed);
        }
    }

    pub fn show(path: &Path) -> Result<(), String> {
        let uri = super::file_uri(path);
        let shown = session().is_some_and(|bus| {
            bus.call_method(Some("org.freedesktop.FileManager1"), "/org/freedesktop/FileManager1", Some("org.freedesktop.FileManager1"), "ShowItems", &(vec![uri.as_str()], ""))
                .is_ok()
        });
        if shown {
            return Ok(());
        }
        // No file manager on the bus: open the folder.
        let dir = path.parent().unwrap_or(path);
        std::process::Command::new("xdg-open").arg(dir).spawn().map(drop).map_err(|e| e.to_string())
    }
}
