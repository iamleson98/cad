//! The multi-pass WebGPU renderer (FR-RD-01 … FR-RD-04).
//!
//! Pass structure per frame (all but the last run offscreen):
//!
//! 1. **Opaque pass** – shaded meshes (MRT: HDR color + view normal) with
//!    depth writing.
//! 2. **Edge line pass** – sharp-feature edges + ground grid, depth-tested
//!    with a slope-scaled bias.
//! 3. **Transparent pass** – W-03: order-independent front depth
//!    peeling (`RenderOptions::peel_layers` layers + one residual
//!    pass, under-blended front-to-back). No CPU sorting; correct for
//!    interpenetrating geometry.
//! 4. **Pick pass** (on demand) – body ids as `Rgba32Uint` + depth, 1×1
//!    readback at the clicked pixel.
//! 5. **Composite pass** – fullscreen triangle sampling color/normal/depth,
//!    screen-space edge detection (silhouettes + hidden lines), tone
//!    mapping, background. Recorded by the caller into the UI render pass.

use crate::camera::{Camera, CameraUniform};
use crate::cull::Frustum;
use crate::scene::{BodyStyle, Scene};
use crate::shaders;
use forge_core::BodyId;
use std::collections::BTreeMap;
use wgpu::util::DeviceExt;

/// Viewport display mode (W-05).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayMode {
    /// Shaded PBR-ish surfaces (default). Edge overlay via `show_edges`.
    #[default]
    Shaded,
    /// Hidden-line wireframe: flat ghost surfaces + screen-space line
    /// art + all feature edges.
    Wireframe,
    /// X-ray: every body translucent (alpha override) — the Fusion/
    /// Onshape "ghost" mode for inspecting internals.
    XRay,
}

/// Section-view clipping plane (W-02).
///
/// Fragments where `dot(normal, p) - offset < 0` are discarded in every
/// pass (shaded, transparent, lines, picking), exposing a cut-away view
/// of the interior.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionPlane {
    /// Unit plane normal.
    pub normal: [f64; 3],
    /// Plane offset `d` (mm): the plane is `{p : dot(n, p) = d}`.
    pub offset: f64,
}

impl SectionPlane {
    /// GPU-ready `(xyz, w)` packing.
    fn pack(&self) -> [f32; 4] {
        [
            self.normal[0] as f32,
            self.normal[1] as f32,
            self.normal[2] as f32,
            self.offset as f32,
        ]
    }
}

/// Display options for one frame.
#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    /// Edge overlay strength (0 = off, 1 = full).
    pub edge_strength: f32,
    /// Edge line darkness multiplier.
    pub line_darkness: f32,
    /// Draw the ground grid.
    pub show_grid: bool,
    /// Draw feature edges.
    pub show_edges: bool,
    /// Display mode (W-05).
    pub display_mode: DisplayMode,
    /// Section-view clip plane; `None` = off (W-02).
    pub section: Option<SectionPlane>,
    /// X-ray alpha when `display_mode == XRay`.
    pub xray_alpha: f32,
    /// W-03: front depth-peel iterations before the residual pass.
    /// More layers = more exactly-sorted transparency; the residual
    /// pass blends everything beyond the last layer in one go.
    pub peel_layers: u32,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            edge_strength: 1.0,
            line_darkness: 0.15,
            show_grid: true,
            show_edges: true,
            display_mode: DisplayMode::default(),
            section: None,
            xray_alpha: 0.35,
            peel_layers: 8,
        }
    }
}

/// GPU-side representation of one body.
struct GpuBody {
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    index_count: u32,
    model_bg: wgpu::BindGroup,
    line_buf: wgpu::Buffer,
    line_count: u32,
    line_bg: wgpu::BindGroup,
    style: BodyStyle,
    /// World-space AABB for frustum culling (K-05).
    aabb_min: [f64; 3],
    aabb_max: [f64; 3],
    pick_id: u32,
}

/// Per-frame composite uniforms.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CompositeUniforms {
    params: [f32; 4],
    background: [f32; 4],
    line_color: [f32; 4],
}

/// Per-body model uniform.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ModelUniform {
    model: [[f32; 4]; 4],
    color: [f32; 4],
    pick_id: [f32; 4],
}

/// Mesh vertex (f32 GPU representation, NFR-PREC-01 translation point).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct MeshVertex {
    pos: [f32; 3],
    normal: [f32; 3],
}

/// Line vertex (world-space, colored per body via the model bind group).
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LineVertex {
    pos: [f32; 3],
    _pad: f32,
}

