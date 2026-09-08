//! Strongly typed identifiers.
//!
//! All ids are plain `u64` counters under the hood but wrapped in newtypes so
//! that an [`EntityId`] can never be accidentally used where a
//! [`FeatureId`] is expected. Ids are *stable* for the lifetime of the
//! document: they survive undo/redo, feature reordering and re-evaluation,
//! which is what makes reference tracking (sketch -> extrude -> fillet …)
//! robust.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! def_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
            Serialize, Deserialize,
        )]
        pub struct $name(pub u64);

        impl $name {
            /// The id reserved for "no id" / optional references.
            pub const NONE: Self = Self(u64::MAX);

            /// Create a new id from a raw counter value.
            pub const fn new(raw: u64) -> Self {
                Self(raw)
            }

            /// Raw counter value.
            pub const fn raw(self) -> u64 {
                self.0
            }

            /// Returns `true` if this is the `NONE` sentinel.
            pub fn is_none(self) -> bool {
                self == Self::NONE
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::NONE
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                if self.is_none() {
                    write!(f, "{}(none)", stringify!($name))
                } else {
                    write!(f, "{}#{}", stringify!($name), self.0)
                }
            }
        }
    };
}

def_id!(
    /// Identifies a 2D sketch entity (line, arc, circle, …) inside a sketch.
    EntityId
);
def_id!(
    /// Identifies a whole sketch (a named collection of entities on a plane).
    SketchId
);
def_id!(
    /// Identifies a node in the parametric feature tree.
    FeatureId
);
def_id!(
    /// Identifies a solid body produced by evaluating a feature.
    BodyId
);
def_id!(
    /// Identifies a face of a body (triangle range on the tessellated mesh).
    FaceId
);
def_id!(
    /// Identifies an edge of a body.
    EdgeId
);
def_id!(
    /// Identifies a vertex of a body.
    VertexId
);
def_id!(
    /// Identifies a named parameter in the document parameter table.
    ParamId
);

/// Monotonic id allocator. One instance lives in the document and hands out
/// fresh ids for every kind of object, guaranteeing global uniqueness.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IdAllocator {
    next: u64,
}

impl IdAllocator {
    /// Create an allocator whose first issued id is `first`.
    pub fn starting_at(first: u64) -> Self {
        Self { next: first }
    }

    /// Issue the next id for the requested kind.
    pub fn next_id(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }

    /// Make sure future ids do not collide with `used` (used after
    /// deserializing a document that already allocated up to some point).
    pub fn reserve(&mut self, used: u64) {
        if used >= self.next {
            self.next = used + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_ordered_and_printable() {
        let a = EntityId::new(1);
        let b = EntityId::new(2);
        assert!(a < b);
        assert_eq!(format!("{a}"), "EntityId#1");
        assert!(EntityId::NONE.is_none());
    }

    #[test]
    fn allocator_is_monotonic() {
        let mut alloc = IdAllocator::starting_at(10);
        assert_eq!(alloc.next_id(), 10);
        assert_eq!(alloc.next_id(), 11);
        alloc.reserve(99);
        assert_eq!(alloc.next_id(), 100);
    }
}
