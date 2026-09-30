//! Server-side sessions (spec §8): a random id in a signed cookie, the
//! player's name in memory. Lost on restart; players log in again.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum_extra::extract::cookie::Cookie;

pub const SESSION_COOKIE: &str = "signalbox_session";
pub const SESSION_TTL: Duration = Duration::from_secs(12 * 3600);

/// `n` random bytes as lower-case hex.
pub fn random_hex(n: usize) -> String {
    (0..n).map(|_| format!("{:02x}", rand::random::<u8>())).collect()
}

/// A cookie with the attributes every signalbox cookie carries.
pub fn cookie(name: &str, value: &str, max_age_s: u64) -> Cookie<'static> {
    Cookie::parse(format!("{name}={value}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age={max_age_s}"))
        .expect("a well-formed cookie")
}

/// Clears `name` in the browser.
pub fn removal(name: &str) -> Cookie<'static> {
    cookie(name, "", 0)
}

#[derive(Default)]
pub struct Sessions {
    map: Mutex<BTreeMap<String, (String, Instant)>>,
}

impl Sessions {
    pub fn new() -> Sessions {
        Sessions::default()
    }

    /// A new session for `user`; returns its id.
    pub fn create(&self, user: &str) -> String {
        self.create_at(user, Instant::now())
    }

    pub fn create_at(&self, user: &str, now: Instant) -> String {
        let id = random_hex(32);
        let mut map = self.map.lock().expect("sessions lock");
        map.retain(|_, (_, expires)| *expires > now);
        map.insert(id.clone(), (user.to_string(), now + SESSION_TTL));
        id
    }

    /// The user of a live session.
    pub fn user(&self, id: &str) -> Option<String> {
        self.user_at(id, Instant::now())
    }

    pub fn user_at(&self, id: &str, now: Instant) -> Option<String> {
        let mut map = self.map.lock().expect("sessions lock");
        match map.get(id) {
            Some((user, expires)) if *expires > now => Some(user.clone()),
            Some(_) => {
                map.remove(id);
                None
            }
            None => None,
        }
    }

    pub fn remove(&self, id: &str) {
        self.map.lock().expect("sessions lock").remove(id);
    }

    pub fn len(&self) -> usize {
        self.map.lock().expect("sessions lock").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
