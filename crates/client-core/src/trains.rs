//! The train list (spec D1 §3): trains at platforms first, then in your
//! area, approaching, and due; within each by booked time (unbooked last),
//! then headcode.

use std::cmp::Ordering;

use protocol::{TrainRow, View};

fn by_booked(a: Option<f64>, b: Option<f64>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

pub fn train_list(v: &View) -> Vec<(&str, &TrainRow)> {
    let mut rows: Vec<(&str, &TrainRow)> = v.trains.iter().map(|(h, r)| (h.as_str(), r)).collect();
    rows.sort_by(|(ha, a), (hb, b)| b.state.cmp(&a.state).then_with(|| by_booked(a.booked, b.booked)).then_with(|| ha.cmp(hb)));
    rows
}
