//! The signalbox client's logic (spec D1 §2): connection, lobby, the game
//! you are in, and what clicks mean, behind a `Transport`. No drawing and
//! no browser: tested natively; `client-ui` draws it and `client-web` runs
//! it in a browser.

pub mod app;
pub mod input;
pub mod log;
pub mod select;
pub mod text;
pub mod trains;
pub mod transport;

pub use app::{App, InGame, Link};
pub use input::Target;
pub use transport::{ConnState, MemHandle, MemTransport, Transport};
