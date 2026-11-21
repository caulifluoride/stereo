//! The source list down the left: Music, Party Shuffle, the smart playlists and the user's
//! playlists, with the artwork pane under it. Songs dropped on a playlist join it; a
//! playlist renames in place on a double-click.

use rune::{Drag, El, Image, Phase, SurfaceFit, Tx, col, el, icon, row, text};
use rune_kit::shell::{Sidebar, SidebarRow, SidebarSection};
use rune_kit::{IconButton, IconButtonVariant, Kit};

use crate::menus::{self, Menu};
use crate::rows::{Smart, View};
use crate::{State, icons};

pub fn new_playlist(s: &mut State, tracks: Vec<crate::library::TrackId>) {
    let name = s.lib.untitled();
    let id = s.lib.new_playlist(&name, tracks);
    s.changed();
    s.select_view(View::Playlist(id));
    start_rename(s, id);
}

pub fn start_rename(s: &mut State, id: u64) {
    let Some(p) = s.lib.playlist(id) else { return };
    s.rename = rune::TextEdit::new(p.name.clone());
    s.rename.anchor = 0;
    s.rename.focused = true;
    s.renaming = Some(id);
}

pub fn delete_smart(s: &mut State, id: u64) {
    s.lib.smart.retain(|p| p.id != id);
    if s.lib.party.source == crate::party::Source::UserSmart(id) {
        s.party_source(crate::party::Source::Library);
    }
    if s.view == View::UserSmart(id) {
        s.select_view(View::Music);
    }
    s.changed();
}

pub fn delete_playlist(s: &mut State, id: u64) {
    s.lib.playlists.retain(|p| p.id != id);
    if s.lib.party.source == crate::party::Source::Playlist(id) {
        s.party_source(crate::party::Source::Library);
    }
    if s.view == View::Playlist(id) {
        s.select_view(View::Music);
    }
    s.changed();
}

fn source_row(s: &State, k: &Kit, v: View, label: String, i: &'static rune::Icon) -> El {
    let mut r = SidebarRow::new(label).icon(i).selected(s.view == v).on_select(move |s: &mut State, _| s.select_view(v));
    if let View::Playlist(id) = v {
        r = r.on_double_click(move |s: &mut State, _| start_rename(s, id));
        if s.renaming == Some(id) {
            r = r.renaming(
                &s.rename,
                |s: &mut State| &mut s.rename,
                |s: &mut State, _| {
                    let name = s.rename.text.trim().to_string();
                    if let Some(p) = s.renaming.take().and_then(|id| s.lib.playlist_mut(id))
                        && !name.is_empty()
                    {
                        p.name = name;
                        s.changed();
                    }
                },
                |s: &mut State, _| s.renaming = None,
            );
        }
    }
    if v == View::Party && s.now.is_some_and(|n| n.party) {
        r = r.trailing(icon(&icons::SPEAKER, 12.0).color(k.c.icon_muted));
    }
    let row = r.build(k).key(v.key()).on_secondary_press(move |s: &mut State, at, _| s.menu = Some(Menu::Source { at, view: v }));
    match v {
        // Songs dragged from Finder or a file manager onto a playlist join it.
        View::Playlist(id) => row
            .accepts_files(|p| crate::library::is_audio(p) || p.is_dir())
            .on_drop_files(move |s: &mut State, files, _| drop_on_playlist(s, id, files)),
        _ => row,
    }
}

/// Files dropped on a playlist: those already in the library join it now; new ones are
/// imported and added once read. A folder's songs join as they're found.
fn drop_on_playlist(s: &mut State, id: u64, files: &[std::path::PathBuf]) {
    let mut all = Vec::new();
    for f in files {
        if f.is_dir() {
            let mut inside: Vec<_> = walkdir::WalkDir::new(f).follow_links(true).into_iter().filter_map(Result::ok)
                .filter(|e| e.file_type().is_file() && crate::library::is_audio(e.path())).map(|e| e.into_path()).collect();
            inside.sort();
            all.extend(inside);
        } else {
            all.push(f.clone());
        }
    }
    s.import_into(id, all);
}

