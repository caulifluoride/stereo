//! Small controls the settings window and the editors share: a slider, a pop-up choice that
//! opens through [`Menu::Pick`], and the label-and-control rows they sit in.

use rune::{Align, Color, Drag, El, Phase, Tx, col, el, row, text};
use rune_kit::{Kit, Select};

use crate::State;
use crate::menus::Menu;

/// A horizontal slider over `min..=max`, moving by `step` from the keyboard.
#[allow(clippy::too_many_arguments)]
pub fn slider(k: &Kit, key: &'static str, label: &str, value: f32, min: f32, max: f32, step: f32, w: f32, set: impl Fn(&mut State, f32) + Clone + 'static) -> El {
    const KNOB: f32 = 12.0;
    let frac = ((value - min) / (max - min)).clamp(0.0, 1.0);
    let travel = w - KNOB;
    let track = el().w_full().h(4.0).radius(2.0).fill(k.c.border_base)
        .child(el().w((KNOB / 2.0 + frac * travel).round()).h_full().radius(2.0).fill(k.c.bg_accent));
    let knob = el().size(KNOB, KNOB).radius(KNOB / 2.0)
        .gradient(Color::WHITE, k.c.layer_02).border(1.0, k.c.border_strong)
        .shadow(1.0, 2.0, Color::BLACK.alpha(0.2))
        .absolute(frac * travel, 0.0);
    el().key(key).size(w, KNOB).center_y()
        .role("slider").label(label.to_string()).focusable(true)
        .range_value(f64::from(value), f64::from(min), f64::from(max)).range_step(f64::from(step))
        .on_step({
            let set = set.clone();
            move |s: &mut State, steps, _| {
                let v = cur(s, key).unwrap_or(min);
                set(s, (v + steps as f32 * step).clamp(min, max));
            }
        })
        .on_drag(move |s: &mut State, d: &mut Drag, tx: &mut Tx| {
            if d.phase != Phase::Up {
                let f = ((d.x - KNOB / 2.0) / travel).clamp(0.0, 1.0);
                let v = min + f * (max - min);
                set(s, (v / step).round() * step);
                tx.instant();
            }
        })
        .child(track)
        .child(knob)
}

/// The value a keyed slider shows now, for stepping from the keyboard.
fn cur(s: &State, key: &str) -> Option<f32> {
    let st = &s.settings;
    if let Some(i) = key.strip_prefix("eq-").and_then(|i| i.parse::<usize>().ok()) {
        return st.eq.get(i).copied();
    }
    match key {
        "crossfade" => Some(st.crossfade),
        "preamp" => Some(st.preamp),
        "text-size" => Some(st.text_size),
        _ => None,
    }
}

/// A pop-up choice among `options`, open while [`Menu::Pick`] names `key`.
#[allow(clippy::too_many_arguments)]
pub fn pick(s: &State, k: &Kit, layer: &mut Vec<El>, key: &'static str, label: &str, options: &[String], current: usize, width: f32, choose: impl Fn(&mut State, usize) + Clone + 'static) -> El {
    pick_as(s, k, layer, Menu::Pick(key), key.to_string(), label, options, current, width, choose)
}

/// [`pick`], open while `s.menu` is `menu`.
#[allow(clippy::too_many_arguments)]
pub fn pick_as(s: &State, k: &Kit, layer: &mut Vec<El>, menu: Menu, key: String, label: &str, options: &[String], current: usize, width: f32, choose: impl Fn(&mut State, usize) + Clone + 'static) -> El {
    let open = s.menu == Some(menu);
    let names = options.to_vec();
    Select::new(key, options)
        .label(label.to_string())
        .current(options.get(current).map(String::as_str))
        .open(open)
        .on_toggle(move |s: &mut State| s.menu = if s.menu == Some(menu) { None } else { Some(menu) })
        .on_close(|s: &mut State| s.menu = None)
        .width(width)
        .on_choose(move |s: &mut State, v: &str, _: &mut Tx| {
            s.menu = None;
            if let Some(i) = names.iter().position(|n| n == v) {
                choose(s, i);
            }
        })
        .build(k, layer)
}

/// A setting's row: its name on the left, the control, and an explanation under it.
pub fn line(k: &Kit, key: impl Into<String>, label: &str, control: El, note: Option<&str>) -> El {
    let mut r = col().key(key).w_full().gap(3.0)
        .child(row().w_full().gap(12.0).center_y()
            .child(text(label.to_string()).font(k.ty.ui).color(k.c.text_base).w(170.0).align(Align::End))
            .child(control));
    if let Some(n) = note {
        r = r.child(row().w_full().child(el().w(182.0)).child(text(n.to_string()).font(k.ty.caption()).color(k.c.text_muted).wrap().grow(1.0).min_w(0.0)));
    }
    r
}

/// A heading over a group of settings.
pub fn heading(k: &Kit, t: &str) -> El {
    text(t.to_string()).font(k.ty.small_medium).color(k.c.text_muted).pt(6.0)
}
