//! View deltas: what changed between two views of the same elements.

use std::collections::BTreeMap;

use crate::view::{Delta, View};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("delta {got} does not follow view {have}")]
pub struct SeqGap {
    pub have: u64,
    pub got: u64,
}

/// The changes from `old` to `new`, numbered `new.seq`, or `None` when
/// nothing but the number differs. Both views must cover the same elements
/// (the game sends a full view whenever a player's visible set changes).
pub fn diff(old: &View, new: &View) -> Option<Delta> {
    let mut d = Delta { seq: new.seq, ..Delta::default() };
    if old.sim_time != new.sim_time {
        d.sim_time = Some(new.sim_time);
    }
    if old.speed != new.speed {
        d.speed = Some(new.speed);
    }
    if old.paused != new.paused {
        d.paused = Some(new.paused);
    }
    if old.vote != new.vote {
        d.vote = Some(new.vote.clone());
    }
    if old.score != new.score {
        d.score = Some(new.score);
    }
    d.holders = changed(&old.holders, &new.holders);
    d.signals = changed(&old.signals, &new.signals);
    d.points = changed(&old.points, &new.points);
    d.sections = changed(&old.sections, &new.sections);
    d.routes = sparse(&old.routes, &new.routes);
    d.berths = sparse(&old.berths, &new.berths);
    d.trains = sparse(&old.trains, &new.trains);
    (!d.is_empty()).then_some(d)
}

/// Entries of `new` that are missing from `old` or differ.
fn changed<V: Clone + PartialEq>(old: &BTreeMap<String, V>, new: &BTreeMap<String, V>) -> BTreeMap<String, V> {
    new.iter().filter(|(k, v)| old.get(*k) != Some(*v)).map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// Like `changed`, plus `None` for entries that went away.
fn sparse<V: Clone + PartialEq>(old: &BTreeMap<String, V>, new: &BTreeMap<String, V>) -> BTreeMap<String, Option<V>> {
    let mut out: BTreeMap<String, Option<V>> = changed(old, new).into_iter().map(|(k, v)| (k, Some(v))).collect();
    for k in old.keys() {
        if !new.contains_key(k) {
            out.insert(k.clone(), None);
        }
    }
    out
}

impl View {
    /// Apply the delta that follows this view. A delta out of sequence is
    /// refused and the view is left as it was.
    pub fn apply(&mut self, d: &Delta) -> Result<(), SeqGap> {
        if d.seq != self.seq + 1 {
            return Err(SeqGap { have: self.seq, got: d.seq });
        }
        self.seq = d.seq;
        if let Some(t) = d.sim_time {
            self.sim_time = t;
        }
        if let Some(x) = d.speed {
            self.speed = x;
        }
        if let Some(p) = d.paused {
            self.paused = p;
        }
        if let Some(v) = &d.vote {
            self.vote = v.clone();
        }
        if let Some(s) = d.score {
            self.score = s;
        }
        self.holders.extend(d.holders.iter().map(|(k, v)| (k.clone(), v.clone())));
        self.signals.extend(d.signals.iter().map(|(k, v)| (k.clone(), *v)));
        self.points.extend(d.points.iter().map(|(k, v)| (k.clone(), *v)));
        self.sections.extend(d.sections.iter().map(|(k, v)| (k.clone(), *v)));
        for (k, v) in &d.routes {
            match v {
                Some(r) => {
                    self.routes.insert(k.clone(), *r);
                }
                None => {
                    self.routes.remove(k);
                }
            }
        }
        for (k, v) in &d.berths {
            match v {
                Some(h) => {
                    self.berths.insert(k.clone(), h.clone());
                }
                None => {
                    self.berths.remove(k);
                }
            }
        }
        for (k, v) in &d.trains {
            match v {
                Some(row) => {
                    self.trains.insert(k.clone(), row.clone());
                }
                None => {
                    self.trains.remove(k);
                }
            }
        }
        Ok(())
    }
}
