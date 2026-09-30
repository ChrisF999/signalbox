//! Clock speed and votes (spec §3.5).

use std::collections::BTreeSet;

use game::clock::{GameClock, SPEEDS, VOTE_LAPSE_S, VoteError};
use protocol::Proposal;

fn holders(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn ticks_follow_the_speed_and_carry_fractions() {
    let mut c = GameClock::new(false);
    assert_eq!((c.paused, c.speed), (false, 1));
    assert_eq!(c.ticks_for(0.1), 1);
    assert_eq!(c.ticks_for(0.05), 0);
    assert_eq!(c.ticks_for(0.05), 1);
    c.speed = 8;
    assert_eq!(c.ticks_for(0.1), 8);
    assert_eq!(c.ticks_for(0.125), 10);
    let total: u64 = (0..1000).map(|_| c.ticks_for(0.1)).sum();
    assert_eq!(total, 8000);
    c.paused = true;
    assert_eq!(c.ticks_for(5.0), 0);
}

#[test]
fn a_lone_holders_proposal_applies_at_once() {
    let h = holders(&["alice"]);
    let mut c = GameClock::new(false);
    assert_eq!(c.vote("alice", Proposal::Pause, &h), Ok(Some(Proposal::Pause)));
    assert!(c.paused && c.vote.is_none());
    assert_eq!(c.vote("alice", Proposal::Speed { x: 4 }, &h), Ok(Some(Proposal::Speed { x: 4 })));
    assert_eq!(c.vote("alice", Proposal::Resume, &h), Ok(Some(Proposal::Resume)));
    assert_eq!((c.paused, c.speed), (false, 4));
}

#[test]
fn every_holder_must_agree() {
    let h = holders(&["alice", "bob"]);
    let mut c = GameClock::new(false);
    assert_eq!(c.vote("alice", Proposal::Speed { x: 4 }, &h), Ok(None));
    assert_eq!(c.speed, 1);
    let v = c.vote_view().unwrap();
    assert_eq!((v.proposal, v.agreed, v.expires_in_s), (Proposal::Speed { x: 4 }, vec!["alice".to_string()], 30));
    assert_eq!(c.vote("alice", Proposal::Speed { x: 4 }, &h), Ok(None), "agreeing twice changes nothing");
    assert_eq!(c.vote("bob", Proposal::Speed { x: 4 }, &h), Ok(Some(Proposal::Speed { x: 4 })));
    assert_eq!((c.speed, c.vote.is_none()), (4, true));
}

#[test]
fn only_voters_vote_and_only_listed_speeds() {
    let h = holders(&["alice"]);
    let mut c = GameClock::new(false);
    assert_eq!(c.vote("sam", Proposal::Pause, &h), Err(VoteError::NotAVoter));
    assert_eq!(c.vote("robot", Proposal::Pause, &h), Err(VoteError::NotAVoter));
    for x in [0, 3, 16, 255] {
        assert_eq!(c.vote("alice", Proposal::Speed { x }, &h), Err(VoteError::BadSpeed), "{x}");
    }
    assert_eq!(SPEEDS, [1, 2, 4, 8]);
    assert!(!c.paused && c.vote.is_none());
}

#[test]
fn a_different_proposal_replaces_the_open_one() {
    let h = holders(&["alice", "bob"]);
    let mut c = GameClock::new(false);
    c.vote("alice", Proposal::Pause, &h).unwrap();
    c.lapse(10.0);
    assert_eq!(c.vote("bob", Proposal::Speed { x: 2 }, &h), Ok(None));
    let v = c.vote_view().unwrap();
    assert_eq!((v.proposal, v.agreed, v.expires_in_s), (Proposal::Speed { x: 2 }, vec!["bob".to_string()], 30));
    assert_eq!(c.vote("alice", Proposal::Speed { x: 2 }, &h), Ok(Some(Proposal::Speed { x: 2 })));
    assert!(!c.paused);
}

#[test]
fn votes_lapse_after_thirty_seconds_of_real_time() {
    let h = holders(&["alice", "bob"]);
    let mut c = GameClock::new(false);
    c.vote("alice", Proposal::Pause, &h).unwrap();
    c.lapse(20.0);
    assert_eq!(c.vote_view().unwrap().expires_in_s, 10);
    c.lapse(VOTE_LAPSE_S - 20.0);
    assert!(c.vote.is_none());
    assert_eq!(c.vote("bob", Proposal::Pause, &h), Ok(None), "a lapsed vote starts again");
}

#[test]
fn a_holder_leaving_can_complete_a_vote() {
    let mut c = GameClock::new(false);
    c.vote("alice", Proposal::Pause, &holders(&["alice", "bob"])).unwrap();
    assert_eq!(c.settle(&holders(&["alice", "bob"])), None);
    assert_eq!(c.settle(&holders(&["alice"])), Some(Proposal::Pause));
    assert!(c.paused);
}

#[test]
fn with_no_holders_the_open_vote_is_dropped_and_the_clock_stays() {
    let mut c = GameClock::new(false);
    c.vote("alice", Proposal::Pause, &holders(&["alice", "bob"])).unwrap();
    assert_eq!(c.settle(&BTreeSet::new()), None);
    assert!(c.vote.is_none() && !c.paused);
}
