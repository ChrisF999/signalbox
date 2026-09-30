//! The diagram as shapes in layout coordinates, built once per layout from
//! its geometry and its lists. What the geometry lacks is left out; what
//! has no geometry at all gives no scene.

use std::collections::{BTreeMap, BTreeSet};

use egui::{Pos2, Rect, Vec2, pos2, vec2};
use protocol::{ExitName, Layout};

#[derive(Clone, Debug, PartialEq)]
pub struct TrackLine {
    pub segment: String,
    pub section: String,
    pub a: Pos2,
    pub b: Pos2,
    pub fringe: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PointsMark {
    pub name: String,
    pub section: String,
    pub at: Pos2,
    pub toe: Option<Pos2>,
    pub normal: Option<Pos2>,
    pub reverse: Option<Pos2>,
    pub fringe: bool,
    pub operable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignalMark {
    pub name: String,
    pub at: Pos2,
    /// Unit direction of travel past the signal, or zero when unknown.
    pub facing: Vec2,
    pub fringe: bool,
    pub operable: bool,
    /// Automatic routes starting here (an "A" is drawn, lit while one auto-works).
    pub auto_routes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BerthMark {
    pub name: String,
    pub at: Pos2,
    /// Drawn this far from `at` on screen (boundary berths sit above their exit).
    pub offset_px: Vec2,
    pub fringe: bool,
    pub operable: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExitMark {
    pub node: String,
    pub at: Pos2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlatformMark {
    pub rect: Rect,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LabelMark {
    pub text: String,
    pub at: Pos2,
}

/// Where a boundary berth's box is drawn relative to its exit node.
pub const BOUNDARY_BERTH_OFFSET_PX: Vec2 = vec2(0.0, -18.0);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub tracks: Vec<TrackLine>,
    pub points: Vec<PointsMark>,
    pub signals: Vec<SignalMark>,
    pub berths: Vec<BerthMark>,
    pub exits: Vec<ExitMark>,
    pub platforms: Vec<PlatformMark>,
    pub labels: Vec<LabelMark>,
    /// Bounds of your own area's drawing (`None` for a spectator).
    pub own: Option<Rect>,
    /// Bounds of everything drawn.
    pub all: Option<Rect>,
}

fn pt(x: f64, y: f64) -> Option<Pos2> {
    let p = pos2(x as f32, y as f32);
    (p.x.is_finite() && p.y.is_finite()).then_some(p)
}

fn grow(r: &mut Option<Rect>, p: Pos2) {
    *r = Some(match r {
        Some(r) => r.union(Rect::from_min_max(p, p)),
        None => Rect::from_min_max(p, p),
    });
}

impl Scene {
    /// `None` when the layout carries no geometry.
    pub fn build(l: &Layout) -> Option<Scene> {
        let g = l.geometry.as_ref()?;
        let fringe_of: BTreeMap<&str, bool> = l.sections.iter().map(|s| (s.name.as_str(), s.fringe)).collect();
        let seg_of: BTreeMap<&str, (&str, &str, &str)> =
            l.segments.iter().map(|s| (s.name.as_str(), (s.section.as_str(), s.from.as_str(), s.to.as_str()))).collect();
        let other_area = |area: &str| l.area.as_deref().is_some_and(|mine| mine != area);
        let mut sc = Scene::default();
        for line in &g.lines {
            let (Some(a), Some(b), Some(&(section, _, _))) = (pt(line.x1, line.y1), pt(line.x2, line.y2), seg_of.get(line.segment.as_str()))
            else {
                continue;
            };
            sc.tracks.push(TrackLine {
                segment: line.segment.clone(),
                section: section.to_string(),
                a,
                b,
                fringe: fringe_of.get(section).copied().unwrap_or(true),
            });
        }
        for p in &g.points {
            let (Some(at), Some(info)) = (pt(p.x, p.y), l.points.iter().find(|i| i.name == p.node)) else { continue };
            let leg = |v: Option<[f64; 2]>| v.and_then(|[x, y]| pt(x, y));
            sc.points.push(PointsMark {
                name: p.node.clone(),
                section: info.section.clone(),
                at,
                toe: leg(p.toe),
                normal: leg(p.normal),
                reverse: leg(p.reverse),
                fringe: fringe_of.get(info.section.as_str()).copied().unwrap_or(true),
                operable: info.operable,
            });
        }
        for s in &g.signals {
            let (Some(at), Some(info)) = (pt(s.x, s.y), l.signals.iter().find(|i| i.name == s.signal)) else { continue };
            let facing = s.facing.map(|[x, y]| vec2(x as f32, y as f32)).filter(|v| v.length() > 0.0 && v.is_finite());
            sc.signals.push(SignalMark {
                name: s.signal.clone(),
                at,
                facing: facing.map_or(Vec2::ZERO, Vec2::normalized),
                fringe: other_area(&info.area),
                operable: info.operable,
                auto_routes: l.routes.iter().filter(|r| r.automatic && r.entrance == s.signal).map(|r| r.name.clone()).collect(),
            });
            for b in l.berths.iter().filter(|b| b.signal.as_deref() == Some(s.signal.as_str())) {
                if let Some(bat) = pt(s.berth_x, s.berth_y) {
                    sc.berths.push(BerthMark {
                        name: b.name.clone(),
                        at: bat,
                        offset_px: Vec2::ZERO,
                        fringe: other_area(&b.area),
                        operable: b.operable,
                    });
                }
            }
        }
        let node_at: BTreeMap<&str, Pos2> =
            g.nodes.iter().filter_map(|n| Some((n.node.as_str(), pt(n.x, n.y)?))).collect();
        let exit_nodes: BTreeSet<&str> = l
            .routes
            .iter()
            .filter_map(|r| match &r.exit {
                ExitName::Node(n) => Some(n.as_str()),
                ExitName::Signal(_) => None,
            })
            .collect();
        for n in &exit_nodes {
            if let Some(&at) = node_at.get(n) {
                sc.exits.push(ExitMark { node: n.to_string(), at });
            }
        }
        for b in &l.berths {
            if let Some(&at) = b.boundary.as_deref().and_then(|n| node_at.get(n)) {
                sc.berths.push(BerthMark {
                    name: b.name.clone(),
                    at,
                    offset_px: BOUNDARY_BERTH_OFFSET_PX,
                    fringe: other_area(&b.area),
                    operable: b.operable,
                });
            }
        }
        for p in &g.platforms {
            if let (Some(a), Some(b)) = (pt(p.x1, p.y1), pt(p.x2, p.y2)) {
                sc.platforms.push(PlatformMark { rect: Rect::from_two_pos(a, b), label: format!("{} {}", p.place, p.platform) });
            }
        }
        for t in &g.labels {
            if let Some(at) = pt(t.x, t.y) {
                sc.labels.push(LabelMark { text: t.text.clone(), at });
            }
        }
        for t in &sc.tracks {
            grow(&mut sc.all, t.a);
            grow(&mut sc.all, t.b);
            if !t.fringe && l.area.is_some() {
                grow(&mut sc.own, t.a);
                grow(&mut sc.own, t.b);
            }
        }
        for s in &sc.signals {
            grow(&mut sc.all, s.at);
            if !s.fringe && l.area.is_some() {
                grow(&mut sc.own, s.at);
            }
        }
        for p in &sc.points {
            grow(&mut sc.all, p.at);
        }
        Some(sc)
    }

    /// What "Fit" frames: your own area, or everything.
    pub fn fit_bounds(&self) -> Option<Rect> {
        self.own.or(self.all)
    }
}