/// Result of an asynchronous GPU pick.
#[derive(Debug, Clone, Copy)]
pub struct PickResult {
    /// The picked body id (raw value), or `None` for empty space.
    pub body: Option<u64>,
}

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const NORMAL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// W-10: the readable depth copy (R32Float color target written by
/// the opaque mesh pass; WebGL2 cannot textureLoad depth textures).
const DEPTH_COPY_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// W-03/W-10: peel-layer depth bounds live in the R channel of an
/// R32Float **color** target. WebGL2/GLSL cannot `textureLoad` from
/// depth textures, and 32-bit float *blending* needs an extension —
/// instead the peel pass relies on its depth test (nearest survivor
/// writes last) and reads the bounds as a plain float texture.
const PEEL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
const PICK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Uint;

/// Multi-pass renderer bound to a device/queue and an output format.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The UI surface format the composite pass writes into.
    target_format: wgpu::TextureFormat,

    model_bgl: wgpu::BindGroupLayout,
    camera_buf: wgpu::Buffer,
    camera_bg: wgpu::BindGroup,

    mesh_pipeline: wgpu::RenderPipeline,
    /// W-03: depth-only peel pass (finds the next front layer).
    peel_pipeline: wgpu::RenderPipeline,
    /// W-03: blend one peeled layer (under-blending).
    peel_blend_pipeline: wgpu::RenderPipeline,
    /// W-03: blend all residual layers behind the last peeled one.
    peel_residual_pipeline: wgpu::RenderPipeline,
    /// W-03: camera uniform + previous-layer depth texture.
    peel_bgl: wgpu::BindGroupLayout,
    /// W-03: bind groups for reading peel layer A / B.
    peel_bg: [Option<wgpu::BindGroup>; 2],
    line_pipeline: wgpu::RenderPipeline,
    /// Unclipped line pipeline (ground grid — never cut by the section
    /// plane).
    grid_pipeline: wgpu::RenderPipeline,
    pick_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    composite_bgl: wgpu::BindGroupLayout,
    composite_uniform_buf: wgpu::Buffer,
    composite_bg: Option<wgpu::BindGroup>,

    bodies: Vec<GpuBody>,
    scene_version: u64,
    grid_buf: wgpu::Buffer,
    grid_bg: wgpu::BindGroup,
    grid_count: u32,

    // Offscreen targets.
    target_size: (u32, u32),
    color_tex: Option<wgpu::Texture>,
    normal_tex: Option<wgpu::Texture>,
    /// W-10: readable NDC depth copy (R32Float color target).
    depth_copy_tex: Option<wgpu::Texture>,
    depth_tex: Option<wgpu::Texture>,
    pick_tex: Option<wgpu::Texture>,
    pick_depth_tex: Option<wgpu::Texture>,
    /// W-03: ping-pong front-layer depth bounds (R32Float color).
    peel_tex: [Option<wgpu::Texture>; 2],
    /// W-03/W-10: scratch depth attachment for the peel pass (the Less
    /// test makes the nearest survivor the last color writer).
    peel_depth: Option<wgpu::Texture>,
    /// W-03: front-to-back under-blended transparency accumulation.
    accum_tex: Option<wgpu::Texture>,

    // Picking state.
    pending_pick: Option<(u32, u32)>,
    pick_readback: Option<PickReadback>,
    pick_result: Option<PickResult>,
}

struct PickReadback {
    buf: wgpu::Buffer,
    done: std::sync::Arc<std::sync::atomic::AtomicBool>,
    data: std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>>,
}

