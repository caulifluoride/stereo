//! The windows over the app: confirming songs leave the library, the sleep timer, the
//! keyboard shortcuts and About. Settings, Get Info and the smart playlist editor have
//! modules of their own.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use rune::{El, col, row, spacer, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog as KitDialog, Kit};

use crate::State;
use crate::control::Sleep;
use crate::library::TrackId;

#[derive(Clone, PartialEq, Debug)]
pub enum Dialog {
    ConfirmRemove(Vec<TrackId>),
    Shortcuts,
    About,
    Settings(crate::prefs::Tab),
    Info,
    SmartEditor,
    Sleep,
}

pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Space", "Play or pause"),
    ("Ctrl+Right / Ctrl+Left", "Next / previous song"),
    ("Ctrl+Up / Ctrl+Down", "Volume up / down"),
    ("Return or double-click", "Play the selected song"),
    ("Ctrl+1 … Ctrl+5, Ctrl+0", "Rate the selection, or clear its rating"),
    ("Delete", "Remove the selection from the playlist or Party Shuffle"),
    ("Alt+Up / Alt+Down", "Move a Party Shuffle song up or down"),
    ("Ctrl+I", "Get Info on the selection"),
    ("Ctrl+Shift+Return", "Play the selection next (Up Next)"),
    ("Ctrl+F", "Search"),
    ("Ctrl+L", "Go to the song playing"),
    ("Ctrl+N / Ctrl+Alt+N", "New playlist / new smart playlist"),
    ("Ctrl+,", "Settings"),
    ("Ctrl+O", "Add a folder to the library"),
    ("Ctrl+B / Ctrl+G", "Show or hide the browser / the artwork"),
    ("Ctrl+P", "Show Party Shuffle"),
];

fn close(s: &mut State) {
    s.dialog = None;
}

fn sleep_dialog(s: &State, k: &Kit, layer: &mut Vec<El>) {
    let choice = |label: &str, key: &'static str, sleep: Option<Sleep>| {
        Button::new(label).variant(ButtonVariant::Outline).control_size(ControlSize::Small).on_press(move |s: &mut State| {
            close(s);
            s.set_sleep(sleep);
        }).build(k).key(key)
    };
    let mins = |m: u64| Some(Sleep::At(Instant::now() + Duration::from_secs(m * 60)));
    let now = match s.sleep {
        None => "Off".to_string(),
        Some(Sleep::EndOfSong) => "Stops at the end of this song.".into(),
        Some(Sleep::EndOfAlbum) => "Stops at the end of this album.".into(),
        Some(Sleep::At(_)) => format!("Pauses in {} minutes.", s.sleep_left().map_or(0, |d| d.as_secs().div_ceil(60))),
    };
    let times = row().key("sleep-times").gap(6.0)
        .child(choice("15 min", "sleep-15", mins(15)))
        .child(choice("30 min", "sleep-30", mins(30)))
        .child(choice("45 min", "sleep-45", mins(45)))
        .child(choice("1 hour", "sleep-60", mins(60)))
        .child(choice("90 min", "sleep-90", mins(90)));
    let ends = row().key("sleep-ends").gap(6.0)
        .child(choice("End of Song", "sleep-song", Some(Sleep::EndOfSong)))
        .child(choice("End of Album", "sleep-album", Some(Sleep::EndOfAlbum)))
        .child(spacer())
        .child(choice("Turn Off", "sleep-off", None));
    let body = col().w_full().px(16.0).pb(16.0).gap(10.0)
        .child(text(now).font(k.ty.ui).color(k.c.text_muted))
        .child(times)
        .child(ends);
    KitDialog::new("sleep", "Sleep Timer").description("Stop the music after a while.").child(body).width(460.0).fit().open(true).on_close(close).build(k, layer);
}

pub fn layer(s: &State, k: &Kit, layer: &mut Vec<El>) {
    let Some(d) = &s.dialog else { return };
    match d {
        Dialog::Settings(tab) => crate::prefs::layer(s, k, layer, *tab),
        Dialog::Info => crate::info::layer(s, k, layer),
        Dialog::SmartEditor => crate::smartedit::layer(s, k, layer),
        Dialog::Sleep => sleep_dialog(s, k, layer),
        Dialog::ConfirmRemove(ids) => {
            let n = ids.len();
            let what = if n == 1 { "this song".to_string() } else { format!("these {n} songs") };
            let ids: HashSet<TrackId> = ids.iter().copied().collect();
            let buttons = row().key("confirm-buttons").w_full().gap(8.0)
                .child(spacer())
                .child(Button::new("Cancel").variant(ButtonVariant::Outline).on_press(close).build(k).key("cancel"))
                .child(Button::new("Remove").variant(ButtonVariant::Contrast).on_press(move |s: &mut State| {
                    close(s);
                    s.remove_tracks(&ids);
                }).build(k).key("remove"));
            KitDialog::new("confirm-remove", format!("Remove {what} from your library?"))
                .description("They also leave every playlist and Party Shuffle. The files stay on disk.")
                .child(buttons)
                .width(420.0)
                .open(true)
                .on_close(close)
                .build(k, layer);
        }
        Dialog::Shortcuts => {
            let mut list = col().key("shortcut-list").w_full().gap(6.0);
            for (keys, what) in SHORTCUTS {
                list = list.child(row().w_full().gap(12.0)
                    .child(text(*keys).font(k.ty.small_medium).color(k.c.text_base).w(170.0))
                    .child(text(*what).font(k.ty.small).color(k.c.text_muted).wrap().grow(1.0).min_w(0.0)));
            }
            KitDialog::new("shortcuts", "Keyboard Shortcuts").child(list).width(480.0).open(true).on_close(close).build(k, layer);
        }
        Dialog::About => {
            let body = col().gap(6.0)
                .child(text(format!("Stereo {}", env!("CARGO_PKG_VERSION"))).font(k.ty.ui_medium).color(k.c.text_base))
                .child(text("A music player in the spirit of iTunes 4 to 7, with Party Shuffle. Drawn with Rune; decoded by symphonia.").font(k.ty.small).color(k.c.text_muted).wrap());
            KitDialog::new("about", "About Stereo").child(body).width(380.0).open(true).on_close(close).build(k, layer);
        }
    }
}
