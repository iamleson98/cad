//! WGSL shader sources for the multi-pass pipeline.

/// Shaded-mesh pass (opaque and transparent variants share the source;
/// pipelines differ in blend/depth state).
///
/// Outputs HDR color to attachment 0 and the view-space normal to
/// attachment 1 (used by the composite edge-detect pass).
///
/// Camera fields beyond the matrices:
/// - `clip_plane` (xyz = unit normal, w = offset): section-view cut plane,
///   kept where `dot(n, p) - w >= 0` (W-02).
/// - `render_params`: x = clipping enabled, y = wireframe mode, z = x-ray
///   alpha override (0 = off), w = spare (W-05).
pub const MESH_SHADER: &str = r#"
struct Camera {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    eye_pos: vec4<f32>,
    light_dir: vec4<f32>,
    depth_params: vec4<f32>,
    clip_plane: vec4<f32>,
    render_params: vec4<f32>,
};
struct Model {
    model: mat4x4<f32>,
    color: vec4<f32>,
    pick_id: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> model: Model;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs_main(
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
) -> VsOut {
    let world = model.model * vec4<f32>(pos, 1.0);
    var out: VsOut;
    out.position = camera.view_proj * world;
    out.world_pos = world.xyz;
    // Rigid transforms: the model matrix rotates normals correctly.
    out.normal = (model.model * vec4<f32>(normal, 0.0)).xyz;
    return out;
}

struct FsOut {
    @location(0) color: vec4<f32>,
    @location(1) normal: vec4<f32>,
};

@fragment
fn fs_main(in: VsOut) -> FsOut {
    // Section view (W-02): discard fragments behind the cut plane.
    if (camera.render_params.x > 0.5) {
        if (dot(camera.clip_plane.xyz, in.world_pos) - camera.clip_plane.w < 0.0) {
            discard;
        }
    }

    let n = normalize(in.normal);
    let base = model.color.rgb;
    var alpha = model.color.a;

    // X-ray (W-05): force a translucent alpha on every body.
    if (camera.render_params.z > 0.0) {
        alpha = camera.render_params.z;
    }

    var lit: vec3<f32>;
    if (camera.render_params.y > 0.5) {
        // Wireframe base (W-05): flat unlit surfaces; the composite pass
        // converts the depth/normal gradients into line art.
        lit = base * 0.35;
    } else {
        let l = normalize(-camera.light_dir.xyz);
        let v = normalize(camera.eye_pos.xyz - in.world_pos);

        // Simple PBR-ish shading: Lambert + Blinn-Phong specular + hemisphere
        // ambient. Parameters are fixed per-material-color in v0.1.
        let diff = max(dot(n, l), 0.0);
        let h = normalize(l + v);
        let spec = pow(max(dot(n, h), 0.0), 48.0) * 0.25;
        let sky = vec3<f32>(0.42, 0.46, 0.52);
        let ground = vec3<f32>(0.18, 0.18, 0.16);
        let ambient = mix(ground, sky, n.z * 0.5 + 0.5) * 0.35;

        lit = base * (diff * 0.85 + ambient) + vec3<f32>(spec);
        // Light fog towards the far plane for depth perception.
        let dist = length(camera.eye_pos.xyz - in.world_pos);
        let fog = clamp(dist / camera.depth_params.y, 0.0, 1.0) * 0.25;
        lit = mix(lit, vec3<f32>(0.55, 0.58, 0.62), fog);
    }

    var out: FsOut;
    out.color = vec4<f32>(lit, alpha);
    out.normal = vec4<f32>(normalize(in.normal) * 0.5 + 0.5, 1.0);
    return out;
}
"#;

/// Edge line pass: world-space lines with per-body color.
///
/// Two entry-point pairs: `vs_main`/`fs_main` (plain, used for the ground
/// grid) and `vs_main_clip`/`fs_main_clip` (section-clipped, used for body
/// feature edges so cut-away geometry loses its lines too — W-02).
pub const LINE_SHADER: &str = r#"
struct Camera {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    eye_pos: vec4<f32>,
    light_dir: vec4<f32>,
    depth_params: vec4<f32>,
    clip_plane: vec4<f32>,
    render_params: vec4<f32>,
};
struct Model {
    model: mat4x4<f32>,
    color: vec4<f32>,
    pick_id: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> model: Model;

struct VsOut {
    @builtin(position) position: vec4<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>) -> VsOut {
    var out: VsOut;
    out.position = camera.view_proj * model.model * vec4<f32>(pos, 1.0);
    return out;
}

@fragment
fn fs_main(_in: VsOut) -> @location(0) vec4<f32> {
    return model.color;
}

struct ClipOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
};

@vertex
fn vs_main_clip(@location(0) pos: vec3<f32>) -> ClipOut {
    let world = model.model * vec4<f32>(pos, 1.0);
    var out: ClipOut;
    out.position = camera.view_proj * world;
    out.world_pos = world.xyz;
    return out;
}

@fragment
fn fs_main_clip(in: ClipOut) -> @location(0) vec4<f32> {
    if (camera.render_params.x > 0.5) {
        if (dot(camera.clip_plane.xyz, in.world_pos) - camera.clip_plane.w < 0.0) {
            discard;
        }
    }
    return model.color;
}
"#;

/// Picking pass: writes the body id as an integer color (FR-RD-02).
/// Respects the section plane so clipped-away geometry cannot be picked.
pub const PICK_SHADER: &str = r#"
struct Camera {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    eye_pos: vec4<f32>,
    light_dir: vec4<f32>,
    depth_params: vec4<f32>,
    clip_plane: vec4<f32>,
    render_params: vec4<f32>,
};
struct Model {
    model: mat4x4<f32>,
    color: vec4<f32>,
    pick_id: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> model: Model;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
};

@vertex
fn vs_main(
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
) -> VsOut {
    let world = model.model * vec4<f32>(pos, 1.0);
    var out: VsOut;
    out.position = camera.view_proj * world;
    out.world_pos = world.xyz;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<u32> {
    if (camera.render_params.x > 0.5) {
        if (dot(camera.clip_plane.xyz, in.world_pos) - camera.clip_plane.w < 0.0) {
            discard;
        }
    }
    let id = u32(model.pick_id.x);
    return vec4<u32>(id, 0u, 0u, 1u);
}
"#;

/// Order-independent transparency (W-03): front depth peeling.
///
/// The renderer runs `peel_layers` iterations of two passes over the
/// transparent geometry, then one residual pass:
///
/// 1. **Peel pass** (`fs_peel`) — depth-only attachment (`Depth32Float`,
///    compare `Less`, cleared to 1.0). A fragment survives when it is
///    strictly behind the previously peeled layer depth (sampled from
///    `t_prev_layer`); the `Less` depth test keeps the minimum surviving
///    depth, which becomes the next layer. Coplanar survivors merge via
///    `LAYER_EPS`.
/// 2. **Blend pass** (`fs_blend`) — fragments within `LAYER_EPS` of the
///    new front layer shade and blend **under** (premultiplied
///    front-to-back accumulation into `accum`), so nearer layers
///    correctly attenuate farther ones — interpenetrating geometry
///    included, no CPU sorting.
/// 3. **Residual pass** (`fs_blend_residual`) — everything behind the
///    last peeled layer blends in one unsorted pass (the order error is
///    confined to the K+1-nested and farther layers).
///
/// Section clipping (W-02) and the X-ray alpha override (W-05) apply in
/// every peel fragment, exactly like the opaque mesh pass. Fragments
/// behind opaque geometry are rejected by the depth test against the
/// opaque depth buffer in every pass.
pub const PEEL_SHADER: &str = r#"
struct Camera {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    eye_pos: vec4<f32>,
    light_dir: vec4<f32>,
    depth_params: vec4<f32>,
    clip_plane: vec4<f32>,
    render_params: vec4<f32>,
};
struct Model {
    model: mat4x4<f32>,
    color: vec4<f32>,
    pick_id: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> model: Model;
// Depth of the previously peeled front layer (Depth32Float).
@group(0) @binding(1) var t_prev_layer: texture_depth_2d;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs_main(
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
) -> VsOut {
    let world = model.model * vec4<f32>(pos, 1.0);
    var out: VsOut;
    out.position = camera.view_proj * world;
    out.world_pos = world.xyz;
    out.normal = (model.model * vec4<f32>(normal, 0.0)).xyz;
    return out;
}

// Two layers whose NDC z differs by less than this merge into one.
// Rasterization is deterministic (same vertices, same matrices), so
// same-fragment z values round identically; the epsilon only guards
// f32 noise across distinct, nearly coplanar triangles.
const LAYER_EPS: f32 = 1e-5;

// Section view (W-02): is this fragment clipped away?
fn section_clipped(world_pos: vec3<f32>) -> bool {
    if (camera.render_params.x > 0.5) {
        return dot(camera.clip_plane.xyz, world_pos) - camera.clip_plane.w < 0.0;
    }
    return false;
}

// Same shading as the mesh pass (single output; the pipeline's
// under-blend state expects a premultiplied color).
fn shade(in: VsOut) -> vec3<f32> {
    let n = normalize(in.normal);
    let base = model.color.rgb;

    var lit: vec3<f32>;
    if (camera.render_params.y > 0.5) {
        // Wireframe base (W-05): flat unlit surfaces.
        lit = base * 0.35;
    } else {
        let l = normalize(-camera.light_dir.xyz);
        let v = normalize(camera.eye_pos.xyz - in.world_pos);
        let diff = max(dot(n, l), 0.0);
        let h = normalize(l + v);
        let spec = pow(max(dot(n, h), 0.0), 48.0) * 0.25;
        let sky = vec3<f32>(0.42, 0.46, 0.52);
        let ground = vec3<f32>(0.18, 0.18, 0.16);
        let ambient = mix(ground, sky, n.z * 0.5 + 0.5) * 0.35;
        lit = base * (diff * 0.85 + ambient) + vec3<f32>(spec);
        let dist = length(camera.eye_pos.xyz - in.world_pos);
        let fog = clamp(dist / camera.depth_params.y, 0.0, 1.0) * 0.25;
        lit = mix(lit, vec3<f32>(0.55, 0.58, 0.62), fog);
    }
    return lit;
}

fn layer_alpha() -> f32 {
    var alpha = model.color.a;
    // X-ray (W-05): force a translucent alpha on every body.
    if (camera.render_params.z > 0.0) {
        alpha = camera.render_params.z;
    }
    return alpha;
}

@fragment
fn fs_peel(in: VsOut) {
    if (section_clipped(in.world_pos)) {
        discard;
    }
    let z = in.position.z;
    let prev = textureLoad(t_prev_layer, vec2<i32>(in.position.xy), 0);
    if (z <= prev + LAYER_EPS) {
        // This layer or nearer — not a survivor.
        discard;
    }
    // Survivors leave their depth; the Less depth test on the
    // cleared-to-1.0 attachment keeps the minimum → next layer.
}

@fragment
fn fs_blend(in: VsOut) -> @location(0) vec4<f32> {
    if (section_clipped(in.world_pos)) {
        discard;
    }
    let z = in.position.z;
    let front = textureLoad(t_prev_layer, vec2<i32>(in.position.xy), 0);
    if (z > front + LAYER_EPS) {
        // Behind the front-most survivor — a later layer.
        discard;
    }
    let lit = shade(in);
    let alpha = layer_alpha();
    return vec4<f32>(lit * alpha, alpha);
}

@fragment
fn fs_blend_residual(in: VsOut) -> @location(0) vec4<f32> {
    if (section_clipped(in.world_pos)) {
        discard;
    }
    let z = in.position.z;
    let last = textureLoad(t_prev_layer, vec2<i32>(in.position.xy), 0);
    if (z <= last + LAYER_EPS) {
        // Already peeled — blended by a layer pass.
        discard;
    }
    let lit = shade(in);
    let alpha = layer_alpha();
    return vec4<f32>(lit * alpha, alpha);
}
"#;

/// Composite pass: fullscreen triangle reading the HDR color, view normal
/// and depth buffers; applies screen-space edge detection (Sobel over
/// depth + normal gradients – the "screen-space derivative" technique for
/// silhouette/hidden-line rendering, FR-RD-03), tone mapping and the
/// background.
///
/// `t_accum` (W-03) carries the front-to-back under-blended transparency
/// accumulation (premultiplied); it composites under the tone-mapped
/// scene: `accum.rgb + (1 - accum.a) * scene`.
pub const COMPOSITE_SHADER: &str = r#"
struct Camera {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    eye_pos: vec4<f32>,
    light_dir: vec4<f32>,
    depth_params: vec4<f32>,
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var t_color: texture_2d<f32>;
@group(0) @binding(2) var t_normal: texture_2d<f32>;
@group(0) @binding(3) var t_depth: texture_2d<f32>;
// W-03: premultiplied front-to-back transparency accumulation.
@group(0) @binding(5) var t_accum: texture_2d<f32>;

struct Uniforms {
    // x: edge strength, y: line darkness, z: wireframe/hidden-line mode
    // (W-05), w: spare.
    params: vec4<f32>,
    background: vec4<f32>,
    line_color: vec4<f32>,
};
@group(0) @binding(4) var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    // Fullscreen triangle.
    var p = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -3.0),
        vec2<f32>(3.0, 1.0),
        vec2<f32>(-1.0, 1.0),
    );
    return vec4<f32>(p[vi], 0.0, 1.0);
}

fn sample_depth(uv: vec2<i32>) -> f32 {
    // textureLoad yields vec4<f32>; the depth value lives in .r.
    return textureLoad(t_depth, uv, 0).r;
}

fn sample_normal(uv: vec2<i32>) -> vec3<f32> {
    return textureLoad(t_normal, uv, 0).xyz * 2.0 - 1.0;
}

fn linearize_depth(d: f32, near: f32, far: f32) -> f32 {
    let ndc = d * 2.0 - 1.0;
    return (2.0 * near * far) / (far + near - ndc * (far - near));
}

@fragment
fn fs_main(@builtin(position) frag_px: vec4<f32>) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(vec2<i32>(textureDimensions(t_depth)));
    let px = vec2<i32>(frag_px.xy);
    let uv = px;

