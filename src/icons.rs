//! Stereo's icons: 16-point SVG bodies drawn in the text colour.

use rune::Icon;

macro_rules! icons {
    ($($name:ident $id:literal $body:literal;)*) => {
        $(pub static $name: Icon = Icon { name: concat!("stereo-", $id), view: 16.0, body: $body };)*
    };
}

icons! {
    PLAY "play" r#"<path d="M5 3.2v9.6a.6.6 0 0 0 .9.5l7.6-4.8a.6.6 0 0 0 0-1L5.9 2.7a.6.6 0 0 0-.9.5Z" fill="currentColor"/>"#;
    PAUSE "pause" r#"<rect x="4" y="3" width="2.8" height="10" rx=".7" fill="currentColor"/><rect x="9.2" y="3" width="2.8" height="10" rx=".7" fill="currentColor"/>"#;
    PREV "prev" r#"<path d="M8 8 14 4v8L8 8Zm-6 0 6-4v8L2 8Z" fill="currentColor" stroke="currentColor" stroke-width=".8" stroke-linejoin="round"/>"#;
    NEXT "next" r#"<path d="M8 8 2 4v8l6-4Zm6 0-6-4v8l6-4Z" fill="currentColor" stroke="currentColor" stroke-width=".8" stroke-linejoin="round"/>"#;
    VOL_LOW "vol-low" r#"<path d="M2.5 6h2.2L8 3.3v9.4L4.7 10H2.5Z" fill="currentColor"/>"#;
    VOL_HIGH "vol-high" r#"<path d="M1.5 6h2.2L7 3.3v9.4L3.7 10H1.5Z" fill="currentColor"/><path d="M9.5 5.8a3 3 0 0 1 0 4.4M11.4 4a5.6 5.6 0 0 1 0 8" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#;
    NOTE "note" r#"<path d="M6 11.5V3.8l7-1.6v7.6" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"/><ellipse cx="4.4" cy="11.6" rx="1.9" ry="1.5" fill="currentColor"/><ellipse cx="11.4" cy="10" rx="1.9" ry="1.5" fill="currentColor"/>"#;
    PARTY "party" r#"<path d="M2.5 13.5 5.2 5.6l5.2 5.2-7.9 2.7Z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/><path d="M9 2.5v1.6M12.6 3.4l-1.1 1.1M13.5 7H11.9M8.6 6.8c.6-1.1 1.6-1.7 2.8-1.7" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#;
    GEAR "gear" r#"<circle cx="8" cy="8" r="2" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="M8 1.8v1.7M8 12.5v1.7M14.2 8h-1.7M3.5 8H1.8M12.4 3.6l-1.2 1.2M4.8 11.2l-1.2 1.2M12.4 12.4l-1.2-1.2M4.8 4.8 3.6 3.6" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/><circle cx="8" cy="8" r="4.4" fill="none" stroke="currentColor" stroke-width="1.2"/>"#;
    LIST "list" r#"<path d="M5.5 4.5h8M5.5 8h8M5.5 11.5h8" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/><circle cx="2.8" cy="4.5" r=".9" fill="currentColor"/><circle cx="2.8" cy="8" r=".9" fill="currentColor"/><circle cx="2.8" cy="11.5" r=".9" fill="currentColor"/>"#;
    PLUS "plus" r#"<path d="M8 3v10M3 8h10" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>"#;
    SHUFFLE "shuffle" r#"<path d="M2 4.5h2.2c1.4 0 2.2.6 3 1.8l1.6 3.4c.8 1.2 1.6 1.8 3 1.8H14M2 11.5h2.2c1.1 0 1.8-.4 2.4-1.1M9.4 5.6c.6-.7 1.3-1.1 2.4-1.1H14M12.3 2.8 14 4.5l-1.7 1.7M12.3 9.8l1.7 1.7-1.7 1.7" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>"#;
    REPEAT "repeat" r#"<path d="M3 7.2V6.5A2 2 0 0 1 5 4.5h8M11.3 2.8 13 4.5l-1.7 1.7M13 8.8v.7a2 2 0 0 1-2 2H3M4.7 13.2 3 11.5l1.7-1.7" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>"#;
    REPEAT_ONE "repeat-one" r#"<path d="M3 7.2V6.5A2 2 0 0 1 5 4.5h8M11.3 2.8 13 4.5l-1.7 1.7M13 8.8v.7a2 2 0 0 1-2 2H3M4.7 13.2 3 11.5l1.7-1.7" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/><path d="M7.4 7.2 8.4 6.6v3.2" fill="none" stroke="currentColor" stroke-width="1.1" stroke-linecap="round" stroke-linejoin="round"/>"#;
    ARTWORK "artwork" r#"<rect x="2.5" y="2.5" width="11" height="11" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="m2.8 11.2 3.4-3.4 2.4 2.4 1.6-1.6 3 3" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/><circle cx="10.5" cy="5.6" r="1.1" fill="currentColor"/>"#;
    BROWSER "browser" r#"<rect x="2" y="2.5" width="12" height="11" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="M2.5 7h11M6 3v4M10 3v4" stroke="currentColor" stroke-width="1.2"/>"#;
    STAR "star" r#"<path d="m8 1.9 1.8 3.8 4.1.5-3 2.9.8 4.1L8 11.2l-3.7 2 .8-4.1-3-2.9 4.1-.5Z" fill="currentColor"/>"#;
    DOT "dot" r#"<circle cx="8" cy="8" r="1.3" fill="currentColor"/>"#;
    SPEAKER "speaker" r#"<path d="M1.5 6h2.2L7 3.3v9.4L3.7 10H1.5Z" fill="currentColor"/><path d="M9.5 5.8a3 3 0 0 1 0 4.4M11.4 4a5.6 5.6 0 0 1 0 8" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#;
    SPEAKER_OFF "speaker-off" r#"<path d="M1.5 6h2.2L7 3.3v9.4L3.7 10H1.5Z" fill="currentColor"/>"#;
    MENU "menu" r#"<circle cx="3.5" cy="8" r="1.2" fill="currentColor"/><circle cx="8" cy="8" r="1.2" fill="currentColor"/><circle cx="12.5" cy="8" r="1.2" fill="currentColor"/>"#;
    REFRESH "refresh" r#"<path d="M13 8a5 5 0 1 1-1.5-3.6M13 2.8v2.6h-2.6" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>"#;
    MINUS "minus" r#"<path d="M3 8h10" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>"#;
    WARN "warn" r#"<path d="M8 2.3 14.2 13H1.8Z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/><path d="M8 6.4v3" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/><circle cx="8" cy="11.2" r=".8" fill="currentColor"/>"#;
    UP_NEXT "up-next" r#"<path d="M2.5 4h8M2.5 8h8M2.5 12h5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/><path d="M10.5 10.2v3.6l3-1.8Z" fill="currentColor" stroke="currentColor" stroke-width=".8" stroke-linejoin="round"/>"#;
    MOON "moon" r#"<path d="M12.8 10.2A5.4 5.4 0 0 1 5.8 3.2a5.4 5.4 0 1 0 7 7Z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/>"#;
    DUPES "dupes" r#"<rect x="2.5" y="4.5" width="8" height="9" rx="1.2" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="M5.5 2.5h6.8a1.2 1.2 0 0 1 1.2 1.2v7.3" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>"#;
    FOLDER "folder" r#"<path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h2.8l1.4 1.5h4.8A1.5 1.5 0 0 1 14 6v5.5a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 2 11.5Z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/>"#;
}
