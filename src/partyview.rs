//! The Party Shuffle list and the controls under it, laid out as iTunes had them: where songs
//! come from, how many recently played and upcoming songs show, whether higher-rated songs
//! play more often, and Refresh.

use std::borrow::Cow;
use std::collections::HashSet;

use rune::{Align, El, Tx, col, icon, row, spacer, text};
use rune_kit::{Button, ButtonVariant, Cell, Checkbox, ControlSize, Kit, Select, Table, TableColumn, TableStyle};

use crate::lists::{cell_text, now_mark, rate, stars};
use crate::menus::Menu;
use crate::party::{Place, RECENT, Source, UPCOMING};
use crate::{State, icons, lcd};

fn party_menu(s: &mut State, serial: u64, at: rune::PointerPress) {
    let key = serial.to_string();
    if !s.table_party.is_selected(&key) {
        s.table_party.select(key);
    }
    s.menu = Some(Menu::Party { at });
    s.request_art();
}

/// The serials of the entries selected.
pub fn selected(s: &State) -> HashSet<u64> {
    s.table_party.selected().filter_map(|id| id.parse().ok()).collect()
}

fn table(s: &State, k: &Kit) -> El {
    let style = TableStyle::from_kit(k);
    let kit = k.clone();
    Table::new(|s: &State| &s.table_party, |s: &mut State| &mut s.table_party, |s: &State| s.party_rows.len(), |s: &State, i| Cow::Owned(s.party_rows[i].0.serial.to_string()))
        .name("Party Shuffle")
        .column(TableColumn::new("now", "").width(26.0).resizable(false))
        .column(TableColumn::new("name", "Name").flex(2.2).min(140.0))
        .column(TableColumn::new("time", "Time").width(58.0).align(Align::End))
        .column(TableColumn::new("artist", "Artist").flex(1.4).min(90.0))
        .column(TableColumn::new("album", "Album").flex(1.4).min(90.0))
        .column(TableColumn::new("rating", "Rating").width(92.0).resizable(false))
        .cell(move |s: &State, i, col| {
            let (e, place) = s.party_rows[i];
            let Some(t) = s.lib.get(e.track) else { return Cell::text("") };
            let serial = e.serial;
            let menu = move |s: &mut State, p| party_menu(s, serial, p);
            // Played songs fade back, as in iTunes.
            let (base, muted) = if place == Place::Played { (style.muted.alpha(0.75), style.muted.alpha(0.6)) } else { (style.text, style.muted) };
            match col {
                "now" if place == Place::Current => now_mark(s, &kit, e.track),
                "now" => Cell::text(""),
                "name" => cell_text(&style, &t.title, base, Align::Start, menu),
                "time" => cell_text(&style, lcd::clock(t.duration), muted, Align::End, menu),
                "artist" => cell_text(&style, &t.artist, base, Align::Start, menu),
                "album" => cell_text(&style, &t.album, muted, Align::Start, menu),
                "rating" => {
                    let id = e.track;
                    Cell::el(stars(&kit, t.rating, move |s: &mut State, n| {
                        rate(s, id, n);
                    }))
                }
                _ => Cell::text(""),
            }
        })
        .on_select(|s: &mut State, _, _| s.request_art())
        .on_open(|s: &mut State, id, _| {
            if let Ok(serial) = id.parse() {
                s.play_party(serial);
            }
        })
        .build(s, k)
        .grow(1.0)
        .min_h(0.0)
}

fn count_select(s: &State, k: &Kit, layer: &mut Vec<El>, key: &'static str, options: &'static [usize], value: usize, set: fn(&mut State, usize)) -> El {
    let labels: Vec<String> = options.iter().map(|n| n.to_string()).collect();
    let current = value.to_string();
    let open = s.menu == Some(Menu::PartyCount(key));
    Select::new(key, &labels)
        .label(if key == "recent" { "Recently played songs shown" } else { "Upcoming songs shown" })
        .current(Some(&current))
        .open(open)
        .on_toggle(move |s: &mut State| s.menu = if s.menu == Some(Menu::PartyCount(key)) { None } else { Some(Menu::PartyCount(key)) })
        .on_close(|s: &mut State| s.menu = None)
        .width(64.0)
        .on_choose(move |s: &mut State, v: &str, _: &mut Tx| {
            s.menu = None;
            if let Ok(n) = v.parse() {
                set(s, n);
            }
        })
        .build(k, layer)
}

