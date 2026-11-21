//! A made-up library for headless runs, so `--snap` has songs, ratings, playlists and a full
//! Party Shuffle to draw without touching anyone's music.

use crate::State;
use crate::library::{Library, Track};
use crate::party::Source;

const ALBUMS: &[(&str, &str, &str, u32, &[&str])] = &[
    ("The Lanterns", "Harbor Lights", "Indie", 2004, &["Tidewater", "Paper Boats", "Northbound", "Salt & Static", "The Long Pier", "Foghorn Waltz", "Ferry at Dawn"]),
    ("Mira Okafor", "Velvet Hours", "Soul", 2006, &["Slow Bloom", "Honey in the Wires", "Sunday Radio", "Hold the Line", "Velvet Hours", "Amber"]),
    ("Static Parade", "Neon Weather", "Electronic", 2007, &["Pulse Avenue", "Afterimage", "Cloud Machine", "Glass District", "Night Bus", "Satellite Hum", "Low Orbit", "Rewind"]),
    ("Juniper & Ash", "Small Fires", "Folk", 2003, &["Kindling", "Old Roads", "Wren Song", "The Orchard", "Small Fires"]),
    ("Calloway Brass", "Downtown Standards", "Jazz", 2005, &["Blue Corner", "Midnight Taxi", "Rooftop Swing", "Sixth Street", "Last Call", "Soft Shoe"]),
    ("Northern Static", "Signal Loss", "Rock", 2008, &["Feedback Loop", "Broken Antenna", "Red Shift", "Wide Awake", "Signal Loss", "Breaker"]),
    ("Pilar Vance", "Coastline", "Pop", 2007, &["Summer Static", "Postcards", "Undertow", "Golden Hour", "Coastline", "Seaglass"]),
    ("The Quiet Hours", "Lamplight", "Ambient", 2002, &["Dusk", "Lamplight", "Snowfield", "Slow Rain", "Embers"]),
];

pub fn library() -> Library {
    let mut lib = Library::default();
    let mut n: u64 = 0;
    for (artist, album, genre, year, songs) in ALBUMS {
        for (i, title) in songs.iter().enumerate() {
            n += 1;
            let mix = n.wrapping_mul(2654435761) % 1000;
            lib.add(Track {
                path: format!("/sample/{artist}/{album}/{:02} {title}.mp3", i + 1).into(),
                title: (*title).into(),
                artist: (*artist).into(),
                album: (*album).into(),
                genre: (*genre).into(),
                year: Some(*year),
                track_no: Some(i as u32 + 1),
                disc_no: Some(1),
                duration: 150.0 + (mix % 210) as f32,
                size: 3_200_000 + mix * 9_000,
                rating: [0, 0, 3, 4, 5, 0, 2, 4, 0, 5][(mix % 10) as usize],
                plays: (mix % 37) as u32,
                last_played: (!mix.is_multiple_of(3)).then_some(1_790_000_000 + mix * 3600),
                added: 1_780_000_000 + n * 86_400,
                ..Track::default()
            });
        }
    }
    let ids: Vec<u64> = lib.tracks.iter().map(|t| t.id).collect();
    lib.new_playlist("Road Trip", ids.iter().copied().step_by(3).collect());
    lib.new_playlist("Late Night", ids.iter().copied().skip(1).step_by(4).collect());
    lib.new_playlist("Dinner Party", ids.iter().copied().skip(2).step_by(5).collect());
    lib.party.source = Source::Library;
    lib
}

/// The sample's state: a few songs into Party Shuffle, one playing a minute in.
pub fn play(s: &mut State) {
    for _ in 0..4 {
        s.start_party();
        s.next(true);
    }
    if let Some(n) = &mut s.now {
        n.playing = true;
    }
    s.pos = 64.0;
    if let Some(id) = s.rows.get(3).copied() {
        s.table.select(id.to_string());
    }
    s.publish_time();
}
