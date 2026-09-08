//! 3MF (3D Manufacturing Format) export + import (I-02).
//!
//! 3MF is a ZIP (OPC) package wrapping an XML mesh description — the
//! production 3D-print interchange format. This module implements the
//! container and the core-spec subset needed for mesh exchange:
//!
//! - **Writer**: `[Content_Types].xml`, `_rels/.rels`, `3D/3dmodel.model`
//!   with one `<object>` per body and matching `<build>` items; entries
//!   are deflate-compressed (`miniz_oxide`, pure Rust).
//! - **Reader**: central-directory ZIP parsing (stored **and** deflate
//!   entries, CRC-verified), model-part discovery via the package
//!   relationships (with extension fallback), `<model unit>` scaling
//!   (micron…meter → mm), multi-object meshes, `<build><item>` transforms
//!   (4×3 row-vector convention, full affine), and recursive
//!   `<components>` expansion with a cycle guard.
//! - Not supported: materials/colors, print-settings and other
//!   extensions (ignored on read).

use crate::{ExportMesh, IoError, Result};
use forge_core::Point3;
use forge_geometry::TriMesh;
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// ZIP container
// ---------------------------------------------------------------------------

/// One ZIP entry, decompressed.
#[derive(Debug, Clone)]
struct ZipEntry {
    name: String,
    data: Vec<u8>,
}