fn source_select(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let mut ids = vec!["library".to_string()];
    let mut labels = vec!["Library".to_string()];
    for p in &s.lib.playlists {
        ids.push(format!("pl{}", p.id));
        labels.push(p.name.clone());
    }
    let current = match s.lib.party.source {
        Source::Library => "library".to_string(),
        Source::Playlist(id) => format!("pl{id}"),
    };
    let current_label = ids.iter().position(|i| *i == current).map(|i| labels[i].clone()).unwrap_or_else(|| "Library".into());
    let open = s.menu == Some(Menu::PartyCount("source"));
    Select::new("party-source", &labels)
        .label("Party Shuffle source")
        .current(Some(&current_label))
        .open(open)
        .on_toggle(|s: &mut State| s.menu = if s.menu == Some(Menu::PartyCount("source")) { None } else { Some(Menu::PartyCount("source")) })
        .on_close(|s: &mut State| s.menu = None)
        .width(180.0)
        .on_choose(move |s: &mut State, v: &str, _: &mut Tx| {
            s.menu = None;
            let src = s.lib.playlists.iter().find(|p| p.name == v).map_or(Source::Library, |p| Source::Playlist(p.id));
            s.party_source(src);
        })
        .build(k, layer)
}

fn controls(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let label = |t: &'static str| text(t).font(k.ty.text_small).color(k.c.text_muted);
    let p = &s.lib.party;
    let line1 = row().key("party-source-row").gap(8.0).center_y()
        .child(label("Source:"))
        .child(source_select(s, k, layer))
        .child(Checkbox::new(p.prefer_rated).label("Play higher rated songs more often").on_toggle(|s: &mut State| {
            s.lib.party.prefer_rated = !s.lib.party.prefer_rated;
            s.changed();
        }).build(k).key("prefer-rated"));
    let line2 = row().key("party-counts").gap(8.0).center_y()
        .child(label("Display:"))
        .child(count_select(s, k, layer, "recent", &RECENT, p.recent_len, |s, n| {
            s.lib.party.recent_len = n;
            s.party_changed();
        }))
        .child(label("recently played songs, and"))
        .child(count_select(s, k, layer, "upcoming", &UPCOMING, p.upcoming_len, |s, n| {
            let pool = s.party_pool();
            s.lib.party.set_upcoming_len(n, &pool, &mut s.rng);
            s.party_changed();
        }))
        .child(label("upcoming songs"));
    let refresh = Button::new("Refresh").icon(&icons::REFRESH).variant(ButtonVariant::Outline).control_size(ControlSize::Small)
        .help("Pick the upcoming songs afresh")
        .on_press(|s: &mut State| s.party_refresh()).build(k).key("party-refresh");
    row().key("party-controls").w_full().px(14.0).py(10.0).gap(16.0).center_y()
        .fill(k.c.layer_01)
        .border_sides([1.0, 0.0, 0.0, 0.0], k.c.border_weak)
        .child(col().gap(8.0).child(line1).child(line2))
        .child(spacer())
        .child(refresh)
}

fn empty(k: &Kit) -> El {
    col().key("empty").w_full().grow(1.0).center().gap(6.0)
        .child(icon(&icons::PARTY, 40.0).color(k.c.icon_faint))
        .child(text("Nothing to shuffle yet").font(k.ty.body_medium).color(k.c.text_base))
        .child(text("Party Shuffle picks from your library, or from the playlist chosen below.").font(k.ty.ui).color(k.c.text_muted))
}

pub fn view(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let head = row().key("party-head").w_full().px(14.0).py(8.0).gap(8.0).center_y()
        .border_sides([0.0, 0.0, 1.0, 0.0], k.c.border_weak)
        .child(icon(&icons::PARTY, 16.0).color(k.c.icon_base))
        .child(text("Party Shuffle").font(k.ty.body_medium).color(k.c.text_base))
        .child(text("Songs are picked as you go. Right-click songs anywhere and choose Play Next or Add to Party Shuffle.").font(k.ty.text_small).color(k.c.text_muted).truncate().shrink().min_w(0.0));
    let body = if s.party_rows.is_empty() { empty(k) } else { table(s, k) };
    col().key("party-pane").w_full().h_full()
        .child(head)
        .child(body)
        .child(controls(s, k, layer))
}
