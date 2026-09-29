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

/// What a driver expects the next signal to show after passing one showing `passed`.
pub fn expected_after(passed: Aspect) -> Aspect {
    match passed {
        Aspect::Red | Aspect::Yellow => Aspect::Red,
        Aspect::DoubleYellow => Aspect::Yellow,
        Aspect::Green => Aspect::Green,
    }
}
