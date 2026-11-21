//! The smart playlist editor: a name, whether songs match all the rules or any, the rules
//! themselves (a field, a comparison, a value), and an optional limit. It shows how many songs
//! match as the rules change.

use rune::{El, TextEdit, Tx, col, row, spacer, text};
use rune_kit::{Button, ButtonVariant, Checkbox, ControlSize, Dialog as KitDialog, IconButton, IconButtonVariant, Kit, SegmentedControl, TextInput};

use crate::dialogs::Dialog;
use crate::menus::Menu;
use crate::rows::View;
use crate::smart::{By, Field, Kind, Limit, Op, Rule, SmartList, Unit};
use crate::widgets::pick_as;
use crate::{State, icons};

pub struct RuleDraft {
    pub field: Field,
    pub op: Op,
    pub value: TextEdit,
}

pub struct Draft {
    /// The list being edited, or None for a new one.
    pub id: Option<u64>,
    pub name: TextEdit,
    pub any: bool,
    pub rules: Vec<RuleDraft>,
    pub limited: bool,
    pub limit_n: TextEdit,
    pub unit: Unit,
    pub by: By,
}

impl Draft {
    fn of(id: Option<u64>, l: &SmartList) -> Draft {
        let limit = l.limit.unwrap_or(Limit { n: 25, unit: Unit::Songs, by: By::Random });
        Draft {
            id,
            name: TextEdit::new(l.name.clone()),
            any: l.any,
            rules: l.rules.iter().map(|r| RuleDraft { field: r.field, op: r.op, value: TextEdit::new(r.value.clone()) }).collect(),
            limited: l.limit.is_some(),
            limit_n: TextEdit::new(limit.n.to_string()),
            unit: limit.unit,
            by: limit.by,
        }
    }

    fn list(&self) -> SmartList {
        SmartList {
            id: self.id.unwrap_or(0),
            name: self.name.text.trim().to_string(),
            any: self.any,
            rules: self.rules.iter().map(|r| Rule { field: r.field, op: r.op, value: r.value.text.clone() }).collect(),
            limit: self.limited.then(|| Limit { n: self.limit_n.text.trim().parse().unwrap_or(25), unit: self.unit, by: self.by }),
        }
    }
}

pub fn open_new(s: &mut State) {
    let name = s.lib.untitled_from("untitled smart playlist");
    let draft = Draft::of(None, &SmartList { name, ..SmartList::default() });
    s.menu = None;
    s.smart_edit = Some(draft);
    s.dialog = Some(Dialog::SmartEditor);
}

pub fn open_edit(s: &mut State, id: u64) {
    let Some(l) = s.lib.smart_list(id) else { return };
    s.smart_edit = Some(Draft::of(Some(id), l));
    s.menu = None;
    s.dialog = Some(Dialog::SmartEditor);
}

fn close(s: &mut State) {
    s.smart_edit = None;
    s.dialog = None;
    s.menu = None;
}

fn save(s: &mut State) {
    let Some(d) = s.smart_edit.take() else { return };
    s.dialog = None;
    s.menu = None;
    let mut list = d.list();
    if list.name.is_empty() {
        list.name = s.lib.untitled_from("untitled smart playlist");
    }
    let id = match d.id {
        Some(id) => {
            if let Some(l) = s.lib.smart_mut(id) {
                *l = list;
            }
            id
        }
        None => s.lib.new_smart(list),
    };
    s.changed();
    if s.view == View::UserSmart(id) {
        s.refresh();
    } else {
        s.select_view(View::UserSmart(id));
    }
    if s.lib.party.source == crate::party::Source::UserSmart(id) {
        s.party_refresh();
    }
}

fn edit<R>(s: &mut State, f: impl FnOnce(&mut Draft) -> R) -> Option<R> {
    s.smart_edit.as_mut().map(f)
}

/// What a rule's value is counted in, after its field.
fn unit_after(f: Field) -> &'static str {
    match f {
        Field::Rating => "stars",
        Field::Time => "m:ss",
        Field::LastPlayed | Field::Added => "days",
        _ => "",
    }
}

fn rule_row(s: &State, k: &Kit, layer: &mut Vec<El>, i: usize, r: &RuleDraft, alone: bool) -> El {
    let fields: Vec<String> = Field::ALL.iter().map(|f| f.name().to_string()).collect();
    let ops = Op::for_kind(r.field.kind());
    let op_names: Vec<String> = ops.iter().map(|o| o.name().to_string()).collect();
    let field = pick_as(s, k, layer, Menu::Rule(i, 0), format!("rule-{i}-field"), "Field", &fields, Field::ALL.iter().position(|&f| f == r.field).unwrap_or(0), 130.0, move |s, j| {
        edit(s, |d| {
            if let Some(r) = d.rules.get_mut(i) {
                let f = Field::ALL[j];
                if f.kind() != r.field.kind() {
                    r.op = Op::for_kind(f.kind())[0];
                    r.value = TextEdit::new("");
                }
                r.field = f;
            }
        });
    });
    let op = pick_as(s, k, layer, Menu::Rule(i, 1), format!("rule-{i}-op"), "Comparison", &op_names, ops.iter().position(|&o| o == r.op).unwrap_or(0), 150.0, move |s, j| {
        edit(s, |d| {
            if let Some(r) = d.rules.get_mut(i) {
                r.op = Op::for_kind(r.field.kind())[j];
            }
        });
    });
    let wide = r.field.kind() == Kind::Text;
    let value = TextInput::new(&r.value)
        .label("Value")
        .width(Some(if wide { 160.0 } else { 70.0 }))
        .on_edit(move |s: &mut State| match s.smart_edit.as_mut() {
            Some(d) if i < d.rules.len() => &mut d.rules[i].value,
            _ => &mut s.scratch,
        })
        .on_submit(|s: &mut State, _| save(s))
        .build(k)
        .key(format!("rule-{i}-value"));
    let remove = IconButton::new(&icons::MINUS).variant(IconButtonVariant::GhostMuted).label("Remove rule").help("Remove rule").disabled(alone)
        .on_press(move |s: &mut State| {
            edit(s, |d| {
                if i < d.rules.len() {
                    d.rules.remove(i);
                }
            });
            s.menu = None;
        })
        .build(k)
        .key(format!("rule-{i}-remove"));
    let add = IconButton::new(&icons::PLUS).variant(IconButtonVariant::GhostMuted).label("Add rule").help("Add a rule after this one")
        .on_press(move |s: &mut State| {
            edit(s, |d| d.rules.insert(i + 1, RuleDraft { field: Field::Name, op: Op::Contains, value: TextEdit::new("") }));
            s.menu = None;
        })
        .build(k)
        .key(format!("rule-{i}-add"));
    row().key(format!("rule-{i}")).w_full().gap(6.0).center_y()
        .child(field)
        .child(op)
        .child(value)
        .child(text(unit_after(r.field)).font(k.ty.small).color(k.c.text_muted))
        .child(spacer())
        .child(remove)
        .child(add)
}

