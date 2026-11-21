//! The HTTP client the online features share: a user agent that names Stereo, as MusicBrainz
//! asks, and a timeout so a dead network never holds a worker for long. Status codes come
//! back as answers, not errors, so a service's own error message can be read.

use std::sync::OnceLock;
use std::time::Duration;

pub fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::config_builder()
            .user_agent(concat!("Stereo/", env!("CARGO_PKG_VERSION"), " ( https://github.com/caulifluoride/stereo )"))
            .timeout_global(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .build()
            .into()
    })
}
