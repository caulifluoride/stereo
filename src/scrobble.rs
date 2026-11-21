//! Scrobbling: telling Last.fm and ListenBrainz what's playing and what played. A song counts
//! once it's longer than 30 seconds and has played half its length or four minutes, the rule
//! both services ask for. Requests go out on a worker thread; scrobbles that fail (offline, a
//! service down) wait there and go out again with the next one, or a minute later.

use std::collections::BTreeMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use md5::{Digest, Md5};
use serde_json::{Value, json};

use crate::library::Track;
use crate::settings::Settings;
use crate::{Job, net};

const LASTFM: &str = "https://ws.audioscrobbler.com/2.0/";
const LISTENBRAINZ: &str = "https://api.listenbrainz.org/1/submit-listens";

/// Whether `pos` seconds into a song `len` long it has played enough to scrobble.
pub fn due(pos: f32, len: f32) -> bool {
    len > 30.0 && (pos >= len / 2.0 || pos >= 240.0)
}

/// Where to send: the Last.fm session and the ListenBrainz token, whichever are set up.
#[derive(Clone, Debug, Default)]
struct To {
    lastfm: Option<(String, String, String)>,
    listenbrainz: Option<String>,
}

impl To {
    fn from(s: &Settings) -> To {
        let f = &s.lastfm;
        To {
            lastfm: f.session.clone().filter(|_| !f.api_key.is_empty() && !f.secret.is_empty()).map(|sk| (f.api_key.clone(), f.secret.clone(), sk)),
            listenbrainz: Some(s.listenbrainz.trim().to_string()).filter(|t| !t.is_empty()),
        }
    }

    fn any(&self) -> bool {
        self.lastfm.is_some() || self.listenbrainz.is_some()
    }
}

#[derive(Clone, Debug)]
struct Song {
    artist: String,
    title: String,
    album: String,
    album_artist: String,
    duration: f32,
    track_no: Option<u32>,
}

impl Song {
    fn of(t: &Track) -> Song {
        Song {
            artist: t.artist.clone(),
            title: t.title.clone(),
            album: t.album.clone(),
            album_artist: t.album_artist.clone(),
            duration: t.duration,
            track_no: t.track_no,
        }
    }
}

enum Work {
    NowPlaying(To, Song),
    Scrobble(To, Song, u64),
    Login { api_key: String, secret: String, user: String, password: String },
}

pub struct Scrobbler {
    tx: Option<Sender<Work>>,
}

impl Scrobbler {
    pub fn none() -> Scrobbler {
        Scrobbler { tx: None }
    }

    /// The worker, reporting results through `jobs`.
    pub fn start(jobs: Sender<Job>, wake: rune::Waker) -> Scrobbler {
        let (tx, rx) = channel();
        let ok = std::thread::Builder::new().name("stereo-scrobble".into()).spawn(move || run(rx, jobs, wake)).is_ok();
        Scrobbler { tx: ok.then_some(tx) }
    }

    fn send(&self, w: Work) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(w);
        }
    }

    pub fn now_playing(&self, s: &Settings, t: &Track) {
        let to = To::from(s);
        if to.any() {
            self.send(Work::NowPlaying(to, Song::of(t)));
        }
    }

    /// Scrobbles `t`, which started at Unix second `started`.
    pub fn scrobble(&self, s: &Settings, t: &Track, started: u64) {
        let to = To::from(s);
        if to.any() {
            self.send(Work::Scrobble(to, Song::of(t), started));
        }
    }

    /// Signs in to Last.fm, answering with [`Job::LastFm`]: the session key, or why not.
    pub fn login(&self, api_key: &str, secret: &str, user: &str, password: &str) {
        self.send(Work::Login { api_key: api_key.trim().into(), secret: secret.trim().into(), user: user.trim().into(), password: password.into() });
    }
}

