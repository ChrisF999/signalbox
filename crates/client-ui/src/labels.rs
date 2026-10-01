//! Placing the diagram's texts so that none is drawn over another or over
//! the track (polish spec §3). `paint::draw` gives every text that may move
//! its role and its other spots, and lists what must be kept clear; `plan`
//! decides, in priority order, the first clear spot of each or that it is
//! hidden; `apply` moves and drops them. Pure: text sizes come from the
//! caller's `measure`. A plan holds offsets from each text's own spot, so it
//! stays right when the camera pans and is made again only on a zoom.

use std::collections::BTreeMap;

use egui::{Align, Align2, Pos2, Rect, Vec2, vec2};

use crate::paint::{Drawing, TextItem};

/// Who wins a collision: earlier first (spec decision P2). Headcodes are
/// not here: they are never moved or hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// A signal number of yours (every signal's, for a spectator).
    Number,
    /// The `A` beside a ○A button.
    AutoLetter,
    LineName,
    Platform,
    Label,
    /// A signal number of another area.
    FringeNumber,
}

/// A text that may move or be hidden.
#[derive(Clone, Debug, PartialEq)]
pub struct Movable {
    /// Index into `Drawing::texts`.
    pub text: usize,
    pub role: Role,
    /// Other spots (point and anchor), best first, after the text's own.
    pub alts: Vec<(Pos2, Align2)>,
    /// A platform number's block: the text must fit inside it.
    pub within: Option<Rect>,
}

/// What no movable text may touch (signal numbers may touch `bars`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeepClear {
    /// Track bars and points legs: ends and width.
    pub bars: Vec<(Pos2, Pos2, f32)>,
    /// Lamps, second-lamp spots and ○A circles: centre and radius.
    pub rounds: Vec<(Pos2, f32)>,
    /// Every berth's box and every exit square.
    pub boxes: Vec<Rect>,
}

/// Where a movable text goes: an offset from its own spot and the anchor
/// there, or `None` for hidden.
pub type Spot = Option<(Vec2, Align2)>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// One per `Drawing::movable`, in order.
    pub spots: Vec<Spot>,
    /// Signal numbers drawn touching a track bar.
    pub tight: usize,
    /// Signal numbers (yours) that had no spot, by text.
    pub hidden_numbers: Vec<String>,
}

/// Overlaps smaller than this (pixels, both ways) do not count.
pub const SLACK: f32 = 0.5;
/// Grid cell for the collision search, in pixels.
const CELL: f32 = 48.0;
/// An object spanning more cells than this is checked against everything
/// (nonsense coordinates must stay cheap).
const MAX_CELLS: i64 = 4096;

/// Nudges for labels and signal numbers: multiples of the text's width
/// and height, nearest first.
const LABEL_NUDGES: [(f32, f32); 24] = [
    (0.0, -0.5), (0.0, 0.5), (-0.5, 0.0), (0.5, 0.0), (0.0, -1.0), (0.0, 1.0), (-0.5, -0.5), (0.5, -0.5),
    (-0.5, 0.5), (0.5, 0.5), (-1.0, 0.0), (1.0, 0.0), (-0.5, -1.0), (0.5, -1.0), (-0.5, 1.0), (0.5, 1.0),
    (-1.0, -0.5), (1.0, -0.5), (-1.0, 0.5), (1.0, 0.5), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0),
];
/// Line names move only up or down (their arrow stays where it is).
const LINE_NUDGES: [(f32, f32); 4] = [(0.0, -0.5), (0.0, 0.5), (0.0, -1.0), (0.0, 1.0)];

/// The anchor that makes a text extend from its point in direction `v`.
pub fn corner(v: Vec2) -> Align2 {
    let pick = |c: f32| {
        if c > 0.3 {
            Align::Min
        } else if c < -0.3 {
            Align::Max
        } else {
            Align::Center
        }
    };
    Align2([pick(v.x), pick(v.y)])
}

fn overlaps(a: Rect, b: Rect) -> bool {
    let i = a.intersect(b);
    i.width() > SLACK && i.height() > SLACK
}

/// Segment a–b crosses or touches `r` (Liang–Barsky).
fn segment_hits(a: Pos2, b: Pos2, r: Rect) -> bool {
    if r.contains(a) || r.contains(b) {
        return true;
    }
    let d = b - a;
    let (mut t0, mut t1) = (0.0_f32, 1.0_f32);
    for (p, q) in [(-d.x, a.x - r.min.x), (d.x, r.max.x - a.x), (-d.y, a.y - r.min.y), (d.y, r.max.y - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
            continue;
        }
        let t = q / p;
        if p < 0.0 {
            if t > t1 {
                return false;
            }
            t0 = t0.max(t);
        } else {
            if t < t0 {
                return false;
            }
            t1 = t1.min(t);
        }
    }
    t0 <= t1
}

