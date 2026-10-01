//! Tutorial lessons (tutorial spec §2–§3): the lesson file, loading and
//! checking a lesson directory, and running one over a `Game`.

mod check;
mod file;
mod run;

pub use check::{Lesson, MAX_SAY, MAX_STEPS, TABS, UI_CONTROLS, check, load_lesson, parse_lesson, spawn_entry, valid_lesson_id};
pub use file::{Action, Condition, LESSON_SCHEMA, LessonFile, Move, Step};
pub use run::{COLLISION_ALERT, LESSON_SEED, Runner, SPAD_ALERT, start};
