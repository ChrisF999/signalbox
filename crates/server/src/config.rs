//! Configuration from the environment (spec §8, §10). Every problem is one
//! clear line; the binary prints it and exits 2.

use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OidcConfig {
    /// e.g. `https://auth.skyes.lgbt/application/o/signalbox/`
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub addr: SocketAddr,
    /// Holds `saves/` and `sockets/`.
    pub data_dir: PathBuf,
    /// Converted worlds, `<name>.json`.
    pub layouts_dir: PathBuf,
    /// Where browsers reach us, without a trailing `/`; the OIDC redirect
    /// URI is this + `/auth/callback`.
    pub public_url: String,
    /// `None` only in `dev-auth` builds.
    pub oidc: Option<OidcConfig>,
    /// Cookie signing key, at least 64 bytes.
    pub session_key: Vec<u8>,
    pub game_bin: PathBuf,
}

pub const DEFAULT_ADDR: &str = "0.0.0.0:9160";
pub const DEFAULT_DATA: &str = "/data";
pub const DEFAULT_LAYOUTS: &str = "/opt/signalbox/layouts";
pub const MIN_KEY_BYTES: usize = 64;

pub fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

impl Config {
    pub fn from_env() -> Result<Config, String> {
        Config::from_lookup(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
    }

    /// `get` answers one variable (tests pass a map).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Config, String> {
        let addr = get("SIGNALBOX_ADDR").unwrap_or_else(|| DEFAULT_ADDR.into());
        let addr: SocketAddr = addr.parse().map_err(|_| format!("SIGNALBOX_ADDR `{addr}` is not host:port"))?;
        let data_dir = PathBuf::from(get("SIGNALBOX_DATA").unwrap_or_else(|| DEFAULT_DATA.into()));
        let layouts_dir = PathBuf::from(get("SIGNALBOX_LAYOUTS").unwrap_or_else(|| DEFAULT_LAYOUTS.into()));
        let key_hex = get("SIGNALBOX_SESSION_KEY").ok_or("SIGNALBOX_SESSION_KEY is required (hex, at least 64 bytes)")?;
        let session_key = decode_hex(&key_hex).ok_or("SIGNALBOX_SESSION_KEY is not hex")?;
        if session_key.len() < MIN_KEY_BYTES {
            return Err(format!("SIGNALBOX_SESSION_KEY has {} bytes; it needs at least {MIN_KEY_BYTES}", session_key.len()));
        }
        let oidc = match (get("OIDC_ISSUER"), get("OIDC_CLIENT_ID"), get("OIDC_CLIENT_SECRET")) {
            (Some(issuer), Some(client_id), Some(client_secret)) => Some(OidcConfig { issuer, client_id, client_secret }),
            (None, None, None) if cfg!(feature = "dev-auth") => None,
            _ => return Err("OIDC_ISSUER, OIDC_CLIENT_ID and OIDC_CLIENT_SECRET are all required".into()),
        };
        let public_url = match get("SIGNALBOX_PUBLIC_URL") {
            Some(u) if u.starts_with("https://") || u.starts_with("http://") => u.trim_end_matches('/').to_string(),
            Some(u) => return Err(format!("SIGNALBOX_PUBLIC_URL `{u}` must start with https:// or http://")),
            None if oidc.is_none() => format!("http://{addr}"),
            None => return Err("SIGNALBOX_PUBLIC_URL is required with OIDC".into()),
        };
        let game_bin = match get("SIGNALBOX_GAME_BIN") {
            Some(p) => PathBuf::from(p),
            None => std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(|d| d.join("signalbox-game")))
                .ok_or("cannot find signalbox-game; set SIGNALBOX_GAME_BIN")?,
        };
        Ok(Config { addr, data_dir, layouts_dir, public_url, oidc, session_key, game_bin })
    }
}
