//! The browser client's files (D1 decision 11), read once at start from
//! `SIGNALBOX_WEB`: `index.html` and the plainly named files of `app/`.
//! Requests are answered from memory; no request touches the filesystem.
//! `scripts/build-web.sh` writes a brotli (`<file>.br`) and a gzip
//! (`<file>.gz`) copy beside each file; they are kept with their file and
//! served in its place to a browser that accepts them, never under their
//! own names.

use std::collections::BTreeMap;
use std::path::Path;

use axum::body::Bytes;

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub body: Bytes,
    pub content_type: &'static str,
    /// A strong ETag, quoted.
    pub etag: String,
    /// The brotli copy (`<file>.br`), with its own ETag.
    pub br: Option<Encoded>,
    /// The gzip copy (`<file>.gz`), with its own ETag.
    pub gzip: Option<Encoded>,
}

/// A precompressed copy of an asset.
#[derive(Clone, Debug, PartialEq)]
pub struct Encoded {
    pub body: Bytes,
    /// A strong ETag of these bytes, quoted.
    pub etag: String,
}

/// The content coding a response is sent with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coding {
    Br,
    Gzip,
    Identity,
}

/// Brotli if the client accepts it and there is a copy, else gzip, else
/// the file as it is. A coding counts as accepted when `Accept-Encoding`
/// lists it (or `*` and not it) with a q-value above 0; an unreadable
/// q-value counts as 0. The client's own order and q-values are otherwise
/// ignored: the brotli copy is always the smallest.
pub fn pick_coding(accept_encoding: Option<&str>, has_br: bool, has_gzip: bool) -> Coding {
    let Some(ae) = accept_encoding else { return Coding::Identity };
    // The q-value of each listed coding, lower-cased; `x-gzip` is gzip.
    let mut listed: Vec<(String, bool)> = Vec::new();
    for item in ae.split(',') {
        let mut parts = item.split(';');
        let name = parts.next().unwrap_or("").trim().to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        let mut ok = true;
        for param in parts {
            if let Some((k, v)) = param.split_once('=') {
                if k.trim().eq_ignore_ascii_case("q") {
                    ok = v.trim().parse::<f32>().is_ok_and(|q| q > 0.0);
                }
            }
        }
        let name = if name == "x-gzip" { "gzip".to_string() } else { name };
        listed.push((name, ok));
    }
    let accepts = |coding: &str| match listed.iter().find(|(n, _)| n == coding) {
        Some((_, ok)) => *ok,
        None => listed.iter().find(|(n, _)| n == "*").is_some_and(|(_, ok)| *ok),
    };
    if has_br && accepts("br") {
        Coding::Br
    } else if has_gzip && accepts("gzip") {
        Coding::Gzip
    } else {
        Coding::Identity
    }
}

impl Asset {
    /// What to send for this `Accept-Encoding`: the coding (for
    /// `Content-Encoding`), the bytes and their ETag.
    pub fn negotiate(&self, accept_encoding: Option<&str>) -> (Coding, &Bytes, &str) {
        match pick_coding(accept_encoding, self.br.is_some(), self.gzip.is_some()) {
            Coding::Br => {
                let e = self.br.as_ref().expect("picked only when present");
                (Coding::Br, &e.body, &e.etag)
            }
            Coding::Gzip => {
                let e = self.gzip.as_ref().expect("picked only when present");
                (Coding::Gzip, &e.body, &e.etag)
            }
            Coding::Identity => (Coding::Identity, &self.body, &self.etag),
        }
    }
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

/// The suffixes of the precompressed copies.
const COPIES: [&str; 2] = [".br", ".gz"];

fn read(path: &Path) -> Result<Encoded, String> {
    let body = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Encoded { etag: etag(&body), body: Bytes::from(body) })
}

/// The file `name` at `path`, with whichever copies `copy` finds for it.
fn asset(
    path: &Path,
    name: &str,
    mut copy: impl FnMut(&str) -> Result<Option<std::path::PathBuf>, String>,
) -> Result<Asset, String> {
    let Encoded { body, etag } = read(path)?;
    let br = copy(&format!("{name}.br"))?.map(|p| read(&p)).transpose()?;
    let gzip = copy(&format!("{name}.gz"))?.map(|p| read(&p)).transpose()?;
    Ok(Asset { body, content_type: content_type(name), etag, br, gzip })
}

/// `path` if it is a regular file, `None` if it does not exist, an error
/// if it is a symlink or anything else.
fn plain_file(path: &Path) -> Result<Option<std::path::PathBuf>, String> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    if meta.file_type().is_symlink() {
        return Err(format!("{} is a symlink; refusing to serve it", path.display()));
    }
    if !meta.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    Ok(Some(path.to_path_buf()))
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
        let index_path = plain_file(&index_path)?.ok_or_else(|| format!("{}: no such file", index_path.display()))?;
        let index = asset(&index_path, "index.html", |copy| plain_file(&dir.join(copy)))?;
        let mut app = BTreeMap::new();
        let app_dir = dir.join("app");
        if app_dir.is_dir() {
            // Every plain regular file by name, then each with its copies.
            let mut files = BTreeMap::new();
            let entries = std::fs::read_dir(&app_dir).map_err(|e| format!("{}: {e}", app_dir.display()))?;
            for entry in entries {
                let entry = entry.map_err(|e| format!("{}: {e}", app_dir.display()))?;
                let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
                let is_file = entry.file_type().map_err(|e| format!("{}: {e}", entry.path().display()))?.is_file();
                if valid_asset_name(&name) && is_file {
                    files.insert(name, entry.path());
                } else {
                    eprintln!("signalbox-server: web: skipping {} (not a plain regular file)", entry.path().display());
                }
            }
            for (name, path) in &files {
                if let Some(base) = COPIES.iter().find_map(|sfx| name.strip_suffix(sfx)) {
                    if !files.contains_key(base) {
                        eprintln!("signalbox-server: web: skipping {} (a copy of no file)", path.display());
                    }
                    continue;
                }
                app.insert(name.clone(), asset(path, name, |copy| Ok(files.get(copy).cloned()))?);
            }
        }
        Ok(Some(WebAssets { index, app }))
    }
}
