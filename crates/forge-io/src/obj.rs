//! OBJ export (v/f format with normals).

use crate::{ExportMesh, Result};
use std::io::Write;
use std::path::Path;

/// Write meshes to an OBJ file (single object per mesh, "o" records).
pub fn write_obj(path: &Path, meshes: &[ExportMesh]) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "# ForgeCAD OBJ export")?;
    let mut base = 1usize; // OBJ indices are 1-based
    for m in meshes {
        let safe = m.name.replace(char::is_whitespace, "_");
        writeln!(f, "o {safe}")?;
        let mesh = &m.mesh;
        for p in &mesh.positions {
            writeln!(f, "v {:.9} {:.9} {:.9}", p.x, p.y, p.z)?;
        }
        if let Some(normals) = &mesh.normals {
            for n in normals {
                writeln!(f, "vn {:.9} {:.9} {:.9}", n.x, n.y, n.z)?;
            }
        }
        let has_normals = mesh.normals.is_some();
        for t in 0..mesh.tri_count() {
            let [a, b, c] = mesh.triangle_idx(t);
            let (a, b, c) = (a as usize + base, b as usize + base, c as usize + base);
            if has_normals {
                // Same index for position and normal (they are parallel
                // arrays in our representation).
                writeln!(f, "f {a}//{a} {b}//{b} {c}//{c}")?;
            } else {
                writeln!(f, "f {a} {b} {c}")?;
            }
        }
        base += mesh.positions.len();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::{Point3, Vector3};

    #[test]
    fn obj_writes_and_parses_back() {
        let meshes = vec![ExportMesh {
            name: "box".into(),
            mesh: forge_geometry::primitives::box_from_center_extents(
                Point3::origin(),
                Vector3::new(4.0, 4.0, 4.0),
            ),
        }];
        let dir = std::env::temp_dir();
        let path = dir.join("forgecad_test.obj");
        write_obj(&path, &meshes).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let verts = text.lines().filter(|l| l.starts_with("v ")).count();
        let faces = text.lines().filter(|l| l.starts_with("f ")).count();
        assert_eq!(verts, 8, "welded box has 8 vertices");
        assert_eq!(faces, 12);
        assert!(text.contains("vn "));
        std::fs::remove_file(&path).ok();
    }
}
