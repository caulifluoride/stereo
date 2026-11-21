//! The windows over the app: confirming songs leave the library, the keyboard shortcuts, and
//! About.

use std::collections::HashSet;

use rune::{El, col, row, spacer, text};
use rune_kit::{Button, ButtonVariant, Dialog as KitDialog, Kit};

use crate::State;
use crate::library::TrackId;

#[derive(Clone, PartialEq, Debug)]
pub enum Dialog {
    ConfirmRemove(Vec<TrackId>),
    Shortcuts,
    About,
}

pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Space", "Play or pause"),
    ("Ctrl+Right / Ctrl+Left", "Next / previous song"),
    ("Ctrl+Up / Ctrl+Down", "Volume up / down"),
    ("Return or double-click", "Play the selected song"),
    ("Ctrl+1 … Ctrl+5, Ctrl+0", "Rate the selection, or clear its rating"),
    ("Delete", "Remove the selection from the playlist or Party Shuffle"),
    ("Alt+Up / Alt+Down", "Move a Party Shuffle song up or down"),
    ("Ctrl+F", "Search"),
    ("Ctrl+L", "Go to the song playing"),
    ("Ctrl+N", "New playlist"),
    ("Ctrl+O", "Add a folder to the library"),
    ("Ctrl+B / Ctrl+G", "Show or hide the browser / the artwork"),
    ("Ctrl+P", "Show Party Shuffle"),
];

fn close(s: &mut State) {
    s.dialog = None;
}

pub fn layer(s: &State, k: &Kit, layer: &mut Vec<El>) {
    let Some(d) = &s.dialog else { return };
    match d {
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
