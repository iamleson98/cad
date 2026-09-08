//! STL read/write (ASCII and binary).
//!
//! STL stores triangle soups without connectivity – the canonical mesh
//! interchange format for 3D printing pipelines. Binary STL is written by
//! default (compact); ASCII is available for debugging.

use crate::{ExportMesh, IoError, Result};
use forge_core::Vector3;
use forge_geometry::TriMesh;
use std::io::{Read, Write};
use std::path::Path;

const BINARY_HEADER_LEN: usize = 80;

/// Write one or more bodies as a single binary STL.
pub fn write_binary_stl(path: &Path, meshes: &[ExportMesh]) -> Result<()> {
    let tris: usize = meshes.iter().map(|m| m.mesh.tri_count()).sum();
    let mut buf: Vec<u8> = Vec::with_capacity(BINARY_HEADER_LEN + 4 + tris * 50);
    buf.extend_from_slice(b"ForgeCAD binary STL export".as_slice());
    buf.resize(BINARY_HEADER_LEN, 0);
    buf.extend_from_slice(&(tris as u32).to_le_bytes());
    for m in meshes {
        append_binary_mesh(&mut buf, &m.mesh);
    }
    std::fs::write(path, buf)?;
    Ok(())
}

fn append_binary_mesh(buf: &mut Vec<u8>, mesh: &TriMesh) {
    let mut normal = [0f32; 3];
    for t in 0..mesh.tri_count() {
        let [a, b, c] = mesh.triangle(t);
        let n = unit_or_zero((b - a).cross(&(c - a)));
        normal = [n.x as f32, n.y as f32, n.z as f32];
        buf.extend_from_slice(&normal.map(|v| v.to_le_bytes()).concat());
        for p in [a, b, c] {
            buf.extend_from_slice(&(p.x as f32).to_le_bytes());
            buf.extend_from_slice(&(p.y as f32).to_le_bytes());
            buf.extend_from_slice(&(p.z as f32).to_le_bytes());
        }
        buf.extend_from_slice(&0u16.to_le_bytes()); // attribute count
    }
}

fn unit_or_zero(v: Vector3) -> Vector3 {
    let len = v.norm();
    if len > 1e-20 {
        v / len
    } else {
        Vector3::zeros()
    }
}

/// Write one or more bodies as ASCII STL.
pub fn write_ascii_stl(path: &Path, meshes: &[ExportMesh]) -> Result<()> {
    let mut out = String::with_capacity(1024 * 1024);
    for m in meshes {
        let safe_name = m.name.replace(char::is_whitespace, "_");
        out.push_str(&format!("solid {safe_name}\n"));
        for t in 0..m.mesh.tri_count() {
            let [a, b, c] = m.mesh.triangle(t);
            let n = unit_or_zero((b - a).cross(&(c - a)));
            out.push_str(&format!(
                "  facet normal {:.6e} {:.6e} {:.6e}\n    outer loop\n",
                n.x, n.y, n.z
            ));
            for p in [a, b, c] {
                out.push_str(&format!(
                    "      vertex {:.6e} {:.6e} {:.6e}\n",
                    p.x, p.y, p.z
                ));
            }
            out.push_str("    endloop\n  endfacet\n");
        }
        out.push_str(&format!("endsolid {safe_name}\n"));
    }
    std::fs::write(path, out)?;
    Ok(())
}

/// Read an STL file (binary or ASCII, auto-detected).
pub fn read_stl(path: &Path) -> Result<TriMesh> {
    let mut data = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut data)?;
    if data.len() < BINARY_HEADER_LEN + 4 {
        return Err(IoError::Malformed("file too small".into()));
    }
    // Detection: a valid binary STL has exactly 84 + 50*n bytes and a
    // plausible triangle count.
    let count = u32::from_le_bytes(
        data[BINARY_HEADER_LEN..BINARY_HEADER_LEN + 4]
            .try_into()
            .expect("slice length 4"),
    ) as usize;
    let expected = BINARY_HEADER_LEN + 4 + count * 50;
    if data.len() == expected && count > 0 {
        read_binary(&data, count)
    } else if looks_ascii(&data) {
        read_ascii(&data)
    } else {
        Err(IoError::Malformed(format!(
            "not a valid STL file (size {}, declared {} triangles)",
            data.len(),
            count
        )))
    }
}

