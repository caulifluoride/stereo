//! The settings window: what happens at launch and how plays count (General); crossfade,
//! gapless playback, volume levelling and the equaliser (Playback); folder watching, the song
//! list's columns and the built-in smart playlists' sizes (Library); theme, accent colour,
//! row height and text size (Appearance); and artwork lookups and scrobbling (Online).

use rune::{El, ScrollSpec, TextEdit, Tx, col, el, row, spacer, text};
use rune_kit::{Button, ButtonVariant, Checkbox, ControlSize, Dialog as KitDialog, Kit, ScrollBar, SegmentedControl, Switch, TextInput};

use crate::dialogs::Dialog;
use crate::settings::{Accent, COLUMNS, Density, EQ_BANDS, EQ_PRESETS, Gain, Launch, PlayCount, Settings, SkipCount, Theme};
use crate::widgets::{heading, line, pick, slider};
use crate::State;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    General,
    Playback,
    Library,
    Appearance,
    Online,
}

impl Tab {
    const ALL: [Tab; 5] = [Tab::General, Tab::Playback, Tab::Library, Tab::Appearance, Tab::Online];

    fn name(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Playback => "Playback",
            Tab::Library => "Library",
            Tab::Appearance => "Appearance",
            Tab::Online => "Online",
        }
    }
}

/// The window's text fields, filled from the settings when it opens.
#[derive(Default)]
pub struct Fields {
    pub lastfm_key: TextEdit,
    pub lastfm_secret: TextEdit,
    pub lastfm_user: TextEdit,
    pub lastfm_password: TextEdit,
    pub listenbrainz: TextEdit,
    /// What the last sign-in or scrobble had to say.
    pub status: Option<String>,
}

pub fn open(s: &mut State, tab: Tab) {
    let f = &s.settings.lastfm;
    s.prefs = Fields {
        lastfm_key: TextEdit::new(f.api_key.clone()),
        lastfm_secret: TextEdit::new(f.secret.clone()),
        lastfm_user: TextEdit::new(f.user.clone()),
        lastfm_password: TextEdit::new(""),
        listenbrainz: TextEdit::new(s.settings.listenbrainz.clone()),
        status: s.prefs.status.take(),
    };
    s.menu = None;
    s.dialog = Some(Dialog::Settings(tab));
}

/// Changes a setting, saves, and applies what it touches.
fn set(s: &mut State, f: impl FnOnce(&mut Settings)) {
    f(&mut s.settings);
    s.settings_changed();
}

fn playback(s: &mut State, f: impl FnOnce(&mut Settings)) {
    set(s, f);
    s.apply_playback();
}

fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

fn general(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let st = &s.settings;
    let launch = [Launch::Paused, Launch::Playing, Launch::Nothing];
    let plays = [PlayCount::End, PlayCount::Half, PlayCount::Scrobble];
    let skips = [SkipCount::Half, SkipCount::Early, SkipCount::Never];
    let notify_note = if cfg!(target_os = "linux") { None } else { Some("Notifications show on Linux only, for now.") };
    col().gap(14.0)
        .child(line(k, "launch", "When Stereo opens", pick(s, k, layer, "pref-launch", "When Stereo opens",
            &strings(&["Resume the last song, paused", "Resume the last song, playing", "Start with nothing playing"]),
            launch.iter().position(|&l| l == st.on_launch).unwrap_or(0), 260.0,
            move |s, i| set(s, |st| st.on_launch = launch[i])), None))
        .child(line(k, "notify", "Notifications",
            Switch::new(st.notify).label("Show the song when it changes").on_toggle(|s: &mut State| set(s, |st| st.notify = !st.notify)).build(k),
            notify_note))
        .child(heading(k, "Play counts"))
        .child(line(k, "plays", "Count a play", pick(s, k, layer, "pref-plays", "Count a play",
            &strings(&["When the song ends", "Halfway through", "Halfway, or after 4 minutes"]),
            plays.iter().position(|&p| p == st.play_count).unwrap_or(0), 260.0,
            move |s, i| set(s, |st| st.play_count = plays[i])),
            Some("Play counts drive Top Most Played, Recently Played and Party Shuffle's leanings.")))
        .child(line(k, "skips", "Count a skip", pick(s, k, layer, "pref-skips", "Count a skip",
            &strings(&["Moving on in the first half", "Moving on in the first 30 seconds", "Never"]),
            skips.iter().position(|&p| p == st.skip_count).unwrap_or(0), 260.0,
            move |s, i| set(s, |st| st.skip_count = skips[i])),
            Some("The first two seconds never count, so a quick look doesn't.")))
}

const EQ_KEYS: [&str; 10] = ["eq-0", "eq-1", "eq-2", "eq-3", "eq-4", "eq-5", "eq-6", "eq-7", "eq-8", "eq-9"];

