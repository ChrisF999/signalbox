//! Lessons the lobby offers (tutorial spec §2): every lesson directory in
//! one directory, read and checked once at startup. A broken lesson is left
//! out with one logged line naming it and why; a missing directory just
//! means no tutorials. The front never fails to start over a lesson.

use std::path::{Path, PathBuf};

use game::lesson::{load_lesson, valid_lesson_id};
use protocol::LessonInfo;

#[derive(Clone, Debug, Default)]
pub struct Lessons {
    /// In id order.
    list: Vec<(LessonInfo, PathBuf)>,
}

impl Lessons {
    /// The valid lessons in `dir`, and a line for each one left out.
    pub fn scan(dir: &Path) -> (Lessons, Vec<String>) {
        let mut list = Vec::new();
        let mut left_out = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return (Lessons::default(), vec![format!("no lessons at {}: no tutorials", dir.display())]);
        };
        let mut dirs: Vec<PathBuf> = entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        dirs.sort();
        for path in dirs {
            let id = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
            if !valid_lesson_id(&id) {
                continue;
            }
            match load_lesson(&path) {
                Ok(l) => {
                    let info = LessonInfo { id, title: l.file.title.clone(), steps: l.file.steps.len() as u32 };
                    list.push((info, path));
                }
                Err(e) => left_out.push(format!("lesson {id} left out: {e}")),
            }
        }
        (Lessons { list }, left_out)
    }

    /// `scan`, logging every lesson left out.
    pub fn load(dir: &Path) -> Lessons {
        let (lessons, left_out) = Lessons::scan(dir);
        for line in left_out {
            eprintln!("signalbox-server: {line}");
        }
        lessons
    }

    pub fn infos(&self) -> Vec<LessonInfo> {
        self.list.iter().map(|(i, _)| i.clone()).collect()
    }

    /// The directory of a listed lesson; `None` for anything else.
    pub fn path(&self, id: &str) -> Option<PathBuf> {
        self.list.iter().find(|(i, _)| i.id == id).map(|(_, p)| p.clone())
    }
}
