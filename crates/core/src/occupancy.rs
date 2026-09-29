//! Which trains are in which track sections (rebuilt every tick).

use crate::ids::{SectionId, TrainId};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Occupancy {
    trains: Vec<Vec<TrainId>>,
    moving: Vec<bool>,
}

impl Occupancy {
    pub fn new(sections: usize) -> Self {
        Occupancy { trains: vec![vec![]; sections], moving: vec![false; sections] }
    }

    pub fn add(&mut self, s: SectionId, t: TrainId, moving: bool) {
        let v = &mut self.trains[s.idx()];
        if !v.contains(&t) {
            v.push(t);
        }
        self.moving[s.idx()] |= moving;
    }

    pub fn occupied(&self, s: SectionId) -> bool {
        !self.trains[s.idx()].is_empty()
    }

    pub fn trains_in(&self, s: SectionId) -> &[TrainId] {
        &self.trains[s.idx()]
    }

    /// Occupied, and every train in it is standing still.
    pub fn stationary(&self, s: SectionId) -> bool {
        self.occupied(s) && !self.moving[s.idx()]
    }
}
