//! The game clock: pause and speed change only when every holder agrees
//! (spec §3.5). Real time is whatever the caller says it is.

use std::collections::BTreeSet;

use protocol::{Proposal, VoteView};

pub const SPEEDS: [u8; 4] = [1, 2, 4, 8];
/// A proposal lapses after this much real time.
pub const VOTE_LAPSE_S: f64 = 30.0;
/// Sim ticks per real second at 1x (one tick per 0.1 s).
pub const TICKS_PER_REAL_S: f64 = 10.0;

#[derive(Clone, Debug, PartialEq)]
pub struct OpenVote {
    pub proposal: Proposal,
    pub agreed: BTreeSet<String>,
    /// Real seconds before it lapses.
    pub left_s: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoteError {
    /// Only players holding an area vote.
    NotAHolder,
    /// Speeds are 1, 2, 4 or 8.
    BadSpeed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GameClock {
    pub paused: bool,
    pub speed: u8,
    /// At most one open proposal.
    pub vote: Option<OpenVote>,
    /// Fraction of a tick owed from earlier calls.
    carry: f64,
}

impl GameClock {
    pub fn new(paused: bool) -> GameClock {
        GameClock { paused, speed: 1, vote: None, carry: 0.0 }
    }

    /// Whole ticks to run for `real_dt` seconds of real time.
    pub fn ticks_for(&mut self, real_dt: f64) -> u64 {
        if self.paused {
            self.carry = 0.0;
            return 0;
        }
        self.carry += real_dt * TICKS_PER_REAL_S * f64::from(self.speed);
        // A hair of slack so 0.1 s steps are never a float's width short.
        let n = (self.carry + 1e-9).floor();
        self.carry = (self.carry - n).max(0.0);
        n as u64
    }

    /// `voter` proposes, or agrees to, `proposal`. Returns it if it applied.
    pub fn vote(&mut self, voter: &str, proposal: Proposal, holders: &BTreeSet<String>) -> Result<Option<Proposal>, VoteError> {
        if !holders.contains(voter) {
            return Err(VoteError::NotAHolder);
        }
        if let Proposal::Speed { x } = proposal {
            if !SPEEDS.contains(&x) {
                return Err(VoteError::BadSpeed);
            }
        }
        let same = self.vote.as_ref().is_some_and(|v| v.proposal == proposal);
        if same {
            if let Some(v) = self.vote.as_mut() {
                v.agreed.insert(voter.to_string());
            }
        } else {
            self.vote = Some(OpenVote { proposal, agreed: BTreeSet::from([voter.to_string()]), left_s: VOTE_LAPSE_S });
        }
        Ok(self.settle(holders))
    }

    /// Apply the open proposal if every holder has agreed. With no holders
    /// at all nobody can agree, so the proposal is dropped.
    pub fn settle(&mut self, holders: &BTreeSet<String>) -> Option<Proposal> {
        let v = self.vote.as_ref()?;
        if holders.is_empty() {
            self.vote = None;
            return None;
        }
        if !holders.iter().all(|h| v.agreed.contains(h)) {
            return None;
        }
        let p = v.proposal;
        self.vote = None;
        match p {
            Proposal::Pause => self.paused = true,
            Proposal::Resume => self.paused = false,
            Proposal::Speed { x } => self.speed = x,
        }
        Some(p)
    }

    /// Let `real_dt` seconds pass for the open proposal.
    pub fn lapse(&mut self, real_dt: f64) {
        let lapsed = match self.vote.as_mut() {
            Some(v) => {
                v.left_s -= real_dt;
                v.left_s <= 0.0
            }
            None => false,
        };
        if lapsed {
            self.vote = None;
        }
    }

    pub fn vote_view(&self) -> Option<VoteView> {
        self.vote.as_ref().map(|v| VoteView {
            proposal: v.proposal,
            agreed: v.agreed.iter().cloned().collect(),
            expires_in_s: v.left_s.max(0.0).ceil() as u32,
        })
    }
}
