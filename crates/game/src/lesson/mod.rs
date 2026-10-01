//! Tutorial lessons (tutorial spec §2): the lesson file, and loading and
//! checking a lesson directory against its world.

mod check;
mod file;

pub use check::{Lesson, MAX_SAY, MAX_STEPS, TABS, UI_CONTROLS, check, load_lesson, parse_lesson, spawn_entry, valid_lesson_id};
pub use file::{Action, Condition, LESSON_SCHEMA, LessonFile, Move, Step};
