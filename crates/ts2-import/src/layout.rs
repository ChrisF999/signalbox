//! Diagram geometry for clients, in TS2 scene coordinates, and the names of
//! TS2's places (polish spec M2: `LIVST` is LIVERPOOL STREET).

use serde_json::{Value, json};

use crate::graph::Graph;
use crate::ts2::{Item, Ts2};

pub fn build(ts2: &Ts2, g: &Graph) -> Value {
    let (mut lines, mut points, mut signals, mut platforms, mut labels) = (vec![], vec![], vec![], vec![], vec![]);
    let mut places = serde_json::Map::new();
    for (id, it) in &ts2.track_items {
        match it {
            Item::LineItem(l) | Item::InvisibleLinkItem(l) => {
                if let Some(seg) = g.line_segments.get(id) {
                    lines.push(json!({"segment": seg, "x1": l.x, "y1": l.y, "x2": l.xf, "y2": l.yf}));
                }
            }
            Item::PointsItem(p) => {
                if let Some(n) = g.points_nodes.get(id) {
                    points.push(json!({"node": n, "x": p.x, "y": p.y}));
                }
            }
            Item::SignalItem(s) => {
                if let Some(n) = g.signal_names.get(id) {
                    signals.push(json!({"signal": n, "x": s.x, "y": s.y, "berth_x": s.xn, "berth_y": s.yn}));
                }
            }
            Item::PlatformItem(p) => platforms.push(
                json!({"place": p.place_code, "platform": p.track_code, "x1": p.x, "y1": p.y, "x2": p.xf, "y2": p.yf}),
            ),
            Item::Place(p) => {
                if let Some(name) = &p.name {
                    labels.push(json!({"text": name, "x": p.x, "y": p.y}));
                    if let Some(code) = p.place_code.as_ref().filter(|c| !c.is_empty()) {
                        places.entry(code.clone()).or_insert_with(|| json!(name));
                    }
                }
            }
            Item::TextItem(t) => {
                if let Some(name) = &t.name {
                    labels.push(json!({"text": name, "x": t.x, "y": t.y}));
                }
            }
            Item::EndItem(_) => {}
        }
    }
    json!({"source": "ts2", "lines": lines, "points": points, "signals": signals, "platforms": platforms, "labels": labels, "places": places})
}
