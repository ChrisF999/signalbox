//! Deltas rebuild views exactly (spec §4.3).

use std::collections::BTreeMap;

use protocol::*;

fn s(x: &str) -> String {
    x.to_string()
}

fn base() -> View {
    View {
        seq: 1,
        sim_time: 25200.0,
        speed: 1,
        paused: false,
        vote: Some(VoteView { proposal: Proposal::Pause, agreed: vec![s("alice")], waiting: vec![], expires_in_s: 12 }),
        holders: BTreeMap::from([(s("East"), s("robot")), (s("West"), s("alice"))]),
        score: Some(0),
        signals: BTreeMap::from([(s("A"), Aspect::Red), (s("W1"), Aspect::Red)]),
        routes: BTreeMap::from([(s("W1-A"), RouteView { state: RouteState::Setting, auto_working: false })]),
        points: BTreeMap::from([(s("P"), PointsView { position: PointsPos::Normal, moving: false, locked: false })]),
        sections: BTreeMap::from([
            (s("TW1"), SectionView { occupied: false, held: Held::Free }),
            (s("TW2"), SectionView { occupied: false, held: Held::Path }),
        ]),
        berths: BTreeMap::from([(s("BW1"), s("1E01"))]),
        trains: BTreeMap::from([(s("1E01"), row(TrainState::InArea, 0)), (s("2W03"), row(TrainState::Due, 0))]),
    }
}

fn row(state: TrainState, late_s: i64) -> TrainRow {
    TrainRow { next_place: Some(s("EST")), next_platform: Some(s("1")), booked: Some(25500.0), late_s, state }
}

fn changed() -> View {
    let mut v = base();
    v.seq = 2;
    v.sim_time = 25201.6;
    v.speed = 8;
    v.paused = true;
    v.vote = None;
    v.score = Some(5);
    v.holders.insert(s("East"), s("bob"));
    v.signals.insert(s("W1"), Aspect::Yellow);
    v.routes.insert(s("W1-A"), RouteView { state: RouteState::Locked, auto_working: true });
    v.routes.insert(s("A-E"), RouteView { state: RouteState::Cancelling, auto_working: false });
    v.points.insert(s("P"), PointsView { position: PointsPos::Reverse, moving: true, locked: true });
    v.sections.insert(s("TW1"), SectionView { occupied: true, held: Held::Overlap });
    v.berths.remove("BW1");
    v.berths.insert(s("BA"), s("1E01"));
    v.trains.insert(s("1E01"), row(TrainState::AtPlatform, 60));
    v.trains.remove("2W03");
    v.trains.insert(s("1W05"), row(TrainState::Approaching, 0));
    v
}

#[test]
fn identical_views_give_no_delta() {
    let mut next = base();
    next.seq = 2;
    assert_eq!(diff(&base(), &next), None);
}

#[test]
fn applying_the_diff_rebuilds_the_new_view() {
    let (old, new) = (base(), changed());
    let d = diff(&old, &new).expect("views differ");
    let mut v = old.clone();
    v.apply(&d).unwrap();
    assert_eq!(v, new);
    let wire = serde_json::to_string(&ServerMsg::Delta(d)).unwrap();
    let ServerMsg::Delta(back) = serde_json::from_str::<ServerMsg>(&wire).unwrap() else { panic!("{wire}") };
    let mut v = old;
    v.apply(&back).unwrap();
    assert_eq!(v, new);
}

#[test]
fn a_delta_carries_only_what_changed() {
    let mut new = base();
    new.seq = 2;
    new.signals.insert(s("A"), Aspect::Green);
    new.berths.remove("BW1");
    let d = diff(&base(), &new).unwrap();
    assert_eq!(
        d,
        Delta {
            seq: 2,
            signals: BTreeMap::from([(s("A"), Aspect::Green)]),
            berths: BTreeMap::from([(s("BW1"), None)]),
            ..Delta::default()
        }
    );
    assert!(!d.is_empty());
    assert!(Delta { seq: 4, ..Delta::default() }.is_empty());
}

#[test]
fn cleared_vote_and_score_travel_as_null() {
    let mut new = base();
    new.seq = 2;
    new.vote = None;
    new.score = None;
    let d = diff(&base(), &new).unwrap();
    assert_eq!((d.vote.clone(), d.score), (Some(None), Some(None)));
    let json = serde_json::to_value(&d).unwrap();
    assert!(json["vote"].is_null() && json["score"].is_null(), "{json}");
    let mut v = base();
    v.apply(&d).unwrap();
    assert_eq!(v, new);
}

#[test]
fn a_gap_is_refused_and_leaves_the_view_alone() {
    let mut new = changed();
    new.seq = 3;
    let d = diff(&base(), &new).unwrap();
    let mut v = base();
    assert_eq!(v.apply(&d), Err(SeqGap { have: 1, got: 3 }));
    assert_eq!(v, base());
}

#[test]
fn trains_travel_like_berths_changed_added_and_removed() {
    let d = diff(&base(), &changed()).unwrap();
    assert_eq!(
        d.trains,
        BTreeMap::from([
            (s("1E01"), Some(row(TrainState::AtPlatform, 60))),
            (s("1W05"), Some(row(TrainState::Approaching, 0))),
            (s("2W03"), None),
        ])
    );
    let json = serde_json::to_value(&d).unwrap();
    assert!(json["trains"]["2W03"].is_null(), "{json}");
    let mut same = base();
    same.seq = 2;
    same.trains.insert(s("1E01"), row(TrainState::InArea, 0));
    assert_eq!(diff(&base(), &same), None, "an unchanged row is not sent");
}

/// A hostile server can send a view at the largest seq; the next delta is a
/// gap, never an overflow.
#[test]
fn a_view_at_the_largest_seq_takes_no_delta() {
    let mut v = base();
    v.seq = u64::MAX;
    let before = v.clone();
    let mut d = diff(&base(), &changed()).unwrap();
    for seq in [0, 1, u64::MAX] {
        d.seq = seq;
        assert_eq!(v.apply(&d), Err(SeqGap { have: u64::MAX, got: seq }));
    }
    assert_eq!(v, before);
}