    let depth = sample_depth(uv);
    let has_geometry = depth < 0.999999;

    // Background with a subtle vertical gradient.
    let bg = uniforms.background.rgb;
    let t = frag_px.y / dims.y;
    let bg_grad = mix(bg * 1.06, bg * 0.94, t);

    // Wireframe / hidden-line display mode (W-05): background everywhere,
    // line color on detected edges — a technical-drawing look. Feature
    // edges from the line pass still overlay on top (drawn before).
    // (Like the pre-peel renderer, the accumulation is ignored here.)
    if (uniforms.params.z > 0.5) {
        let line_rgb = max(uniforms.line_color.rgb, vec3<f32>(0.02, 0.03, 0.05));
        let edge = wireframe_edge(px);
        let art = mix(bg_grad, line_rgb, edge);
        return vec4<f32>(art, 1.0);
    }

    // Scene color: the shaded geometry, or the background.
    var scene = bg_grad;
    if (has_geometry) {
        scene = textureLoad(t_color, uv, 0).rgb;
    }

    // W-03: composite the transparency accumulation under the scene
    // (premultiplied, front-to-back).
    let accum = textureLoad(t_accum, uv, 0);
    var color = accum.rgb + (1.0 - accum.a) * scene;

    if (!has_geometry) {
        // Background (+ transparents over it); no edges, no tonemap —
        // the gradient is authored in display space.
        return vec4<f32>(color, 1.0);
    }

