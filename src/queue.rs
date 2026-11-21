//! What plays next outside Party Shuffle: the list a song was started from, in the order it
//! showed (or shuffled), with repeat off, all or one.

use rand::Rng;
use rand::seq::SliceRandom;
use serde::{Deserialize, Serialize};

use crate::library::TrackId;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

impl Repeat {
    pub fn next(self) -> Repeat {
        match self {
            Repeat::Off => Repeat::All,
            Repeat::All => Repeat::One,
            Repeat::One => Repeat::Off,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Queue {
    /// The list as it showed when a song in it was started.
    pub list: Vec<TrackId>,
    /// The order it plays in: `list`, or `list` shuffled with the started song first.
    pub order: Vec<TrackId>,
    pub pos: usize,
}

impl Queue {
    pub fn start(list: Vec<TrackId>, at: usize, shuffle: bool, rng: &mut impl Rng) -> Queue {
        let mut q = Queue { order: list.clone(), list, pos: at };
        if shuffle {
            q.shuffle(rng);
        }
        q
    }

    pub fn current(&self) -> Option<TrackId> {
        self.order.get(self.pos).copied()
    }

    /// Shuffles what's left around the song playing, which stays first.
    pub fn shuffle(&mut self, rng: &mut impl Rng) {
        let now = self.current();
        let mut rest: Vec<TrackId> = self.list.clone();
        if let Some(n) = now
            && let Some(i) = rest.iter().position(|&t| t == n)
        {
            rest.remove(i);
        }
        rest.shuffle(rng);
        self.order = now.into_iter().chain(rest).collect();
        self.pos = 0;
    }

    /// Back to the list's own order, still on the song playing.
    pub fn unshuffle(&mut self) {
        let now = self.current();
        self.order = self.list.clone();
        self.pos = now.and_then(|n| self.order.iter().position(|&t| t == n)).unwrap_or(0);
    }

    /// The song after this one. `ended` is true when the song played to its end, where repeat
    /// one plays it again; a press of Next moves on regardless.
    pub fn next(&mut self, repeat: Repeat, ended: bool) -> Option<TrackId> {
        if self.order.is_empty() {
            return None;
        }
        if ended && repeat == Repeat::One {
            return self.current();
        }
        if self.pos + 1 < self.order.len() {
            self.pos += 1;
        } else if repeat == Repeat::Off && ended {
            return None;
        } else {
            self.pos = 0;
        }
        self.current()
    }

    /// What [`Queue::next`] would give for a song playing to its end, without moving on.
    pub fn peek(&self, repeat: Repeat) -> Option<TrackId> {
        self.clone().next(repeat, true)
    }

    pub fn prev(&mut self, repeat: Repeat) -> Option<TrackId> {
        if self.order.is_empty() {
            return None;
        }
        if self.pos > 0 {
            self.pos -= 1;
        } else if repeat != Repeat::Off {
            self.pos = self.order.len() - 1;
        }
        self.current()
    }

    /// Drops songs that left the library.
    pub fn retain(&mut self, keep: impl Fn(TrackId) -> bool) {
        let now = self.current();
        self.list.retain(|&t| keep(t));
        self.order.retain(|&t| keep(t));
        self.pos = now.and_then(|n| self.order.iter().position(|&t| t == n)).unwrap_or(0).min(self.order.len().saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::*;

    #[test]
    fn plays_through_and_stops_or_wraps() {
        let mut rng = StdRng::seed_from_u64(1);
        let mut q = Queue::start(vec![1, 2, 3], 1, false, &mut rng);
        assert_eq!(q.current(), Some(2));
        assert_eq!(q.next(Repeat::Off, true), Some(3));
        assert_eq!(q.next(Repeat::Off, true), None);
        assert_eq!(q.next(Repeat::All, true), Some(1));
        assert_eq!(q.next(Repeat::One, true), Some(1));
        assert_eq!(q.next(Repeat::One, false), Some(2));
        assert_eq!(q.prev(Repeat::Off), Some(1));
        assert_eq!(q.prev(Repeat::Off), Some(1));
        assert_eq!(q.prev(Repeat::All), Some(3));
        assert_eq!(q.peek(Repeat::Off), None);
        assert_eq!(q.peek(Repeat::All), Some(1));
        assert_eq!(q.peek(Repeat::One), Some(3));
        assert_eq!(q.current(), Some(3));
    }

    #[test]
    fn shuffle_keeps_the_started_song_first_and_every_song_once() {
        let mut rng = StdRng::seed_from_u64(2);
        let list: Vec<TrackId> = (1..=50).collect();
        let mut q = Queue::start(list.clone(), 9, true, &mut rng);
        assert_eq!(q.current(), Some(10));
        let mut sorted = q.order.clone();
        sorted.sort();
        assert_eq!(sorted, list);
        assert_ne!(q.order, list);
        q.next(Repeat::Off, true);
        let now = q.current();
        q.unshuffle();
        assert_eq!(q.current(), now);
        assert_eq!(q.order, list);
    }

    #[test]
    fn removed_songs_leave_the_queue_and_it_stays_on_the_current_one() {
        let mut rng = StdRng::seed_from_u64(3);
        let mut q = Queue::start(vec![1, 2, 3, 4], 2, false, &mut rng);
        q.retain(|t| t != 1);
        assert_eq!(q.current(), Some(3));
        q.retain(|t| t != 3);
        assert!(q.current().is_some());
    }
}
