//! Typed indices into the world's arenas.

use serde::{Deserialize, Serialize};

macro_rules! ids {
    ($($name:ident),* $(,)?) => {$(
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        pub struct $name(pub u32);

        impl $name {
            pub fn idx(self) -> usize {
                self.0 as usize
            }

            pub fn from_idx(i: usize) -> Self {
                Self(i as u32)
            }
        }
    )*};
}

ids!(
    NodeId, SegmentId, SectionId, SignalId, BerthId, PlatformId, RouteId, AreaId, TrainTypeId,
    ServiceId, TrainId,
);