/// CRC-32 (IEEE 802.3, polynomial 0xEDB88320).
fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *t = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn get_u16(data: &[u8], off: usize) -> Option<u16> {
    let b = data.get(off..off + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn get_u32(data: &[u8], off: usize) -> Option<u32> {
    let b = data.get(off..off + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Write a ZIP archive with deflate-compressed entries.
fn write_zip(path: &Path, entries: &[ZipEntry]) -> std::io::Result<()> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    // Fixed timestamp 1980-01-01 00:00 (DOS date must be >= 1980).
    const DATE: u16 = 0x0021;
    for entry in entries {
        let offset = out.len() as u32;
        let crc = crc32(&entry.data);
        let comp = miniz_oxide::deflate::compress_to_vec(&entry.data, 8);
        // Local file header.
        out.extend_from_slice(b"PK\x03\x04");
        put_u16(&mut out, 20); // version needed
        put_u16(&mut out, 0); // flags
        put_u16(&mut out, 8); // method: deflate
        put_u16(&mut out, 0); // time
        put_u16(&mut out, DATE);
        put_u32(&mut out, crc);
        put_u32(&mut out, comp.len() as u32);
        put_u32(&mut out, entry.data.len() as u32);
        put_u16(&mut out, entry.name.len() as u16);
        put_u16(&mut out, 0); // extra len
        out.extend_from_slice(entry.name.as_bytes());
        out.extend_from_slice(&comp);
        // Central directory record.
        central.extend_from_slice(b"PK\x01\x02");
        put_u16(&mut central, 20); // version made by
        put_u16(&mut central, 20); // version needed
        put_u16(&mut central, 0); // flags
        put_u16(&mut central, 8); // method
        put_u16(&mut central, 0); // time
        put_u16(&mut central, DATE);
        put_u32(&mut central, crc);
        put_u32(&mut central, comp.len() as u32);
        put_u32(&mut central, entry.data.len() as u32);
        put_u16(&mut central, entry.name.len() as u16);
        put_u16(&mut central, 0); // extra len
        put_u16(&mut central, 0); // comment len
        put_u16(&mut central, 0); // disk start
        put_u16(&mut central, 0); // internal attrs
        put_u32(&mut central, 0); // external attrs
        put_u32(&mut central, offset);
        central.extend_from_slice(entry.name.as_bytes());
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    let cd_size = central.len() as u32;
    // End of central directory.
    out.extend_from_slice(b"PK\x05\x06");
    put_u16(&mut out, 0); // disk
    put_u16(&mut out, 0); // cd disk
    put_u16(&mut out, entries.len() as u16);
    put_u16(&mut out, entries.len() as u16);
    put_u32(&mut out, cd_size);
    put_u32(&mut out, cd_offset);
    put_u16(&mut out, 0); // comment len
    std::fs::write(path, out)
}

/// Read a ZIP archive: all entries, decompressed and CRC-verified.
/// Supports stored (0) and deflate (8) entries — the two methods any
/// 3MF writer produces.
fn read_zip(data: &[u8]) -> Result<Vec<ZipEntry>> {
    // Locate EOCD (scan backwards; take the last signature).
    let scan_start = data.len().saturating_sub(22 + 65_535);
    let mut eocd = None;
    let mut i = data.len();
    while i > scan_start {
        i -= 1;
        if data.get(i..i + 4) == Some(b"PK\x05\x06") {
            eocd = Some(i);
            break;
        }
    }
    let eocd = eocd.ok_or_else(|| IoError::Malformed("zip: no end record".into()))?;
    let entry_count = get_u16(data, eocd + 10)
        .ok_or_else(|| IoError::Malformed("zip: truncated EOCD".into()))?
        as usize;
    let cd_offset = get_u32(data, eocd + 16)
        .ok_or_else(|| IoError::Malformed("zip: truncated EOCD".into()))?
        as usize;

    let mut entries = Vec::with_capacity(entry_count);
    let mut off = cd_offset;
    for _ in 0..entry_count {
        if data.get(off..off + 4) != Some(b"PK\x01\x02") {
            return Err(IoError::Malformed("zip: bad central record".into()));
        }
        let method = get_u16(data, off + 10).ok_or_else(malformed)?;
        let crc = get_u32(data, off + 16).ok_or_else(malformed)?;
        let comp_size = get_u32(data, off + 20).ok_or_else(malformed)? as usize;
        let uncomp_size = get_u32(data, off + 24).ok_or_else(malformed)? as usize;
        let name_len = get_u16(data, off + 28).ok_or_else(malformed)? as usize;
        let extra_len = get_u16(data, off + 30).ok_or_else(malformed)? as usize;
        let comment_len = get_u16(data, off + 32).ok_or_else(malformed)? as usize;
        let lho = get_u32(data, off + 42).ok_or_else(malformed)? as usize;
        let name = String::from_utf8_lossy(data.get(off + 46..off + 46 + name_len).unwrap_or(&[]))
            .into_owned();

        // Local header: skip to the data (its own name/extra lengths are
        // authoritative for the data start).
        if data.get(lho..lho + 4) != Some(b"PK\x03\x04") {
            return Err(IoError::Malformed(format!(
                "zip: bad local header for {name}"
            )));
        }
        let l_name = get_u16(data, lho + 26).ok_or_else(malformed)? as usize;
        let l_extra = get_u16(data, lho + 28).ok_or_else(malformed)? as usize;
        let data_start = lho + 30 + l_name + l_extra;
        let raw = data
            .get(data_start..data_start + comp_size)
            .ok_or_else(|| IoError::Malformed(format!("zip: truncated data for {name}")))?;

        let plain = match method {
            0 => raw.to_vec(),
            8 => {
                let out = miniz_oxide::inflate::decompress_to_vec(raw)
                    .map_err(|e| IoError::Malformed(format!("zip: inflate {name}: {e:?}")))?;
                out
            }
            other => {
                return Err(IoError::Unsupported(format!(
                    "zip entry {name}: compression method {other}"
                )))
            }
        };
        if plain.len() != uncomp_size {
            return Err(IoError::Malformed(format!(
                "zip entry {name}: size mismatch ({} vs {})",
                plain.len(),
                uncomp_size
            )));
        }
        if crc32(&plain) != crc {
            return Err(IoError::Malformed(format!(
                "zip entry {name}: CRC mismatch"
            )));
        }
        entries.push(ZipEntry { name, data: plain });
        off += 46 + name_len + extra_len + comment_len;
    }
    Ok(entries)
}

fn malformed() -> IoError {
    IoError::Malformed("zip: truncated central record".into())
}

/// Case-insensitive entry lookup.
fn find_entry<'a>(entries: &'a [ZipEntry], name: &str) -> Option<&'a ZipEntry> {
    let lower = name.to_ascii_lowercase();
    entries
        .iter()
        .find(|e| e.name.to_ascii_lowercase() == lower)
}

// ---------------------------------------------------------------------------
// XML helpers
// ---------------------------------------------------------------------------

/// XML-unescape a text/attribute value (writer escapes, reader
/// decodes).
fn xml_unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let (entity, len) = [
            ("&lt;", 4),
            ("&gt;", 4),
            ("&amp;", 5),
            ("&quot;", 6),
            ("&apos;", 6),
        ]
        .into_iter()
        .find(|(e, _)| rest.starts_with(e))
        .unwrap_or(("&", 1));
        out.push_str(match entity {
            "&lt;" => "<",
            "&gt;" => ">",
            "&amp;" => "&",
            "&quot;" => "\"",
            "&apos;" => "'",
            _ => "&",
        });
        rest = &rest[len..];
    }
    out.push_str(rest);
    out
}

/// XML-escape a text/attribute value.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// One parsed XML tag.
#[derive(Debug, Clone)]
struct XmlTag {
    /// Element name with namespace prefix stripped ("vertex").
    name: String,
    /// Attributes in document order.
    attrs: Vec<(String, String)>,
    /// `true` for self-closing `<vertex ... />`.
    self_closing: bool,
    /// `true` for an end tag `</name>`.
    end: bool,
}

