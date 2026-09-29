mod common;

use common::*;
use signalbox_core::ids::*;
use signalbox_core::network::*;
use signalbox_core::trains::Train;

struct Pts(Option<PointsPos>);

impl PointsView for Pts {
    fn position(&self, _: NodeId) -> Option<PointsPos> {
        self.0
    }
}

const NORMAL: Pts = Pts(Some(PointsPos::Normal));

fn train_at(w: &signalbox_core::world::World, segment: &str, head_m: f64) -> Train {
    let mut t = Train::new(TrainId(0), ServiceId(0), "2A01", TrainTypeId(0), 100.0, seg(w, segment), Dir::Up, 0.0);
    t.head_m = head_m;
    t
}

#[test]
fn advance_within_segment() {
    let w = world("plain_line");
    let mut t = train_at(&w, "a", 0.0);
    let m = t.advance(&w.net, &NORMAL, 50.0);
    assert_eq!(t.head_m, 50.0);
    assert_eq!(m.swept.len(), 1);
    assert_eq!((m.swept[0].from, m.swept[0].to), (0.0, 50.0));
    assert!(m.entered.is_empty());
}

#[test]
fn advance_across_segments_records_each() {
    let w = world("plain_line");
    let mut t = train_at(&w, "a", 0.0);
    let m = t.advance(&w.net, &NORMAL, 2500.0);
    let swept: Vec<_> = m.swept.iter().map(|s| (s.seg, s.from, s.to)).collect();
    assert_eq!(swept, vec![(seg(&w, "a"), 0.0, 1000.0), (seg(&w, "b"), 0.0, 1000.0), (seg(&w, "c"), 0.0, 500.0)]);
    assert_eq!(m.entered, vec![(seg(&w, "b"), Dir::Up), (seg(&w, "c"), Dir::Up)]);
    assert_eq!(t.path.iter().copied().collect::<Vec<_>>(), vec![(seg(&w, "c"), Dir::Up)]);
}

#[test]
fn signal_at_segment_end_swept_once() {
    let w = world("plain_line");
    let mut t = train_at(&w, "a", 900.0);
    let mut swept = t.advance(&w.net, &NORMAL, 100.0).swept;
    swept.extend(t.advance(&w.net, &NORMAL, 10.0).swept);
    let a = seg(&w, "a");
    let hits = swept.iter().filter(|s| s.seg == a && s.from < 1000.0 && 1000.0 <= s.to).count();
    assert_eq!(hits, 1);
}

#[test]
fn stops_at_the_end_of_the_track() {
    let w = world("plain_line");
    let mut t = train_at(&w, "c", 950.0);
    let m = t.advance(&w.net, &NORMAL, 100.0);
    assert_eq!(m.end_node, Some(node(&w, "E")));
    assert_eq!(t.head_m, 1000.0);
}

#[test]
fn tail_segments_are_kept_until_cleared() {
    let w = world("plain_line");
    let mut t = train_at(&w, "a", 980.0);
    t.advance(&w.net, &NORMAL, 70.0);
    assert_eq!(t.segments().collect::<Vec<_>>(), vec![seg(&w, "a"), seg(&w, "b")]);
    t.advance(&w.net, &NORMAL, 60.0);
    assert_eq!(t.segments().collect::<Vec<_>>(), vec![seg(&w, "b")]);
}

#[test]
fn entering_train_is_partly_off_network() {
    let w = world("plain_line");
    let mut t = train_at(&w, "a", 0.0);
    assert_eq!(t.off_network_m(&w.net), 100.0);
    t.advance(&w.net, &NORMAL, 30.0);
    assert_eq!(t.off_network_m(&w.net), 70.0);
}

#[test]
fn reverse_swaps_head_and_tail() {
    let w = world("plain_line");
    let mut t = train_at(&w, "a", 1000.0);
    t.advance(&w.net, &NORMAL, 50.0);
    t.speed = 3.0;
    assert!(t.reverse(&w.net));
    assert_eq!(t.path.iter().copied().collect::<Vec<_>>(), vec![(seg(&w, "b"), Dir::Down), (seg(&w, "a"), Dir::Down)]);
    assert_eq!(t.head_m, 50.0);
    assert_eq!(t.speed, 0.0);
}

#[test]
fn reverse_refused_while_partly_off_network() {
    let w = world("plain_line");
    let mut t = train_at(&w, "a", 40.0);
    assert!(!t.reverse(&w.net));
}

#[test]
fn advance_follows_points() {
    let w = world("terminus");
    let mut t = train_at(&w, "app", 0.0);
    t.advance(&w.net, &Pts(Some(PointsPos::Reverse)), 60.0);
    assert_eq!(t.head(), (seg(&w, "r"), Dir::Up));
    assert_eq!(t.head_m, 10.0);
    assert_eq!(t.segments().collect::<Vec<_>>(), vec![seg(&w, "app"), seg(&w, "r")]);
}