    // Screen-space edge detection: depth + normal gradients.
    let edge_strength = uniforms.params.x;
    let line_dark = uniforms.params.y;

    // Sobel over the linearized depth.
    let near = camera.depth_params.x;
    let far = camera.depth_params.y;
    let d0 = linearize_depth(depth, near, far);
    let d_right = linearize_depth(sample_depth(uv + vec2<i32>(1, 0)), near, far);
    let d_down = linearize_depth(sample_depth(uv + vec2<i32>(0, 1)), near, far);
    let d_left = linearize_depth(sample_depth(uv - vec2<i32>(1, 0)), near, far);
    let d_up = linearize_depth(sample_depth(uv - vec2<i32>(0, 1)), near, far);
    // Relative depth discontinuities indicate silhouette/creases.
    let gx = (d_right - d0) - (d0 - d_left);
    let gy = (d_down - d0) - (d0 - d_up);
    let rel = max(abs(gx), abs(gy)) / max(d0, 1.0);
    let depth_edge = clamp(rel * 400.0, 0.0, 1.0);

    // Normal gradient (creases on curved surfaces).
    let n0 = sample_normal(uv);
    let n1 = sample_normal(uv + vec2<i32>(1, 0));
    let n2 = sample_normal(uv + vec2<i32>(0, 1));
    let nd = max(1.0 - dot(n0, n1), 1.0 - dot(n0, n2));
    let normal_edge = clamp(nd * 12.0, 0.0, 1.0);