impl XmlTag {
    fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// Minimal XML tag scanner: yields tags in document order, skipping
/// declarations, comments, doctypes and CDATA (as text). Attribute
/// values may contain `>`; quoted spans are respected while scanning
/// for the tag end.
fn scan_tags(xml: &str) -> Vec<XmlTag> {
    let bytes = xml.as_bytes();
    let mut tags = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let Some(open) = xml[i..].find('<') else {
            break;
        };
        let start = i + open;
        // Find the true end of the tag (respecting quotes).
        let mut j = start + 1;
        let mut quote: Option<u8> = None;
        while j < bytes.len() {
            let c = bytes[j];
            match quote {
                Some(q) if c == q => quote = None,
                Some(_) => {}
                None => match c {
                    b'"' | b'\'' => quote = Some(c),
                    b'>' => break,
                    _ => {}
                },
            }
            j += 1;
        }
        if j >= bytes.len() {
            break; // unterminated tag: stop
        }
        let inner = &xml[start + 1..j];
        i = j + 1;
        if let Some(rest) = inner.strip_prefix('?') {
            let _ = rest; // declaration
            continue;
        }
        if inner.starts_with("!--") || inner.starts_with("![CDATA[") || inner.starts_with('!') {
            // Comment / CDATA: find the matching close beyond j.
            let close = if inner.starts_with("!--") {
                "-->"
            } else if inner.starts_with("![CDATA[") {
                "]]>"
            } else {
                ">"
            };
            if let Some(p) = xml[i..].find(close) {
                i += p + close.len();
            }
            continue;
        }
        let end = inner.starts_with('/');
        let body = if end { &inner[1..] } else { inner };
        let self_closing = body.trim_end().ends_with('/');
        let body = body.trim_end().trim_end_matches('/');

        // Split name and attributes.
        let (name, attr_str) = match body.find(|c: char| c.is_whitespace()) {
            Some(p) => (body[..p].trim(), &body[p..]),
            None => (body.trim(), ""),
        };
        let mut attrs = Vec::new();
        let mut k = 0usize;
        let a = attr_str.as_bytes();
        while k < a.len() {
            // Skip whitespace.
            while k < a.len() && a[k].is_ascii_whitespace() {
                k += 1;
            }
            let key_start = k;
            while k < a.len() && a[k] != b'=' && !a[k].is_ascii_whitespace() {
                k += 1;
            }
            if k == key_start {
                break;
            }
            let key = attr_str[key_start..k].to_string();
            if k < a.len() && a[k] == b'=' {
                k += 1;
                if k < a.len() && (a[k] == b'"' || a[k] == b'\'') {
                    let q = a[k];
                    k += 1;
                    let v_start = k;
                    while k < a.len() && a[k] != q {
                        k += 1;
                    }
                    attrs.push((key, xml_unescape(&attr_str[v_start..k])));
                    k += 1;
                } else {
                    let v_start = k;
                    while k < a.len() && !a[k].is_ascii_whitespace() {
                        k += 1;
                    }
                    attrs.push((key, xml_unescape(&attr_str[v_start..k])));
                }
            } else {
                attrs.push((key, String::new()));
            }
        }
        // Strip namespace prefix ("ns:vertex" -> "vertex").
        let name = name.rsplit(':').next().unwrap_or(name).to_string();
        if !name.is_empty() {
            tags.push(XmlTag {
                name,
                attrs,
                self_closing,
                end,
            });
        }
    }
    tags
}

/// Parse a whitespace-separated list of `f64`s (3MF transform values).
fn parse_floats(s: &str) -> Vec<f64> {
    s.split_whitespace()
        .filter_map(|t| t.parse::<f64>().ok())
        .collect()
}

// ---------------------------------------------------------------------------
// 3MF model XML: writing
// ---------------------------------------------------------------------------

const CONTENT_TYPES_XML: &str = "\
<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/>\
</Types>";

const RELS_XML: &str = "\
<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" \
Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/>\
</Relationships>";

/// Write a 3MF package with one object per mesh (I-02 export).
pub fn write_3mf(path: &Path, meshes: &[ExportMesh]) -> Result<()> {
    if meshes.is_empty() {
        return Err(IoError::Malformed("no meshes to export".into()));
    }
    let mut model = String::from(
        "\
<?xml version=\"1.0\" encoding=\"UTF-8\"?>
<model unit=\"millimeter\" xml:lang=\"en-US\" \
xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">
<resources>
",
    );
    for (i, m) in meshes.iter().enumerate() {
        let id = i + 1;
        let name = xml_escape(&m.name);
        model.push_str(&format!(
            "<object id=\"{id}\" name=\"{name}\" type=\"model\">
<mesh>
<vertices>
"
        ));
        for p in &m.mesh.positions {
            model.push_str(&format!(
                "<vertex x=\"{}\" y=\"{}\" z=\"{}\"/>\n",
                p.x, p.y, p.z
            ));
        }
        model.push_str("</vertices>\n<triangles>\n");
        for t in 0..m.mesh.tri_count() {
            let [a, b, c] = m.mesh.triangle_idx(t);
            model.push_str(&format!("<triangle v1=\"{a}\" v2=\"{b}\" v3=\"{c}\"/>\n"));
        }
        model.push_str("</triangles>\n</mesh>\n</object>\n");
    }
    model.push_str("</resources>\n<build>\n");
    for i in 0..meshes.len() {
        model.push_str(&format!("<item objectid=\"{}\"/>\n", i + 1));
    }
    model.push_str("</build>\n</model>\n");

    let entries = [
        ZipEntry {
            name: "[Content_Types].xml".into(),
            data: CONTENT_TYPES_XML.as_bytes().to_vec(),
        },
        ZipEntry {
            name: "_rels/.rels".into(),
            data: RELS_XML.as_bytes().to_vec(),
        },
        ZipEntry {
            name: "3D/3dmodel.model".into(),
            data: model.into_bytes(),
        },
    ];
    write_zip(path, &entries)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 3MF model XML: reading
// ---------------------------------------------------------------------------

/// A 4×3 affine transform in 3MF row-vector convention:
/// `p' = [x y z 1] · M` (rows 0..2 linear, row 3 translation).
#[derive(Debug, Clone, Copy)]
struct Matrix4x3 {
    m: [[f64; 3]; 4],
}

impl Matrix4x3 {
    const IDENTITY: Self = Self {
        m: [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 0.0],
        ],
    };

    /// Parse the 12-float 3MF `transform` attribute (identity on
    /// anything malformed — 3MF consumers apply transforms leniently).
    fn parse(s: &str) -> Self {
        let v = parse_floats(s);
        if v.len() != 12 {
            return Self::IDENTITY;
        }
        Self {
            m: [
                [v[0], v[1], v[2]],
                [v[3], v[4], v[5]],
                [v[6], v[7], v[8]],
                [v[9], v[10], v[11]],
            ],
        }
    }

    fn apply(&self, p: &Point3) -> Point3 {
        let m = &self.m;
        Point3::new(
            p.x * m[0][0] + p.y * m[1][0] + p.z * m[2][0] + m[3][0],
            p.x * m[0][1] + p.y * m[1][1] + p.z * m[2][1] + m[3][1],
            p.x * m[0][2] + p.y * m[1][2] + p.z * m[2][2] + m[3][2],
        )
    }
}

