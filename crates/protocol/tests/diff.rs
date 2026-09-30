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
        vote: Some(VoteView { proposal: Proposal::Pause, agreed: vec![s("alice")], expires_in_s: 12 }),
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
    }
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