    let edge = clamp(max(depth_edge, normal_edge) * edge_strength, 0.0, 1.0);
    color = mix(color, color * line_dark, edge);

    // Reinhard tone mapping + slight gamma lift.
    color = color / (color + vec3<f32>(1.0));
    color = pow(color, vec3<f32>(1.0 / 1.05));

    return vec4<f32>(color, 1.0);
}

// Edge magnitude for the wireframe mode (same detectors, no line-dark
// mixing — the line color replaces the edge pixels instead).
fn wireframe_edge(px: vec2<i32>) -> f32 {
    let near = camera.depth_params.x;
    let far = camera.depth_params.y;
    let d0 = linearize_depth(sample_depth(px), near, far);
    let d_right = linearize_depth(sample_depth(px + vec2<i32>(1, 0)), near, far);
    let d_down = linearize_depth(sample_depth(px + vec2<i32>(0, 1)), near, far);
    let d_left = linearize_depth(sample_depth(px - vec2<i32>(1, 0)), near, far);
    let d_up = linearize_depth(sample_depth(px - vec2<i32>(0, 1)), near, far);
    let gx = (d_right - d0) - (d0 - d_left);
    let gy = (d_down - d0) - (d0 - d_up);
    let rel = max(abs(gx), abs(gy)) / max(d0, 1.0);
    let depth_edge = clamp(rel * 400.0, 0.0, 1.0);

    let n0 = sample_normal(px);
    let n1 = sample_normal(px + vec2<i32>(1, 0));
    let n2 = sample_normal(px + vec2<i32>(0, 1));
    let nd = max(1.0 - dot(n0, n1), 1.0 - dot(n0, n2));
    let normal_edge = clamp(nd * 12.0, 0.0, 1.0);

    let edge = clamp(max(depth_edge, normal_edge) * uniforms.params.x, 0.0, 1.0);
    return edge;
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse + validate a WGSL source with naga — the same validator
    /// wgpu runs at pipeline creation, executed headless so every
    /// shader regression fails in CI on all three platforms without
    /// needing a GPU.
    fn validate(src: &str) {
        let module = wgpu::naga::front::wgsl::parse_str(src)
            .unwrap_or_else(|e| panic!("wgsl parse error: {e}"));
        wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("naga validation error:\n{}", e.emit_to_string(src)));
    }

    #[test]
    fn mesh_shader_validates() {
        validate(MESH_SHADER);
    }

    #[test]
    fn line_shader_validates() {
        validate(LINE_SHADER);
    }

    #[test]
    fn pick_shader_validates() {
        validate(PICK_SHADER);
    }

    #[test]
    fn peel_shader_validates() {
        validate(PEEL_SHADER);
    }

    #[test]
    fn composite_shader_validates() {
        validate(COMPOSITE_SHADER);
    }

    /// Every peel entry point must exist by name (the renderer binds
    /// them by string at pipeline creation).
    #[test]
    fn peel_shader_entry_points_exist() {
        let module = wgpu::naga::front::wgsl::parse_str(PEEL_SHADER).expect("wgsl parse");
        let names: Vec<String> = module.entry_points.iter().map(|e| e.name.clone()).collect();
        for want in ["vs_main", "fs_peel", "fs_blend", "fs_blend_residual"] {
            assert!(
                names.contains(&want.to_string()),
                "missing entry point {want}"
            );
        }
    }
}
