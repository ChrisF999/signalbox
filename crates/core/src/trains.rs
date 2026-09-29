//! Trains and how they move along the track.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::aspect::Aspect;
use crate::ids::*;
use crate::network::{Dir, Network, PointsView, Position};

/// A train standing at a stopping call.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dwell {
    pub platform: PlatformId,
    pub depart_at_s: f64,
}

/// A stretch of one segment the head covered, as along-distances in `dir`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Swept {
    pub seg: SegmentId,
    pub dir: Dir,
    pub from: f64,
    pub to: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Moved {
    pub swept: Vec<Swept>,
    /// Segments the head entered, in order.
    pub entered: Vec<(SegmentId, Dir)>,
    /// Set when the track ended ahead of the head (buffer stop, boundary, points).
    pub end_node: Option<NodeId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Train {
    pub id: TrainId,
    pub service: ServiceId,
    pub headcode: String,
    pub train_type: TrainTypeId,
    pub length_m: f64,
    /// Segments under the train, tail first; the last one holds the head.
    pub path: VecDeque<(SegmentId, Dir)>,
    /// How far the head is into the head segment, along its direction.
    pub head_m: f64,
    /// m/s
    pub speed: f64,
    pub emergency: bool,
    pub hold_until_s: Option<f64>,
    pub next_call: usize,
    pub dwell: Option<Dwell>,
    pub end_done: bool,
    pub stabled: bool,
    pub last_passed_aspect: Option<Aspect>,
}

impl Train {
    /// A train with its head at `at`, the rest laid back along the track
    /// through the current points (any part past the end of the track is
    /// treated as still entering).
    #[allow(clippy::too_many_arguments)]
    pub fn placed(
        id: TrainId,
        service: ServiceId,
        headcode: &str,
        train_type: TrainTypeId,
        length_m: f64,
        at: Position,
        speed: f64,
        net: &Network,
        pts: &impl PointsView,
    ) -> Train {
        let seg = &net.segments[at.segment.idx()];
        let head_m = seg.along(at.offset_m, at.dir);
        let (steps, _) = net.walk_ahead(at.segment, at.dir.rev(), seg.length_m - head_m, length_m, pts);
        let mut t = Train::new(id, service, headcode, train_type, length_m, at.segment, at.dir, speed);
        t.path = steps.iter().rev().map(|s| (s.seg, s.dir.rev())).collect();
        t.head_m = head_m;
        t.trim(net);
        t
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: TrainId,
        service: ServiceId,
        headcode: &str,
        train_type: TrainTypeId,
        length_m: f64,
        seg: SegmentId,
        dir: Dir,
        speed: f64,
    ) -> Train {
        Train {
            id,
            service,
            headcode: headcode.to_string(),
            train_type,
            length_m,
            path: VecDeque::from([(seg, dir)]),
            head_m: 0.0,
            speed,
            emergency: false,
            hold_until_s: None,
            next_call: 0,
            dwell: None,
            end_done: false,
            stabled: false,
            last_passed_aspect: None,
        }
    }

    pub fn head(&self) -> (SegmentId, Dir) {
        *self.path.back().expect("a train always has a head segment")
    }

    pub fn segments(&self) -> impl Iterator<Item = SegmentId> {
        self.path.iter().map(|&(s, _)| s)
    }

    /// Metres of track from the start of the tail segment to the head.
    pub fn covered(&self, net: &Network) -> f64 {
        let n = self.path.len();
        self.head_m + self.path.iter().take(n - 1).map(|&(s, _)| net.segments[s.idx()].length_m).sum::<f64>()
    }

    /// How much of the train is still outside the network (while entering).
    pub fn off_network_m(&self, net: &Network) -> f64 {
        (self.length_m - self.covered(net)).max(0.0)
    }

    /// Move the head `dist` metres forward through the current points positions.
    pub fn advance(&mut self, net: &Network, pts: &impl PointsView, dist: f64) -> Moved {
        let mut m = Moved::default();
        if dist <= 0.0 {
            return m;
        }
        let mut remaining = dist;
        loop {
            let (seg, dir) = self.head();
            let len = net.segments[seg.idx()].length_m;
            let from = self.head_m;
            if self.head_m + remaining <= len {
                self.head_m += remaining;
                m.swept.push(Swept { seg, dir, from, to: self.head_m });
                break;
            }
            remaining -= len - self.head_m;
            self.head_m = len;
            m.swept.push(Swept { seg, dir, from, to: len });
            match net.next(seg, dir, pts) {
                Some(nx) => {
                    self.path.push_back(nx);
                    self.head_m = 0.0;
                    m.entered.push(nx);
                }
                None => {
                    m.end_node = Some(net.segments[seg.idx()].end_node(dir));
                    break;
                }
            }
        }
        self.trim(net);
        m
    }

    /// Drop tail segments the train no longer covers.
    pub fn trim(&mut self, net: &Network) {
        let mut covered = self.head_m;
        let mut keep = 1;
        for (i, &(s, _)) in self.path.iter().rev().enumerate().skip(1) {
            if covered >= self.length_m {
                break;
            }
            covered += net.segments[s.idx()].length_m;
            keep = i + 1;
        }
        while self.path.len() > keep {
            self.path.pop_front();
        }
    }

    /// Swap head and tail. Refused while part of the train is off the network.
    pub fn reverse(&mut self, net: &Network) -> bool {
        let covered = self.covered(net);
        if covered < self.length_m {
            return false;
        }
        let (tail_seg, _) = self.path[0];
        let tail_len = net.segments[tail_seg.idx()].length_m;
        let tail_into_segment = covered - self.length_m;
        self.path = self.path.iter().rev().map(|&(s, d)| (s, d.rev())).collect();
        self.head_m = tail_len - tail_into_segment;
        self.speed = 0.0;
        true
    }
}
