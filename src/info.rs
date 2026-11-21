//! Get Info: a song's tags (or several songs' at once) to read and change, its artwork to
//! replace or take out, and where its file is. With several songs, a field they disagree on
//! shows empty with "Mixed" in it, and only fields that are typed in change. The library
//! takes changes at once; a worker writes them into the files.

use std::collections::HashSet;

use rune::{El, FileDialogOptions, TextEdit, Tx, col, row, spacer, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Dialog as KitDialog, Kit, TextInput};

use crate::dialogs::Dialog;
use crate::library::{TagEdit, Track, TrackId, write_tags};
use crate::rows::View;
use crate::{Job, State, lcd, partyview};

const FIELDS: [&str; 8] = ["Name", "Artist", "Album", "Album Artist", "Genre", "Year", "Track", "Disc"];

pub struct Draft {
    pub ids: Vec<TrackId>,
    pub edits: [TextEdit; 8],
    /// What each field held when the window opened (empty where the songs disagree).
    original: [String; 8],
    mixed: [bool; 8],
    /// New artwork for every song, or `Some(None)` to take it out.
    cover: Option<Option<Vec<u8>>>,
}

fn values(t: &Track) -> [String; 8] {
    let n = |v: Option<u32>| v.map(|v| v.to_string()).unwrap_or_default();
    [t.title.clone(), t.artist.clone(), t.album.clone(), t.album_artist.clone(), t.genre.clone(), n(t.year), n(t.track_no), n(t.disc_no)]
}

/// Opens Get Info on the songs selected.
pub fn open(s: &mut State) {
    let ids = if s.view == View::Party {
        let sel = partyview::selected(s);
        s.party_rows.iter().filter(|(e, _)| sel.contains(&e.serial)).map(|(e, _)| e.track).collect()
    } else {
        s.selected_tracks()
    };
    let ts: Vec<&Track> = ids.iter().filter_map(|&id| s.lib.get(id)).collect();
    let Some(first) = ts.first() else { return };
    let mut original = values(first);
    let mut mixed = [false; 8];
    for t in &ts[1..] {
        for (i, v) in values(t).into_iter().enumerate() {
            if v != original[i] {
                mixed[i] = true;
            }
        }
    }
    for (i, m) in mixed.iter().enumerate() {
        if *m {
            original[i].clear();
        }
    }
    let edits = original.clone().map(TextEdit::new);
    s.menu = None;
    s.info = Some(Draft { ids, edits, original, mixed, cover: None });
    s.dialog = Some(Dialog::Info);
}

fn close(s: &mut State) {
    s.info = None;
    s.dialog = None;
}

fn number(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() { Some(0) } else { s.split('/').next()?.trim().parse().ok() }
}

/// The changes the draft makes: the fields typed in, and the artwork.
fn edit(d: &Draft) -> TagEdit {
    let changed = |i: usize| {
        let t = d.edits[i].text.trim();
        let was = d.original[i].trim();
        // A mixed field changes only when something is typed in it.
        (if d.mixed[i] { !t.is_empty() } else { t != was }).then(|| t.to_string())
    };
    let num = |i: usize| changed(i).and_then(|v| number(&v));
    TagEdit {
        title: changed(0).filter(|t| !t.is_empty()),
        artist: changed(1),
        album: changed(2),
        album_artist: changed(3),
        genre: changed(4),
        year: num(5),
        track_no: num(6),
        disc_no: num(7),
        cover: d.cover.clone(),
    }
}

fn save(s: &mut State) {
    let Some(d) = s.info.take() else { return };
    s.dialog = None;
    let e = edit(&d);
    if e.is_empty() {
        return;
    }
    let mut files = Vec::new();
    for &id in &d.ids {
        if let Some(t) = s.lib.get_mut(id) {
            e.apply(t);
            files.push(t.path.clone());
        }
    }
    s.changed();
    s.refresh();
    let (Some(wake), jobs) = (s.waker.clone(), s.jobs.clone()) else { return };
    let _ = rune::task::spawn("stereo-tags", move || {
        let failed: Vec<String> = files.iter().filter_map(|p| write_tags(p, &e).err().map(|why| format!("{}: {why}", p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()))).collect();
        let _ = jobs.send(Job::TagsWritten(failed));
        wake();
    });
}

fn choose_art(_: &mut State, tx: &mut Tx) {
    let options = FileDialogOptions { title: "Choose Artwork".into(), ..Default::default() }.filter("Images", &["jpg", "jpeg", "png", "webp", "gif", "bmp"]);
    tx.open_file(options, |s: &mut State, result, _| match result {
        Ok(Some(path)) => match std::fs::read(&path) {
            Ok(b) => {
                if let Some(d) = &mut s.info {
                    d.cover = Some(Some(b));
                }
            }
            Err(e) => s.notice = Some(format!("Couldn't read {}: {e}", path.display())),
        },
        Ok(None) => {}
        Err(e) => s.notice = Some(format!("Couldn't open the file chooser: {e}")),
    });
}

