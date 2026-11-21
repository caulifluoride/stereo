//! The track list for Music, the smart playlists and playlists, the column browser over it,
//! and the status bar under everything.

use std::borrow::Cow;

use rune::{Align, Color, El, PointerPress, col, el, icon, row, spacer, text};
use rune_kit::{Cell, Kit, Table, TableColumn, TableStyle};

use crate::library::{Track, TrackId};
use crate::menus::Menu;
use crate::rows::View;
use crate::{State, icons, lcd};

/// Selects row `id` under a right-click (unless it's already part of the selection) and opens
/// the track menu there.
fn track_menu(s: &mut State, id: TrackId, at: PointerPress) {
    let key = id.to_string();
    if !s.table.is_selected(&key) {
        s.table.select(key);
    }
    s.menu = Some(Menu::Tracks { at });
    s.request_art();
}

/// A text cell that opens the track menu on a right-click anywhere in it.
pub fn cell_text(style: &TableStyle, t: impl Into<String>, color: Color, align: Align, menu: impl Fn(&mut State, PointerPress) + 'static) -> Cell {
    let t = t.into();
    let label = text(t.clone()).font(style.font).color(color).align(align).truncate().shrink().min_w(0.0).help_when_truncated(t);
    let mut r = row().w_full().h_full().center_y().child(label);
    if align == Align::End {
        r = r.justify_end();
    }
    Cell::el(r.on_secondary_press(move |s: &mut State, p, _| menu(s, p)))
}

/// Five stars, the ones up to the rating lit. A click sets the rating; clicking the rating it
/// already has clears it.
pub fn stars(k: &Kit, rating: u8, set: impl Fn(&mut State, u8) + Clone + 'static) -> El {
    let mut r = row().gap(1.0).center_y().role("group").label(format!("{rating} stars"));
    for n in 1..=5u8 {
        let lit = n <= rating;
        let set = set.clone();
        r = r.child(
            el().key(format!("star{n}")).size(13.0, 16.0).center()
                .child(icon(if lit { &icons::STAR } else { &icons::DOT }, if lit { 11.0 } else { 12.0 }).color(if lit { k.c.text_base } else { k.c.icon_faint }))
                .on_press(move |s: &mut State| set(s, if rating == n { 0 } else { n })),
        );
    }
    r
}

pub fn rate(s: &mut State, id: TrackId, n: u8) {
    if let Some(t) = s.lib.get_mut(id) {
        t.rating = n;
        s.changed();
        s.refresh();
    }
}

/// The speaker beside the song playing.
pub fn now_mark(s: &State, k: &Kit, id: TrackId) -> Cell {
    match s.now.filter(|n| n.track == id) {
        Some(n) => Cell::el(icon(if n.playing { &icons::SPEAKER } else { &icons::SPEAKER_OFF }, 13.0).color(k.c.text_accent)),
        None => Cell::text(""),
    }
}

fn year(t: &Track) -> String {
    t.year.map(|y| y.to_string()).unwrap_or_default()
}

fn plays(t: &Track) -> String {
    if t.plays == 0 { String::new() } else { t.plays.to_string() }
}

fn tracks_table(s: &State, k: &Kit) -> El {
    let style = TableStyle::from_kit(k);
    let kit = k.clone();
    let playlist = matches!(s.view, View::Playlist(_));
    let mut t = Table::new(|s: &State| &s.table, |s: &mut State| &mut s.table, |s: &State| s.rows.len(), |s: &State, i| Cow::Owned(s.rows[i].to_string()))
        .name("Songs")
        .column(TableColumn::new("now", "").width(26.0).resizable(false));
    if playlist {
        t = t.column(TableColumn::new("pos", "#").width(36.0).align(Align::End).resizable(false));
    }
    t = t
        .column(TableColumn::new("name", "Name").flex(2.2).min(140.0).sortable())
        .column(TableColumn::new("time", "Time").width(58.0).min(48.0).max(90.0).align(Align::End).sortable())
        .column(TableColumn::new("artist", "Artist").flex(1.4).min(90.0).sortable())
        .column(TableColumn::new("album", "Album").flex(1.4).min(90.0).sortable())
        .column(TableColumn::new("genre", "Genre").flex(0.8).min(70.0).sortable())
        .column(TableColumn::new("year", "Year").width(60.0).align(Align::End).sortable())
        .column(TableColumn::new("rating", "Rating").width(92.0).resizable(false).sortable())
        .column(TableColumn::new("plays", "Plays").width(54.0).align(Align::End).sortable());
    t.cell(move |s: &State, i, col| {
        let id = s.rows[i];
        let Some(t) = s.lib.get(id) else { return Cell::text("") };
        let menu = move |s: &mut State, p| track_menu(s, id, p);
        let (base, muted) = (style.text, style.muted);
        match col {
            "now" => now_mark(s, &kit, id),
            "pos" => cell_text(&style, (i + 1).to_string(), muted, Align::End, menu),
            "name" => cell_text(&style, &t.title, base, Align::Start, menu),
            "time" => cell_text(&style, lcd::clock(t.duration), muted, Align::End, menu),
            "artist" => cell_text(&style, &t.artist, base, Align::Start, menu),
            "album" => cell_text(&style, &t.album, base, Align::Start, menu),
            "genre" => cell_text(&style, &t.genre, muted, Align::Start, menu),
            "year" => cell_text(&style, year(t), muted, Align::End, menu),
            "plays" => cell_text(&style, plays(t), muted, Align::End, menu),
            "rating" => Cell::el(stars(&kit, t.rating, move |s: &mut State, n| rate(s, id, n))),
            _ => Cell::text(""),
        }
    })
    .on_sort(|s: &mut State, _, _, _| s.refresh())
    .on_select(|s: &mut State, _, _| s.request_art())
    .on_open(|s: &mut State, id, _| {
        if let Some(i) = id.parse().ok().and_then(|id: TrackId| s.rows.iter().position(|&r| r == id)) {
            s.play_row(i);
        }
    })
    .zebra(true)
    .build(s, k)
    .grow(1.0)
    .min_h(0.0)
}

