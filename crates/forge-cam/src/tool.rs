//! Tool library: cutters, drills, and their machining defaults.
//!
//! Units are mm / mm-min / RPM (converted at the UI boundary). The library
//! ships with sane starter presets for aluminium, brass, plastic and wood,
//! and is fully serializable so documents can carry their own tool tables.

use serde::{Deserialize, Serialize};

/// Cutting-tool geometry class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolKind {
    /// Flat end mill.
    Flat,
    /// Bull-nose (flat with corner radius).
    BullNose,
    /// Ball end mill.
    Ball,
    /// Twist drill (plunge only).
    Drill,
}

/// Flute material (drives the speeds & feeds advisor defaults).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolMaterial {
    /// High speed steel.
    Hss,
    /// Solid carbide.
    Carbide,
    /// Coated carbide (TiAlN etc.).
    CoatedCarbide,
}

impl ToolMaterial {
    /// Relative surface-speed factor vs HSS (carbide 2.5x, coated 3.5x).
    pub fn surface_speed_factor(self) -> f64 {
        match self {
            ToolMaterial::Hss => 1.0,
            ToolMaterial::Carbide => 2.5,
            ToolMaterial::CoatedCarbide => 3.5,
        }
    }
}

/// One cutting tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// Library-unique id (stable within a session / document).
    pub id: u32,
    /// Human name ("6 mm 3FL carbide end mill").
    pub name: String,
    /// Geometry class.
    pub kind: ToolKind,
    /// Cutter diameter (mm).
    pub diameter: f64,
    /// Corner radius for bull-nose, ball radius for ball tools (mm).
    #[serde(default)]
    pub corner_radius: f64,
    /// Flute length (mm) — plunge depth guard.
    pub flute_length: f64,
    /// Overall length (mm).
    pub overall_length: f64,
    /// Number of flutes.
    pub flutes: u32,
    /// Shank diameter (mm), for collision sanity checks.
    pub shank_diameter: f64,
    /// Flute material.
    pub material: ToolMaterial,
    /// Default spindle speed this library entry suggests (RPM).
    pub default_rpm: f64,
    /// Default cutting feed (mm/min).
    pub default_feed: f64,
    /// Default plunge feed (mm/min).
    pub default_plunge: f64,
}

impl Tool {
    /// Tool radius in mm.
    pub fn radius(&self) -> f64 {
        self.diameter * 0.5
    }

    /// Effective offset radius for the heightfield dilation. Flat and
    /// bull-nose tools keep the full radius; a ball tool is handled
    /// analytically by the kernel.
    pub fn offset_radius(&self) -> f64 {
        match self.kind {
            ToolKind::Ball | ToolKind::BullNose => self.corner_radius.max(0.0),
            _ => self.radius(),
        }
    }
}

/// Workpiece material preset (feeds & speeds advisor input).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Material {
    /// Name ("Aluminium 6061").
    pub name: String,
    /// Suggested surface speed for HSS tooling (m/min).
    pub surface_speed_hss: f64,
    /// Chip load per flute (mm) for a 6 mm tool; scales with diameter.
    pub chipload_6mm: f64,
}

impl Material {
    /// Starter material table.
    pub fn presets() -> Vec<Material> {
        vec![
            Material {
                name: "Aluminium 6061".into(),
                surface_speed_hss: 90.0,
                chipload_6mm: 0.05,
            },
            Material {
                name: "Brass (free cutting)".into(),
                surface_speed_hss: 110.0,
                chipload_6mm: 0.06,
            },
            Material {
                name: "Mild steel S235".into(),
                surface_speed_hss: 28.0,
                chipload_6mm: 0.04,
            },
            Material {
                name: "Plastic (acrylic/ABS)".into(),
                surface_speed_hss: 200.0,
                chipload_6mm: 0.10,
            },
            Material {
                name: "Hardwood / plywood".into(),
                surface_speed_hss: 300.0,
                chipload_6mm: 0.15,
            },
        ]
    }

    /// Chip load scaled from the 6 mm reference (sqrt diameter scaling).
    pub fn chipload(&self, diameter: f64) -> f64 {
        self.chipload_6mm * (diameter / 6.0).sqrt().max(0.25)
    }

    /// Suggested RPM for a tool in this material.
    pub fn suggested_rpm(&self, tool: &Tool) -> f64 {
        let v_c = self.surface_speed_hss * tool.material.surface_speed_factor(); // m/min
        let circumference_mm = std::f64::consts::PI * tool.diameter; // mm
        (v_c * 1000.0 / circumference_mm).clamp(500.0, 24000.0)
    }

    /// Suggested cutting feed (mm/min).
    pub fn suggested_feed(&self, tool: &Tool, rpm: f64) -> f64 {
        rpm * self.chipload(tool.diameter) * tool.flutes as f64
    }
}

/// A document's tool library.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolLibrary {
    /// Tools in id order.
    pub tools: Vec<Tool>,
    /// Next free tool id.
    next_id: u32,
}

impl ToolLibrary {
    /// New library pre-populated with starter tools (T1..T5).
    pub fn starter() -> Self {
        let mut lib = ToolLibrary::default();
        for tool in Tool::presets() {
            lib.add(tool);
        }
        lib
    }

    /// Insert a tool, assigning it a fresh id.
    pub fn add(&mut self, mut tool: Tool) -> u32 {
        tool.id = self.next_id;
        self.next_id += 1;
        let id = tool.id;
        self.tools.push(tool);
        id
    }

