//! Geometric and dimensional constraints.
//!
//! This module defines the constraint *data model*; the residual and
//! Jacobian assembly lives in [`crate::solver`], next to the packed
//! parameter layout it depends on.

use forge_core::EntityId;
use serde::{Deserialize, Serialize};

/// Which point of an entity a constraint refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PointRole {
    /// Segment start / arc start.
    Start,
    /// Segment end / arc end.
    End,
    /// Circle or arc center (also: single points).
    Center,
}

/// Tangency flavor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TangentKind {
    /// Externally tangent (surfaces on opposite sides).
    External,
    /// Internally tangent (one inside the other).
    Internal,
}

/// A sketch constraint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Constraint {
    /// Two entity points coincide (FR-SK-02, geometric).
    Coincident {
        /// First entity.
        a: EntityId,
        /// Point on the first entity.
        a_point: PointRole,
        /// Second entity.
        b: EntityId,
        /// Point on the second entity.
        b_point: PointRole,
    },
    /// Line is horizontal.
    Horizontal {
        /// Line entity.
        line: EntityId,
    },
    /// Line is vertical.
    Vertical {
        /// Line entity.
        line: EntityId,
    },
    /// Two lines parallel.
    Parallel {
        /// First line.
        a: EntityId,
        /// Second line.
        b: EntityId,
    },
    /// Two lines perpendicular.
    Perpendicular {
        /// First line.
        a: EntityId,
        /// Second line.
        b: EntityId,
    },
    /// Line tangent to a circle/arc.
    TangentLineCircle {
        /// Line entity.
        line: EntityId,
        /// Circle entity.
        circle: EntityId,
        /// Tangency flavor.
        kind: TangentKind,
    },
    /// Two circles/arcs tangent.
    TangentCircles {
        /// First circle.
        a: EntityId,
        /// Second circle.
        b: EntityId,
        /// Tangency flavor.
        kind: TangentKind,
    },
    /// Two circles/arcs share a center.
    Concentric {
        /// First circle.
        a: EntityId,
        /// Second circle.
        b: EntityId,
    },
    /// Dimensional: segment length.
    Length {
        /// Line entity.
        line: EntityId,
        /// Target length (mm).
        value: f64,
    },
    /// Dimensional: circle radius.
    Radius {
        /// Circle entity.
        circle: EntityId,
        /// Target radius (mm).
        value: f64,
    },
    /// Dimensional: point-to-point distance.
    Distance {
        /// First entity.
        a: EntityId,
        /// Point on the first entity.
        a_point: PointRole,
        /// Second entity.
        b: EntityId,
        /// Point on the second entity.
        b_point: PointRole,
        /// Target distance (mm).
        value: f64,
    },
    /// Dimensional: angle between two line directions.
    Angle {
        /// First line.
        a: EntityId,
        /// Second line.
        b: EntityId,
        /// Target angle (radians).
        value: f64,
    },
    /// Anchor a point in place.
    FixPoint {
        /// Entity.
        entity: EntityId,
        /// Point on the entity.
        point: PointRole,
        /// Anchor position.
        position: (f64, f64),
    },
    /// Equal segment lengths.
    EqualLength {
        /// First line.
        a: EntityId,
        /// Second line.
        b: EntityId,
    },
    /// Equal circle radii.
    EqualRadius {
        /// First circle.
        a: EntityId,
        /// Second circle.
        b: EntityId,
    },
}

impl Constraint {
    /// Number of scalar equations contributed.
    pub fn equation_count(&self) -> usize {
        match self {
            Constraint::Coincident { .. }
            | Constraint::Concentric { .. }
            | Constraint::FixPoint { .. } => 2,
            _ => 1,
        }
    }

    /// Human-readable label for the UI.
    pub fn label(&self) -> String {
        match self {
            Constraint::Coincident { .. } => "Coincident".into(),
            Constraint::Horizontal { .. } => "Horizontal".into(),
            Constraint::Vertical { .. } => "Vertical".into(),
            Constraint::Parallel { .. } => "Parallel".into(),
            Constraint::Perpendicular { .. } => "Perpendicular".into(),
            Constraint::TangentLineCircle { .. } => "Tangent (line/circle)".into(),
            Constraint::TangentCircles { .. } => "Tangent (circle/circle)".into(),
            Constraint::Concentric { .. } => "Concentric".into(),
            Constraint::Length { value, .. } => format!("Length = {value:.3}"),
            Constraint::Radius { value, .. } => format!("Radius = {value:.3}"),
            Constraint::Distance { value, .. } => format!("Distance = {value:.3}"),
            Constraint::Angle { value, .. } => format!("Angle = {:.1}\u{00b0}", value.to_degrees()),
            Constraint::FixPoint { .. } => "Fixed".into(),
            Constraint::EqualLength { .. } => "Equal length".into(),
            Constraint::EqualRadius { .. } => "Equal radius".into(),
        }
    }

    /// Entities referenced by this constraint (for UI highlighting and
    /// solver dependency graphs).
    pub fn referenced_entities(&self) -> Vec<EntityId> {
        use Constraint::*;
        match self {
            Coincident { a, b, .. }
            | Parallel { a, b }
            | Perpendicular { a, b }
            | TangentCircles { a, b, .. }
            | Concentric { a, b }
            | Distance { a, b, .. }
            | Angle { a, b, .. }
            | EqualLength { a, b }
            | EqualRadius { a, b } => vec![*a, *b],
            Horizontal { line } | Vertical { line } | Length { line, .. } => vec![*line],
            TangentLineCircle { line, circle, .. } => vec![*line, *circle],
            Radius { circle, .. } => vec![*circle],
            FixPoint { entity, .. } => vec![*entity],
        }
    }
}
