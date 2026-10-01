//! Login with Authentik (spec §8): the OpenID Connect authorization code
//! flow with PKCE, a nonce and a one-shot `state`. The ID token is verified
//! by `openidconnect` (issuer, audience, expiry, signature against the
//! provider's JWKS, nonce); who may play is then decided by `admit`, a pure
//! function of the verified token's `preferred_username` and `groups`.
//!
//! Provider metadata and keys are fetched on the first login and again
//! after `METADATA_TTL`, so the front starts while Authentik is down.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreProviderMetadata};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet, EndpointSet, IssuerUrl, Nonce,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RequestTokenError, Scope, TokenResponse, reqwest,
};

use crate::config::OidcConfig;

/// Only members of this Authentik group may play.
pub const REQUIRED_GROUP: &str = "signalbox-users";
/// Signed cookie holding a login's `state` between `/auth/login` and
/// `/auth/callback`.
pub const LOGIN_COOKIE: &str = "signalbox_login";
/// A login must come back within this long.
pub const PENDING_TTL: Duration = Duration::from_secs(600);
/// Logins in flight at most; the oldest is dropped beyond this.
pub const MAX_PENDING_LOGINS: usize = 256;
/// Provider metadata and keys are re-fetched after this long.
pub const METADATA_TTL: Duration = Duration::from_secs(3600);
/// Every request to the provider gives up after this long.
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// An error with its sources, `a: b: c` (reqwest hides the useful part in them).
fn chain(e: &dyn std::error::Error) -> String {
    let mut s = e.to_string();
    let mut cause = e.source();
    while let Some(c) = cause {
        s.push_str(": ");
        s.push_str(&c.to_string());
        cause = c.source();
    }
    s
}

/// A client made from discovered metadata (auth URL set, token URL maybe).
type Client = CoreClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointMaybeSet, EndpointMaybeSet>;

/// Why a verified user may not play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Denied {
    /// The token has no (or an empty) `preferred_username`.
    NoUsername,
    /// `groups` does not contain `signalbox-users`.
    NotInGroup,
    /// The username is `robot` or `seed` (any case), names of the automatic
    /// signaller; the supervisor would refuse it every game anyway.
    ReservedName,
}

/// The player name for a verified token, if its user may play.
pub fn admit(preferred_username: Option<&str>, groups: &[String]) -> Result<String, Denied> {
    let name = preferred_username.filter(|n| !n.is_empty()).ok_or(Denied::NoUsername)?;
    if !groups.iter().any(|g| g == REQUIRED_GROUP) {
        return Err(Denied::NotInGroup);
    }
    if crate::supervisor::is_reserved(name) {
        return Err(Denied::ReservedName);
    }
    Ok(name.to_string())
}

/// The `groups` claim from a JWT's payload (absent or null = none). Call it
/// only on a token whose signature and claims were already verified.
pub fn groups_from_id_token(jwt: &str) -> Result<Vec<String>, String> {
    let payload = jwt.split('.').nth(1).ok_or("the ID token is not a JWT")?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).map_err(|e| format!("ID token payload: {e}"))?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| format!("ID token payload: {e}"))?;
    match v.get("groups") {
        None | Some(serde_json::Value::Null) => Ok(vec![]),
        Some(serde_json::Value::Array(a)) => {
            a.iter().map(|g| g.as_str().map(str::to_string).ok_or_else(|| "`groups` holds a non-string".to_string())).collect()
        }
        Some(_) => Err("`groups` is not a list".into()),
    }
}

/// What a login remembers until its callback.
#[derive(Clone, Debug)]
pub struct Pending {
    pub nonce: String,
    pub pkce_verifier: String,
    pub created: Instant,
}

/// Logins in flight, by `state`. Each can be taken once.
#[derive(Default)]
pub struct PendingLogins {
    map: Mutex<BTreeMap<String, Pending>>,
}

impl PendingLogins {
    pub fn new() -> PendingLogins {
        PendingLogins::default()
    }

    pub fn insert_at(&self, state: String, nonce: String, pkce_verifier: String, now: Instant) {
        let mut map = self.map.lock().expect("pending lock");
        map.retain(|_, p| now.saturating_duration_since(p.created) < PENDING_TTL);
        while map.len() >= MAX_PENDING_LOGINS {
            let oldest = map.iter().min_by_key(|(_, p)| p.created).map(|(k, _)| k.clone()).expect("not empty");
            map.remove(&oldest);
        }
        map.insert(state, Pending { nonce, pkce_verifier, created: now });
    }

    /// The login `state` started, if it is still live; it is gone afterwards.
    pub fn take_at(&self, state: &str, now: Instant) -> Option<Pending> {
        let p = self.map.lock().expect("pending lock").remove(state)?;
        (now.saturating_duration_since(p.created) < PENDING_TTL).then_some(p)
    }