/// One of the browser's three lists: "All (n Genres)" over its names.
fn browse_column(s: &State, k: &Kit, which: Which) -> El {
    let style = TableStyle::from_kit(k);
    let table = match which {
        Which::Genre => Table::new(|s: &State| &s.table_genre, |s: &mut State| &mut s.table_genre, |s: &State| s.browser.genres.len() + 1, |s: &State, i| browse_id(&s.browser.genres, i)),
        Which::Artist => Table::new(|s: &State| &s.table_artist, |s: &mut State| &mut s.table_artist, |s: &State| s.browser.artists.len() + 1, |s: &State, i| browse_id(&s.browser.artists, i)),
        Which::Album => Table::new(|s: &State| &s.table_album, |s: &mut State| &mut s.table_album, |s: &State| s.browser.albums.len() + 1, |s: &State, i| browse_id(&s.browser.albums, i)),
    };
    let (title, plural) = match which {
        Which::Genre => ("Genre", "Genres"),
        Which::Artist => ("Artist", "Artists"),
        Which::Album => ("Album", "Albums"),
    };
    table.name(title)
        .column(TableColumn::new("v", title).flex(1.0).resizable(false))
        .cell(move |s: &State, i, _| {
            let list = which.list(s);
            if i == 0 {
                let n = list.len();
                Cell::text(format!("All ({n} {})", if n == 1 { &plural[..plural.len() - 1] } else { plural })).color(style.text)
            } else {
                Cell::text(list[i - 1].clone())
            }
        })
        .on_select(move |s: &mut State, id, _| {
            let pick = id.strip_prefix('v').map(str::to_string);
            let slot = match which {
                Which::Genre => &mut s.browser.genre,
                Which::Artist => &mut s.browser.artist,
                Which::Album => &mut s.browser.album,
            };
            if *slot != pick {
                *slot = pick;
                s.table.clear_selection();
                s.refresh();
            }
        })
        .build(s, k)
        .grow(1.0)
        .min_w(0.0)
        .h_full()
}

fn browse_id(list: &[String], i: usize) -> Cow<'_, str> {
    if i == 0 { Cow::Borrowed("all") } else { Cow::Owned(format!("v{}", list[i - 1])) }
}

#[derive(Clone, Copy)]
enum Which {
    Genre,
    Artist,
    Album,
}

impl Which {
    fn list(self, s: &State) -> &[String] {
        match self {
            Which::Genre => &s.browser.genres,
            Which::Artist => &s.browser.artists,
            Which::Album => &s.browser.albums,
        }
    }
}

fn empty(k: &Kit, s: &State) -> El {
    let (title, body) = match s.view {
        View::Music if s.scan.is_some() => ("Importing your music…".to_string(), format!("{} songs so far", s.scanned)),
        View::Music if s.search.text.is_empty() => (
            "Your library is empty".into(),
            "Drop a folder of music here, or choose Add Folder to Library from the menu at the top right.".into(),
        ),
        View::Playlist(_) if s.search.text.is_empty() => ("This playlist is empty".into(), "Drag songs here, or right-click songs in Music and choose Add to Playlist.".into()),
        View::Smart(_) if s.search.text.is_empty() => ("Nothing here yet".into(), "This list fills itself as you play and rate songs.".into()),
        _ => ("No matches".into(), format!("Nothing in {} matches “{}”.", s.view.title(&s.lib), s.search.text.trim())),
    };
    col().key("empty").w_full().grow(1.0).center().gap(6.0).px(40.0)
        .child(icon(&icons::NOTE, 40.0).color(k.c.icon_faint))
        .child(text(title).font(k.ty.body_medium).color(k.c.text_base))
        .child(text(body).font(k.ty.ui).color(k.c.text_muted).wrap().align(Align::Center).max_w(420.0))
}

