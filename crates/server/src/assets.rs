//! The browser client's files (D1 decision 11), read once at start from
//! `SIGNALBOX_WEB`: `index.html` and the plainly named files of `app/`.
//! Requests are answered from memory; no request touches the filesystem.

use std::collections::BTreeMap;
use std::path::Path;

use axum::body::Bytes;

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub body: Bytes,
    pub content_type: &'static str,
    /// A strong ETag, quoted.
    pub etag: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WebAssets {
    pub index: Asset,
    /// By file name, as served under `/app/`.
    pub app: BTreeMap<String, Asset>,
}

/// 1–100 of `A-Z a-z 0-9 _ . -`, not starting with a dot: no paths.
pub fn valid_asset_name(s: &str) -> bool {
    (1..=100).contains(&s.len()) && !s.starts_with('.') && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}

pub fn content_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        _ => "application/octet-stream",
    }
}

/// FNV-1a over the bytes, and the length: changes whenever the file does.
pub fn etag(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("\"{h:016x}-{:x}\"", bytes.len())
}

fn asset(path: &Path, name: &str) -> Result<Asset, String> {
    let body = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Asset { etag: etag(&body), content_type: content_type(name), body: Bytes::from(body) })
}

impl WebAssets {
    /// `Ok(None)` when `dir` does not exist (dev and test fronts keep the
    /// placeholder page); an error when it exists without `index.html` or
    /// a file cannot be read.
    pub fn load(dir: &Path) -> Result<Option<WebAssets>, String> {
        match std::fs::metadata(dir) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("{}: {e}", dir.display())),
        }
        let index_path = dir.join("index.html");
        let meta = std::fs::symlink_metadata(&index_path).map_err(|e| format!("{}: {e}", index_path.display()))?;
        if meta.file_type().is_symlink() {
            return Err(format!("{} is a symlink; refusing to serve it", index_path.display()));
        }
        if !meta.is_file() {
            return Err(format!("{} is not a regular file", index_path.display()));
        }
        let index = asset(&index_path, "index.html")?;
        let mut app = BTreeMap::new();
        let app_dir = dir.join("app");
        if app_dir.is_dir() {
            let entries = std::fs::read_dir(&app_dir).map_err(|e| format!("{}: {e}", app_dir.display()))?;
            for entry in entries {
                let entry = entry.map_err(|e| format!("{}: {e}", app_dir.display()))?;
                let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
                let is_file = entry.file_type().map_err(|e| format!("{}: {e}", entry.path().display()))?.is_file();
                if valid_asset_name(&name) && is_file {
                    app.insert(name.clone(), asset(&entry.path(), &name)?);
                } else {
                    eprintln!("signalbox-server: web: skipping {} (not a plain regular file)", entry.path().display());
                }
            }
        }
        Ok(Some(WebAssets { index, app }))
    }
}