fn db(v: f32) -> String {
    if v.abs() < 0.05 { "0 dB".into() } else { format!("{v:+.1} dB") }
}

fn hz(f: f32) -> String {
    if f >= 1000.0 { format!("{}K", f / 1000.0) } else { format!("{f}") }
}

fn playback_tab(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let st = &s.settings;
    let value = |t: String| text(t).font(k.ty.small).color(k.c.text_muted).w(60.0);
    let fade = row().gap(10.0).center_y()
        .child(slider(k, "crossfade", "Crossfade", st.crossfade, 0.0, 12.0, 0.5, 220.0, |s, v| playback(s, |st| st.crossfade = v)))
        .child(value(if st.crossfade < 0.5 { "Off".into() } else { format!("{:.1} s", st.crossfade) }));
    let gains = [Gain::Off, Gain::Track, Gain::Album];
    let preamp = row().gap(10.0).center_y()
        .child(slider(k, "preamp", "Preamp", st.preamp, -12.0, 12.0, 0.5, 220.0, |s, v| playback(s, |st| st.preamp = v)))
        .child(value(db(st.preamp)));
    let presets: Vec<String> = EQ_PRESETS.iter().map(|(n, _)| n.to_string()).chain((!EQ_PRESETS.iter().any(|(n, _)| *n == st.eq_preset)).then(|| "Manual".to_string())).collect();
    let current = presets.iter().position(|n| *n == st.eq_preset).unwrap_or(presets.len() - 1);
    let mut bands = col().key("eq-bands").gap(6.0);
    for (i, f) in EQ_BANDS.iter().enumerate() {
        bands = bands.child(row().key(EQ_KEYS[i]).gap(10.0).center_y()
            .child(text(hz(*f)).font(k.ty.small).color(k.c.text_muted).w(160.0).align(rune::Align::End))
            .child(slider(k, EQ_KEYS[i], &format!("{} Hz", f), st.eq[i], -12.0, 12.0, 0.5, 220.0, move |s, v| playback(s, |st| {
                st.eq[i] = v;
                st.eq_preset = "Manual".into();
            })))
            .child(value(db(st.eq[i]))));
    }
    let crossfading = st.crossfade >= 0.5;
    col().gap(14.0)
        .child(line(k, "crossfade-row", "Crossfade songs", fade, Some("Each song fades into the next over this long.")))
        .child(line(k, "gapless", "Gapless playback",
            Switch::new(st.gapless && !crossfading).label("Start the next song with no gap").disabled(crossfading)
                .on_toggle(|s: &mut State| playback(s, |st| st.gapless = !st.gapless)).build(k),
            Some(if crossfading { "Off while songs crossfade." } else { "For live albums and DJ mixes that run from one song into the next." })))
        .child(heading(k, "Volume levelling"))
        .child(line(k, "gain", "Sound Check", pick(s, k, layer, "pref-gain", "Sound Check",
            &strings(&["Off", "Each song at the same volume", "Each album at the same volume"]),
            gains.iter().position(|&g| g == st.gain).unwrap_or(0), 260.0,
            move |s, i| playback(s, |st| st.gain = gains[i])),
            Some("From the songs' ReplayGain tags; songs without them play as they are.")))
        .child(line(k, "preamp-row", "Preamp", preamp, None))
        .child(heading(k, "Equaliser"))
        .child(line(k, "eq-on", "Equaliser",
            Switch::new(st.eq_on).label("Shape the sound").on_toggle(|s: &mut State| playback(s, |st| st.eq_on = !st.eq_on)).build(k), None))
        .child(line(k, "eq-preset", "Preset", pick(s, k, layer, "pref-eq", "Equaliser preset", &presets, current, 200.0, move |s, i| {
            if let Some((name, gains)) = EQ_PRESETS.get(i) {
                playback(s, |st| {
                    st.eq = *gains;
                    st.eq_preset = name.to_string();
                    st.eq_on = true;
                });
            }
        }), None))
        .child(bands)
}