/// Scale factor of a 3MF `<model unit>` to millimeters.
fn unit_scale(unit: Option<&str>) -> f64 {
    match unit
        .unwrap_or("millimeter")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "micron" => 0.001,
        "millimeter" | "mm" => 1.0,
        "centimeter" | "cm" => 10.0,
        "inch" => 25.4,
        "foot" => 304.8,
        "meter" | "metre" => 1000.0,
        _ => 1.0,
    }
}

/// One `<object>`: either a direct mesh or a component list.
#[derive(Debug, Default)]
struct Object3mf {
    name: String,
    mesh: Option<TriMesh>,
    /// `(objectid, transform)` pairs from `<components>`.
    components: Vec<(u64, Matrix4x3)>,
}

/// Scale the translation of a parsed transform: 3MF transform values
/// share the model unit, and the unit factor was already applied to the
/// vertex coordinates at parse time (the linear part is dimensionless).
fn scaled_transform(mut t: Matrix4x3, scale: f64) -> Matrix4x3 {
    for j in 0..3 {
        t.m[3][j] *= scale;
    }
    t
}

/// Read every mesh of a 3MF package: build items expanded (components
/// resolved, transforms applied), or all mesh objects when the package
/// has no `<build>` section. Meshes are welded, repaired and
/// consistently oriented, ready to become body features.
pub fn read_3mf(path: &Path) -> Result<Vec<(String, TriMesh)>> {
    let data = std::fs::read(path)?;
    let entries = read_zip(&data)?;
    if entries.is_empty() {
        return Err(IoError::Malformed("3mf: empty package".into()));
    }

    // Find the model part: via the package relationships, else any
    // .model part.
    let model_entry = {
        let target = find_entry(&entries, "_rels/.rels").and_then(|rels| {
            let text = String::from_utf8_lossy(&rels.data);
            scan_tags(&text)
                .into_iter()
                .find(|t| {
                    t.name == "Relationship"
                        && t.attr("Type")
                            .map(|ty| ty.ends_with("3dmodel"))
                            .unwrap_or(false)
                })
                .and_then(|t| {
                    t.attr("Target")
                        .map(|s| s.trim_start_matches('/').to_string())
                })
        });
        target
            .as_deref()
            .and_then(|t| find_entry(&entries, t))
            .or_else(|| {
                entries
                    .iter()
                    .find(|e| e.name.to_ascii_lowercase().ends_with(".model"))
            })
    };
    let model_entry = model_entry.ok_or_else(|| {
        IoError::Malformed("3mf: no 3D model part (missing relationship target)".into())
    })?;
    let xml = String::from_utf8_lossy(&model_entry.data).into_owned();
    let tags = scan_tags(&xml);

    let mut scale = 1.0;
    let mut objects: HashMap<u64, Object3mf> = HashMap::new();
    let mut build_items: Vec<(u64, Matrix4x3)> = Vec::new();
    // Parser state: current object id while inside its <mesh>.
    let mut current: Option<(u64, TriMesh)> = None;
    let mut in_vertices = false;
    let mut in_triangles = false;
    let mut pending_object: Option<u64> = None;

    for tag in &tags {
        match tag.name.as_str() {
            "model" if !tag.end => {
                scale = unit_scale(tag.attr("unit"));
            }
            "object" if !tag.end && !tag.self_closing => {
                let id: u64 = tag.attr("id").and_then(|s| s.parse().ok()).unwrap_or(0);
                let name = tag.attr("name").unwrap_or_default().to_string();
                let obj = objects.entry(id).or_default();
                if !name.is_empty() {
                    obj.name = name;
                }
                pending_object = Some(id);
            }
            "mesh" if !tag.end => {
                current = Some((pending_object.unwrap_or(0), TriMesh::default()));
            }
            "mesh" if tag.end => {
                if let Some((id, mesh)) = current.take() {
                    objects.entry(id).or_default().mesh = Some(mesh);
                }
            }
            "vertices" if !tag.end => in_vertices = true,
            "vertices" if tag.end => in_vertices = false,
            "vertex" if in_vertices => {
                let x: f64 = tag.attr("x").and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let y: f64 = tag.attr("y").and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let z: f64 = tag.attr("z").and_then(|s| s.parse().ok()).unwrap_or(0.0);
                if let Some((_, mesh)) = current.as_mut() {
                    mesh.positions
                        .push(Point3::new(x * scale, y * scale, z * scale));
                }
            }
            "triangles" if !tag.end => in_triangles = true,
            "triangles" if tag.end => in_triangles = false,
            "triangle" if in_triangles => {
                let v1: Option<u32> = tag.attr("v1").and_then(|s| s.parse().ok());
                let v2: Option<u32> = tag.attr("v2").and_then(|s| s.parse().ok());
                let v3: Option<u32> = tag.attr("v3").and_then(|s| s.parse().ok());
                if let (Some(v1), Some(v2), Some(v3), Some((_, mesh))) =
                    (v1, v2, v3, current.as_mut())
                {
                    mesh.indices.extend_from_slice(&[v1, v2, v3]);
                }
            }
            "component" => {
                // Inside <components> of the pending object.
                let id: Option<u64> = tag.attr("objectid").and_then(|s| s.parse().ok());
                let transform = tag
                    .attr("transform")
                    .map(|s| scaled_transform(Matrix4x3::parse(s), scale));
                if let (Some(cid), Some(oid)) = (id, pending_object) {
                    let obj = objects.entry(oid).or_default();
                    obj.components
                        .push((cid, transform.unwrap_or(Matrix4x3::IDENTITY)));
                }
            }
            "item" => {
                let id: Option<u64> = tag.attr("objectid").and_then(|s| s.parse().ok());
                let transform = tag
                    .attr("transform")
                    .map(|s| scaled_transform(Matrix4x3::parse(s), scale));
                if let Some(id) = id {
                    build_items.push((id, transform.unwrap_or(Matrix4x3::IDENTITY)));
                }
            }
            _ => {}
        }
    }

    if objects.is_empty() {
        return Err(IoError::Malformed("3mf: no objects found".into()));
    }

    /// Resolve an object to a mesh, expanding components recursively
    /// (with a cycle guard). Transforms compose parent-last.
    fn resolve(
        id: u64,
        objects: &HashMap<u64, Object3mf>,
        visited: &mut Vec<u64>,
    ) -> Option<TriMesh> {
        if visited.contains(&id) {
            return None; // cyclic components: skip (malformed package)
        }
        visited.push(id);
        let obj = objects.get(&id)?;
        let result = if let Some(mesh) = &obj.mesh {
            Some(mesh.clone())
        } else if !obj.components.is_empty() {
            // Merge the referenced children, each with its transform.
            let mut merged = TriMesh::default();
            for (cid, t) in &obj.components {
                if let Some(child) = resolve(*cid, objects, visited) {
                    merged.merge(&transform_mesh(&child, t));
                }
            }
            if merged.tri_count() > 0 {
                Some(merged)
            } else {
                None
            }
        } else {
            None
        };
        visited.pop();
        result
    }

    /// Apply a 3MF transform to every vertex (full affine; normals are
    /// recomputed afterwards).
    fn transform_mesh(mesh: &TriMesh, t: &Matrix4x3) -> TriMesh {
        let mut out = mesh.clone();
        // Scale-aware: coordinates are already in mm (unit_scale applied
        // at parse time), and 3MF transform values share the model unit
        // — so scale the translation by the same factor the vertices
        // were scaled by. The linear part is dimensionless.
        for p in out.positions.iter_mut() {
            *p = t.apply(p);
        }
        out
    }

    // Build the result list: build items if present, else every object
    // with a mesh.
    let mut out: Vec<(String, TriMesh)> = Vec::new();
    if build_items.is_empty() {
        let mut ids: Vec<u64> = objects.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let obj = &objects[&id];
            let name = if obj.name.is_empty() {
                format!("object {id}")
            } else {
                obj.name.clone()
            };
            let mut visited = Vec::new();
            if let Some(mesh) = resolve(id, &objects, &mut visited) {
                out.push((name, mesh));
            }
        }
    } else {
        for (id, t) in build_items {
            let obj_name = objects
                .get(&id)
                .map(|o| {
                    if o.name.is_empty() {
                        format!("object {id}")
                    } else {
                        o.name.clone()
                    }
                })
                .unwrap_or_else(|| format!("object {id}"));
            let mut visited = Vec::new();
            if let Some(mesh) = resolve(id, &objects, &mut visited) {
                out.push((obj_name, transform_mesh(&mesh, &t)));
            }
        }
    }

    // Post-process: the standard import pipeline.
    for (_, mesh) in out.iter_mut() {
        mesh.weld(forge_geometry::WELD_EPS);
        mesh.remove_degenerate(1e-12);
        mesh.repair_orientation();
        mesh.compute_vertex_normals();
    }
    out.retain(|(_, m)| m.tri_count() > 0);
    if out.is_empty() {
        return Err(IoError::Malformed("3mf: no mesh data found".into()));
    }
    // Sanity: indices in range.
    for (name, mesh) in &out {
        let nv = mesh.positions.len();
        if mesh.indices.iter().any(|&i| i as usize >= nv) {
            return Err(IoError::Malformed(format!(
                "3mf: object \"{name}\" has out-of-range triangle indices"
            )));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use forge_core::Vector3;
    use forge_geometry::primitives;

    fn tetra_mesh() -> TriMesh {
        primitives::box_from_center_extents(Point3::origin(), Vector3::new(10.0, 10.0, 10.0))
    }

    /// Round-trip: export a box + a second translated body, read back,
    /// verify names, counts and volumes survive the deflate ZIP +
    /// XML pipeline.
    #[test]
    fn roundtrip_multi_body() {
        let dir = std::env::temp_dir();
        let path = dir.join("forgecad_test_3mf.3mf");
        let a = tetra_mesh();
        let mut b = tetra_mesh();
        b = b.transformed(&forge_core::Transform::translation(40.0, 0.0, 0.0));
        let meshes = vec![
            ExportMesh {
                name: "body one".into(),
                mesh: a.clone(),
            },
            ExportMesh {
                name: "body <two> & \"quotes\"".into(),
                mesh: b,
            },
        ];
        write_3mf(&path, &meshes).unwrap();
        let back = read_3mf(&path).unwrap();
        assert_eq!(back.len(), 2, "both objects come back");
        assert_eq!(back[0].0, "body one");
        // The XML-escaped name must survive escaping.
        assert_eq!(back[1].0, "body <two> & \"quotes\"");
        for (name, mesh) in &back {
            let v = mesh.volume_signed();
            assert!(
                (v - 1000.0).abs() < 1e-6,
                "{name}: volume {v} vs 1000 (box 10×10×10)"
            );
            assert!(mesh.normals.is_some());
        }
        // The second body is translated 40 mm along +X.
        let center = back[1].1.centroid();
        assert!((center.x - 40.0).abs() < 1e-6, "centroid {center:?}");
        std::fs::remove_file(&path).ok();
    }

    /// A stored (uncompressed) ZIP 3MF — the other compression path real
    /// files may use. Hand-built with a tiny model referencing two build
    /// items, one with a translation transform.
    #[test]
    fn reads_stored_entries_and_transforms() {
        let model = br#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
<resources><object id="7" name="cube" type="model"><mesh><vertices>
<vertex x="0" y="0" z="0"/><vertex x="10" y="0" z="0"/>
<vertex x="0" y="10" z="0"/><vertex x="10" y="10" z="0"/>
<vertex x="0" y="0" z="10"/><vertex x="10" y="0" z="10"/>
<vertex x="0" y="10" z="10"/><vertex x="10" y="10" z="10"/>
</vertices><triangles>
<triangle v1="0" v2="2" v3="3"/><triangle v1="0" v2="3" v3="1"/>
<triangle v1="4" v2="5" v3="7"/><triangle v1="4" v2="7" v3="6"/>
<triangle v1="0" v2="1" v3="5"/><triangle v1="0" v2="5" v3="4"/>
<triangle v1="2" v2="6" v3="7"/><triangle v1="2" v2="7" v3="3"/>
<triangle v1="1" v2="3" v3="7"/><triangle v1="1" v2="7" v3="5"/>
<triangle v1="0" v2="4" v3="6"/><triangle v1="0" v2="6" v3="2"/>
</triangles></mesh></object></resources>
<build>
<item objectid="7" transform="1 0 0 0 1 0 0 0 1 30 0 0"/>
<item objectid="7"/>
</build>
</model>"#;
        // Stored ZIP: local headers + central directory + EOCD, method 0.
        let mut zip: Vec<u8> = Vec::new();
        let mut central: Vec<u8> = Vec::new();
        let entries: Vec<(&str, Vec<u8>)> = vec![
            ("[Content_Types].xml", CONTENT_TYPES_XML.as_bytes().to_vec()),
            ("_rels/.rels", RELS_XML.as_bytes().to_vec()),
            ("3D/3dmodel.model", model.to_vec()),
        ];
        let mut offsets = Vec::new();
        for (name, data) in &entries {
            offsets.push(zip.len() as u32);
            let crc = crc32(data);
            zip.extend_from_slice(b"PK\x03\x04");
            put_u16(&mut zip, 20);
            put_u16(&mut zip, 0);
            put_u16(&mut zip, 0); // stored
            put_u16(&mut zip, 0);
            put_u16(&mut zip, 0x21);
            put_u32(&mut zip, crc);
            put_u32(&mut zip, data.len() as u32);
            put_u32(&mut zip, data.len() as u32);
            put_u16(&mut zip, name.len() as u16);
            put_u16(&mut zip, 0);
            zip.extend_from_slice(name.as_bytes());
            zip.extend_from_slice(data);
        }
        let cd_offset = zip.len() as u32;
        for ((name, data), offset) in entries.iter().zip(&offsets) {
            let crc = crc32(data);
            central.extend_from_slice(b"PK\x01\x02");
            put_u16(&mut central, 20);
            put_u16(&mut central, 20);
            put_u16(&mut central, 0);
            put_u16(&mut central, 0);
            put_u16(&mut central, 0);
            put_u16(&mut central, 0x21);
            put_u32(&mut central, crc);
            put_u32(&mut central, data.len() as u32);
            put_u32(&mut central, data.len() as u32);
            put_u16(&mut central, name.len() as u16);
            put_u16(&mut central, 0);
            put_u16(&mut central, 0);
            put_u16(&mut central, 0);
            put_u16(&mut central, 0);
            put_u32(&mut central, 0);
            put_u32(&mut central, *offset);
            central.extend_from_slice(name.as_bytes());
        }
        zip.extend_from_slice(&central);
        let cd_size = central.len() as u32;
        zip.extend_from_slice(b"PK\x05\x06");
        put_u16(&mut zip, 0);
        put_u16(&mut zip, 0);
        put_u16(&mut zip, 3);
        put_u16(&mut zip, 3);
        put_u32(&mut zip, cd_size);
        put_u32(&mut zip, cd_offset);
        put_u16(&mut zip, 0);

        let parsed = read_zip(&zip).expect("stored zip parses");
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[2].name, "3D/3dmodel.model");

        let dir = std::env::temp_dir();
        let path = dir.join("forgecad_test_3mf_stored.3mf");
        std::fs::write(&path, &zip).unwrap();
        let meshes = read_3mf(&path).unwrap();
        // Two build items referencing the same object → two meshes, the
        // first translated 30 mm along +X (row-vector convention: the
        // translation lives in the last 3 values).
        assert_eq!(meshes.len(), 2);
        for (name, mesh) in &meshes {
            assert_eq!(name, "cube");
            assert!(
                (mesh.volume_signed() - 1000.0).abs() < 1e-6,
                "volume {}",
                mesh.volume_signed()
            );
        }
        let c0 = meshes[0].1.centroid();
        // Cube spans 0..10 (centroid x = 5) + 30 mm translation → 35.
        assert!((c0.x - 35.0).abs() < 1e-9, "transformed centroid {c0:?}");
        let c1 = meshes[1].1.centroid();
        assert!((c1.x - 5.0).abs() < 1e-9, "identity centroid {c1:?}");
        std::fs::remove_file(&path).ok();
    }

    /// Unit handling: a centimeter-unit model scales ×10 into mm.
    #[test]
    fn unit_scaling_on_import() {
        let model = br#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="centimeter"><resources><object id="1" type="model"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="1" y="0" z="0"/>
<vertex x="0" y="1" z="0"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/></triangles>
</mesh></object></resources></model>"#;
        // Reuse the deflate writer with a custom model part.
        let path = std::env::temp_dir().join("forgecad_test_3mf_units.3mf");
        let entries = [
            ZipEntry {
                name: "[Content_Types].xml".into(),
                data: CONTENT_TYPES_XML.as_bytes().to_vec(),
            },
            ZipEntry {
                name: "_rels/.rels".into(),
                data: RELS_XML.as_bytes().to_vec(),
            },
            ZipEntry {
                name: "3D/3dmodel.model".into(),
                data: model.to_vec(),
            },
        ];
        write_zip(&path, &entries).unwrap();
        let meshes = read_3mf(&path).unwrap();
        assert_eq!(meshes.len(), 1);
        // 1 cm becomes 10 mm on every axis.
        let v = meshes[0].1.positions[1];
        assert!((v.x - 10.0).abs() < 1e-9, "vertex {v:?}");
        std::fs::remove_file(&path).ok();
    }

    /// Components: an object assembling two translated instances of a
    /// child mesh must import as the merged, transformed geometry.
    #[test]
    fn components_expand_on_import() {
        let model = br#"<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter"><resources>
<object id="1" name="unit cube" type="model"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="10" y="0" z="0"/>
<vertex x="0" y="10" z="0"/><vertex x="10" y="10" z="0"/>
<vertex x="0" y="0" z="10"/><vertex x="10" y="0" z="10"/>
<vertex x="0" y="10" z="10"/><vertex x="10" y="10" z="10"/></vertices>
<triangles>
<triangle v1="0" v2="2" v3="3"/><triangle v1="0" v2="3" v3="1"/>
<triangle v1="4" v2="5" v3="7"/><triangle v1="4" v2="7" v3="6"/>
<triangle v1="0" v2="1" v3="5"/><triangle v1="0" v2="5" v3="4"/>
<triangle v1="2" v2="6" v3="7"/><triangle v1="2" v2="7" v3="3"/>
<triangle v1="1" v2="3" v3="7"/><triangle v1="1" v2="7" v3="5"/>
<triangle v1="0" v2="4" v3="6"/><triangle v1="0" v2="6" v3="2"/>
</triangles></mesh></object>
<object id="2" name="assembly" type="model"><components>
<component objectid="1"/>
<component objectid="1" transform="1 0 0 0 1 0 0 0 1 100 0 0"/>
</components></object>
</resources>
<build><item objectid="2"/></build></model>"#;
        let path = std::env::temp_dir().join("forgecad_test_3mf_components.3mf");
        let entries = [
            ZipEntry {
                name: "[Content_Types].xml".into(),
                data: CONTENT_TYPES_XML.as_bytes().to_vec(),
            },
            ZipEntry {
                name: "_rels/.rels".into(),
                data: RELS_XML.as_bytes().to_vec(),
            },
            ZipEntry {
                name: "3D/3dmodel.model".into(),
                data: model.to_vec(),
            },
        ];
        write_zip(&path, &entries).unwrap();
        let meshes = read_3mf(&path).unwrap();
        assert_eq!(meshes.len(), 1, "the assembly object");
        assert_eq!(meshes[0].0, "assembly");
        // Two disjoint 10 mm cubes: total volume 2000, bbox 110 wide.
        let v = meshes[0].1.volume_signed();
        assert!((v - 2000.0).abs() < 1e-6, "volume {v}");
        let bb = meshes[0].1.bbox();
        assert!((bb.max.x - 110.0).abs() < 1e-6, "bbox {bb:?}");
        std::fs::remove_file(&path).ok();
    }

    /// Malformed inputs fail with clear errors, not panics.
    #[test]
    fn malformed_inputs_rejected() {
        // Not a zip at all.
        let path = std::env::temp_dir().join("forgecad_test_3mf_bad.3mf");
        std::fs::write(&path, b"definitely not a zip file").unwrap();
        assert!(read_3mf(&path).is_err());
        // Valid zip, no model part.
        let entries = [ZipEntry {
            name: "readme.txt".into(),
            data: b"hello".to_vec(),
        }];
        write_zip(&path, &entries).unwrap();
        let err = read_3mf(&path).expect_err("must fail");
        assert!(err.to_string().contains("3D model part"), "{err}");
        std::fs::remove_file(&path).ok();
    }

    /// CRC mismatch is detected (data corruption).
    #[test]
    fn crc_corruption_detected() {
        let data = b"hello world, hello world, hello world".to_vec();
        let mut zip: Vec<u8> = Vec::new();
        let crc = crc32(&data) ^ 0xDEAD_BEEF; // wrong on purpose
        zip.extend_from_slice(b"PK\x03\x04");
        put_u16(&mut zip, 20);
        put_u16(&mut zip, 0);
        put_u16(&mut zip, 0);
        put_u16(&mut zip, 0);
        put_u16(&mut zip, 0x21);
        put_u32(&mut zip, crc);
        put_u32(&mut zip, data.len() as u32);
        put_u32(&mut zip, data.len() as u32);
        put_u16(&mut zip, 1);
        put_u16(&mut zip, 0);
        zip.extend_from_slice(b"a");
        zip.extend_from_slice(&data);
        let cd_offset = zip.len() as u32;
        zip.extend_from_slice(b"PK\x01\x02");
        put_u16(&mut zip, 20);
        put_u16(&mut zip, 20);
        put_u16(&mut zip, 0);
        put_u16(&mut zip, 0);
        put_u16(&mut zip, 0);
        put_u16(&mut zip, 0x21);
        put_u32(&mut zip, crc);
        put_u32(&mut zip, data.len() as u32);
        put_u32(&mut zip, data.len() as u32);
        put_u16(&mut zip, 1);
        put_u16(&mut zip, 0);
        put_u16(&mut zip, 0);
        put_u16(&mut zip, 0);
        put_u16(&mut zip, 0);
        put_u32(&mut zip, 0);
        put_u32(&mut zip, 0);
        zip.extend_from_slice(b"a");
        zip.extend_from_slice(b"PK\x05\x06");
        put_u16(&mut zip, 0);
        put_u16(&mut zip, 0);
        put_u16(&mut zip, 1);
        put_u16(&mut zip, 1);
        let cd_size_pre = (zip.len() + 12) as u64 - cd_offset as u64;
        put_u32(&mut zip, cd_size_pre as u32);
        put_u32(&mut zip, cd_offset);
        put_u16(&mut zip, 0);
        let err = read_zip(&zip).expect_err("crc must fail");
        assert!(err.to_string().contains("CRC"), "{err}");
    }

    /// The XML scanner handles quoted `>` inside attributes, comments,
    /// namespace prefixes and self-closing tags.
    #[test]
    fn xml_scanner_edge_cases() {
        let xml = r#"<?xml version="1.0"?>
<!-- a comment with <tags> inside -->
<n:model xmlns:n="urn:x" unit="mm" note="a > b &amp; c">
<vertex x="1" y="2" z="3"/>
<empty/>
</n:model>"#;
        let tags = scan_tags(xml);
        assert_eq!(tags.len(), 4, "{tags:?}");
        assert_eq!(tags[0].name, "model");
        assert_eq!(tags[0].attr("unit"), Some("mm"));
        assert_eq!(tags[0].attr("note"), Some("a > b & c"));
        assert_eq!(tags[1].name, "vertex");
        assert!(tags[1].self_closing);
        assert_eq!(tags[1].attr("y"), Some("2"));
        assert_eq!(tags[2].name, "empty");
        assert!(tags[3].end);
        assert_eq!(tags[3].name, "model");
    }
}