    /// Tool by id.
    pub fn get(&self, id: u32) -> Option<&Tool> {
        self.tools.iter().find(|t| t.id == id)
    }

    /// Replace a tool definition in place.
    pub fn update(&mut self, tool: Tool) -> bool {
        match self.tools.iter_mut().find(|t| t.id == tool.id) {
            Some(slot) => {
                *slot = tool;
                true
            }
            None => false,
        }
    }

    /// Remove a tool by id.
    pub fn remove(&mut self, id: u32) -> Option<Tool> {
        let idx = self.tools.iter().position(|t| t.id == id)?;
        Some(self.tools.remove(idx))
    }
}

impl Tool {
    /// Starter tool table (T1..T5): a workhorse 6 mm flat end mill, a
    /// small 3 mm detail mill, a 10 mm rougher, a ball-nose for 3D
    /// finishing and a jobber drill.
    pub fn presets() -> Vec<Tool> {
        let coated = ToolMaterial::CoatedCarbide;
        vec![
            Tool {
                id: 0,
                name: "6 mm 3FL carbide end mill".into(),
                kind: ToolKind::Flat,
                diameter: 6.0,
                corner_radius: 0.0,
                flute_length: 16.0,
                overall_length: 50.0,
                flutes: 3,
                shank_diameter: 6.0,
                material: coated,
                default_rpm: 12000.0,
                default_feed: 1800.0,
                default_plunge: 400.0,
            },
            Tool {
                id: 0,
                name: "3 mm 2FL detail mill".into(),
                kind: ToolKind::Flat,
                diameter: 3.0,
                corner_radius: 0.0,
                flute_length: 9.0,
                overall_length: 38.0,
                flutes: 2,
                shank_diameter: 4.0,
                material: coated,
                default_rpm: 16000.0,
                default_feed: 700.0,
                default_plunge: 200.0,
            },
            Tool {
                id: 0,
                name: "10 mm 2FL rougher".into(),
                kind: ToolKind::Flat,
                diameter: 10.0,
                corner_radius: 0.0,
                flute_length: 25.0,
                overall_length: 60.0,
                flutes: 2,
                shank_diameter: 10.0,
                material: ToolMaterial::Carbide,
                default_rpm: 9000.0,
                default_feed: 2400.0,
                default_plunge: 500.0,
            },
            Tool {
                id: 0,
                name: "6 mm 2FL ball nose".into(),
                kind: ToolKind::Ball,
                diameter: 6.0,
                corner_radius: 3.0,
                flute_length: 16.0,
                overall_length: 50.0,
                flutes: 2,
                shank_diameter: 6.0,
                material: coated,
                default_rpm: 14000.0,
                default_feed: 1200.0,
                default_plunge: 300.0,
            },
            Tool {
                id: 0,
                name: "5 mm HSS jobber drill".into(),
                kind: ToolKind::Drill,
                diameter: 5.0,
                corner_radius: 0.0,
                flute_length: 52.0,
                overall_length: 86.0,
                flutes: 2,
                shank_diameter: 5.0,
                material: ToolMaterial::Hss,
                default_rpm: 3000.0,
                default_feed: 250.0,
                default_plunge: 250.0,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_library_ids_unique_and_dense() {
        let lib = ToolLibrary::starter();
        let ids: Vec<u32> = lib.tools.iter().map(|t| t.id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(ids, sorted);
        assert_eq!(ids.len(), 5);
    }

    #[test]
    fn add_update_remove_roundtrip() {
        let mut lib = ToolLibrary::starter();
        let id = lib.add(Tool::presets()[1].clone());
        assert_eq!(lib.get(id).unwrap().diameter, 3.0);
        let mut edited = lib.get(id).unwrap().clone();
        edited.diameter = 4.0;
        assert!(lib.update(edited));
        assert_eq!(lib.get(id).unwrap().diameter, 4.0);
        assert!(lib.remove(id).is_some());
        assert!(lib.get(id).is_none());
    }

    #[test]
    fn rpm_and_feed_scale_sensibly() {
        let alu = &Material::presets()[0];
        let tool = &Tool::presets()[0]; // 6 mm carbide 3FL
        let rpm = alu.suggested_rpm(tool);
        assert!((10_000.0..=24_000.0).contains(&rpm), "rpm {rpm}");
        let feed = alu.suggested_feed(tool, rpm);
        assert!((500.0..=6000.0).contains(&feed), "feed {feed}");
        let steel = &Material::presets()[2];
        let drill = &Tool::presets()[4];
        assert!(steel.suggested_rpm(drill) < alu.suggested_rpm(tool));
    }

    #[test]
    fn tool_kinds_have_correct_offset_radius() {
        let flat = &Tool::presets()[0];
        assert_eq!(flat.offset_radius(), 3.0);
        let ball = &Tool::presets()[3];
        assert_eq!(ball.offset_radius(), 3.0);
    }

    #[test]
    fn library_serializes_ron_roundtrip() {
        let lib = ToolLibrary::starter();
        let s = ron::to_string(&lib).unwrap();
        let back: ToolLibrary = ron::from_str(&s).unwrap();
        assert_eq!(back.tools.len(), lib.tools.len());
        assert_eq!(back.next_id, lib.next_id);
    }
}
