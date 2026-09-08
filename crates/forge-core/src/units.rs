//! Units of measure.
//!
//! The document model stores everything in **millimeters** and **radians**
//! internally; the unit system exists for presentation, import/export and
//! dimensional constraint display.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Length units supported by the UI and importers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LengthUnit {
    /// Millimeters – internal model unit.
    #[default]
    Millimeter,
    /// Centimeters.
    Centimeter,
    /// Meters.
    Meter,
    /// Inches.
    Inch,
    /// Feet.
    Foot,
}

impl LengthUnit {
    /// Conversion factor: how many millimeters one unit equals.
    pub const fn to_mm_factor(self) -> f64 {
        match self {
            LengthUnit::Millimeter => 1.0,
            LengthUnit::Centimeter => 10.0,
            LengthUnit::Meter => 1000.0,
            LengthUnit::Inch => 25.4,
            LengthUnit::Foot => 304.8,
        }
    }

    /// Canonical short symbol (`mm`, `in`, …).
    pub const fn symbol(self) -> &'static str {
        match self {
            LengthUnit::Millimeter => "mm",
            LengthUnit::Centimeter => "cm",
            LengthUnit::Meter => "m",
            LengthUnit::Inch => "in",
            LengthUnit::Foot => "ft",
        }
    }

    /// Convert a value from this unit into millimeters.
    pub fn to_mm(self, v: f64) -> f64 {
        v * self.to_mm_factor()
    }

    /// Convert a value from millimeters into this unit.
    pub fn from_mm(self, mm: f64) -> f64 {
        mm / self.to_mm_factor()
    }
}

impl fmt::Display for LengthUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}

/// Angle units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AngleUnit {
    /// Radians – internal model unit.
    #[default]
    Radian,
    /// Degrees.
    Degree,
}

impl AngleUnit {
    /// Conversion factor to radians.
    pub const fn to_radian_factor(self) -> f64 {
        match self {
            AngleUnit::Radian => 1.0,
            AngleUnit::Degree => std::f64::consts::PI / 180.0,
        }
    }

    /// Convert from this unit to radians.
    pub fn to_radians(self, v: f64) -> f64 {
        v * self.to_radian_factor()
    }

    /// Convert from radians to this unit.
    pub fn from_radians(self, rad: f64) -> f64 {
        rad / self.to_radian_factor()
    }

    /// Canonical short symbol.
    pub const fn symbol(self) -> &'static str {
        match self {
            AngleUnit::Radian => "rad",
            AngleUnit::Degree => "deg",
        }
    }
}

impl fmt::Display for AngleUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.symbol())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_conversion_roundtrip() {
        for u in [
            LengthUnit::Millimeter,
            LengthUnit::Centimeter,
            LengthUnit::Meter,
            LengthUnit::Inch,
            LengthUnit::Foot,
        ] {
            let v = 12.5;
            assert!((u.from_mm(u.to_mm(v)) - v).abs() < 1e-12);
        }
        assert!((LengthUnit::Inch.to_mm(1.0) - 25.4).abs() < 1e-12);
    }

    #[test]
    fn angle_conversion() {
        assert!((AngleUnit::Degree.to_radians(180.0) - std::f64::consts::PI).abs() < 1e-12);
        assert!((AngleUnit::Degree.from_radians(std::f64::consts::FRAC_PI_2) - 90.0).abs() < 1e-12);
    }
}