pub fn touches_bar(r: Rect, (a, b, w): (Pos2, Pos2, f32)) -> bool {
    segment_hits(a, b, r.expand(w / 2.0 - SLACK))
}

pub fn touches_round(r: Rect, (c, radius): (Pos2, f32)) -> bool {
    let nearest = c.clamp(r.min, r.max);
    nearest.distance(c) < radius - SLACK
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Obj {
    Bar(u32),
    Round(u32),
    Box(u32),
    Placed(u32),
}

/// A uniform grid of what is kept clear and what is placed.
#[derive(Default)]
struct Grid {
    cells: BTreeMap<(i64, i64), Vec<Obj>>,
    everywhere: Vec<Obj>,
}

/// Which grid cells a rectangle covers.
enum Span {
    /// Not finite: it meets nothing in the grid.
    Nothing,
    /// Too many cells to walk (or too far out to count them).
    Everywhere,
    Cells(i64, i64, i64, i64),
}

impl Grid {
    fn span(r: Rect) -> Span {
        if !(r.min.is_finite() && r.max.is_finite()) {
            return Span::Nothing;
        }
        // Counted in f32 first: far coordinates saturate the i64 cells.
        let count = (r.width() / CELL + 1.0) * (r.height() / CELL + 1.0);
        if !(count <= MAX_CELLS as f32) {
            return Span::Everywhere;
        }
        let c = |v: f32| (v / CELL).floor() as i64;
        Span::Cells(c(r.min.x), c(r.min.y), c(r.max.x), c(r.max.y))
    }

    fn insert(&mut self, r: Rect, o: Obj) {
        match Grid::span(r) {
            Span::Nothing => {}
            Span::Everywhere => self.everywhere.push(o),
            Span::Cells(x0, y0, x1, y1) => {
                for x in x0..=x1 {
                    for y in y0..=y1 {
                        self.cells.entry((x, y)).or_default().push(o);
                    }
                }
            }
        }
    }

    /// Everything that may meet `r`, each once, in order.
    fn near(&self, r: Rect) -> Vec<Obj> {
        let mut out = self.everywhere.clone();
        match Grid::span(r) {
            Span::Nothing => {}
            Span::Everywhere => out.extend(self.cells.values().flatten().copied()),
            Span::Cells(x0, y0, x1, y1) => {
                for x in x0..=x1 {
                    for y in y0..=y1 {
                        out.extend(self.cells.get(&(x, y)).into_iter().flatten().copied());
                    }
                }
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

fn bar_box((a, b, w): (Pos2, Pos2, f32)) -> Rect {
    Rect::from_two_pos(a, b).expand(w / 2.0)
}

/// What a text at `r` would hit: (anything but track, track).
fn hits(r: Rect, d: &Drawing, grid: &Grid, placed: &[Rect]) -> (bool, bool) {
    let (mut solid, mut track) = (false, false);
    for o in grid.near(r) {
        match o {
            Obj::Bar(i) => track |= touches_bar(r, d.keep.bars[i as usize]),
            Obj::Round(i) => solid |= touches_round(r, d.keep.rounds[i as usize]),
            Obj::Box(i) => solid |= overlaps(r, d.keep.boxes[i as usize]),
            Obj::Placed(i) => solid |= overlaps(r, placed[i as usize]),
        }
        if solid {
            break;
        }
    }
    (solid, track)
}

/// Decide where every movable text goes. Greedy, in role order and then
/// drawing order; a spot is clear when it overlaps no text placed before
/// and touches nothing kept clear. A signal number of yours with no clear
/// spot takes the first that only touches track ("tight"), else it is
/// hidden, as is any other text with no clear spot. Texts without a
/// `Movable` (headcodes, anything added later) are neither moved nor
/// considered: berth boxes stand in for headcodes.
pub fn plan(d: &Drawing, measure: &mut dyn FnMut(&TextItem) -> Vec2) -> Plan {
    let mut grid = Grid::default();
    for (i, &b) in d.keep.bars.iter().enumerate() {
        grid.insert(bar_box(b), Obj::Bar(i as u32));
    }
    for (i, &(c, r)) in d.keep.rounds.iter().enumerate() {
        grid.insert(Rect::from_center_size(c, vec2(r, r) * 2.0), Obj::Round(i as u32));
    }
    for (i, &b) in d.keep.boxes.iter().enumerate() {
        grid.insert(b, Obj::Box(i as u32));
    }
    let mut order: Vec<usize> = (0..d.movable.len()).filter(|&m| d.movable[m].text < d.texts.len()).collect();
    order.sort_by_key(|&m| (d.movable[m].role, d.movable[m].text));
    let mut out = Plan { spots: vec![None; d.movable.len()], ..Plan::default() };
    let mut placed: Vec<Rect> = Vec::new();
    for m in order {
        let mv = &d.movable[m];
        let t = &d.texts[mv.text];
        let size = measure(t);
        if !size.is_finite() {
            if mv.role == Role::Number {
                out.hidden_numbers.push(t.text.clone());
            }
            continue;
        }
        let own = t.anchor.anchor_size(t.at, size);
        // Its own spot, its other spots, then nudges of its own spot.
        let mut spots: Vec<(Pos2, Align2, Rect)> = vec![(t.at, t.anchor, own)];
        for &(p, a) in &mv.alts {
            spots.push((p, a, a.anchor_size(p, size)));
        }
        let nudges: &[(f32, f32)] = match mv.role {
            Role::Number | Role::Label => &LABEL_NUDGES,
            Role::LineName => &LINE_NUDGES,
            _ => &[],
        };
        for &(dx, dy) in nudges {
            let off = vec2(dx * size.x, dy * size.y);
            spots.push((t.at + off, t.anchor, own.translate(off)));
        }
        let fits = |r: &Rect| mv.within.is_none_or(|w| w.expand(SLACK).contains_rect(*r));
        let looked: Vec<(bool, bool)> =
            spots.iter().map(|(_, _, r)| if fits(r) && r.is_finite() { hits(*r, d, &grid, &placed) } else { (true, true) }).collect();
        let mut pick = looked.iter().position(|&(solid, track)| !solid && !track);
        if pick.is_none() && mv.role == Role::Number {
            pick = looked.iter().position(|&(solid, _)| !solid);
            out.tight += usize::from(pick.is_some());
        }
        match pick {
            Some(k) => {
                let (p, a, r) = spots[k];
                out.spots[m] = Some((p - t.at, a));
                grid.insert(r, Obj::Placed(placed.len() as u32));
                placed.push(r);
            }
            None if mv.role == Role::Number => out.hidden_numbers.push(t.text.clone()),
            None => {}
        }
    }
    out
}

/// Move and drop the movable texts as `p` says. A plan made for another
/// drawing (a different number of movable texts) changes nothing. The
/// texts' `Movable`s stay, pointing at their new indices, with no
/// alternatives left.
pub fn apply(mut d: Drawing, p: &Plan) -> Drawing {
    if p.spots.len() != d.movable.len() {
        return d;
    }
    let mut keep = vec![true; d.texts.len()];
    for (mv, spot) in d.movable.iter().zip(&p.spots) {
        let Some(t) = d.texts.get_mut(mv.text) else { continue };
        match spot {
            Some((off, anchor)) => {
                t.at += *off;
                t.anchor = *anchor;
            }
            None => keep[mv.text] = false,
        }
    }
    let mut new_index = vec![usize::MAX; d.texts.len()];
    let mut n = 0;
    for (i, k) in keep.iter().enumerate() {
        if *k {
            new_index[i] = n;
            n += 1;
        }
    }
    let mut i = 0;
    d.texts.retain(|_| {
        i += 1;
        keep[i - 1]
    });
    d.movable = d
        .movable
        .into_iter()
        .filter(|mv| new_index.get(mv.text).is_some_and(|&j| j != usize::MAX))
        .map(|mv| Movable { text: new_index[mv.text], alts: Vec::new(), ..mv })
        .collect();
    d
}

/// How legible a drawing is (spec §3.1), for tests and the owner's table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Audit {
    /// Pairs of drawn texts that overlap.
    pub overlaps: usize,
    /// Movable texts other than signal numbers touching track, a lamp, a
    /// ○A circle or a box; signal numbers touching anything but track.
    pub covered: usize,
    /// Signal numbers touching a track bar.
    pub tight: usize,
    /// Texts drawn, by role (headcodes and other fixed texts not counted).
    pub shown: BTreeMap<String, usize>,
}

pub fn audit(d: &Drawing, measure: &mut dyn FnMut(&TextItem) -> Vec2) -> Audit {
    let rects: Vec<Rect> = d.texts.iter().map(|t| t.anchor.anchor_size(t.at, measure(t))).collect();
    let mut a = Audit::default();
    for i in 0..rects.len() {
        for j in i + 1..rects.len() {
            if overlaps(rects[i], rects[j]) {
                a.overlaps += 1;
            }
        }
    }
    for mv in &d.movable {
        let Some(&r) = rects.get(mv.text) else { continue };
        *a.shown.entry(format!("{:?}", mv.role)).or_default() += 1;
        let track = d.keep.bars.iter().any(|&b| touches_bar(r, b));
        let solid = d.keep.rounds.iter().any(|&c| touches_round(r, c)) || d.keep.boxes.iter().any(|&b| overlaps(r, b));
        match mv.role {
            Role::Number | Role::FringeNumber => {
                a.tight += usize::from(track);
                a.covered += usize::from(solid);
            }
            _ => a.covered += usize::from(track || solid),
        }
    }
    a
}