fn library_tab(s: &State, k: &Kit, layer: &mut Vec<El>) -> El {
    let st = &s.settings;
    let mut folders = col().gap(4.0);
    for f in &s.lib.folders {
        folders = folders.child(text(f.display().to_string()).font(k.ty.small).color(k.c.text_muted).truncate());
    }
    if s.lib.folders.is_empty() {
        folders = folders.child(text("No folders yet.").font(k.ty.small).color(k.c.text_muted));
    }
    let watching = s.watch.as_ref().map_or(0, |w| w.folders.len());
    let watch_note = if st.watch && watching < s.lib.folders.len() && s.live {
        "Some folders can't be watched; Check Folders for New Songs reads them."
    } else {
        "New songs copied into your music folders join the library by themselves."
    };
    let mut cols = row().key("columns").w(330.0).flex_wrap().gap(6.0);
    for (id, name) in COLUMNS {
        let id = *id;
        cols = cols.child(el().w(160.0).child(Checkbox::new(st.shows(id)).label(*name).on_toggle(move |s: &mut State| set(s, |st| {
            if st.shows(id) {
                st.hidden_columns.push(id.to_string());
            } else {
                st.hidden_columns.retain(|c| c != id);
            }
        })).build(k).key(format!("col-{id}"))));
    }
    let sizes = [10usize, 25, 50, 100, 250, 500];
    let size_names: Vec<String> = sizes.iter().map(|n| format!("{n} songs")).collect();
    let mut size = |key: &'static str, label: &str, v: usize, f: fn(&mut Settings, usize)| -> El {
        let names = size_names.clone();
        pick(s, k, layer, key, label, &names, sizes.iter().position(|&n| n == v).unwrap_or(1), 120.0, move |s: &mut State, i| {
            set(s, |st| f(st, sizes[i]));
            s.refresh();
        })
    };
    let most = size("pref-most", "Top Most Played", st.smart.most_played, |st, n| st.smart.most_played = n);
    let added = size("pref-added", "Recently Added", st.smart.recently_added, |st, n| st.smart.recently_added = n);
    let played = size("pref-played", "Recently Played", st.smart.recently_played, |st, n| st.smart.recently_played = n);
    col().gap(14.0)
        .child(line(k, "folders", "Music folders", folders, None))
        .child(line(k, "watch", "Watch folders",
            Switch::new(st.watch).label("Add new songs as they appear").on_toggle(|s: &mut State| {
                set(s, |st| st.watch = !st.watch);
                s.restart_watch();
            }).build(k),
            Some(watch_note)))
        .child(heading(k, "Song list"))
        .child(line(k, "cols", "Columns", cols, None))
        .child(heading(k, "Smart playlists"))
        .child(line(k, "most", "Top Most Played", most, None))
        .child(line(k, "added", "Recently Added", added, None))
        .child(line(k, "played", "Recently Played", played, None))
}

fn appearance(s: &State, k: &Kit) -> El {
    let st = &s.settings;
    let themes = [Theme::System, Theme::Light, Theme::Dark];
    let theme = SegmentedControl::new(&["Match System", "Light", "Dark"], themes.iter().position(|&t| t == st.theme))
        .label("Theme").fit()
        .on_pick(move |s: &mut State, i, _: &mut Tx| set(s, |st| st.theme = themes[i]))
        .build(k);
    let mut swatches = row().gap(8.0);
    for a in Accent::ALL {
        let c = crate::kit(st, k.dark).c.bg_accent;
        let colour = if a == st.accent { c } else { crate::kit(&Settings { accent: a, ..Settings::default() }, k.dark).c.bg_accent };
        let chosen = a == st.accent;
        swatches = swatches.child(el().key(format!("accent-{}", a.name())).size(22.0, 22.0).radius(11.0).fill(colour)
            .border(if chosen { 2.0 } else { 1.0 }, if chosen { k.c.text_base } else { k.c.border_weak })
            .role("radio").label(a.name()).help(a.name()).checked(chosen)
            .on_press(move |s: &mut State| set(s, |st| st.accent = a)));
    }
    let densities = [Density::Compact, Density::Normal, Density::Large];
    let density = SegmentedControl::new(&["Compact", "Normal", "Large"], densities.iter().position(|&d| d == st.density))
        .label("Row height").fit()
        .on_pick(move |s: &mut State, i, _: &mut Tx| set(s, |st| st.density = densities[i]))
        .build(k);
    let zooms = [0.9f32, 1.0, 1.1, 1.25, 1.5];
    let zoom = SegmentedControl::new(&["90%", "100%", "110%", "125%", "150%"], zooms.iter().position(|&z| (z - st.text_size).abs() < 0.01))
        .label("Text size").fit()
        .on_pick(move |s: &mut State, i, _: &mut Tx| set(s, |st| st.text_size = zooms[i]))
        .build(k);
    col().gap(14.0)
        .child(line(k, "theme", "Theme", theme, None))
        .child(line(k, "accent", "Accent colour", swatches, Some(st.accent.name())))
        .child(line(k, "density", "Row height", density, None))
        .child(line(k, "zoom", "Text size", zoom, Some("Everything in the window grows with it.")))
}

fn field(k: &Kit, edit: &TextEdit, key: &'static str, label: &'static str, secure: bool, get: fn(&mut State) -> &mut TextEdit) -> El {
    let mut t = TextInput::new(edit).label(label).placeholder(label).width(Some(300.0)).on_edit(get);
    if secure {
        t = t.secure();
    }
    t.build(k).key(key)
}

