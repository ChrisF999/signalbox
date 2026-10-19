//! The players panel's data (players-panel spec): for every area who holds
//! it (a player, or the robot) and whether that player's connection is up,
//! then the players who hold nothing and are watching. Derived from the
//! view alone, so it follows every delta; no state of its own.

use std::collections::BTreeMap;

use protocol::View;

use crate::app::ROBOT;

/// A human in the game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub name: String,
    /// This client's own player.
    pub you: bool,
    /// Connection dropped: a holder keeping the area through the grace period.
    pub away: bool,
}

/// Who works an area.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Holder {
    Robot,
    Player(Person),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AreaRow {
    pub area: String,
    /// The workstation letter, on layouts with more than one area.
    pub workstation: Option<String>,
    pub holder: Holder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Roster {
    /// Every area, in the layout's order.
    pub areas: Vec<AreaRow>,
    /// Players holding no area, in name order. You are among them while you watch.
    pub watching: Vec<Person>,
}

impl Roster {
    /// `areas` in layout order; `workstations` as the layout lists them.
    pub fn new(areas: &[String], workstations: &BTreeMap<String, String>, view: &View, you: &str) -> Roster {
        // A server from before the players list says nothing about presence:
        // everyone named is taken to be there.
        let away = |name: &str| view.players.get(name) == Some(&false);
        let person = |name: &str| Person { name: name.to_string(), you: name == you, away: away(name) };
        let single = areas.len() <= 1;
        let rows: Vec<AreaRow> = areas
            .iter()
            .map(|a| AreaRow {
                area: a.clone(),
                workstation: workstations.get(a).filter(|_| !single).cloned(),
                holder: match view.holders.get(a).map(String::as_str) {
                    None | Some(ROBOT) => Holder::Robot,
                    Some(name) => Holder::Player(person(name)),
                },
            })
            .collect();
        let holding = |name: &str| rows.iter().any(|r| matches!(&r.holder, Holder::Player(p) if p.name == name));
        let mut watching: Vec<Person> = view.players.keys().filter(|n| !holding(n)).map(|n| person(n)).collect();
        if !holding(you) && !watching.iter().any(|p| p.you) {
            watching.push(person(you));
            watching.sort_by(|a, b| a.name.cmp(&b.name));
        }
        Roster { areas: rows, watching }
    }

    /// Humans connected now (holders who dropped are not counted).
    pub fn online(&self) -> usize {
        let held = self.areas.iter().filter_map(|r| match &r.holder {
            Holder::Player(p) => Some(p),
            Holder::Robot => None,
        });
        held.chain(self.watching.iter()).filter(|p| !p.away).count()
    }

    /// Areas the robot works.
    pub fn robot_areas(&self) -> usize {
        self.areas.iter().filter(|r| r.holder == Holder::Robot).count()
    }

    /// The panel's heading count: `1 player`, `3 players`.
    pub fn count_text(&self) -> String {
        let n = self.online();
        format!("{n} {}", if n == 1 { "player" } else { "players" })
    }
}