pub fn layer(s: &State, k: &Kit, layer: &mut Vec<El>) {
    let Some(d) = &s.smart_edit else { return };
    let name = TextInput::new(&d.name).label("Name").placeholder("Name").width(Some(300.0))
        .on_edit(|s: &mut State| match s.smart_edit.as_mut() {
            Some(d) => &mut d.name,
            None => &mut s.scratch,
        })
        .on_submit(|s: &mut State, _| save(s))
        .build(k)
        .key("smart-name");
    let matching = SegmentedControl::new(&["all", "any"], Some(usize::from(d.any))).label("Match").fit()
        .on_pick(|s: &mut State, i, _: &mut Tx| {
            edit(s, |d| d.any = i == 1);
        })
        .build(k);
    let mut rules = col().key("smart-rules").w_full().gap(6.0);
    for (i, r) in d.rules.iter().enumerate() {
        rules = rules.child(rule_row(s, k, layer, i, r, d.rules.len() == 1));
    }
    if d.rules.is_empty() {
        rules = rules.child(Button::new("Add a Rule").icon(&icons::PLUS).variant(ButtonVariant::Outline).control_size(ControlSize::Small).on_press(|s: &mut State| {
            edit(s, |d| d.rules.push(RuleDraft { field: Field::Name, op: Op::Contains, value: TextEdit::new("") }));
        }).build(k).key("smart-add-first"));
    }
    let units: Vec<String> = Unit::ALL.iter().map(|u| u.name().to_string()).collect();
    let bys: Vec<String> = By::ALL.iter().map(|b| b.name().to_string()).collect();
    let limit = row().key("smart-limit").w_full().gap(6.0).center_y()
        .child(Checkbox::new(d.limited).label("Limit to").on_toggle(|s: &mut State| {
            edit(s, |d| d.limited = !d.limited);
        }).build(k).key("smart-limited"))
        .child(TextInput::new(&d.limit_n).label("How many").width(Some(60.0)).disabled(!d.limited)
            .on_edit(|s: &mut State| match s.smart_edit.as_mut() {
                Some(d) => &mut d.limit_n,
                None => &mut s.scratch,
            })
            .build(k).key("smart-limit-n"))
        .child(pick_as(s, k, layer, Menu::Rule(usize::MAX, 0), "smart-unit".into(), "Limit unit", &units, Unit::ALL.iter().position(|&u| u == d.unit).unwrap_or(0), 100.0, |s, j| {
            edit(s, |d| d.unit = Unit::ALL[j]);
        }))
        .child(text("selected by").font(k.ty.ui).color(k.c.text_muted))
        .child(pick_as(s, k, layer, Menu::Rule(usize::MAX, 1), "smart-by".into(), "Selected by", &bys, By::ALL.iter().position(|&b| b == d.by).unwrap_or(0), 180.0, |s, j| {
            edit(s, |d| d.by = By::ALL[j]);
        }));
    let n = d.list().pick(&s.lib).len();
    let count = format!("{n} {} match", if n == 1 { "song" } else { "songs" });
    let buttons = row().key("smart-buttons").w_full().gap(8.0).pt(6.0).center_y()
        .child(text(count).font(k.ty.small).color(k.c.text_muted))
        .child(spacer())
        .child(Button::new("Cancel").variant(ButtonVariant::Outline).on_press(close).build(k).key("smart-cancel"))
        .child(Button::new("Save").variant(ButtonVariant::Contrast).on_press(save).build(k).key("smart-save"));
    let body = col().w_full().px(16.0).pb(16.0).gap(12.0)
        .child(row().w_full().gap(12.0).center_y().child(text("Name").font(k.ty.ui).color(k.c.text_muted)).child(name))
        .child(row().w_full().gap(8.0).center_y().child(text("Match").font(k.ty.ui).color(k.c.text_muted)).child(matching).child(text("of the following rules:").font(k.ty.ui).color(k.c.text_muted)))
        .child(rules)
        .child(limit)
        .child(buttons);
    let title = if d.id.is_some() { "Edit Smart Playlist" } else { "New Smart Playlist" };
    KitDialog::new("smart-editor", title)
        .description("It fills itself with the songs that match, and keeps up as your library changes.")
        .child(body)
        .width(720.0)
        .fit()
        .open(true)
        .on_close(close)
        .build(k, layer);
}
