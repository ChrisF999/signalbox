//! Colour-light aspects and the rules that relate them.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aspect {
    Red,
    Yellow,
    DoubleYellow,
    Green,
}

/// The aspect a cleared signal with `n_aspects` shows, given its exit's aspect.
/// A 2-aspect signal only shows red or green.
pub fn cleared_aspect(n_aspects: u8, exit: Aspect) -> Aspect {
    match (n_aspects, exit) {
        (2, _) => Aspect::Green,
        (_, Aspect::Red) => Aspect::Yellow,
        (4, Aspect::Yellow) => Aspect::DoubleYellow,
        _ => Aspect::Green,
    }
}

/// The most restrictive aspect a driver can meet at the next signal after
/// passing one showing `passed`, where the passed signal has `n_aspects`
/// aspects. A green only promises the next signal is not red on 3 aspects,
/// or at least double yellow on 4; a 2-aspect green (which also shows before
/// a red, spec §4.3) promises nothing.
pub fn expected_after(passed: Aspect, n_aspects: u8) -> Aspect {
    match (passed, n_aspects) {
        (Aspect::Red | Aspect::Yellow, _) => Aspect::Red,
        (Aspect::DoubleYellow, _) => Aspect::Yellow,
        (Aspect::Green, 2) => Aspect::Red,
        (Aspect::Green, 4) => Aspect::DoubleYellow,
        (Aspect::Green, _) => Aspect::Yellow,
    }
}
