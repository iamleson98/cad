//! glTF 2.0 export with an embedded binary buffer (GLB-style single file
//! via `.gltf` + external-free DATA URI, or `.glb` container).
//!
//! This writer produces a standard glTF 2.0 JSON with the triangle data in
//! a binary buffer embedded as a base64 data URI, which every glTF loader
//! (three.js, Blender, Windows 3D Viewer) accepts. Position and normal
//! attributes are `f32` (the GPU representation).

use crate::{ExportMesh, Result};
use serde_json::json;
use std::path::Path;

/// Write meshes to a self-contained `.gltf` file (embedded base64 buffer).
pub fn write_gltf(path: &Path, meshes: &[ExportMesh]) -> Result<()> {
    if meshes.is_empty() {
        return Err(crate::IoError::Malformed("no meshes to export".into()));
    }

    // Pack all meshes into one binary buffer: positions + normals,
    // 4-byte aligned per mesh.
    let mut buffer: Vec<u8> = Vec::new();
    let mut accessors = Vec::new();
    let mut buffer_views = Vec::new();
    let mut meshes_json = Vec::new();
    let mut nodes_json = Vec::new();

    for (i, m) in meshes.iter().enumerate() {
        let mesh = &m.mesh;
        let normals = mesh.normals.clone().unwrap_or_else(|| {
            let mut tmp = mesh.clone();
            tmp.compute_vertex_normals();
            tmp.normals.unwrap_or_default()
        });

        // Positions.
        let pos_offset = buffer.len();
        let pos_len = mesh.positions.len() * 12;
        buffer.reserve(pos_len + normals.len() * 12);
        for p in &mesh.positions {
            buffer.extend_from_slice(&(p.x as f32).to_le_bytes());
            buffer.extend_from_slice(&(p.y as f32).to_le_bytes());
            buffer.extend_from_slice(&(p.z as f32).to_le_bytes());
        }
        let pos_view = buffer_views.len();
        buffer_views.push(json!({
            "buffer": 0,
            "byteOffset": pos_offset,
            "byteLength": pos_len,
        }));
        accessors.push(json!({
            "bufferView": pos_view,
            "componentType": 5126, // FLOAT
            "count": mesh.positions.len(),
            "type": "VEC3",
            "min": [
                mesh.positions.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
                mesh.positions.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
                mesh.positions.iter().map(|p| p.z).fold(f64::INFINITY, f64::min),
            ],
            "max": [
                mesh.positions.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max),
                mesh.positions.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max),
                mesh.positions.iter().map(|p| p.z).fold(f64::NEG_INFINITY, f64::max),
            ],
        }));
        let pos_accessor = accessors.len() - 1;

        // Normals.
        let norm_offset = buffer.len();
        for n in &normals {
            buffer.extend_from_slice(&(n.x as f32).to_le_bytes());
            buffer.extend_from_slice(&(n.y as f32).to_le_bytes());
            buffer.extend_from_slice(&(n.z as f32).to_le_bytes());
        }
        let norm_view = buffer_views.len();
        buffer_views.push(json!({
            "buffer": 0,
            "byteOffset": norm_offset,
            "byteLength": normals.len() * 12,
        }));
        accessors.push(json!({
            "bufferView": norm_view,
            "componentType": 5126,
            "count": normals.len(),
            "type": "VEC3",
        }));
        let norm_accessor = accessors.len() - 1;

        // Indices (u32).
        let idx_offset = buffer.len();
        for idx in &mesh.indices {
            buffer.extend_from_slice(&idx.to_le_bytes());
        }
        let idx_view = buffer_views.len();
        buffer_views.push(json!({
            "buffer": 0,
            "byteOffset": idx_offset,
            "byteLength": mesh.indices.len() * 4,
        }));
        accessors.push(json!({
            "bufferView": idx_view,
            "componentType": 5125, // UNSIGNED_INT
            "count": mesh.indices.len(),
            "type": "SCALAR",
        }));
        let idx_accessor = accessors.len() - 1;

        meshes_json.push(json!({
            "name": m.name,
            "primitives": [{
                "attributes": {
                    "POSITION": pos_accessor,
                    "NORMAL": norm_accessor,
                },
                "indices": idx_accessor,
                "mode": 4, // TRIANGLES
                "material": 0,
            }],
        }));
        nodes_json.push(json!({ "mesh": i, "name": m.name }));
    }

    let b64 = {
        // base64 (standard alphabet, padded) without external deps.
        const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::with_capacity(buffer.len().div_ceil(3) * 4);
        for chunk in buffer.chunks(3) {
            let b = [
                chunk[0],
                chunk.get(1).copied().unwrap_or(0),
                chunk.get(2).copied().unwrap_or(0),
            ];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            out.push(TABLE[(n >> 18) as usize & 63]);
            out.push(TABLE[(n >> 12) as usize & 63]);
            out.push(if chunk.len() > 1 {
                TABLE[(n >> 6) as usize & 63]
            } else {
                b'='
            });
            out.push(if chunk.len() > 2 {
                TABLE[n as usize & 63]
            } else {
                b'='
            });
        }
        let mut s = String::with_capacity(out.len());
        for b in out {
            s.push(b as char);
        }
        s
    };

    let gltf = json!({
        "asset": {
            "version": "2.0",
            "generator": "ForgeCAD 0.1",
        },
        "scene": 0,
        "scenes": [{ "nodes": (0..meshes.len()).collect::<Vec<_>>() }],
        "nodes": nodes_json,
        "meshes": meshes_json,
        "materials": [{
            "name": "default",
            "pbrMetallicRoughness": {
                "baseColorFactor": [0.72, 0.75, 0.78, 1.0],
                "metallicFactor": 0.05,
                "roughnessFactor": 0.6,
            },
        }],
        "buffers": [{
            "byteLength": buffer.len(),
            "uri": format!("data:application/octet-stream;base64,{b64}"),
        }],
        "bufferViews": buffer_views,
        "accessors": accessors,
    });

    let bytes = serde_json::to_vec_pretty(&gltf)
        .map_err(|e| crate::IoError::Serde(format!("json encode: {e}")))?;
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::{Point3, Vector3};

    #[test]
    fn gltf_json_is_valid_and_embeds_buffer() {
        let meshes = vec![ExportMesh {
            name: "box".into(),
            mesh: forge_geometry::primitives::box_from_center_extents(
                Point3::origin(),
                Vector3::new(8.0, 8.0, 8.0),
            ),
        }];
        let dir = std::env::temp_dir();
        let path = dir.join("forgecad_test.gltf");
        write_gltf(&path, &meshes).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(doc["asset"]["version"], "2.0");
        assert_eq!(doc["meshes"].as_array().unwrap().len(), 1);
        assert_eq!(doc["accessors"].as_array().unwrap().len(), 3);
        let uri = doc["buffers"][0]["uri"].as_str().unwrap();
        assert!(uri.starts_with("data:application/octet-stream;base64,"));
        // Buffer length must match the packed data (12 tris * 3 verts... the
        // welded box has 8 verts).
        let declared = doc["buffers"][0]["byteLength"].as_u64().unwrap() as usize;
        let decoded_len = base64_len(uri.split(',').nth(1).unwrap());
        assert_eq!(declared, decoded_len);
        std::fs::remove_file(&path).ok();
    }

    fn base64_len(s: &str) -> usize {
        let pad = s.chars().filter(|c| *c == '=').count();
        s.len() * 3 / 4 - pad
    }
}