fn artwork(s: &State, k: &Kit, w: f32, img: &Image) -> El {
    let caption = match (s.art_track(), s.now) {
        (Some(a), Some(n)) if a == n.track => "Now Playing",
        (Some(_), _) => "Selected Song",
        (None, _) => "Nothing Playing",
    };
    // The pane's padding and the resize grip beside it.
    let side = (w - 21.0).max(60.0);
    let picture = el().key("art-image").size(side, side).radius(4.0).pixels_fit(img.pixels(), SurfaceFit::Fill);
    col().key("artwork").w_full().px(8.0).pb(8.0).gap(6.0)
        .child(text(caption).font(k.ty.text_xsmall_medium).color(k.c.text_muted))
        .child(picture.border(1.0, k.c.border_weak))
}

pub fn view(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let w = s.settings.sidebar;
    let mut playlists = SidebarSection::new("Playlists").key("sec-playlists")
        .collapsed(s.closed.contains("playlists"))
        .on_toggle(|s: &mut State, _| toggle_section(s, "playlists"))
        .action(
            IconButton::new(&icons::PLUS).variant(IconButtonVariant::GhostMuted).label("New playlist").help("New playlist (Ctrl+N)")
                .on_press(|s: &mut State| new_playlist(s, Vec::new())).build(k).key("add-playlist"),
        )
        .row(source_row(s, k, View::Party, "Party Shuffle".into(), &icons::PARTY));
    for sm in Smart::ALL {
        playlists = playlists.row(source_row(s, k, View::Smart(sm), sm.name(&s.settings.smart), &icons::GEAR));
    }
    for p in &s.lib.smart {
        playlists = playlists.row(source_row(s, k, View::UserSmart(p.id), p.name.clone(), &icons::GEAR));
    }
    for p in &s.lib.playlists {
        playlists = playlists.row(source_row(s, k, View::Playlist(p.id), p.name.clone(), &icons::LIST));
    }
    let mut library = SidebarSection::new("Library").key("sec-library")
        .collapsed(s.closed.contains("library"))
        .on_toggle(|s: &mut State, _| toggle_section(s, "library"))
        .row(source_row(s, k, View::Music, "Music".into(), &icons::NOTE));
    // These show while they have something in them, or while they're the list showing.
    if !s.up_next.is_empty() || s.view == View::UpNext {
        library = library.row(source_row(s, k, View::UpNext, format!("Up Next ({})", s.up_next.len()), &icons::UP_NEXT));
    }
    if !s.missing.is_empty() || s.view == View::Missing {
        library = library.row(source_row(s, k, View::Missing, format!("Missing Files ({})", s.missing.len()), &icons::WARN));
    }
    if s.view == View::Duplicates {
        library = library.row(source_row(s, k, View::Duplicates, "Duplicates".into(), &icons::DUPES));
    }
    let mut bar = Sidebar::new().label("Sources").section(library).section(playlists);
    // A song without artwork shows no pane, rather than an empty square.
    if let (true, Some(img)) = (s.settings.artwork, &s.art.image) {
        bar = bar.footer(artwork(s, k, w, img));
    }
    let list = menus::source_menu(s, k, bar.build(k), layer);
    let grip = el().key("sidebar-grip").w(5.0).h_full().cursor(rune::Cursor::ResizeColumn)
        .on_drag(|s: &mut State, d: &mut Drag, tx: &mut Tx| match d.phase {
            Phase::Down => d.data = s.settings.sidebar,
            Phase::Move => {
                s.settings.sidebar = (d.data + d.dx).clamp(150.0, 420.0).round();
                tx.instant();
            }
            Phase::Up => s.settings_changed(),
        })
        .on_double_press(|s: &mut State| {
            s.settings.sidebar = crate::settings::Settings::default().sidebar;
            s.settings_changed();
        });
    row().key("sidebar-pane").w(w).h_full()
        .fill(k.c.layer_01)
        .border_sides([0.0, 1.0, 0.0, 0.0], k.c.border_weak)
        .child(col().key("sidebar-col").grow(1.0).min_w(0.0).h_full().child(list))
        .child(grip)
}

fn toggle_section(s: &mut State, name: &'static str) {
    if !s.closed.remove(name) {
        s.closed.insert(name);
    }
}
