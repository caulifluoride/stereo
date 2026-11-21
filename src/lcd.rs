//! The bar across the top, as iTunes had it: back, play and next, the volume, the LCD showing
//! the song playing with its scrubber, and the search field.
//!
//! The LCD's times and the scrubber's fill are live values the poll moves four times a second,
//! so a playing song redraws them without rebuilding the view.

use rune::{Color, Cx, Drag, El, Icon, Phase, Tx, col, el, icon, oklch, row, spacer, text};
use rune_kit::{IconButton, IconButtonVariant, Kit, TextInput};

use crate::{State, icons, menus};

pub const HEIGHT: f32 = 66.0;
pub const SCRUB_H: f32 = 4.0;

/// `m:ss`, or `h:mm:ss` from an hour.
pub fn clock(secs: f32) -> String {
    let s = secs.max(0.0) as u64;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// The LCD's colours: a pale panel in light mode, a dark one in dark mode.
struct Lcd {
    top: Color,
    bottom: Color,
    border: Color,
    text: Color,
    muted: Color,
    track: Color,
    fill: Color,
}

impl Lcd {
    fn new(k: &Kit) -> Lcd {
        if k.dark {
            Lcd {
                top: oklch(0.30, 0.012, 240.0),
                bottom: oklch(0.26, 0.012, 240.0),
                border: Color::BLACK.alpha(0.55),
                text: oklch(0.94, 0.01, 240.0),
                muted: oklch(0.72, 0.012, 240.0),
                track: Color::WHITE.alpha(0.12),
                fill: oklch(0.85, 0.02, 240.0),
            }
        } else {
            Lcd {
                top: oklch(0.975, 0.022, 105.0),
                bottom: oklch(0.935, 0.03, 105.0),
                border: Color::BLACK.alpha(0.28),
                text: oklch(0.25, 0.02, 250.0),
                muted: oklch(0.45, 0.02, 250.0),
                track: Color::BLACK.alpha(0.12),
                fill: oklch(0.35, 0.02, 250.0),
            }
        }
    }
}

pub fn bar(s: &State, k: &Kit, cx: &mut Cx<State>, layer: &mut Vec<El>) -> El {
    let playing = s.now.is_some_and(|n| n.playing);
    let transport = row().key("transport").gap(6.0).center_y()
        .child(round(k, "prev", &icons::PREV, 32.0, "Previous (Ctrl+Left)", |s: &mut State| s.prev()))
        .child(round(k, "play", if playing { &icons::PAUSE } else { &icons::PLAY }, 40.0, if playing { "Pause (Space)" } else { "Play (Space)" }, |s: &mut State| s.play_pause()))
        .child(round(k, "next", &icons::NEXT, 32.0, "Next (Ctrl+Right)", |s: &mut State| s.next(false)));
    let volume = row().key("volume").gap(4.0).center_y()
        .child(icon(&icons::VOL_LOW, 14.0).color(k.c.icon_muted).a11y_hidden())
        .child(volume_slider(s, k))
        .child(icon(&icons::VOL_HIGH, 14.0).color(k.c.icon_muted).a11y_hidden());
    let search = TextInput::new(&s.search)
        .placeholder("Search")
        .label("Search")
        .leading_icon(k.glyph.search)
        .clear_button()
        .width(Some(200.0))
        .on_edit(|s: &mut State| &mut s.search)
        .on_change(|s: &mut State, _| s.refresh())
        .on_key(|s: &mut State, ev, tx: &mut Tx| {
            if ev.key == rune::Key::Escape && !s.search.text.is_empty() {
                s.search = rune::TextEdit::new("");
                s.refresh();
                tx.blur();
                return true;
            }
            false
        })
        .build(k)
        .key("search");
    let menu = menus::app_menu(s, k, layer);
    row().key("topbar").w_full().h(HEIGHT).px(14.0).gap(16.0).center_y()
        .gradient(k.c.layer_01, k.c.page)
        .border_sides([0.0, 0.0, 1.0, 0.0], k.c.border_weak)
        .child(transport)
        .child(volume)
        .child(spacer())
        .child(lcd(s, k, cx))
        .child(spacer())
        .child(search)
        .child(menu)
}

fn round(k: &Kit, key: &'static str, i: &'static Icon, d: f32, tip: &'static str, act: fn(&mut State)) -> El {
    let (top, bottom) = if k.dark { (k.c.layer_03, k.c.layer_01) } else { (Color::WHITE, k.c.layer_02) };
    el().key(key).size(d, d).radius(d / 2.0).center()
        .gradient(top, bottom)
        .border(1.0, k.c.border_base)
        .shadow(1.0, 2.0, Color::BLACK.alpha(if k.dark { 0.4 } else { 0.12 }))
        .child(icon(i, d * 0.42).color(k.c.icon_base).a11y_hidden())
        .role("button").label(tip).help(tip)
        .press_scale(0.94)
        .press(k.c.pressed)
        .on_press(act)
}

fn volume_slider(s: &State, k: &Kit) -> El {
    const W: f32 = 84.0;
    const KNOB: f32 = 12.0;
    let v = s.settings.volume;
    let travel = W - KNOB;
    let to_value = move |x: f32| ((x - KNOB / 2.0) / travel).clamp(0.0, 1.0);
    let track = el().key("vol-track").w_full().h(4.0).radius(2.0).fill(k.c.border_base)
        .child(el().key("vol-fill").w((KNOB / 2.0 + v * travel).round()).h_full().radius(2.0).fill(k.c.icon_muted));
    let knob = el().key("vol-knob").size(KNOB, KNOB).radius(KNOB / 2.0)
        .gradient(Color::WHITE, k.c.layer_02).border(1.0, k.c.border_strong)
        .shadow(1.0, 2.0, Color::BLACK.alpha(0.2))
        .absolute(v * travel, 0.0);
    el().key("vol-slider").size(W, KNOB).center_y()
        .role("slider").label("Volume").focusable(true)
        .range_value(f64::from(v), 0.0, 1.0).range_step(0.05)
        .a11y_value(format!("{:.0}%", v * 100.0))
        .on_step(|s: &mut State, steps, _| s.set_volume(s.settings.volume + steps as f32 * 0.05))
        .on_drag(move |s: &mut State, d: &mut Drag, tx: &mut Tx| {
            if d.phase != Phase::Up {
                s.set_volume(to_value(d.x));
                tx.instant();
            }
        })
        .child(track)
        .child(knob)
}

fn lcd(s: &State, k: &Kit, cx: &mut Cx<State>) -> El {
    let p = Lcd::new(k);
    let panel = col().key("lcd").w(560.0).max_w(560.0).shrink().min_w(260.0).h(54.0).px(14.0).pb(2.0).radius(8.0)
        .gradient(p.top, p.bottom)
        .border(1.0, p.border)
        .justify_center();
    let Some(t) = s.now.and_then(|n| s.lib.get(n.track)) else {
        let line = match (&s.scan, &s.notice) {
            (Some(_), _) => format!("Importing… {} songs", s.scanned),
            (None, Some(n)) => n.clone(),
            (None, None) => "Stereo".into(),
        };
        return panel.items_center().child(text(line).font(k.ty.ui_medium).color(p.text).truncate());
    };
    if let Some([w, _]) = cx.layout_size("scrub")
        && w != s.scrub_w.get()
    {
        s.scrub_w.set(w);
        s.publish_time();
    }
    let by = [t.artist.as_str(), t.album.as_str()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" — ");
    let dur = t.duration;
    let scrub = el().key("scrub").grow(1.0).min_w(40.0).h(14.0).center_y()
        .role("slider").label("Song position").focusable(true)
        .range_value(f64::from(s.pos), 0.0, f64::from(dur.max(1.0)))
        .a11y_value(clock(s.pos))
        .on_step(|s: &mut State, steps, _| s.seek(s.pos + steps as f32 * 5.0))
        .on_drag(move |s: &mut State, d: &mut Drag, tx: &mut Tx| {
            let at = (d.x / d.w.max(1.0)).clamp(0.0, 1.0) * dur;
            match d.phase {
                Phase::Down | Phase::Move => {
                    s.scrubbing = Some(at);
                    s.publish_time();
                    tx.live_only();
                }
                Phase::Up => {
                    s.scrubbing = None;
                    if !d.cancelled() {
                        s.seek(at);
                    }
                }
            }
        })
        .child(el().key("scrub-track").w_full().h(SCRUB_H).radius(SCRUB_H / 2.0).fill(p.track)
            .child(el().key("scrub-fill").h(SCRUB_H).radius(SCRUB_H / 2.0).fill(p.fill).live_rect(&s.progress)));
    let time = rune::TextStyle::new(11.0, 500.0).features("'tnum' 1");
    let times = row().key("times").w_full().gap(8.0).center_y()
        .child(text("0:00:00").live_text(&s.time_in).font(time).color(p.muted).w(44.0).align(rune::Align::End))
        .child(scrub)
        .child(text("-0:00:00").live_text(&s.time_left).font(time).color(p.muted).w(48.0));
    panel.gap(1.0).items_center()
        .child(text(t.title.clone()).key("lcd-title").font(k.ty.text_small_medium).color(p.text).truncate())
        .child(text(by).key("lcd-by").font(k.ty.text_xsmall).color(p.muted).truncate())
        .child(times)
        .on_double_press_tx(|s: &mut State, tx: &mut Tx| s.reveal_now(tx))
        .help("Double-click to show the song playing")
}

/// The small square buttons along the bottom bar: shuffle, repeat, artwork and the browser.
pub fn toggle(k: &Kit, key: &'static str, i: &'static Icon, on: bool, tip: &'static str, act: fn(&mut State)) -> El {
    IconButton::new(i).variant(IconButtonVariant::Ghost).label(tip).help(tip).expanded(on)
        .on_press(act).build(k).key(key)
        .fill(if on { k.c.pressed } else { Color::TRANSPARENT })
        .radius(6.0)
}

#[cfg(test)]
mod tests {
    use super::clock;

    #[test]
    fn clocks_read_as_itunes_wrote_them() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(61.4), "1:01");
        assert_eq!(clock(3725.0), "1:02:05");
        assert_eq!(clock(-3.0), "0:00");
    }
}
