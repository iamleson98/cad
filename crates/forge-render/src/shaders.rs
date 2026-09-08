//! WGSL shader sources for the multi-pass pipeline.

/// Shaded-mesh pass (opaque and transparent variants share the source;
/// pipelines differ in blend/depth state).
///
/// Outputs HDR color to attachment 0 and the view-space normal to
/// attachment 1 (used by the composite edge-detect pass).
pub const MESH_SHADER: &str = r#"
struct Camera {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    eye_pos: vec4<f32>,
    light_dir: vec4<f32>,
    depth_params: vec4<f32>,
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
    let n = normalize(in.normal);
    let l = normalize(-camera.light_dir.xyz);
    let v = normalize(camera.eye_pos.xyz - in.world_pos);

    // Simple PBR-ish shading: Lambert + Blinn-Phong specular + hemisphere
    // ambient. Parameters are fixed per-material-color in v0.1.
    let base = model.color.rgb;
    let diff = max(dot(n, l), 0.0);
    let h = normalize(l + v);
    let spec = pow(max(dot(n, h), 0.0), 48.0) * 0.25;
    let sky = vec3<f32>(0.42, 0.46, 0.52);
    let ground = vec3<f32>(0.18, 0.18, 0.16);
    let ambient = mix(ground, sky, n.z * 0.5 + 0.5) * 0.35;

    var lit = base * (diff * 0.85 + ambient) + vec3<f32>(spec);
    // Light fog towards the far plane for depth perception.
    let dist = length(camera.eye_pos.xyz - in.world_pos);
    let fog = clamp(dist / camera.depth_params.y, 0.0, 1.0) * 0.25;
    lit = mix(lit, vec3<f32>(0.55, 0.58, 0.62), fog);

    var out: FsOut;
    out.color = vec4<f32>(lit, model.color.a);
    out.normal = vec4<f32>(normalize(in.normal) * 0.5 + 0.5, 1.0);
    return out;
}
"#;

/// Edge line pass: world-space lines with per-body color.
pub const LINE_SHADER: &str = r#"
struct Camera {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    eye_pos: vec4<f32>,
    light_dir: vec4<f32>,
    depth_params: vec4<f32>,
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
"#;

/// Picking pass: writes the body id as an integer color (FR-RD-02).
pub const PICK_SHADER: &str = r#"
struct Camera {
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    eye_pos: vec4<f32>,
    light_dir: vec4<f32>,
    depth_params: vec4<f32>,
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
fn vs_main(
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
) -> VsOut {
    var out: VsOut;
    out.position = camera.view_proj * model.model * vec4<f32>(pos, 1.0);
    return out;
}

@fragment
fn fs_main(_in: VsOut) -> @location(0) vec4<u32> {
    let id = u32(model.pick_id.x);
    return vec4<u32>(id, 0u, 0u, 1u);
}
"#;

/// Composite pass: fullscreen triangle reading the HDR color, view normal
/// and depth buffers; applies screen-space edge detection (Sobel over
/// depth + normal gradients – the "screen-space derivative" technique for
/// silhouette/hidden-line rendering, FR-RD-03), tone mapping and the
/// background.
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

struct Uniforms {
    params: vec4<f32>, // edge strength, line darkness, background mode, grid flag
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
    return textureLoad(t_depth, uv, 0);
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

    if (!has_geometry) {
        // Background.
        let bg = uniforms.background.rgb;
        // Subtle vertical gradient.
        let t = frag_px.y / dims.y;
        let grad = mix(bg * 1.06, bg * 0.94, t);
        return vec4<f32>(grad, 1.0);
    }

    var color = textureLoad(t_color, uv, 0).rgb;

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
"#;
