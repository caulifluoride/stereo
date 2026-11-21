//! The menus: a right-click on songs, on Party Shuffle's entries or on a playlist in the
//! sidebar, and the app menu at the top right.

use std::collections::HashSet;

use rune::{El, FileDialogOptions, PointerPress, Tx};
use rune_kit::{IconButton, IconButtonVariant, Kit, MenuItem, context_menu};

use crate::dialogs::Dialog;
use crate::library::TrackId;
use crate::party::Source;
use crate::rows::View;
use crate::settings::Theme;
use crate::{State, icons, partyview, sidebar};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Menu {
    /// Songs in the track list, at the pointer.
    Tracks { at: PointerPress },
    /// Entries in Party Shuffle.
    Party { at: PointerPress },
    /// A source in the sidebar.
    Source { at: PointerPress, view: View },
    /// The menu at the top right.
    App,
    /// One of Party Shuffle's pop-up choices, by key.
    PartyCount(&'static str),
}

impl Menu {
    /// The track menu over the first song, for the headless `menu` scene.
    pub fn sample(_: &State) -> Menu {
        Menu::Tracks { at: PointerPress { x: 520.0, y: 330.0 } }
    }
}

fn close(s: &mut State) {
    s.menu = None;
}

fn point(s: &State, f: fn(&Menu) -> Option<PointerPress>) -> Option<PointerPress> {
    s.menu.as_ref().and_then(f)
}

/// Wraps the track list (or Party Shuffle) in its right-click menu.
pub fn wrap(s: &State, k: &Kit, pane: El, layer: &mut Vec<El>) -> El {
    if s.view == View::Party {
        let at = point(s, |m| if let Menu::Party { at } = m { Some(*at) } else { None });
        return party_menu(s, k, pane, at, layer);
    }
    let at = point(s, |m| if let Menu::Tracks { at } = m { Some(*at) } else { None });
    let sel = s.selected_tracks();
    let n = sel.len();
    let songs = if n == 1 { "Song".to_string() } else { format!("{n} Songs") };
    let mut m = context_menu("track-menu", pane, at)
        .min_width(250.0)
        .item(MenuItem::new("Play").on_select(|s: &mut State| {
            close(s);
            let first = s.selected_tracks().first().copied();
            if let Some(i) = first.and_then(|id| s.rows.iter().position(|&r| r == id)) {
                s.play_row(i);
            }
        }))
        .item(MenuItem::new("Play Next in Party Shuffle").on_select(|s: &mut State| {
            close(s);
            let ids = s.selected_tracks();
            s.party_play_next(&ids);
        }))
        .item(MenuItem::new("Add to Party Shuffle").on_select(|s: &mut State| {
            close(s);
            let ids = s.selected_tracks();
            s.party_add(&ids);
        }))
        .separator()
        .group_label("Add to Playlist")
        .item(MenuItem::new("New Playlist from Selection").on_select(|s: &mut State| {
            close(s);
            let ids = s.selected_tracks();
            sidebar::new_playlist(s, ids);
        }));
    for p in &s.lib.playlists {
        if s.view == View::Playlist(p.id) {
            continue;
        }
        let id = p.id;
        m = m.item(MenuItem::new(p.name.clone()).on_select(move |s: &mut State| {
            close(s);
            let ids = s.selected_tracks();
            add_to_playlist(s, id, &ids);
        }));
    }
    m = m.separator();
    m = match s.view {
        View::Playlist(id) => m.item(MenuItem::new(format!("Remove {songs} from Playlist")).on_select(move |s: &mut State| {
            close(s);
            remove_from_playlist(s, id);
        })),
        _ => m.item(MenuItem::new(format!("Remove {songs} from Library…")).danger(true).on_select(|s: &mut State| {
            close(s);
            s.dialog = Some(Dialog::ConfirmRemove(s.selected_tracks()));
        })),
    };
    fill(m.on_close(close).build(k, layer))
}

/// A context menu's trigger wraps its pane in a row of its own; it fills where the pane did.
fn fill(trigger: El) -> El {
    trigger.w_full().h_full().min_h(0.0).min_w(0.0)
}

pub fn add_to_playlist(s: &mut State, id: u64, ids: &[TrackId]) {
    if let Some(p) = s.lib.playlist_mut(id) {
        p.tracks.extend_from_slice(ids);
        s.changed();
        s.refresh();
    }
}

/// Takes the selected rows out of playlist `id`, by position, so one of two copies can go.
pub fn remove_from_playlist(s: &mut State, id: u64) {
    let sel: HashSet<TrackId> = s.selected_tracks().into_iter().collect();
    if let Some(p) = s.lib.playlist_mut(id) {
        p.tracks.retain(|t| !sel.contains(t));
    }
    s.table.clear_selection();
    s.changed();
    s.refresh();
}

fn party_menu(s: &State, k: &Kit, pane: El, at: Option<PointerPress>, layer: &mut Vec<El>) -> El {
    let sel = partyview::selected(s);
    let one = (sel.len() == 1).then(|| *sel.iter().next().expect("one"));
    let upcoming = one.is_some_and(|serial| s.lib.party.upcoming.iter().any(|e| e.serial == serial));
    context_menu("party-menu", pane, at)
        .min_width(250.0)
        .item(MenuItem::new("Play").disabled(one.is_none()).on_select(move |s: &mut State| {
            close(s);
            if let Some(serial) = one {
                s.play_party(serial);
            }
        }))
        .item(MenuItem::new("Play Next").disabled(!upcoming).on_select(move |s: &mut State| {
            close(s);
            if let Some(serial) = one
                && let Some(i) = s.lib.party.upcoming.iter().position(|e| e.serial == serial)
            {
                s.lib.party.shift(serial, -(i as isize));
                s.party_changed();
            }
        }))
        .item(MenuItem::new("Move Up").shortcut("Alt+Up").disabled(!upcoming).on_select(move |s: &mut State| {
            close(s);
            if let Some(serial) = one && s.lib.party.shift(serial, -1) {
                s.party_changed();
            }
        }))
        .item(MenuItem::new("Move Down").shortcut("Alt+Down").disabled(!upcoming).on_select(move |s: &mut State| {
            close(s);
            if let Some(serial) = one && s.lib.party.shift(serial, 1) {
                s.party_changed();
            }
        }))
        .separator()
        .item(MenuItem::new("Remove from Party Shuffle").shortcut("Delete").on_select(|s: &mut State| {
            close(s);
            let sel = partyview::selected(s);
            s.party_remove(&sel);
            s.table_party.clear_selection();
        }))
        .on_close(close)
        .build(k, layer)
        .w_full()
        .h_full()
        .min_h(0.0)
}

/// The right-click menu on a sidebar source.
pub fn source_menu(s: &State, k: &Kit, list: El, layer: &mut Vec<El>) -> El {
    let (at, view) = match s.menu {
        Some(Menu::Source { at, view }) => (Some(at), Some(view)),
        _ => (None, None),
    };
    let mut m = context_menu("source-menu", list, at).item(MenuItem::new("Play").on_select(move |s: &mut State| {
        close(s);
        if let Some(v) = view {
            s.select_view(v);
            if v == View::Party {
                s.start_party();
            } else {
                s.play_row(0);
            }
        }
    }));
    if let Some(View::Playlist(id)) = view {
        let used = s.lib.party.source == Source::Playlist(id);
        m = m
            .item(MenuItem::new("Use for Party Shuffle").checked(used).on_select(move |s: &mut State| {
                close(s);
                s.party_source(if used { Source::Library } else { Source::Playlist(id) });
            }))
            .separator()
            .item(MenuItem::new("Rename").on_select(move |s: &mut State| {
                close(s);
                sidebar::start_rename(s, id);
            }))
            .item(MenuItem::new("Delete Playlist").danger(true).on_select(move |s: &mut State| {
                close(s);
                sidebar::delete_playlist(s, id);
            }));
    }
    if view == Some(View::Party) {
        m = m.item(MenuItem::new("Refresh").on_select(|s: &mut State| {
            close(s);
            s.party_refresh();
        }));
    }
    fill(m.on_close(close).build(k, layer))
}

pub fn add_folder(_: &mut State, tx: &mut Tx) {
    let options = FileDialogOptions { title: "Add Folder to Library".into(), directory: crate::library::music_dir(), ..Default::default() };
    tx.open_folder(options, |s: &mut State, result, _| match result {
        Ok(Some(dir)) => s.import(vec![dir]),
        Ok(None) => {}
        Err(e) => s.notice = Some(format!("Couldn't open the folder chooser ({e}). Drop a folder on the window, or run `stereo <folder>`.")),
    });
}

pub fn app_menu(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let open = s.menu == Some(Menu::App);
    let theme = |label: &str, t: Theme| {
        MenuItem::new(label).checked(s.settings.theme == t).on_select(move |s: &mut State| {
            close(s);
            s.settings.theme = t;
            s.settings_changed();
        })
    };
    let trigger = IconButton::new(&icons::MENU).variant(IconButtonVariant::Ghost).label("Menu").help("Menu")
        .expanded(open).on_press(|s: &mut State| s.menu = if s.menu == Some(Menu::App) { None } else { Some(Menu::App) })
        .build(k).key("app-menu-button");
    rune_kit::Menu::new("app-menu").standard().min_width(240.0).trigger(trigger)
        .item(MenuItem::new("Add Folder to Library…").icon(&icons::FOLDER).shortcut("Ctrl+O").on_select_tx(|s: &mut State, tx: &mut Tx| {
            close(s);
            add_folder(s, tx);
        }))
        .item(MenuItem::new("Check Folders for New Songs").disabled(s.lib.folders.is_empty() || s.scan.is_some()).on_select(|s: &mut State| {
            close(s);
            let folders = s.lib.folders.clone();
            s.import(folders);
        }))
        .item(MenuItem::new("New Playlist").shortcut("Ctrl+N").on_select(|s: &mut State| {
            close(s);
            sidebar::new_playlist(s, Vec::new());
        }))
        .separator()
        .item(MenuItem::new("Show Browser").checked(s.settings.browser).shortcut("Ctrl+B").on_select(|s: &mut State| {
            close(s);
            s.settings.browser = !s.settings.browser;
            s.settings_changed();
            s.refresh();
        }))
        .item(MenuItem::new("Show Artwork").checked(s.settings.artwork).shortcut("Ctrl+G").on_select(|s: &mut State| {
            close(s);
            s.settings.artwork = !s.settings.artwork;
            s.settings_changed();
        }))
        .item(MenuItem::new("Go to Current Song").shortcut("Ctrl+L").disabled(s.now.is_none()).on_select_tx(|s: &mut State, tx: &mut Tx| {
            close(s);
            s.reveal_now(tx);
        }))
        .separator()
        .group_label("Appearance")
        .item(theme("Match System", Theme::System))
        .item(theme("Light", Theme::Light))
        .item(theme("Dark", Theme::Dark))
        .separator()
        .item(MenuItem::new("Keyboard Shortcuts").on_select(|s: &mut State| {
            close(s);
            s.dialog = Some(Dialog::Shortcuts);
        }))
        .item(MenuItem::new("About Stereo").on_select(|s: &mut State| {
            close(s);
            s.dialog = Some(Dialog::About);
        }))
        .open(open)
        .on_close(close)
        .build(k, layer)
}