impl Renderer {
    /// Create the renderer from eframe-provided wgpu state.
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let camera_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let model_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("model-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let camera_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera-buffer"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera-bind-group"),
            layout: &camera_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buf.as_entire_binding(),
            }],
        });

        let mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh-shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::MESH_SHADER.into()),
        });
        let line_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("line-shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::LINE_SHADER.into()),
        });
        let pick_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pick-shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::PICK_SHADER.into()),
        });
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite-shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::COMPOSITE_SHADER.into()),
        });

        let mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh-pipeline-layout"),
            bind_group_layouts: &[Some(&camera_bgl), Some(&model_bgl)],
            immediate_size: 0,
        });

        let vertex_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 12,
                    shader_location: 1,
                },
            ],
        })];

        let depth_stencil_write = wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState {
                constant: 0,
                slope_scale: 0.0,
                clamp: 0.0,
            },
        };

        let mesh_targets = vec![
            Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            }),
            Some(wgpu::ColorTargetState {
                format: NORMAL_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            }),
            // W-10: NDC depth as an R32Float color target — the
            // composite pass reads depth via textureLoad, which
            // returns zeros for depth textures on WebGL2.
            Some(wgpu::ColorTargetState {
                format: DEPTH_COPY_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            }),
        ];

        let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mesh-pipeline"),
            layout: Some(&mesh_layout),
            vertex: wgpu::VertexState {
                module: &mesh_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &vertex_buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: &mesh_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &mesh_targets,
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(depth_stencil_write.clone()),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // W-03: depth-peeling pipelines. The peel pass is depth-only
        // (no color targets, Less test + write on the ping-pong Depth32
        // bounds); the blend passes shade one layer into the
        // accumulation texture with premultiplied under-blending
        // (front-to-back — nearer fragments attenuate farther ones).
        let peel_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("peel-shader"),
            source: wgpu::ShaderSource::Wgsl(shaders::PEEL_SHADER.into()),
        });
        let peel_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("peel-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        // R32Float peel bounds (W-10): a plain float
                        // sample, NOT a depth texture — WebGL2 cannot
                        // load depth textures, so peeling writes the
                        // bounds as color.
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let peel_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("peel-pipeline-layout"),
            bind_group_layouts: &[Some(&peel_bgl), Some(&model_bgl)],
            immediate_size: 0,
        });
        let under_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let make_peel_pipeline = |label: &str,
                                  entry: &str,
                                  targets: &[Option<wgpu::ColorTargetState>],
                                  depth: wgpu::DepthStencilState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&peel_layout),
                vertex: wgpu::VertexState {
                    module: &peel_shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &vertex_buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &peel_shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets,
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(depth),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let peel_pipeline = make_peel_pipeline(
            "peel-pipeline",
            "fs_peel",
            &[Some(wgpu::ColorTargetState {
                format: PEEL_FORMAT,
                // No blending: the Less depth test on the scratch
                // attachment makes the nearest survivor the last
                // writer, so color.r = min survivor z.
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                // Scratch depth (cleared to 1.0 per pass): the Less
                // test min-accumulates survivor z through the color
                // write — depth attachments are unreadable on WebGL2
                // (W-10).
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 0,
                    slope_scale: 0.0,
                    clamp: 0.0,
                },
            },
        );
        let peel_blend_pipeline = make_peel_pipeline(
            "peel-blend-pipeline",
            "fs_blend",
            &[Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend: Some(under_blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                // Blend passes depth-test against the OPAQUE depth only.
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 0,
                    slope_scale: 0.0,
                    clamp: 0.0,
                },
            },
        );
        let peel_residual_pipeline = make_peel_pipeline(
            "peel-residual-pipeline",
            "fs_blend_residual",
            &[Some(wgpu::ColorTargetState {
                format: COLOR_FORMAT,
                blend: Some(under_blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 0,
                    slope_scale: 0.0,
                    clamp: 0.0,
                },
            },
        );

        let line_buffers = [Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            }],
        })];
        let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("line-pipeline-layout"),
            bind_group_layouts: &[Some(&camera_bgl), Some(&model_bgl)],
            immediate_size: 0,
        });
        // Body feature edges: section-clipped entry points (W-02).
        let line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("line-pipeline"),
            layout: Some(&line_layout),
            vertex: wgpu::VertexState {
                module: &line_shader,
                entry_point: Some("vs_main_clip"),
                compilation_options: Default::default(),
                buffers: &line_buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: &line_shader,
                entry_point: Some("fs_main_clip"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                // No depth bias: WebGL2 rejects bias on LineList —
                // the LINE_SHADER nudges clip-space z instead (W-10).
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        // Ground grid: the plain (unclipped) entry points — the reference
        // grid must survive every section cut.
        let grid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("grid-pipeline"),
            layout: Some(&line_layout),
            vertex: wgpu::VertexState {
                module: &line_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &line_buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: &line_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // Pick pipeline (integer target, own depth texture).
        let pick_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pick-pipeline"),
            layout: Some(&mesh_layout),
            vertex: wgpu::VertexState {
                module: &pick_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &vertex_buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: &pick_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: PICK_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(depth_stencil_write.clone()),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // Composite pipeline: camera + 3 textures + uniforms in group 0.
        let composite_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite-layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // W-03: transparency accumulation (binding 5).
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let composite_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("composite-uniforms"),
            size: std::mem::size_of::<CompositeUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composite-pipeline-layout"),
            bind_group_layouts: &[Some(&composite_bgl)],
            immediate_size: 0,
        });
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("composite-pipeline"),
            layout: Some(&composite_layout),
            vertex: wgpu::VertexState {
                module: &composite_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &composite_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // Ground grid (static): 1 m x 1 m cells over 4 m, at z = 0.
        let (grid_data, grid_count) = build_grid(2000.0, 100.0);
        let grid_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grid-buffer"),
            contents: bytemuck::cast_slice(&grid_data),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let grid_model = ModelUniform {
            model: glam::Mat4::IDENTITY.to_cols_array_2d(),
            color: [0.55, 0.57, 0.62, 0.30],
            pick_id: [0.0; 4],
        };
        let grid_uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("grid-model"),
            contents: bytemuck::bytes_of(&grid_model),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let grid_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grid-bind-group"),
            layout: &model_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: grid_uniform_buf.as_entire_binding(),
            }],
        });

        Self {
            device,
            queue,
            target_format,
            model_bgl,
            camera_buf,
            camera_bg,
            mesh_pipeline,
            peel_pipeline,
            peel_blend_pipeline,
            peel_residual_pipeline,
            peel_bgl,
            peel_bg: [None, None],
            line_pipeline,
            grid_pipeline,
            pick_pipeline,
            composite_pipeline,
            composite_bgl,
            composite_uniform_buf,
            composite_bg: None,
            bodies: Vec::new(),
            scene_version: u64::MAX,
            grid_buf,
            grid_bg,
            grid_count,
            target_size: (0, 0),
            color_tex: None,
            normal_tex: None,
            depth_copy_tex: None,
            depth_tex: None,
            pick_tex: None,
            pick_depth_tex: None,
            peel_tex: [None, None],
            peel_depth: None,
            accum_tex: None,
            pending_pick: None,
            pick_readback: None,
            pick_result: None,
        }
    }

    /// The output format this renderer composites into.
    pub fn target_format(&self) -> wgpu::TextureFormat {
        self.target_format
    }

    /// Upload the scene (rebuilds GPU buffers when the version changed).
    pub fn update_scene(&mut self, scene: &Scene) {
        if scene.version == self.scene_version {
            return;
        }
        self.scene_version = scene.version;
        self.bodies.clear();

        for body in &scene.bodies {
            let mesh = &body.mesh;
            let mut vertices = Vec::with_capacity(mesh.positions.len());
            let normals = mesh.normals.clone().unwrap_or_else(|| {
                let mut m = mesh.clone();
                m.compute_vertex_normals();
                m.normals.clone().unwrap_or_default()
            });
            for (p, n) in mesh.positions.iter().zip(normals.iter()) {
                vertices.push(MeshVertex {
                    pos: [p.x as f32, p.y as f32, p.z as f32],
                    normal: [n.x as f32, n.y as f32, n.z as f32],
                });
            }
            let vertex_buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("body-vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let index_buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("body-indices"),
                    contents: bytemuck::cast_slice(&mesh.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

            // Feature edge lines.
            let edges = if scene.show_edges {
                mesh.sharp_edges(scene.edge_angle_deg.to_radians())
            } else {
                Vec::new()
            };
            let mut line_verts = Vec::with_capacity(edges.len() * 2);
            for [a, b] in &edges {
                let pa = mesh.positions[*a as usize];
                let pb = mesh.positions[*b as usize];
                line_verts.push(LineVertex {
                    pos: [pa.x as f32, pa.y as f32, pa.z as f32],
                    _pad: 0.0,
                });
                line_verts.push(LineVertex {
                    pos: [pb.x as f32, pb.y as f32, pb.z as f32],
                    _pad: 0.0,
                });
            }
            let line_buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("body-lines"),
                    contents: bytemuck::cast_slice(&line_verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });

            let model = ModelUniform {
                model: glam::Mat4::IDENTITY.to_cols_array_2d(),
                color: body.style.color,
                pick_id: [body.id.raw() as f32, 0.0, 0.0, 0.0],
            };
            let model_buf = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("body-model"),
                    contents: bytemuck::bytes_of(&model),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                });
            let model_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("body-model-bg"),
                layout: &self.model_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: model_buf.as_entire_binding(),
                }],
            });

            // Line bind group: same model matrix, dark line color.
            let line_model = ModelUniform {
                model: glam::Mat4::IDENTITY.to_cols_array_2d(),
                color: [0.06, 0.07, 0.09, 0.9],
                pick_id: [0.0; 4],
            };
            let line_buf_uniform =
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("line-model"),
                        contents: bytemuck::bytes_of(&line_model),
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    });
            let line_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("line-model-bg"),
                layout: &self.model_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: line_buf_uniform.as_entire_binding(),
                }],
            });

            // World-space AABB for the culling test (K-05). Degenerate
            // boxes (empty mesh) test as always-visible.
            let bb = mesh.bbox();
            self.bodies.push(GpuBody {
                vertex_buf,
                index_buf,
                index_count: mesh.indices.len() as u32,
                model_bg,
                line_buf,
                line_count: line_verts.len() as u32,
                line_bg,
                style: body.style,
                aabb_min: [bb.min.x, bb.min.y, bb.min.z],
                aabb_max: [bb.max.x, bb.max.y, bb.max.z],
                pick_id: body.id.raw() as u32,
            });
        }
    }

    /// (Re)create offscreen targets for the given size.
    fn ensure_targets(&mut self, size: (u32, u32)) {
        if size == self.target_size && self.color_tex.is_some() {
            return;
        }
        self.target_size = size;
        self.color_tex = None;
        self.normal_tex = None;
        self.depth_copy_tex = None;
        self.depth_tex = None;
        self.pick_tex = None;
        self.pick_depth_tex = None;

        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        self.color_tex = Some(self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("color-target"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage,
            view_formats: &[],
        }));
        self.normal_tex = Some(self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("normal-target"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: NORMAL_FORMAT,
            usage,
            view_formats: &[],
        }));
        self.depth_copy_tex = Some(self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth-copy-target"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_COPY_FORMAT,
            usage,
            view_formats: &[],
        }));
        self.depth_tex = Some(self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth-target"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage,
            view_formats: &[],
        }));
        // Pick target needs COPY_SRC for the readback.
        self.pick_tex = Some(self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pick-target"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: PICK_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        }));
        self.pick_depth_tex = Some(self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pick-depth"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        }));

        // W-03/W-10: ping-pong peel bounds (R32Float color targets —
        // read back as plain float textures, WebGL2-safe) + one scratch
        // depth attachment + the accumulation target.
        self.peel_tex = [None, None];
        self.peel_depth = None;
        self.accum_tex = None;
        let make_peel_tex = |label: &str| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: PEEL_FORMAT,
                usage,
                view_formats: &[],
            })
        };
        self.peel_tex = [
            Some(make_peel_tex("peel-bounds-a")),
            Some(make_peel_tex("peel-bounds-b")),
        ];
        self.peel_depth = Some(self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("peel-scratch-depth"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        }));
        self.accum_tex = Some(self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("transparent-accum"),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage,
            view_formats: &[],
        }));

        // Bind groups reading peel layer A / B (camera uniform + the
        // R32Float bounds as a plain color view).
        let make_peel_bg = |label: &str, tex: &wgpu::Texture| {
            let view = tex.create_view(&wgpu::TextureViewDescriptor {
                label: Some(label),
                format: Some(PEEL_FORMAT),
                dimension: Some(wgpu::TextureViewDimension::D2),
                aspect: wgpu::TextureAspect::All,
                base_mip_level: 0,
                mip_level_count: None,
                base_array_layer: 0,
                array_layer_count: None,
                usage: None,
            });
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.peel_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.camera_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                ],
            })
        };
        self.peel_bg = [
            self.peel_tex[0]
                .as_ref()
                .map(|t| make_peel_bg("peel-bg-a", t)),
            self.peel_tex[1]
                .as_ref()
                .map(|t| make_peel_bg("peel-bg-b", t)),
        ];
    }

    /// Record the offscreen passes (opaque, lines, transparent, pick).
    /// Called from `CallbackTrait::prepare`.
    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        size: (u32, u32),
        camera: &Camera,
        options: &RenderOptions,
    ) {
        if size.0 == 0 || size.1 == 0 {
            return;
        }
        self.ensure_targets(size);
        let aspect = size.0 as f64 / size.1 as f64;

        // K-05: per-body AABB frustum culling, computed once per frame and
        // applied to every body pass (opaque, lines, transparency, pick).
        let frustum = Frustum::from_view_proj(camera.view_proj(aspect));
        let visible = |b: &GpuBody| frustum.intersects_aabb(&b.aabb_min, &b.aabb_max);

        // Camera uniform.
        let mut cam_uniform = camera.uniform(aspect, (size.0 as f64, size.1 as f64));
        // Section-view clip plane + display mode flags (W-02/W-05).
        if let Some(section) = &options.section {
            cam_uniform.clip_plane = section.pack();
            cam_uniform.render_params[0] = 1.0;
        }
        match options.display_mode {
            DisplayMode::Shaded => {}
            DisplayMode::Wireframe => cam_uniform.render_params[1] = 1.0,
            DisplayMode::XRay => cam_uniform.render_params[2] = options.xray_alpha.clamp(0.05, 1.0),
        }
        self.queue
            .write_buffer(&self.camera_buf, 0, bytemuck::bytes_of(&cam_uniform));

        // Composite uniforms. params.z drives the hidden-line wireframe
        // look (W-05); edge strength is boosted for crisper line art.
        let wireframe = options.display_mode == DisplayMode::Wireframe;
        let composite = CompositeUniforms {
            params: [
                if wireframe {
                    options.edge_strength * 1.5
                } else {
                    options.edge_strength
                },
                options.line_darkness,
                if wireframe { 1.0 } else { 0.0 },
                0.0,
            ],
            background: [0.118, 0.121, 0.133, 1.0],
            line_color: [0.06, 0.07, 0.09, 1.0],
        };
        self.queue.write_buffer(
            &self.composite_uniform_buf,
            0,
            bytemuck::bytes_of(&composite),
        );

        let color_view = self
            .color_tex
            .as_ref()
            .expect("targets ensured")
            .create_view(&wgpu::TextureViewDescriptor::default());
        let normal_view = self
            .normal_tex
            .as_ref()
            .expect("targets ensured")
            .create_view(&wgpu::TextureViewDescriptor::default());
        let depth_view = self
            .depth_tex
            .as_ref()
            .expect("targets ensured")
            .create_view(&wgpu::TextureViewDescriptor::default());
        let depth_copy_view = self
            .depth_copy_tex
            .as_ref()
            .expect("targets ensured")
            .create_view(&wgpu::TextureViewDescriptor::default());

        // --- Pass 1: opaque meshes (color + normal + depth-copy MRT). ---
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("opaque-pass"),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: &color_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &normal_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.5,
                                g: 0.5,
                                b: 1.0,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    }),
                    // W-10: readable depth copy — cleared to 1.0 ("far").
                    Some(wgpu::RenderPassColorAttachment {
                        view: &depth_copy_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 1.0,
                                g: 0.0,
                                b: 0.0,
                                a: 0.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None, // Depth32Float has no stencil aspect (WebGL2-strict)
                }),
                multiview_mask: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rpass.set_pipeline(&self.mesh_pipeline);
            rpass.set_bind_group(0, &self.camera_bg, &[]);
            let xray = options.display_mode == DisplayMode::XRay;
            for body in &self.bodies {
                if xray || body.style.transparent || body.style.color[3] < 1.0 {
                    continue;
                }
                if !visible(body) {
                    continue; // K-05: entirely off-screen
                }
                rpass.set_bind_group(1, &body.model_bg, &[]);
                rpass.set_index_buffer(body.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                rpass.set_vertex_buffer(0, body.vertex_buf.slice(..));
                rpass.draw_indexed(0..body.index_count, 0, 0..1);
            }
        }

        // --- Pass 2: feature edge lines + grid. ---
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("line-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None, // Depth32Float has no stencil aspect (WebGL2-strict)
                }),
                multiview_mask: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rpass.set_pipeline(&self.line_pipeline);
            rpass.set_bind_group(0, &self.camera_bg, &[]);
            if options.show_edges {
                for body in &self.bodies {
                    if body.line_count == 0 {
                        continue;
                    }
                    if !visible(body) {
                        continue; // K-05: entirely off-screen
                    }
                    rpass.set_bind_group(1, &body.line_bg, &[]);
                    rpass.set_vertex_buffer(0, body.line_buf.slice(..));
                    rpass.draw(0..body.line_count, 0..1);
                }
            }
            if options.show_grid {
                rpass.set_pipeline(&self.grid_pipeline);
                rpass.set_bind_group(1, &self.grid_bg, &[]);
                rpass.set_vertex_buffer(0, self.grid_buf.slice(..));
                rpass.draw(0..self.grid_count, 0..1);
            }
        }

        // --- Pass 3 (W-03): order-independent front depth peeling. ---
        {
            let xray = options.display_mode == DisplayMode::XRay;
            let transparent: Vec<&GpuBody> = self
                .bodies
                .iter()
                .filter(|b| (xray || b.style.transparent || b.style.color[3] < 1.0) && visible(b))
                .collect();

            let (Some(accum_tex), Some(peel_depth), [Some(peel_a), Some(peel_b)]) = (
                self.accum_tex.as_ref(),
                self.peel_depth.as_ref(),
                [&self.peel_tex[0], &self.peel_tex[1]].map(|t| t.as_ref()),
            ) else {
                // Targets not created (degenerate size): no transparency.
                if !transparent.is_empty() {
                    log::warn!("peel targets missing; transparent bodies skipped");
                }
                return;
            };
            let accum_view = accum_tex.create_view(&wgpu::TextureViewDescriptor::default());
            let peel_view_a = peel_a.create_view(&wgpu::TextureViewDescriptor::default());
            let peel_view_b = peel_b.create_view(&wgpu::TextureViewDescriptor::default());
            let peel_depth_view = peel_depth.create_view(&wgpu::TextureViewDescriptor::default());

            // Seed: clear the accumulation and the layer-0 bounds to
            // 0.0 ("nothing peeled yet" — survivors must be strictly
            // behind). A clear-only pass, no draws.
            {
                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("peel-clear"),
                    color_attachments: &[
                        Some(wgpu::RenderPassColorAttachment {
                            view: &accum_view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                            depth_slice: None,
                        }),
                        Some(wgpu::RenderPassColorAttachment {
                            view: &peel_view_a,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                            depth_slice: None,
                        }),
                    ],
                    depth_stencil_attachment: None,
                    multiview_mask: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                // No pipeline, no draws — this pass only clears.
                let _ = &mut rpass;
            }

            if !transparent.is_empty() {
                let layers = options.peel_layers.max(1) as usize;
                for i in 1..=layers {
                    // Ping-pong: iteration i reads bounds[i-1] (bind
                    // group, R32Float color), writes bounds[i] as
                    // color. The scratch depth attachment (cleared to
                    // 1.0 = far) carries the Less test that
                    // min-accumulates survivors into the color target.
                    let (write_view, read_bg, write_bg) = if (i - 1) % 2 == 0 {
                        (
                            &peel_view_b,
                            self.peel_bg[0].as_ref().expect("peel bind groups"),
                            self.peel_bg[1].as_ref().expect("peel bind groups"),
                        )
                    } else {
                        (
                            &peel_view_a,
                            self.peel_bg[1].as_ref().expect("peel bind groups"),
                            self.peel_bg[0].as_ref().expect("peel bind groups"),
                        )
                    };

                    // (a) Peel: find the next front layer. Survivors
                    // write their NDC z into the R32Float color
                    // target; the Less-tested scratch depth makes the
                    // nearest survivor the last writer per pixel.
                    {
                        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("peel-pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: write_view,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                    store: wgpu::StoreOp::Store,
                                },
                                depth_slice: None,
                            })],
                            depth_stencil_attachment: Some(
                                wgpu::RenderPassDepthStencilAttachment {
                                    view: &peel_depth_view,
                                    depth_ops: Some(wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(1.0),
                                        store: wgpu::StoreOp::Store,
                                    }),
                                    stencil_ops: None, // Depth32Float has no stencil aspect (WebGL2-strict)
                                },
                            ),
                            multiview_mask: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        rpass.set_pipeline(&self.peel_pipeline);
                        rpass.set_bind_group(0, read_bg, &[]);
                        for body in &transparent {
                            rpass.set_bind_group(1, &body.model_bg, &[]);
                            rpass.set_index_buffer(
                                body.index_buf.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            rpass.set_vertex_buffer(0, body.vertex_buf.slice(..));
                            rpass.draw_indexed(0..body.index_count, 0, 0..1);
                        }
                    }

                    // (b) Blend the new front layer (premultiplied
                    // under-blending into the accumulation). The
                    // fragment reads the just-written layer depth via
                    // the WRITE side's bind group.
                    {
                        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("peel-blend-pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &accum_view,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Load,
                                    store: wgpu::StoreOp::Store,
                                },
                                depth_slice: None,
                            })],
                            depth_stencil_attachment: Some(
                                wgpu::RenderPassDepthStencilAttachment {
                                    view: &depth_view,
                                    depth_ops: Some(wgpu::Operations {
                                        load: wgpu::LoadOp::Load,
                                        store: wgpu::StoreOp::Store,
                                    }),
                                    stencil_ops: None, // Depth32Float has no stencil aspect (WebGL2-strict)
                                },
                            ),
                            multiview_mask: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });
                        rpass.set_pipeline(&self.peel_blend_pipeline);
                        rpass.set_bind_group(0, write_bg, &[]);
                        for body in &transparent {
                            rpass.set_bind_group(1, &body.model_bg, &[]);
                            rpass.set_index_buffer(
                                body.index_buf.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            rpass.set_vertex_buffer(0, body.vertex_buf.slice(..));
                            rpass.draw_indexed(0..body.index_count, 0, 0..1);
                        }
                    }
                }

                // (c) Residual: everything behind the last peeled layer
                // blends in one unsorted pass. The loop's last write
                // went to side (layers % 2).
                let last_bg = self.peel_bg[layers % 2].as_ref().expect("peel bind groups");
                {
                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("peel-residual-pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &accum_view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            },
                            depth_slice: None,
                        })],
                        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                            view: &depth_view,
                            depth_ops: Some(wgpu::Operations {
                                load: wgpu::LoadOp::Load,
                                store: wgpu::StoreOp::Store,
                            }),
                            stencil_ops: None, // Depth32Float has no stencil aspect (WebGL2-strict)
                        }),
                        multiview_mask: None,
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    rpass.set_pipeline(&self.peel_residual_pipeline);
                    rpass.set_bind_group(0, last_bg, &[]);
                    for body in &transparent {
                        rpass.set_bind_group(1, &body.model_bg, &[]);
                        rpass.set_index_buffer(body.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                        rpass.set_vertex_buffer(0, body.vertex_buf.slice(..));
                        rpass.draw_indexed(0..body.index_count, 0, 0..1);
                    }
                }
            }
        }

        // --- Pass 4 (on demand): picking. ---
        if let Some((px, py)) = self.pending_pick.take() {
            self.record_pick_pass(encoder, size, camera, px, py);
        }

        // Composite bind group needs the current textures.
        let accum_view = self
            .accum_tex
            .as_ref()
            .expect("targets ensured")
            .create_view(&wgpu::TextureViewDescriptor::default());
        let composite_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite-bind-group"),
            layout: &self.composite_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.camera_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&color_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&normal_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    // W-10: the R32Float depth COPY — textureLoad on a
                    // real depth texture returns zeros on WebGL2.
                    resource: wgpu::BindingResource::TextureView(&depth_copy_view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: self.composite_uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&accum_view),
                },
            ],
        });
        self.composite_bg = Some(composite_bg);
    }

    /// Record the composite draw into the UI render pass. Called from
    /// `CallbackTrait::paint` with the scissor already set by the caller.
    pub fn composite(&mut self, render_pass: &mut wgpu::RenderPass<'_>) {
        let Some(bg) = self.composite_bg.take() else {
            return;
        };
        render_pass.set_pipeline(&self.composite_pipeline);
        render_pass.set_bind_group(0, &bg, &[]);
        render_pass.draw(0..3, 0..1);
    }

    fn record_pick_pass(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        size: (u32, u32),
        camera: &Camera,
        px: u32,
        py: u32,
    ) {
        // Finish any previous readback first.
        self.poll_pick();

        let pick_view = match self.pick_tex.as_ref() {
            Some(t) => t.create_view(&wgpu::TextureViewDescriptor::default()),
            None => return,
        };
        let pick_depth = match self.pick_depth_tex.as_ref() {
            Some(t) => t.create_view(&wgpu::TextureViewDescriptor::default()),
            None => return,
        };

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pick-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &pick_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &pick_depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                multiview_mask: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rpass.set_pipeline(&self.pick_pipeline);
            rpass.set_bind_group(0, &self.camera_bg, &[]);
            // Same culling rule as the visible passes: a body that cannot
            // appear on screen cannot be picked at this pixel either.
            let pick_frustum =
                Frustum::from_view_proj(camera.view_proj(size.0 as f64 / size.1 as f64));
            for body in &self.bodies {
                if !pick_frustum.intersects_aabb(&body.aabb_min, &body.aabb_max) {
                    continue;
                }
                rpass.set_bind_group(1, &body.model_bg, &[]);
                rpass.set_index_buffer(body.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                rpass.set_vertex_buffer(0, body.vertex_buf.slice(..));
                rpass.draw_indexed(0..body.index_count, 0, 0..1);
            }
        }

        // Copy the single clicked pixel (16 bytes for Rgba32Uint).
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pick-readback"),
            size: 16,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let clamped_x = px.min(size.0.saturating_sub(1));
        let clamped_y = py.min(size.1.saturating_sub(1));
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: self.pick_tex.as_ref().expect("pick target exists"),
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: clamped_x,
                    y: clamped_y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(16),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let _ = camera;

        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let data: std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        {
            // wgpu::Buffer is a ref-counted handle: clone it for the
            // callback so the original can be stored in PickReadback.
            let cb_buf = buf.clone();
            let call_buf = buf.clone();
            let done = done.clone();
            let data = data.clone();
            call_buf
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |_result| {
                    if let Ok(mapping) = cb_buf.slice(..).get_mapped_range() {
                        // Copy the mapped bytes out.
                        *data.lock().unwrap() = Some(mapping.to_vec());
                    }
                    done.store(true, std::sync::atomic::Ordering::Release);
                });
        }
        self.pick_readback = Some(PickReadback { buf, done, data });
    }

    /// Request a pick at the given pixel (executed on the next render).
    pub fn schedule_pick(&mut self, pixel: (u32, u32)) {
        self.pending_pick = Some(pixel);
    }

    /// Poll the pending pick readback; returns the result when ready.
    pub fn poll_pick(&mut self) -> Option<PickResult> {
        if let Some(readback) = self.pick_readback.take() {
            // Non-blocking poll to drive the callback.
            let _ = self.device.poll(wgpu::PollType::Poll);
            if readback.done.load(std::sync::atomic::Ordering::Acquire) {
                let data = readback.data.lock().unwrap().take();
                readback.buf.unmap();
                let body = data.and_then(|bytes| {
                    let id = u32::from_le_bytes(bytes[0..4].try_into().expect("4 bytes"));
                    if id == 0 {
                        None
                    } else {
                        Some(id as u64)
                    }
                });
                let result = PickResult { body };
                self.pick_result = Some(result);
            } else {
                // Still in flight: keep waiting.
                self.pick_readback = Some(readback);
            }
        }
        self.pick_result.take()
    }

    /// Draw statistics (for the status bar).
    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }

    /// Total uploaded triangle count.
    pub fn triangle_count(&self) -> u64 {
        self.bodies.iter().map(|b| b.index_count as u64 / 3).sum()
    }

    /// Map from raw pick ids to body ids (for the app layer).
    pub fn pick_id_map(&self) -> BTreeMap<u64, BodyId> {
        self.bodies
            .iter()
            .map(|b| (b.pick_id as u64, BodyId::new(b.pick_id as u64)))
            .collect()
    }
}

fn build_grid(extent: f64, step: f64) -> (Vec<LineVertex>, u32) {
    let mut verts = Vec::new();
    let n = (extent / step) as i64;
    for i in -n..=n {
        let v = i as f32 * step as f32;
        verts.push(LineVertex {
            pos: [-extent as f32, v, 0.0],
            _pad: 0.0,
        });
        verts.push(LineVertex {
            pos: [extent as f32, v, 0.0],
            _pad: 0.0,
        });
        verts.push(LineVertex {
            pos: [v, -extent as f32, 0.0],
            _pad: 0.0,
        });
        verts.push(LineVertex {
            pos: [v, extent as f32, 0.0],
            _pad: 0.0,
        });
    }
    let count = verts.len() as u32;
    (verts, count)
}