/// Last.fm's request signature: the parameters sorted by name and run together, then the
/// secret, through MD5.
fn signature(params: &BTreeMap<&str, String>, secret: &str) -> String {
    let mut h = Md5::new();
    for (k, v) in params {
        h.update(k.as_bytes());
        h.update(v.as_bytes());
    }
    h.update(secret.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn lastfm(method: &str, mut params: BTreeMap<&'static str, String>, api_key: &str, secret: &str) -> Result<Value, String> {
    params.insert("method", method.to_string());
    params.insert("api_key", api_key.to_string());
    let sig = signature(&params, secret);
    params.insert("api_sig", sig);
    params.insert("format", "json".into());
    let mut resp = net::agent().post(LASTFM).send_form(params.iter().map(|(k, v)| (*k, v.as_str()))).map_err(|e| e.to_string())?;
    let body: Value = resp.body_mut().read_json().map_err(|e| e.to_string())?;
    if let Some(msg) = body.get("message").and_then(Value::as_str).filter(|_| body.get("error").is_some()) {
        return Err(msg.to_string());
    }
    if !resp.status().is_success() {
        return Err(format!("Last.fm answered {}", resp.status()));
    }
    Ok(body)
}

fn lastfm_song(song: &Song) -> BTreeMap<&'static str, String> {
    let mut p = BTreeMap::new();
    p.insert("artist", song.artist.clone());
    p.insert("track", song.title.clone());
    if !song.album.is_empty() {
        p.insert("album", song.album.clone());
    }
    if !song.album_artist.is_empty() {
        p.insert("albumArtist", song.album_artist.clone());
    }
    if song.duration > 0.0 {
        p.insert("duration", (song.duration as u64).to_string());
    }
    if let Some(n) = song.track_no {
        p.insert("trackNumber", n.to_string());
    }
    p
}

fn listenbrainz(token: &str, kind: &str, song: &Song, at: Option<u64>) -> Result<(), String> {
    let mut listen = json!({
        "track_metadata": {
            "artist_name": song.artist,
            "track_name": song.title,
            "additional_info": { "media_player": "Stereo", "submission_client": "Stereo", "submission_client_version": env!("CARGO_PKG_VERSION") },
        }
    });
    if !song.album.is_empty() {
        listen["track_metadata"]["release_name"] = json!(song.album);
    }
    if song.duration > 0.0 {
        listen["track_metadata"]["additional_info"]["duration_ms"] = json!((song.duration * 1000.0) as u64);
    }
    if let Some(at) = at {
        listen["listened_at"] = json!(at);
    }
    let resp = net::agent()
        .post(LISTENBRAINZ)
        .header("Authorization", format!("Token {token}"))
        .send_json(json!({ "listen_type": kind, "payload": [listen] }))
        .map_err(|e| e.to_string())?;
    if resp.status().is_success() { Ok(()) } else { Err(format!("ListenBrainz answered {}", resp.status())) }
}

/// Sends one scrobble to every service set up. Returns the services it failed for.
fn scrobble(to: &To, song: &Song, at: u64) -> (To, Option<String>) {
    let mut failed = To::default();
    let mut why = None;
    if let Some((key, secret, sk)) = &to.lastfm {
        let mut p = lastfm_song(song);
        p.insert("timestamp", at.to_string());
        p.insert("sk", sk.clone());
        if let Err(e) = lastfm("track.scrobble", p, key, secret) {
            failed.lastfm = to.lastfm.clone();
            why = Some(format!("Last.fm: {e}"));
        }
    }
    if let Some(token) = &to.listenbrainz
        && let Err(e) = listenbrainz(token, "single", song, Some(at))
    {
        failed.listenbrainz = Some(token.clone());
        why = Some(format!("ListenBrainz: {e}"));
    }
    (failed, why)
}

fn run(rx: Receiver<Work>, jobs: Sender<Job>, wake: rune::Waker) {
    let report = |j: Job| {
        let _ = jobs.send(j);
        wake();
    };
    // Scrobbles waiting to go out again.
    let mut waiting: Vec<(To, Song, u64)> = Vec::new();
    loop {
        let work = match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(w) => Some(w),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        match work {
            Some(Work::NowPlaying(to, song)) => {
                if let Some((key, secret, sk)) = &to.lastfm {
                    let mut p = lastfm_song(&song);
                    p.insert("sk", sk.clone());
                    let _ = lastfm("track.updateNowPlaying", p, key, secret);
                }
                if let Some(token) = &to.listenbrainz {
                    let _ = listenbrainz(token, "playing_now", &song, None);
                }
            }
            Some(Work::Scrobble(to, song, at)) => waiting.push((to, song, at)),
            Some(Work::Login { api_key, secret, user, password }) => {
                let mut p = BTreeMap::new();
                p.insert("username", user);
                p.insert("password", password);
                let answer = lastfm("auth.getMobileSession", p, &api_key, &secret)
                    .and_then(|v| v["session"]["key"].as_str().map(str::to_string).ok_or_else(|| "Last.fm sent no session".to_string()));
                report(Job::LastFm(answer));
            }
            None => {}
        }
        let mut still = Vec::new();
        let mut last_error = None;
        for (to, song, at) in waiting.drain(..) {
            let (failed, why) = scrobble(&to, &song, at);
            if failed.any() {
                still.push((failed, song, at));
            }
            last_error = why.or(last_error);
        }
        // A day's worth at most; older ones are past what the services take anyway.
        still.truncate(500);
        waiting = still;
        if let Some(e) = last_error {
            report(Job::ScrobbleFailed(e));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_song_scrobbles_halfway_or_at_four_minutes() {
        assert!(!due(10.0, 25.0), "too short to count");
        assert!(!due(60.0, 200.0));
        assert!(due(100.0, 200.0));
        assert!(due(240.0, 900.0));
        assert!(!due(239.0, 900.0));
    }

    #[test]
    fn signatures_follow_lastfms_recipe() {
        let mut p = BTreeMap::new();
        p.insert("method", "auth.getMobileSession".to_string());
        p.insert("api_key", "key".to_string());
        p.insert("username", "me".to_string());
        // md5("api_keykeymethodauth.getMobileSessionusernamemesecret")
        let mut h = Md5::new();
        h.update(b"api_keykeymethodauth.getMobileSessionusernamemesecret");
        let want: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(signature(&p, "secret"), want);
    }
}
