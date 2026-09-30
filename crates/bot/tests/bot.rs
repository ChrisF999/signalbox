//! The bot keeps its view in step with the game and recovers from a gap.

use std::collections::BTreeMap;

use bot::Bot;
use game::{Game, GameMeta};
use protocol::*;
use signalbox_core::world::World;

fn view(seq: u64) -> View {
    View {
        seq,
        sim_time: 25200.0,
        speed: 1,
        paused: false,
        vote: None,
        holders: BTreeMap::new(),
        score: None,
        signals: BTreeMap::from([("A".to_string(), Aspect::Red)]),
        routes: BTreeMap::new(),
        points: BTreeMap::new(),
        sections: BTreeMap::new(),
        berths: BTreeMap::new(),
    }
}

fn delta(seq: u64, aspect: Aspect) -> ServerMsg {
    ServerMsg::Delta(Delta { seq, signals: BTreeMap::from([("A".to_string(), aspect)]), ..Delta::default() })
}

#[test]
fn applies_views_and_deltas_in_order() {
    let mut b = Bot::new();
    assert_eq!(b.receive(ServerMsg::View(view(1))), None);
    assert_eq!(b.receive(delta(2, Aspect::Green)), None);
    let v = b.view().unwrap();
    assert_eq!((v.seq, v.signals["A"]), (2, Aspect::Green));
    assert_eq!(b.resyncs(), 0);
}

#[test]
fn a_gap_asks_for_one_resync_and_the_next_view_recovers() {
    let mut b = Bot::new();
    b.receive(ServerMsg::View(view(1)));
    assert_eq!(b.receive(delta(3, Aspect::Green)), Some(ClientMsg::Resync));
    assert_eq!(b.receive(delta(4, Aspect::Yellow)), None, "one resync is enough");
    assert_eq!(b.view().unwrap().seq, 1, "the stale view is left alone");
    assert_eq!(b.receive(ServerMsg::View(view(5))), None);
    assert_eq!(b.receive(delta(6, Aspect::Yellow)), None);
    let v = b.view().unwrap();
    assert_eq!((v.seq, v.signals["A"]), (6, Aspect::Yellow));
    assert_eq!(b.resyncs(), 1);
}

#[test]
fn a_delta_before_any_view_asks_for_a_resync() {
    let mut b = Bot::new();
    assert_eq!(b.receive(delta(1, Aspect::Red)), Some(ClientMsg::Resync));
    assert!(b.view().is_none());
    assert_eq!(b.receive(delta(2, Aspect::Red)), None);
}

#[test]
fn notices_are_kept_until_taken() {
    let mut b = Bot::new();
    assert_eq!(b.receive(ServerMsg::Notice(Notice::Replaced)), None);
    assert_eq!(b.take_notices(), vec![Notice::Replaced]);
    assert!(b.take_notices().is_empty());
}

#[test]
fn recovers_from_a_dropped_delta_against_a_real_game() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../game/tests/fixtures/twobox.json")).unwrap();
    let mut g = Game::new(World::from_json(&text).unwrap(), GameMeta { layout: "twobox".into(), seed: 1 });
    let mut b = Bot::new();
    for (_, m) in g.connect("alice") {
        assert_eq!(b.receive(m), None);
    }
    for (_, m) in g.handle("alice", ClientMsg::Claim { area: "West".into() }) {
        assert_eq!(b.receive(m), None);
    }
    assert_eq!(b.area(), Some("West"));
    g.advance(1.0);
    let _lost = g.flush();
    g.advance(1.0);
    let mut replies = Vec::new();
    for (_, m) in g.flush() {
        replies.extend(b.receive(m));
    }
    assert_eq!(replies, vec![ClientMsg::Resync]);
    for r in replies {
        for (_, m) in g.handle("alice", r) {
            assert_eq!(b.receive(m), None);
        }
    }
    assert_eq!(b.view(), g.view_of("alice").as_ref());
    assert_eq!(b.layout(), g.layout_of("alice").as_ref());
}
