//! Smart playlists the user makes: rules on a song's tags, ratings and history, matched all or
//! any, and an optional limit ("25 songs selected by most played"). They fill themselves from
//! the library every time it changes, as the built-in ones do.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::library::{Library, Track, TrackId, now};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Field {
    Name,
    Artist,
    AlbumArtist,
    Album,
    Genre,
    Year,
    Rating,
    Plays,
    Skips,
    /// The song's length, in minutes and seconds.
    Time,
    LastPlayed,
    Added,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Text,
    Number,
    Date,
}

impl Field {
    pub const ALL: [Field; 12] = [
        Field::Name, Field::Artist, Field::AlbumArtist, Field::Album, Field::Genre, Field::Year,
        Field::Rating, Field::Plays, Field::Skips, Field::Time, Field::LastPlayed, Field::Added,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Field::Name => "Name",
            Field::Artist => "Artist",
            Field::AlbumArtist => "Album Artist",
            Field::Album => "Album",
            Field::Genre => "Genre",
            Field::Year => "Year",
            Field::Rating => "Rating",
            Field::Plays => "Plays",
            Field::Skips => "Skips",
            Field::Time => "Time",
            Field::LastPlayed => "Last Played",
            Field::Added => "Date Added",
        }
    }

    pub fn kind(self) -> Kind {
        match self {
            Field::Name | Field::Artist | Field::AlbumArtist | Field::Album | Field::Genre => Kind::Text,
            Field::Year | Field::Rating | Field::Plays | Field::Skips | Field::Time => Kind::Number,
            Field::LastPlayed | Field::Added => Kind::Date,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    Contains,
    NotContains,
    Is,
    IsNot,
    StartsWith,
    EndsWith,
    Greater,
    Less,
    /// Within the last so many days.
    InLast,
    /// Not within the last so many days (or never).
    NotInLast,
}

impl Op {
    pub fn name(self) -> &'static str {
        match self {
            Op::Contains => "contains",
            Op::NotContains => "does not contain",
            Op::Is => "is",
            Op::IsNot => "is not",
            Op::StartsWith => "starts with",
            Op::EndsWith => "ends with",
            Op::Greater => "is greater than",
            Op::Less => "is less than",
            Op::InLast => "is in the last",
            Op::NotInLast => "is not in the last",
        }
    }

    /// The comparisons a field of `kind` offers, the first its default.
    pub fn for_kind(kind: Kind) -> &'static [Op] {
        match kind {
            Kind::Text => &[Op::Contains, Op::NotContains, Op::Is, Op::IsNot, Op::StartsWith, Op::EndsWith],
            Kind::Number => &[Op::Is, Op::IsNot, Op::Greater, Op::Less],
            Kind::Date => &[Op::InLast, Op::NotInLast],
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Rule {
    pub field: Field,
    pub op: Op,
    pub value: String,
}

impl Rule {
    pub fn new(field: Field) -> Rule {
        Rule { field, op: Op::for_kind(field.kind())[0], value: String::new() }
    }

    fn matches(&self, t: &Track, today: u64) -> bool {
        let fold = |s: &str| s.to_lowercase();
        match self.field.kind() {
            Kind::Text => {
                let have = fold(match self.field {
                    Field::Name => &t.title,
                    Field::Artist => &t.artist,
                    Field::AlbumArtist => t.album_artist_or_artist(),
                    Field::Album => &t.album,
                    _ => &t.genre,
                });
                let want = fold(self.value.trim());
                match self.op {
                    Op::Contains => have.contains(&want),
                    Op::NotContains => !have.contains(&want),
                    Op::Is => have == want,
                    Op::IsNot => have != want,
                    Op::StartsWith => have.starts_with(&want),
                    Op::EndsWith => have.ends_with(&want),
                    _ => false,
                }
            }
            Kind::Number => {
                let Some(want) = number(self.field, &self.value) else { return false };
                let have = match self.field {
                    Field::Year => t.year.map_or(0.0, f64::from),
                    Field::Rating => f64::from(t.rating),
                    Field::Plays => f64::from(t.plays),
                    Field::Skips => f64::from(t.skips),
                    _ => f64::from(t.duration).floor(),
                };
                match self.op {
                    Op::Is => have == want,
                    Op::IsNot => have != want,
                    Op::Greater => have > want,
                    Op::Less => have < want,
                    _ => false,
                }
            }
            Kind::Date => {
                let Some(days) = self.value.trim().parse::<u64>().ok() else { return false };
                let when = match self.field {
                    Field::LastPlayed => t.last_played,
                    _ => Some(t.added),
                };
                let within = when.is_some_and(|w| today.saturating_sub(w) <= days * 86_400);
                if self.op == Op::InLast { within } else { !within }
            }
        }
    }
}

/// A rule's number: a year, stars, a count, or a time as `m:ss` or seconds.
fn number(field: Field, s: &str) -> Option<f64> {
    let s = s.trim();
    if field == Field::Time
        && let Some((m, sec)) = s.split_once(':')
    {
        return Some(m.trim().parse::<f64>().ok()? * 60.0 + sec.trim().parse::<f64>().ok()?);
    }
    s.parse().ok()
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    Songs,
    Minutes,
    Hours,
}

impl Unit {
    pub const ALL: [Unit; 3] = [Unit::Songs, Unit::Minutes, Unit::Hours];

    pub fn name(self) -> &'static str {
        match self {
            Unit::Songs => "songs",
            Unit::Minutes => "minutes",
            Unit::Hours => "hours",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum By {
    Random,
    MostPlayed,
    LeastPlayed,
    HighestRated,
    LowestRated,
    RecentlyPlayed,
    LeastRecentlyPlayed,
    RecentlyAdded,
    LeastRecentlyAdded,
}

impl By {
    pub const ALL: [By; 9] = [
        By::Random, By::MostPlayed, By::LeastPlayed, By::HighestRated, By::LowestRated,
        By::RecentlyPlayed, By::LeastRecentlyPlayed, By::RecentlyAdded, By::LeastRecentlyAdded,
    ];

    pub fn name(self) -> &'static str {
        match self {
            By::Random => "random",
            By::MostPlayed => "most played",
            By::LeastPlayed => "least played",
            By::HighestRated => "highest rating",
            By::LowestRated => "lowest rating",
            By::RecentlyPlayed => "most recently played",
            By::LeastRecentlyPlayed => "least recently played",
            By::RecentlyAdded => "most recently added",
            By::LeastRecentlyAdded => "least recently added",
        }
    }

    fn order(self, a: &Track, b: &Track, seed: u64) -> Ordering {
        match self {
            // A shuffle that stays put while the library does: each song's place comes from
            // its id and the list's.
            By::Random => mix(a.id ^ seed).cmp(&mix(b.id ^ seed)),
            By::MostPlayed => b.plays.cmp(&a.plays),
            By::LeastPlayed => a.plays.cmp(&b.plays),
            By::HighestRated => b.rating.cmp(&a.rating),
            By::LowestRated => a.rating.cmp(&b.rating),
            By::RecentlyPlayed => b.last_played.cmp(&a.last_played),
            By::LeastRecentlyPlayed => a.last_played.cmp(&b.last_played),
            By::RecentlyAdded => b.added.cmp(&a.added),
            By::LeastRecentlyAdded => a.added.cmp(&b.added),
        }
    }
}

fn mix(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^ (x >> 33)
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Limit {
    pub n: u32,
    pub unit: Unit,
    pub by: By,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct SmartList {
    pub id: u64,
    pub name: String,
    /// Match any rule, rather than all of them.
    pub any: bool,
    pub rules: Vec<Rule>,
    pub limit: Option<Limit>,
}

impl Default for SmartList {
    fn default() -> Self {
        SmartList { id: 0, name: "untitled smart playlist".into(), any: false, rules: vec![Rule::new(Field::Artist)], limit: None }
    }
}

impl SmartList {
    /// The library's songs that match, limited and in the limit's order (else the library's).
    pub fn pick(&self, lib: &Library) -> Vec<TrackId> {
        let today = now();
        let ok = |t: &Track| {
            if self.rules.is_empty() {
                return true;
            }
            if self.any { self.rules.iter().any(|r| r.matches(t, today)) } else { self.rules.iter().all(|r| r.matches(t, today)) }
        };
        let mut ts: Vec<&Track> = lib.tracks.iter().filter(|t| ok(t)).collect();
        let Some(limit) = self.limit else {
            ts.sort_by(|a, b| crate::rows::default_order(a, b));
            return ts.into_iter().map(|t| t.id).collect();
        };
        ts.sort_by(|a, b| limit.by.order(a, b, self.id).then_with(|| crate::rows::default_order(a, b)));
        let mut out = Vec::new();
        let mut secs = 0.0;
        let cap = match limit.unit {
            Unit::Songs => f64::from(limit.n),
            Unit::Minutes => f64::from(limit.n) * 60.0,
            Unit::Hours => f64::from(limit.n) * 3600.0,
        };
        for t in ts {
            let more = match limit.unit {
                Unit::Songs => (out.len() as f64) < cap,
                _ => secs + f64::from(t.duration) <= cap,
            };
            if !more {
                break;
            }
            secs += f64::from(t.duration);
            out.push(t.id);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> Library {
        let mut lib = Library::default();
        let t = |title: &str, artist: &str, genre: &str, rating: u8, plays: u32, secs: f32| Track {
            path: format!("/m/{title}").into(),
            title: title.into(),
            artist: artist.into(),
            genre: genre.into(),
            rating,
            plays,
            duration: secs,
            added: now(),
            ..Track::default()
        };
        lib.add(t("One", "Abba", "Pop", 5, 10, 200.0));
        lib.add(t("Two", "Abba", "Pop", 2, 3, 200.0));
        lib.add(t("Three", "Zed", "Rock", 4, 30, 200.0));
        lib.add(t("Four", "Zed", "Jazz", 0, 0, 200.0));
        lib
    }

    fn titles(lib: &Library, ids: &[TrackId]) -> Vec<String> {
        ids.iter().map(|&id| lib.get(id).unwrap().title.clone()).collect()
    }

    #[test]
    fn rules_match_all_or_any() {
        let lib = lib();
        let mut s = SmartList {
            rules: vec![Rule { field: Field::Artist, op: Op::Is, value: "abba".into() }, Rule { field: Field::Rating, op: Op::Greater, value: "3".into() }],
            ..SmartList::default()
        };
        assert_eq!(titles(&lib, &s.pick(&lib)), ["One"]);
        s.any = true;
        assert_eq!(titles(&lib, &s.pick(&lib)), ["One", "Two", "Three"]);
    }

    #[test]
    fn limits_take_the_top_by_their_order_in_songs_or_time() {
        let lib = lib();
        let mut s = SmartList { rules: vec![], limit: Some(Limit { n: 2, unit: Unit::Songs, by: By::MostPlayed }), ..SmartList::default() };
        assert_eq!(titles(&lib, &s.pick(&lib)), ["Three", "One"]);
        s.limit = Some(Limit { n: 10, unit: Unit::Minutes, by: By::HighestRated });
        assert_eq!(titles(&lib, &s.pick(&lib)), ["One", "Three", "Two"]);
        s.limit = Some(Limit { n: 3, unit: Unit::Songs, by: By::Random });
        let a = s.pick(&lib);
        assert_eq!(a.len(), 3);
        assert_eq!(a, s.pick(&lib), "a random limit stays put");
    }

    #[test]
    fn dates_and_times_compare_as_people_write_them() {
        let lib = lib();
        let never = SmartList { rules: vec![Rule { field: Field::LastPlayed, op: Op::NotInLast, value: "30".into() }], ..SmartList::default() };
        assert_eq!(never.pick(&lib).len(), 4);
        let added = SmartList { rules: vec![Rule { field: Field::Added, op: Op::InLast, value: "7".into() }], ..SmartList::default() };
        assert_eq!(added.pick(&lib).len(), 4);
        let short = SmartList { rules: vec![Rule { field: Field::Time, op: Op::Less, value: "3:30".into() }], ..SmartList::default() };
        assert_eq!(short.pick(&lib).len(), 4);
        let jazz = SmartList { rules: vec![Rule { field: Field::Genre, op: Op::NotContains, value: "o".into() }], ..SmartList::default() };
        assert_eq!(titles(&lib, &jazz.pick(&lib)), ["Four"]);
    }
}