pub fn view(s: &State, k: &Kit) -> El {
    let mut out = col().key("list-pane").w_full().h_full();
    if s.view == View::Music && s.settings.browser && !s.lib.tracks.is_empty() {
        out = out.child(
            row().key("browser").w_full().h(170.0).gap(1.0).fill(k.c.border_weak)
                .border_sides([0.0, 0.0, 1.0, 0.0], k.c.border_weak)
                .child(col().key("b-genre").grow(1.0).min_w(0.0).h_full().fill(k.c.bg_base).child(browse_column(s, k, Which::Genre)))
                .child(col().key("b-artist").grow(1.0).min_w(0.0).h_full().fill(k.c.bg_base).child(browse_column(s, k, Which::Artist)))
                .child(col().key("b-album").grow(1.0).min_w(0.0).h_full().fill(k.c.bg_base).child(browse_column(s, k, Which::Album))),
        );
    }
    if s.rows.is_empty() {
        return out.child(empty(k, s));
    }
    out.child(tracks_table(s, k))
}

fn bytes(n: u64) -> String {
    let n = n as f64;
    if n >= 1e9 { format!("{:.2} GB", n / 1e9) } else { format!("{:.1} MB", n / 1e6) }
}

/// "1,234 songs, 3.2 days, 5.12 GB", as iTunes summed up a list.
pub fn summary(ts: &[&Track]) -> String {
    let n = ts.len();
    if n == 0 {
        return String::new();
    }
    let secs: f32 = ts.iter().map(|t| t.duration).sum();
    let size: u64 = ts.iter().map(|t| t.size).sum();
    let time = if secs >= 86400.0 {
        format!("{:.1} days", secs / 86400.0)
    } else if secs >= 3600.0 {
        format!("{:.1} hours", secs / 3600.0)
    } else {
        format!("{} minutes", (secs / 60.0).round())
    };
    let count = thousands(n);
    let mut out = format!("{count} {}, {time}", if n == 1 { "song" } else { "songs" });
    if size > 0 {
        out.push_str(", ");
        out.push_str(&bytes(size));
    }
    out
}

fn thousands(n: usize) -> String {
    let d = n.to_string();
    d.chars().enumerate().fold(String::new(), |mut out, (i, ch)| {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
        out
    })
}

pub fn status_bar(s: &State, k: &Kit) -> El {
    let ids: Vec<TrackId> = if s.view == View::Party { s.party_rows.iter().map(|(e, _)| e.track).collect() } else { s.rows.clone() };
    let ts: Vec<&Track> = ids.iter().filter_map(|&id| s.lib.get(id)).collect();
    let mut middle = summary(&ts);
    if s.scan.is_some() {
        middle = format!("Importing… {} songs found", s.scanned);
    }
    let repeat_icon = if s.settings.repeat == crate::queue::Repeat::One { &icons::REPEAT_ONE } else { &icons::REPEAT };
    let repeat_tip = match s.settings.repeat {
        crate::queue::Repeat::Off => "Repeat: off",
        crate::queue::Repeat::All => "Repeat: all",
        crate::queue::Repeat::One => "Repeat: one",
    };
    let left = row().key("status-left").gap(2.0).center_y().w(220.0)
        .child(lcd::toggle(k, "new-playlist", &icons::PLUS, false, "New playlist (Ctrl+N)", |s: &mut State| crate::sidebar::new_playlist(s, Vec::new())))
        .child(lcd::toggle(k, "shuffle", &icons::SHUFFLE, s.settings.shuffle, "Shuffle", |s: &mut State| s.toggle_shuffle()))
        .child(lcd::toggle(k, "repeat", repeat_icon, s.settings.repeat != crate::queue::Repeat::Off, repeat_tip, |s: &mut State| {
            s.settings.repeat = s.settings.repeat.next();
            s.settings_changed();
        }))
        .child(lcd::toggle(k, "artwork", &icons::ARTWORK, s.settings.artwork, "Show artwork", |s: &mut State| {
            s.settings.artwork = !s.settings.artwork;
            s.settings_changed();
        }));
    let right = row().key("status-right").w(220.0).justify_end().center_y()
        .child(lcd::toggle(k, "browse", &icons::BROWSER, s.settings.browser, "Show browser (Ctrl+B)", |s: &mut State| {
            s.settings.browser = !s.settings.browser;
            s.settings_changed();
            s.refresh();
        }));
    row().key("statusbar").w_full().h(34.0).px(8.0).center_y()
        .fill(k.c.page)
        .border_sides([1.0, 0.0, 0.0, 0.0], k.c.border_weak)
        .child(left)
        .child(spacer())
        .child(text(middle).key("summary").font(k.ty.text_xsmall).color(k.c.text_muted))
        .child(spacer())
        .child(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_summary_reads_like_itunes() {
        let t = |d: f32, size: u64| Track { duration: d, size, ..Track::default() };
        let a = t(200.0, 4_000_000);
        assert_eq!(summary(&[&a]), "1 song, 3 minutes, 4.0 MB");
        let long: Vec<Track> = (0..1500).map(|_| t(240.0, 5_000_000)).collect();
        let refs: Vec<&Track> = long.iter().collect();
        assert_eq!(summary(&refs), "1,500 songs, 4.2 days, 7.50 GB");
    }
}
