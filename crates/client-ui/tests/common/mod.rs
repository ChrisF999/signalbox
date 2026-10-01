//! Drawn twobox layouts and views, from a real in-process game.

#![allow(dead_code)]

use game::{Game, GameMeta};
use protocol::{ClientMsg, Layout, View};
use serde_json::Value;
use signalbox_core::world::World;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../game/tests/fixtures");

pub fn s(x: &str) -> String {
    x.to_string()
}

/// twobox with `twobox-layout.json` as its drawing.
pub fn drawn_twobox() -> World {
    drawn_twobox_with(|_| {})
}

/// `drawn_twobox`, its world JSON changed by `f` first.
pub fn drawn_twobox_with(f: impl FnOnce(&mut Value)) -> World {
    let mut w: Value = serde_json::from_str(&std::fs::read_to_string(format!("{FIXTURES}/twobox.json")).unwrap()).unwrap();
    w["layout"] = serde_json::from_str(&std::fs::read_to_string(format!("{FIXTURES}/twobox-layout.json")).unwrap()).unwrap();
    f(&mut w);
    World::from_json(&w.to_string()).unwrap()
}

/// A game with "ann" holding `area` (a spectator for `None`).
pub fn game_for(area: Option<&str>) -> Game {
    let mut g = Game::new(drawn_twobox(), GameMeta { layout: s("twobox"), seed: 1 });
    g.connect("ann");
    if let Some(a) = area {
        g.handle("ann", ClientMsg::Claim { area: s(a) });
    }
    g
}

pub fn layout_for(area: Option<&str>) -> Layout {
    game_for(area).layout_of("ann").unwrap()
}

pub fn view_for(area: Option<&str>) -> View {
    game_for(area).view_of("ann").unwrap()
}