fn online(s: &State, k: &Kit) -> El {
    let st = &s.settings;
    let p = &s.prefs;
    let signed_in = st.lastfm.session.is_some();
    let lastfm = if signed_in {
        col().gap(8.0)
            .child(text(format!("Scrobbling as {}.", if st.lastfm.user.is_empty() { "you" } else { &st.lastfm.user })).font(k.ty.ui).color(k.c.text_base))
            .child(Button::new("Sign Out").variant(ButtonVariant::Outline).control_size(ControlSize::Small).on_press(|s: &mut State| {
                set(s, |st| st.lastfm.session = None);
                s.prefs.status = None;
            }).build(k).key("lastfm-out"))
    } else {
        let ready = [&p.lastfm_key, &p.lastfm_secret, &p.lastfm_user, &p.lastfm_password].iter().all(|e| !e.text.trim().is_empty());
        col().gap(8.0)
            .child(field(k, &p.lastfm_key, "lastfm-key", "API key", false, |s| &mut s.prefs.lastfm_key))
            .child(field(k, &p.lastfm_secret, "lastfm-secret", "Shared secret", true, |s| &mut s.prefs.lastfm_secret))
            .child(field(k, &p.lastfm_user, "lastfm-user", "Username", false, |s| &mut s.prefs.lastfm_user))
            .child(field(k, &p.lastfm_password, "lastfm-password", "Password", true, |s| &mut s.prefs.lastfm_password))
            .child(Button::new("Sign In").variant(ButtonVariant::Contrast).control_size(ControlSize::Small).disabled(!ready).on_press(|s: &mut State| {
                let f = &s.prefs;
                let (key, secret, user) = (f.lastfm_key.text.trim().to_string(), f.lastfm_secret.text.trim().to_string(), f.lastfm_user.text.trim().to_string());
                s.scrobbler.login(&key, &secret, &user, &f.lastfm_password.text);
                set(s, |st| {
                    st.lastfm.api_key = key;
                    st.lastfm.secret = secret;
                    st.lastfm.user = user;
                });
                s.prefs.status = Some("Signing in…".into());
            }).build(k).key("lastfm-in"))
    };
    let lb = TextInput::new(&p.listenbrainz).label("User token").placeholder("User token").width(Some(300.0)).secure()
        .on_edit(|s: &mut State| &mut s.prefs.listenbrainz)
        .on_change(|s: &mut State, _| {
            let t = s.prefs.listenbrainz.text.trim().to_string();
            set(s, |st| st.listenbrainz = t);
        })
        .build(k).key("listenbrainz");
    let mut c = col().gap(14.0)
        .child(line(k, "fetch-art", "Artwork",
            Switch::new(st.fetch_art).label("Look up missing artwork online").on_toggle(|s: &mut State| {
                set(s, |st| st.fetch_art = !st.fetch_art);
                s.art.forget();
                s.request_art();
            }).build(k),
            Some("Searches MusicBrainz for the album and fetches its cover from the Cover Art Archive. Each album is looked up once.")))
        .child(heading(k, "Last.fm"))
        .child(line(k, "lastfm", "Account", lastfm,
            Some(if signed_in { "Songs scrobble when they've played halfway or four minutes." } else { "Last.fm needs an API account of your own: make one at last.fm/api/account/create. The password isn't kept." })))
        .child(heading(k, "ListenBrainz"))
        .child(line(k, "lb", "Token", lb, Some("Your token is on listenbrainz.org/settings. Listens are sent while it's set.")));
    if let Some(msg) = &p.status {
        c = c.child(line(k, "status", "", text(msg.clone()).font(k.ty.small).color(k.c.text_muted).wrap().w(330.0), None));
    }
    c
}

pub fn layer(s: &State, k: &Kit, layer: &mut Vec<El>, tab: Tab) {
    let tabs = SegmentedControl::new(&Tab::ALL.map(Tab::name), Tab::ALL.iter().position(|&t| t == tab))
        .label("Settings section").fit()
        .on_pick(|s: &mut State, i, _: &mut Tx| {
            s.menu = None;
            s.dialog = Some(Dialog::Settings(Tab::ALL[i]));
        })
        .build(k);
    let body = match tab {
        Tab::General => general(s, k, layer),
        Tab::Playback => playback_tab(s, k, layer),
        Tab::Library => library_tab(s, k, layer),
        Tab::Appearance => appearance(s, k),
        Tab::Online => online(s, k),
    };
    let page = col().key(format!("prefs-{}", tab.name())).w_full().h(460.0).min_h(0.0).px(4.0).pb(16.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .child(body)
        .scroll_bar(k);
    let content = col().key("prefs").w_full().gap(12.0).px(16.0).pb(16.0)
        .child(row().w_full().child(spacer()).child(tabs).child(spacer()))
        .child(page);
    KitDialog::new("settings", "Settings").child(content).width(640.0).fit().open(true).on_close(|s: &mut State| {
        s.dialog = None;
        s.menu = None;
    }).build(k, layer);
}