fn looks_ascii(data: &[u8]) -> bool {
    let head = &data[..data.len().min(512)];
    let text = String::from_utf8_lossy(head).to_lowercase();
    text.contains("solid") && text.contains("facet")
}

fn read_binary(data: &[u8], count: usize) -> Result<TriMesh> {
    let mut mesh = TriMesh::with_capacity(count * 3, count);
    let mut off = BINARY_HEADER_LEN + 4;
    for _ in 0..count {
        off += 12; // normal (recomputed on read)
        let mut tri = [forge_core::Point3::origin(); 3];
        for p in tri.iter_mut() {
            let x = f32::from_le_bytes(data[off..off + 4].try_into().expect("4"));
            let y = f32::from_le_bytes(data[off + 4..off + 8].try_into().expect("4"));
            let z = f32::from_le_bytes(data[off + 8..off + 12].try_into().expect("4"));
            *p = forge_core::Point3::new(x as f64, y as f64, z as f64);
            off += 12;
        }
        off += 2; // attribute
        mesh.push_triangle(tri[0], tri[1], tri[2]);
    }
    mesh.weld(forge_geometry::WELD_EPS);
    mesh.compute_vertex_normals();
    Ok(mesh)
}

fn read_ascii(data: &[u8]) -> Result<TriMesh> {
    let text = String::from_utf8_lossy(data);
    let mut mesh = TriMesh::default();
    let mut current: Vec<forge_core::Point3> = Vec::with_capacity(3);
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("vertex").or_else(|| {
            let l = line.to_lowercase();
            if l.starts_with("vertex") {
                Some(line)
            } else {
                None
            }
        }) {
            let mut it = rest.split_whitespace().filter_map(|v| v.parse::<f64>().ok());
            let (x, y, z) = (it.next(), it.next(), it.next());
            if let (Some(x), Some(y), Some(z)) = (x, y, z) {
                current.push(forge_core::Point3::new(x, y, z));
            }
        } else if line.to_lowercase().starts_with("endfacet") {
            if current.len() == 3 {
                let tri = [current[0], current[1], current[2]];
                mesh.push_triangle(tri[0], tri[1], tri[2]);
            }
            current.clear();
        }
    }
    if mesh.tri_count() == 0 {
        return Err(IoError::Malformed("no triangles found in ASCII STL".into()));
    }
    mesh.weld(forge_geometry::WELD_EPS);
    mesh.compute_vertex_normals();
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::Point3;

    fn box_export() -> Vec<ExportMesh> {
        vec![ExportMesh {
            name: "box".into(),
            mesh: forge_geometry::primitives::box_from_center_extents(
                Point3::origin(),
                Vector3::new(10.0, 10.0, 10.0),
            ),
        }]
    }

    #[test]
    fn binary_roundtrip() {
        let dir = std::env::temp_dir();
        let path = dir.join("forgecad_test_bin.stl");
        write_binary_stl(&path, &box_export()).unwrap();
        let mesh = read_stl(&path).unwrap();
        assert_eq!(mesh.tri_count(), 12);
        let v = mesh.volume_signed();
        assert!((v - 1000.0).abs() < 1e-6, "volume {v}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn ascii_roundtrip() {
        let dir = std::env::temp_dir();
        let path = dir.join("forgecad_test_ascii.stl");
        write_ascii_stl(&path, &box_export()).unwrap();
        let mesh = read_stl(&path).unwrap();
        assert_eq!(mesh.tri_count(), 12);
        let v = mesh.volume_signed();
        assert!((v - 1000.0).abs() < 1e-6, "volume {v}");
        std::fs::remove_file(&path).ok();
    }
}
