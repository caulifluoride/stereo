//! The window's keys: transport, volume, ratings, removing and moving songs, and jumping
//! about. Keys a focused field or the tables take never get here.

use rune::{Key, KeyEvent, Tx};

use crate::rows::View;
use crate::{State, menus, partyview, sidebar};

pub fn window_key(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let m = ev.mods;
    if ev.key == Key::Text(" ".into()) && !m.cmd {
        s.play_pause();
        return true;
    }
    if m.cmd && !m.shift {
        match ev.key {
            Key::Right => s.next(false),
            Key::Left => s.prev(),
            Key::Up => s.set_volume(s.settings.volume + 0.1),
            Key::Down => s.set_volume(s.settings.volume - 0.1),
            _ => return command(s, ev, tx),
        }
        return true;
    }
    if m.alt && s.view == View::Party && matches!(ev.key, Key::Up | Key::Down) {
        let sel = partyview::selected(s);
        if let [serial] = sel.into_iter().collect::<Vec<_>>()[..]
            && s.lib.party.shift(serial, if ev.key == Key::Up { -1 } else { 1 })
        {
            s.party_changed();
        }
        return true;
    }
    if matches!(ev.key, Key::Delete | Key::Backspace) {
        match s.view {
            View::Party => {
                let sel = partyview::selected(s);
                s.party_remove(&sel);
                s.table_party.clear_selection();
            }
            View::Playlist(id) => menus::remove_from_playlist(s, id),
            View::Music => {
                let sel = s.selected_tracks();
                if !sel.is_empty() {
                    s.dialog = Some(crate::dialogs::Dialog::ConfirmRemove(sel));
                }
            }
            View::Smart(_) => return false,
        }
        return true;
    }
    false
}

/// Ctrl (Cmd on macOS) with a letter or digit.
fn command(s: &mut State, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let Key::Command(c) = ev.key else { return false };
    match c {
        '0'..='5' => {
            let n = c as u8 - b'0';
            for id in s.selected_tracks() {
                if let Some(t) = s.lib.get_mut(id) {
                    t.rating = n;
                }
            }
            s.changed();
            s.refresh();
        }
        'f' => s.search.focused = true,
        'l' => s.reveal_now(tx),
        'n' => sidebar::new_playlist(s, Vec::new()),
        'o' => menus::add_folder(s, tx),
        'p' => s.select_view(View::Party),
        'b' => {
            s.settings.browser = !s.settings.browser;
            s.settings_changed();
            s.refresh();
        }
        'g' => {
            s.settings.artwork = !s.settings.artwork;
            s.settings_changed();
        }
        _ => return false,
    }
    true
}