pub fn layer(s: &State, k: &Kit, layer: &mut Vec<El>) {
    let Some(d) = &s.info else { return };
    let n = d.ids.len();
    let title = if n == 1 { s.lib.get(d.ids[0]).map_or_else(|| "Get Info".into(), |t| t.title.clone()) } else { format!("{n} Songs") };
    let mut form = col().key("info-fields").w_full().gap(8.0);
    for (i, name) in FIELDS.iter().enumerate() {
        let width = if i >= 5 { 80.0 } else { 320.0 };
        let input = TextInput::new(&d.edits[i])
            .label(*name)
            .placeholder(if d.mixed[i] { "Mixed" } else { "" })
            .width(Some(width))
            .on_edit(move |s: &mut State| match s.info.as_mut() {
                Some(d) => &mut d.edits[i],
                None => &mut s.scratch,
            })
            .on_submit(|s: &mut State, _| save(s))
            .build(k)
            .key(format!("info-{i}"));
        form = form.child(row().w_full().gap(12.0).center_y()
            .child(text(*name).font(k.ty.ui).color(k.c.text_muted).w(96.0).align(rune::Align::End))
            .child(input));
    }
    let art_state = match &d.cover {
        None => "Artwork stays as it is.",
        Some(Some(_)) => "New artwork will be saved.",
        Some(None) => "The artwork will be taken out.",
    };
    let art = row().key("info-art").w_full().gap(8.0).center_y()
        .child(text("Artwork").font(k.ty.ui).color(k.c.text_muted).w(96.0).align(rune::Align::End))
        .child(Button::new("Choose…").variant(ButtonVariant::Outline).control_size(ControlSize::Small).on_press_tx(choose_art).build(k).key("info-choose"))
        .child(Button::new("Remove").variant(ButtonVariant::Outline).control_size(ControlSize::Small).on_press(|s: &mut State| {
            if let Some(d) = &mut s.info {
                d.cover = Some(None);
            }
        }).build(k).key("info-remove"))
        .child(text(art_state).font(k.ty.small).color(k.c.text_muted));
    form = form.child(art);
    if n == 1
        && let Some(t) = s.lib.get(d.ids[0])
    {
        let kind = t.path.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default();
        let facts = format!("{kind}, {}, {:.1} MB · played {} times, skipped {}", lcd::clock(t.duration), t.size as f64 / 1e6, t.plays, t.skips);
        form = form
            .child(row().w_full().gap(12.0).child(text("Kind").font(k.ty.ui).color(k.c.text_muted).w(96.0).align(rune::Align::End)).child(text(facts).font(k.ty.small).color(k.c.text_muted).wrap().grow(1.0).min_w(0.0)))
            .child(row().w_full().gap(12.0).child(text("Where").font(k.ty.ui).color(k.c.text_muted).w(96.0).align(rune::Align::End)).child(text(t.path.display().to_string()).font(k.ty.small).color(k.c.text_muted).wrap().grow(1.0).min_w(0.0)));
    }
    let ids: HashSet<TrackId> = d.ids.iter().copied().collect();
    let missing = ids.iter().any(|id| s.missing.contains(id));
    let buttons = row().key("info-buttons").w_full().gap(8.0).pt(6.0)
        .child(if missing { text("Some of these files are missing; their tags change in the library only.").font(k.ty.small).color(k.c.text_muted) } else { spacer() })
        .child(spacer())
        .child(Button::new("Cancel").variant(ButtonVariant::Outline).on_press(close).build(k).key("info-cancel"))
        .child(Button::new("Save").variant(ButtonVariant::Contrast).on_press(save).build(k).key("info-save"));
    KitDialog::new("info", title)
        .description(if n == 1 { "Changes are saved into the song's file." } else { "Fields you type in change every song; the others stay as they are." })
        .child(col().w_full().px(16.0).pb(16.0).gap(10.0).child(form).child(buttons))
        .width(520.0)
        .fit()
        .open(true)
        .on_close(close)
        .build(k, layer);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(original: [&str; 8], mixed: [bool; 8], now: [&str; 8]) -> Draft {
        Draft { ids: vec![1], edits: now.map(TextEdit::new), original: original.map(String::from), mixed, cover: None }
    }

    #[test]
    fn only_fields_that_changed_are_written() {
        let d = draft(["T", "A", "Al", "", "Pop", "2001", "3", ""], [false; 8], ["T", "B", "Al", "", "", "2002", "3/12", ""]);
        let e = edit(&d);
        assert_eq!(e.title, None);
        assert_eq!(e.artist.as_deref(), Some("B"));
        assert_eq!(e.genre.as_deref(), Some(""), "emptied clears it");
        assert_eq!(e.year, Some(2002));
        assert_eq!(e.track_no, Some(3), "3/12 reads as track 3");
        assert_eq!(e.disc_no, None);
    }

    #[test]
    fn mixed_fields_change_only_when_typed_in() {
        let mut mixed = [false; 8];
        mixed[2] = true;
        mixed[5] = true;
        let d = draft(["T", "A", "", "", "", "", "", ""], mixed, ["T", "A", "", "", "", "1999", "", ""]);
        let e = edit(&d);
        assert_eq!(e.album, None);
        assert_eq!(e.year, Some(1999));
    }
}
