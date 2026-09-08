//! OBJ export/import (v/vn/f format).

use crate::{ExportMesh, IoError, Result};
use forge_geometry::TriMesh;
use std::path::Path;

/// Write meshes to an OBJ file (single object per mesh, "o" records).
pub fn write_obj(path: &Path, meshes: &[ExportMesh]) -> Result<()> {
    std::fs::write(path, obj_bytes(meshes))?;
    Ok(())
}

/// Serialize meshes as OBJ text into memory (wasm: browser download).
pub fn obj_bytes(meshes: &[ExportMesh]) -> Vec<u8> {
    let mut out = String::with_capacity(1024 * 1024);
    out.push_str("# ForgeCAD OBJ export\n");
    let mut base = 1usize; // OBJ indices are 1-based
    for m in meshes {
        let safe = m.name.replace(char::is_whitespace, "_");
        out.push_str(&format!("o {safe}\n"));
        let mesh = &m.mesh;
        for p in &mesh.positions {
            out.push_str(&format!("v {:.9} {:.9} {:.9}\n", p.x, p.y, p.z));
        }
        if let Some(normals) = &mesh.normals {
            for n in normals {
                out.push_str(&format!("vn {:.9} {:.9} {:.9}\n", n.x, n.y, n.z));
            }
        }
        let has_normals = mesh.normals.is_some();
        for t in 0..mesh.tri_count() {
            let [a, b, c] = mesh.triangle_idx(t);
            let (a, b, c) = (a as usize + base, b as usize + base, c as usize + base);
            if has_normals {
                // Same index for position and normal (they are parallel
                // arrays in our representation).
                out.push_str(&format!("f {a}//{a} {b}//{b} {c}//{c}\n"));
            } else {
                out.push_str(&format!("f {a} {b} {c}\n"));
            }
        }
        base += mesh.positions.len();
    }
    out.into_bytes()
}

/// Read an OBJ file into a single welded mesh (I-01).
///
/// Accepts `v`, `vn` and `f` records; face entries may carry texture
/// (`a/t`), normal (`a//b`) or both (`a/t/b`) indices; negative (relative)
/// indices per the OBJ spec; polygonal faces (quad+) are fan-triangulated.
/// `o`/`g` records and materials are ignored (all objects merge into one
/// body). Normals are recomputed after welding, so stale `vn` data cannot
/// poison the import.
pub fn read_obj(path: &Path) -> Result<TriMesh> {
    let text = std::fs::read_to_string(path)?;
    read_obj_bytes(&text)
}

/// Parse OBJ text from memory (wasm: dropped-file bytes). Pure — no
/// filesystem access.
pub fn read_obj_bytes(text: &str) -> Result<TriMesh> {
    let mut mesh = TriMesh::default();
    // Positions are collected raw (per-face duplicates welded later).
    let mut positions: Vec<forge_core::Point3> = Vec::new();
    // Vertex -> face slots (pushed in face order, welded afterwards).
    let mut triangles: Vec<[usize; 3]> = Vec::new();
    let mut current: Vec<usize> = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut tokens = line.split_whitespace();
        let Some(keyword) = tokens.next() else {
            continue;
        };
        match keyword {
            "v" => {
                let mut coords = [0.0f64; 3];
                let mut seen = 0;
                for tok in tokens {
                    if seen >= 3 {
                        break;
                    }
                    if let Ok(v) = tok.parse::<f64>() {
                        coords[seen] = v;
                        seen += 1;
                    }
                }
                if seen == 3 {
                    positions.push(forge_core::Point3::new(coords[0], coords[1], coords[2]));
                }
            }
            "f" => {
                current.clear();
                for tok in tokens {
                    // Strip everything after the first '/' (texture /
                    // normal indices are irrelevant: we recompute normals).
                    let vert_str = tok.split('/').next().unwrap_or("");
                    let idx = resolve_index(vert_str, positions.len());
                    if let Some(i) = idx {
                        current.push(i);
                    }
                }
                if current.len() >= 3 {
                    // Fan-triangulate polygons (correct for convex faces;
                    // concave polygons may need an ear clip — documented
                    // limitation).
                    for k in 1..current.len() - 1 {
                        triangles.push([current[0], current[k], current[k + 1]]);
                    }
                }
            }
            _ => {} // o, g, usemtl, mtllib, s, vn, vt: ignored
        }
    }

    if triangles.is_empty() {
        return Err(IoError::Malformed("no faces found in OBJ".into()));
    }

    for [a, b, c] in triangles {
        mesh.push_triangle(positions[a], positions[b], positions[c]);
    }
    mesh.weld(forge_geometry::WELD_EPS);
    mesh.remove_degenerate(1e-12);
    mesh.repair_orientation();
    mesh.compute_vertex_normals();
    Ok(mesh)
}

/// Resolve a 1-based OBJ vertex index, supporting negative (relative)
/// indices per the spec: `-1` = last declared vertex.
fn resolve_index(token: &str, vertex_count: usize) -> Option<usize> {
    let n: i64 = token.parse().ok()?;
    let i = if n < 0 {
        vertex_count as i64 + n
    } else {
        n - 1
    };
    if i >= 0 && (i as usize) < vertex_count {
        Some(i as usize)
    } else {
        None
    }
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

    #[test]
    fn obj_roundtrip_preserves_volume() {
        let meshes = vec![ExportMesh {
            name: "box".into(),
            mesh: forge_geometry::primitives::box_from_center_extents(
                Point3::origin(),
                Vector3::new(4.0, 4.0, 4.0),
            ),
        }];
        let dir = std::env::temp_dir();
        let path = dir.join("forgecad_test_read.obj");
        write_obj(&path, &meshes).unwrap();
        let mesh = read_obj(&path).unwrap();
        assert_eq!(mesh.tri_count(), 12);
        assert_eq!(mesh.vertex_count(), 8, "welding restores shared corners");
        assert!(mesh.is_closed());
        assert!((mesh.volume_signed() - 64.0).abs() < 1e-6);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn obj_read_handles_quad_faces_and_texture_indices() {
        let dir = std::env::temp_dir();
        let path = dir.join("forgecad_test_quad.obj");
        std::fs::write(
            &path,
            "# hand-written quad with a/t/b face entries\n\
             v 0 0 0\n\
             v 1 0 0\n\
             v 1 1 0\n\
             v 0 1 0\n\
             f 1/1/1 2/2/2 3/3/3 4/4/4\n",
        )
        .unwrap();
        let mesh = read_obj(&path).unwrap();
        assert_eq!(mesh.tri_count(), 2, "quad fan-triangulates to 2 tris");
        assert!((mesh.area() - 1.0).abs() < 1e-9);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn obj_read_rejects_faceless_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("forgecad_test_bad.obj");
        std::fs::write(&path, "v 0 0 0\nv 1 0 0\nv 0 1 0\n").unwrap();
        assert!(read_obj(&path).is_err());
        std::fs::remove_file(&path).ok();
    }
}