    pub fn len(&self) -> usize {
        self.map.lock().expect("pending lock").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoginError {
    /// The provider could not be reached or its metadata is unusable.
    #[error("the login provider is unavailable: {0}")]
    Unavailable(String),
    /// Unknown, expired or already used `state`.
    #[error("unknown or expired login")]
    BadState,
    /// The code exchange failed or the ID token did not verify.
    #[error("the login was not accepted: {0}")]
    Rejected(String),
    /// Verified, but not allowed to play.
    #[error("not allowed: {0:?}")]
    Denied(Denied),
}

pub struct Oidc {
    cfg: OidcConfig,
    redirect: RedirectUrl,
    http: reqwest::Client,
    provider: tokio::sync::Mutex<Option<(CoreProviderMetadata, Instant)>>,
    pub pending: PendingLogins,
}

impl Oidc {
    /// Checks the URLs; talks to nobody.
    pub fn new(cfg: &OidcConfig, public_url: &str) -> Result<Oidc, String> {
        IssuerUrl::new(cfg.issuer.clone()).map_err(|e| format!("OIDC_ISSUER `{}`: {e}", cfg.issuer))?;
        let redirect = RedirectUrl::new(format!("{public_url}/auth/callback"))
            .map_err(|e| format!("SIGNALBOX_PUBLIC_URL `{public_url}`: {e}"))?;
        let http = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        Ok(Oidc { cfg: cfg.clone(), redirect, http, provider: tokio::sync::Mutex::new(None), pending: PendingLogins::new() })
    }

    /// `SIGNALBOX_PUBLIC_URL` + `/auth/callback`.
    pub fn redirect_url(&self) -> &str {
        self.redirect.as_str()
    }

    async fn client(&self) -> Result<Client, LoginError> {
        let mut provider = self.provider.lock().await;
        let fresh = matches!(&*provider, Some((_, at)) if at.elapsed() < METADATA_TTL);
        if !fresh {
            let issuer = IssuerUrl::new(self.cfg.issuer.clone()).map_err(|e| LoginError::Unavailable(e.to_string()))?;
            let meta = CoreProviderMetadata::discover_async(issuer, &self.http)
                .await
                .map_err(|e| LoginError::Unavailable(format!("discovery: {}", chain(&e))))?;
            *provider = Some((meta, Instant::now()));
        }
        let meta = provider.as_ref().expect("fetched above").0.clone();
        Ok(CoreClient::from_provider_metadata(
            meta,
            ClientId::new(self.cfg.client_id.clone()),
            Some(ClientSecret::new(self.cfg.client_secret.clone())),
        )
        .set_redirect_uri(self.redirect.clone()))
    }

    /// Start a login: the provider URL to send the browser to, and the
    /// `state` the caller keeps in the login cookie.
    pub async fn begin(&self) -> Result<(String, String), LoginError> {
        let client = self.client().await?;
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state, nonce) = client
            .authorize_url(CoreAuthenticationFlow::AuthorizationCode, CsrfToken::new_random, Nonce::new_random)
            .add_scope(Scope::new("profile".into()))
            .add_scope(Scope::new("email".into()))
            .set_pkce_challenge(challenge)
            .url();
        let state = state.secret().clone();
        self.pending.insert_at(state.clone(), nonce.secret().clone(), verifier.secret().clone(), Instant::now());
        Ok((url.to_string(), state))
    }

    /// Finish a login: exchange the code, verify the ID token, admit the
    /// user. Returns the player name.
    pub async fn finish(&self, state: &str, code: &str) -> Result<String, LoginError> {
        let pending = self.pending.take_at(state, Instant::now()).ok_or(LoginError::BadState)?;
        let client = self.client().await?;
        let request = client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .map_err(|e| LoginError::Unavailable(format!("the provider has no token endpoint: {e}")))?;
        let token = request.set_pkce_verifier(PkceCodeVerifier::new(pending.pkce_verifier)).request_async(&self.http).await.map_err(
            |e| match e {
                RequestTokenError::Request(e) => LoginError::Unavailable(format!("token request: {}", chain(&e))),
                e => LoginError::Rejected(format!("token request: {}", chain(&e))),
            },
        )?;
        let id_token = token.id_token().ok_or_else(|| LoginError::Rejected("no ID token".into()))?;
        let claims = id_token
            .claims(&client.id_token_verifier(), &Nonce::new(pending.nonce))
            .map_err(|e| LoginError::Rejected(format!("ID token: {e}")))?;
        let groups = groups_from_id_token(&id_token.to_string()).map_err(LoginError::Rejected)?;
        admit(claims.preferred_username().map(|u| u.as_str()), &groups).map_err(LoginError::Denied)
    }
}
