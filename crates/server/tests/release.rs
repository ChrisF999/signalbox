//! The release build (no `dev-auth`): the dev login does not exist.

#![cfg(not(feature = "dev-auth"))]

use std::path::PathBuf;

use bot::net::{Conn, NetError, http_get};
use server::config::{Config, OidcConfig};

#[tokio::test]
async fn the_release_build_has_no_dev_login() {
    let root = std::env::temp_dir().join(format!("sbx-release-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("layouts")).unwrap();
    let cfg = Config {
        addr: "127.0.0.1:0".parse().unwrap(),
        data_dir: root.join("data"),
        layouts_dir: root.join("layouts"),
        public_url: "http://127.0.0.1".into(),
        // Nothing listens on port 9: the provider is only asked on a login.
        oidc: Some(OidcConfig { issuer: "http://127.0.0.1:9/".into(), client_id: "sbx".into(), client_secret: "x".into() }),
        session_key: vec![7; 64],
        game_bin: PathBuf::from(env!("CARGO_BIN_EXE_signalbox-game")),
        web_dir: root.join("web"),
    };
    let running = server::start(cfg).await.unwrap();
    let base = running.base();
    let r = http_get(&base, "/auth/dev?user=ann", None).await.unwrap();
    assert_eq!(r.status, 404);
    assert!(r.header("set-cookie").is_none());
    let e = Conn::connect(&base, None).await.err().expect("refused");
    assert!(matches!(e, NetError::Status(401)), "{e}");
    running.stop().await;
}
