//! The train describer: headcodes in berths.

use serde::{Deserialize, Serialize};

use crate::events::Event;
use crate::ids::BerthId;
use crate::network::Network;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Describer {
    pub berths: Vec<Option<String>>,
}

impl Describer {
    pub fn new(net: &Network) -> Self {
        Describer { berths: vec![None; net.berths.len()] }
    }

    pub fn get(&self, b: BerthId) -> Option<&str> {
        self.berths[b.idx()].as_deref()
    }

    pub fn interpose(&mut self, b: BerthId, headcode: &str) -> Vec<Event> {
        self.berths[b.idx()] = Some(headcode.to_string());
        vec![Event::BerthChanged { berth: b, headcode: Some(headcode.to_string()) }]
    }

    pub fn cancel(&mut self, b: BerthId) -> Option<Event> {
        self.berths[b.idx()].take().map(|_| Event::BerthChanged { berth: b, headcode: None })
    }

    /// Move whatever is in `from` into `to` (or clear it when `to` is `None`).
    /// Nothing happens if `from` is empty.
    pub fn step(&mut self, from: BerthId, to: Option<BerthId>) -> Vec<Event> {
        let Some(hc) = self.berths[from.idx()].take() else {
            return vec![];
        };
        let mut ev = vec![Event::BerthChanged { berth: from, headcode: None }];
        if let Some(t) = to {
            self.berths[t.idx()] = Some(hc.clone());
            ev.push(Event::BerthChanged { berth: t, headcode: Some(hc) });
        }
        ev
    }
}
