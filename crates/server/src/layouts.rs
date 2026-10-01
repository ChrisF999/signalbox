//! Layouts the lobby offers (converted worlds in one directory, read once at
//! startup) and the names the front accepts from clients.

use std::path::{Path, PathBuf};

use protocol::LayoutInfo;

/// A layout name: 1–40 of `a-z`, `0-9`, `-` (it names a file).
pub fn valid_layout_name(s: &str) -> bool {
    (1..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A game id: `g-` and 12 of `a-z2-7` (spec §2.2; it names a file).
pub fn valid_game_id(s: &str) -> bool {
    s.len() == 14 && s.starts_with("g-") && s.bytes().skip(2).all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
}

pub fn new_game_id() -> String {
    const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
    let tail: String = (0..12).map(|_| ALPHABET[usize::from(rand::random::<u8>() % 32)] as char).collect();
    format!("g-{tail}")
}

/// A world's area names, in world order. Only `areas[].name` is read:
/// the rest of the world is skipped without being built, so the front never
/// holds a whole world in memory (Liverpool Street's is about 8 MB as a
/// `serde_json::Value`).
fn area_names(text: &str) -> Result<Vec<String>, serde_json::Error> {
    #[derive(serde::Deserialize)]
    struct World {
        areas: Vec<Area>,
    }
    #[derive(serde::Deserialize)]
    struct Area {
        name: String,
    }
    let w: World = serde_json::from_str(text)?;
    Ok(w.areas.into_iter().map(|a| a.name).collect())
}

#[derive(Clone, Debug)]
pub struct Layouts {
    dir: PathBuf,
    list: Vec<LayoutInfo>,
}

impl Layouts {
    /// Every `<name>.json` in `dir` whose name is valid, in name order. A
    /// file that is not a world with named areas is an error: the image is
    /// broken and the front should not start.
    pub fn load(dir: &Path) -> Result<Layouts, String> {
        let entries = std::fs::read_dir(dir).map_err(|e| format!("layouts {}: {e}", dir.display()))?;
        let mut list = Vec::new();
        for entry in entries {
            let path = entry.map_err(|e| format!("layouts {}: {e}", dir.display()))?.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_suffix(".json")) else { continue };
            if !valid_layout_name(name) {
                continue;
            }
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let areas = area_names(&text).map_err(|e| match e.is_data() {
                true => format!("{}: no named areas ({e})", path.display()),
                false => format!("{}: {e}", path.display()),
            })?;
            list.push(LayoutInfo { name: name.to_string(), areas });
        }
        list.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Layouts { dir: dir.to_path_buf(), list })
    }

    pub fn infos(&self) -> Vec<LayoutInfo> {
        self.list.clone()
    }

    /// The world file of a listed layout; `None` for anything else.
    pub fn path(&self, name: &str) -> Option<PathBuf> {
        self.list.iter().any(|l| l.name == name).then(|| self.dir.join(format!("{name}.json")))
    }
}
