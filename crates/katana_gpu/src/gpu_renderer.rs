//! Draws a DrawList through a Camera with wgpu (docs/gpu.md; gpu_renderer.hpp).
//!
//! The GPU twin of katana_render's Rasterizer, behind the same seam: it sees a
//! draw list and a camera and nothing of the document, so everything the scene
//! builder decides (what is drawn, in what colour) reaches the screen the same
//! way down both paths. What it does differently is how it draws:
//!
//! * positions relative to a scene origin kept in double (scene_origin);
//! * reversed-Z into a 32-bit float depth buffer, with an infinite far plane
//!   for perspective;
//! * 4x multisampling (the render target's choice; see OffscreenGpu);
//! * lines and points widened into screen-space quads on the GPU, by
//!   instancing (shader_library, Expansion), and antialiased analytically in
//!   the fragment shader;
//! * optionally, lighting per pixel from face normals (LightingMode);
//! * vertical exaggeration as a uniform, so changing it rebuilds nothing.
//!
//! LAYERS. A scene is one or more draw lists drawn in order into one depth
//! buffer, each with its own depth rule - the GPU twin of katana_cad's
//! render_layers, which the 3D view draws its grid, terrain, edges, drawing and
//! selection with (the grid and the edges test depth but do not write it).
//! Every layer is packed against ONE origin, the centre of their joint bounds,
//! so one uniform block serves them all. set_draw_list is the one-layer case.
//!
//! UPLOADS happen only when the scene changes. set_layers packs every layer
//! (gpu_scene) and update_layer one, marking what changed dirty; the next frame
//! uploads only that. Every other frame writes one 160-byte uniform block -
//! the camera - and draws. upload_count() counts the frames that uploaded
//! anything, as the C++'s did: the proof, for a test, that a frame which
//! changed only the camera uploaded nothing.
//!
//! The renderer owns no render target. It is given a device and the formats
//! of the targets it will draw into (initialise), and each frame either a
//! target to clear and draw into (render: OffscreenGpu's texture) or a pass
//! someone else began (prepare, then draw: the window's frame, through an
//! egui-wgpu paint callback). A pass it draws into must have a colour target
//! of `TargetFormat::color` and a depth attachment of `TargetFormat::depth`,
//! both at `TargetFormat::sample_count`.

use crate::gpu_scene::{
    GpuCloudPoint, GpuLine, GpuPoint, GpuSceneData, GpuVertex, PointCloudData,
    pack_draw_list_with_bounds, pack_draw_list_with_origin,
};
use crate::names::to_string;
use crate::plain::bytes_of;
use crate::scene_origin::{
    ClipSpace, ProjectionOptions, choose_scene_origin, relative_view_projection,
    to_float_column_major,
};
use crate::shader_compiler::compiled_program;
use crate::shader_library::{
    ALL_PROGRAMS, DESCRIPTION, Expansion, FRAGMENT_ENTRY, Program, VERTEX_ENTRY,
};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_math::{AABB, Vec3, Vec4, cpp};
use katana_render::{
    Camera, DrawList, Projection, Rgba, alpha_of, blue_of, green_of, red_of, rgba,
};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum LightingMode {
    /// Use the draw list's colours as they are. The scene builder bakes flat
    /// lighting into them (its SceneOptions::light_direction), so this is the
    /// mode that matches the software path pixel for pixel.
    #[default]
    Baked,
    /// Light each pixel from its face normal and FrameSettings::light_direction.
    /// For a draw list built with the scene builder's own lighting switched off
    /// (a zero light direction); lighting baked colours again would darken them
    /// twice.
    PerPixel,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameSettings {
    pub background: Rgba,
    pub lighting: LightingMode,
    /// World direction TOWARDS the light; normalised here. The scene builder's
    /// default, so switching modes keeps the sun where it was.
    pub light_direction: Vec3,
    pub ambient: f64,
    /// z' = datum + (z - datum) * factor, applied in the vertex shader. A
    /// draw list for this renderer is built with exaggeration 1.
    pub vertical_exaggeration: f64,
    pub exaggeration_datum: f64,
    /// Device pixels per logical pixel. Line widths and point sizes are in
    /// logical pixels (as DrawLine::width is) and are scaled by this, so a
    /// 1 px line is one LOGICAL pixel wide on a 125% display, not a thinner one.
    pub pixel_ratio: f64,
    /// Screen diameter of a cloud point, in logical pixels.
    pub cloud_point_size: f64,
    /// Perspective depth to infinity (scene_origin). Off only to compare with
    /// the software path's clipping at the camera's far plane.
    pub infinite_far: bool,
}

impl Default for FrameSettings {
    fn default() -> Self {
        Self {
            background: rgba(28, 30, 36),
            lighting: LightingMode::Baked,
            light_direction: Vec3::new(0.35, -0.5, 0.79),
            ambient: 0.35,
            vertical_exaggeration: 1.0,
            exaggeration_datum: 0.0,
            pixel_ratio: 1.0,
            cloud_point_size: 2.0,
            infinite_far: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GpuFrameStats {
    pub triangles: usize,
    pub lines: usize,
    pub points: usize,
    pub cloud_points: usize,
    /// True when this frame uploaded the scene (it had changed).
    pub uploaded: bool,
    pub uploaded_bytes: usize,
}

/// One pass of a layered scene (GpuRenderer::set_layers).
#[derive(Clone, Copy, Debug)]
pub struct LayerSource<'a> {
    /// None or empty: nothing drawn.
    pub list: Option<&'a DrawList>,
    /// False for a layer that is tested against the depth drawn before it but
    /// writes none: the cad layer's render_layers' grid and edges.
    pub depth_write: bool,
}

/// The formats of the targets the renderer draws into: what its pipelines are
/// made for (the C++'s render pass descriptor and sample count).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TargetFormat {
    pub color: wgpu::TextureFormat,
    /// A float format: reversed Z needs the float's precision near zero.
    pub depth: wgpu::TextureFormat,
    pub sample_count: u32,
}

/// One frame's target for GpuRenderer::render: the views it clears and draws
/// into, and their size in pixels.
#[derive(Debug)]
pub struct RenderTarget<'a> {
    /// What is drawn into: multisampled when the renderer's sample count is.
    pub color: &'a wgpu::TextureView,
    /// Where a multisampled colour is resolved to; None when it is not.
    pub resolve: Option<&'a wgpu::TextureView>,
    pub depth: &'a wgpu::TextureView,
    pub width: u32,
    pub height: u32,
    /// Where the GPU writes the pass's start and end times, when it is timed.
    pub timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'a>>,
}

// FILL OFFSET. Filled triangles are pushed AWAY from the eye by a polygon
// offset, so that a line lying in a surface - a TIN edge, a string draped on
// it - draws over it rather than z-fighting into dashes. Reversed Z puts far
// at 0, so "away" is negative.
//
// The slope term does the work: a line quad is flat in depth across its
// width while the surface under it slopes, and at a grazing angle the surface
// changes depth by metres per pixel. The slope the offset is scaled by is the
// larger of the surface's depth changes per pixel across and down the screen,
// and a quad corner r pixels across and r down from where the mark touches
// the surface is at most twice r of that deeper, so two of it keep whole any
// mark reaching one pixel from its centre: a 1 px line, whose quad reaches
// half a pixel of antialiasing fringe past its half width (quad.wgsl). That
// is the dense TIN's edges, and no more, because the same offset is what lets
// linework behind a ridge show through the ridge: two pixels of it, next to
// the silhouette. The constant term covers a surface seen face on, where the
// slope is zero: for a float depth buffer one unit is 2^(e - 23) of the
// value's own exponent, so 64 units is a relative 4e-6 to 8e-6 of the depth -
// millimetres at a kilometre.
//
// This replaces the software path's constant NDC bias, which was sized for a
// standard-Z buffer and pulls lines metres forward, through buildings.
const FILL_CONSTANT_BIAS: i32 = -64;
const FILL_SLOPE_BIAS: f32 = -2.0;

// MARK PULL. A mark reaching further than the fill's offset covers - a size-5
// point reaches 3 px (3.6 px at 125%), a selected line 1.5 px - is moved
// towards the eye along its own ray, so it stays on the same pixels, by this
// many pixels' worth of world at its depth for every pixel it reaches beyond
// the first (quad.wgsl, pulled_towards_eye). Only the mark moves, so the
// TIN's own 1 px edges, which reach one pixel, are not pulled and see no more
// through a ridge than the fill offset lets them.
//
// Worked out: the pull and the fill offset together keep a mark of reach r
// whole on a surface whose depth changes by up to 3 (r - 1) / (r - 2) pixels'
// worth per pixel up the screen - 6 for a size-5 point, a surface 80 degrees
// from face on - or 1.5 along each axis at once, 65 degrees on a diagonal.
// Measured (MarksLyingOnASlopedSurfaceDrawWholeOverIt, size-5 points on a
// relief 24 degrees steep seen 35 degrees down; at 100% and 125%): with no
// pull 86.8% and 82.5% of the points' pixels showed, with a pull of 1 99.3%
// and 98.7%, 2 99.85% and 99.90%, 3 100% and 99.97%.
//
// The price is the other side of the same depth: a mark shows through a
// surface less than 3 (r - 1) pixels' worth in front of it - for a size-5
// point 6 px, about its own width. A wall twice a point's size in front still
// hides it (APointWellBehindAWallStaysHidden), which a pull of 4 already did
// not.
const MARK_PULL: f64 = 3.0;

const PROGRAM_COUNT: usize = ALL_PROGRAMS.len();

// The constant buffer every program reads: the twin of frame.wgsl's Frame,
// field for field; vec4s only, so WGSL's uniform layout puts each exactly
// where this writes it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct FrameUniforms {
    mvp: [f32; 16],
    viewport: [f32; 4],
    light: [f32; 4],
    eye_rel: [f32; 4],
    forward_mode: [f32; 4],
    params: [f32; 4],
    mark_pull: [f32; 4],
}

const UNIFORM_BYTES: usize = 160;
const _: () = assert!(std::mem::size_of::<FrameUniforms>() == UNIFORM_BYTES);

impl FrameUniforms {
    // The block's bytes in field order, native-endian as the GPU reads them.
    fn to_bytes(self) -> [u8; UNIFORM_BYTES] {
        let mut bytes = [0u8; UNIFORM_BYTES];
        let words = self
            .mvp
            .iter()
            .chain(&self.viewport)
            .chain(&self.light)
            .chain(&self.eye_rel)
            .chain(&self.forward_mode)
            .chain(&self.params)
            .chain(&self.mark_pull);
        for (slot, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(words) {
            *slot = word.to_ne_bytes();
        }
        bytes
    }
}

fn float4(x: f64, y: f64, z: f64, w: f64) -> [f32; 4] {
    [x as f32, y as f32, z as f32, w as f32]
}

// The scene's buffers hold the same bytes whichever way they are drawn
// (gpu_scene); instanced, a line is one 40-byte instance and a point one
// 24-byte instance of a six-vertex quad.
static TRIANGLE_ATTRIBUTES: [wgpu::VertexAttribute; 2] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Unorm8x4,
        offset: 12,
        shader_location: 1,
    },
];
static LINE_ATTRIBUTES: [wgpu::VertexAttribute; 6] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Unorm8x4,
        offset: 12,
        shader_location: 1,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32,
        offset: 16,
        shader_location: 2,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 20,
        shader_location: 3,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Unorm8x4,
        offset: 32,
        shader_location: 4,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32,
        offset: 36,
        shader_location: 5,
    },
];
static POINT_ATTRIBUTES: [wgpu::VertexAttribute; 3] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Unorm8x4,
        offset: 12,
        shader_location: 1,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32,
        offset: 16,
        shader_location: 2,
    },
];
static CLOUD_ATTRIBUTES: [wgpu::VertexAttribute; 2] = [
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Float32x3,
        offset: 0,
        shader_location: 0,
    },
    wgpu::VertexAttribute {
        format: wgpu::VertexFormat::Unorm8x4,
        offset: 12,
        shader_location: 1,
    },
];

fn layout_for(program: Program) -> wgpu::VertexBufferLayout<'static> {
    let (stride, step_mode, attributes): (
        usize,
        wgpu::VertexStepMode,
        &'static [wgpu::VertexAttribute],
    ) = match program {
        Program::Triangles => (
            std::mem::size_of::<GpuVertex>(),
            wgpu::VertexStepMode::Vertex,
            &TRIANGLE_ATTRIBUTES,
        ),
        Program::Lines => (
            std::mem::size_of::<GpuLine>(),
            wgpu::VertexStepMode::Instance,
            &LINE_ATTRIBUTES,
        ),
        Program::Points => (
            std::mem::size_of::<GpuPoint>(),
            wgpu::VertexStepMode::Instance,
            &POINT_ATTRIBUTES,
        ),
        Program::CloudPoints => (
            std::mem::size_of::<GpuCloudPoint>(),
            wgpu::VertexStepMode::Instance,
            &CLOUD_ATTRIBUTES,
        ),
    };
    wgpu::VertexBufferLayout {
        array_stride: stride as wgpu::BufferAddress,
        step_mode,
        attributes,
    }
}

/// What `make` did, or wgpu's description of the first error it raised.
/// wgpu reports a resource it could not make on the device's error scopes,
/// which are the calling thread's own, so a device shared between threads
/// reports each thread's errors to that thread.
pub(crate) fn guarded<T>(
    device: &wgpu::Device,
    make: impl FnOnce() -> T,
) -> std::result::Result<T, String> {
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let made = make();
    let internal = pollster::block_on(internal.pop());
    let memory = pollster::block_on(memory.pop());
    let validation = pollster::block_on(validation.pop());
    match internal.or(memory).or(validation) {
        Some(error) => Err(format!("{error}")),
        None => Ok(made),
    }
}

// One layer of the scene: what was packed, its GPU buffers, and whether it
// must be uploaded again.
#[derive(Debug)]
struct GpuLayer {
    data: GpuSceneData,
    depth_write: bool,
    dirty: bool,
    vertices: Option<wgpu::Buffer>,
    indices: Option<wgpu::Buffer>,
    lines: Option<wgpu::Buffer>,
    points: Option<wgpu::Buffer>,
}

impl Default for GpuLayer {
    fn default() -> Self {
        Self {
            data: GpuSceneData::default(),
            depth_write: true,
            dirty: true,
            vertices: None,
            indices: None,
            lines: None,
            points: None,
        }
    }
}

impl GpuLayer {
    fn release_gpu(&mut self) {
        self.vertices = None;
        self.indices = None;
        self.lines = None;
        self.points = None;
        // Whatever was uploaded went with the buffers.
        self.dirty = true;
    }
}

// What initialise made on the device.
#[derive(Debug)]
struct Resources {
    device: wgpu::Device,
    queue: wgpu::Queue,
    // Two uniform blocks: the scene and a point cloud are packed against
    // different origins, so each needs its own matrix.
    scene_uniforms: wgpu::Buffer,
    cloud_uniforms: wgpu::Buffer,
    scene_bindings: wgpu::BindGroup,
    cloud_bindings: wgpu::BindGroup,
    // Per program, the pipeline that writes depth and, for the programs a
    // layer that writes none can hold (not the cloud), the one that does not.
    pipelines: [Option<wgpu::RenderPipeline>; PROGRAM_COUNT],
    no_write_pipelines: [Option<wgpu::RenderPipeline>; PROGRAM_COUNT],
}

// The scene as it lies in memory before the first upload: what scene() gives
// when there are no layers.
static EMPTY_SCENE: GpuSceneData = GpuSceneData {
    origin: Vec3::new(0.0, 0.0, 0.0),
    bounds: AABB::new(
        Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    ),
    vertices: Vec::new(),
    triangle_indices: Vec::new(),
    lines: Vec::new(),
    points: Vec::new(),
};

#[derive(Debug, Default)]
pub struct GpuRenderer {
    resources: Option<Resources>,
    sample_count: u32,
    expansion: Expansion,
    // The scene, in drawing order, every layer relative to `origin`.
    layers: Vec<GpuLayer>,
    origin: Vec3,
    cloud: Option<wgpu::Buffer>,
    cloud_data: PointCloudData,
    cloud_dirty: bool,
    uploads: usize,
}

impl GpuRenderer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates the pipelines for targets of `target`'s formats, on `device`,
    /// widening lines and points by instancing, wgpu's one way. Fails with
    /// RenderingFailure when a shader does not compile or a pipeline cannot be
    /// made: the caller falls back to the software path rather than drawing
    /// nothing.
    pub fn initialise(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: TargetFormat,
    ) -> Status {
        self.release_resources();
        self.sample_count = std::cmp::max(target.sample_count, 1);
        self.expansion = Expansion::Instanced;

        let uniform_buffer = |label: &'static str| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: UNIFORM_BYTES as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let Ok((scene_uniforms, cloud_uniforms)) = guarded(device, || {
            (
                uniform_buffer("katana scene uniforms"),
                uniform_buffer("katana cloud uniforms"),
            )
        }) else {
            return Err(Error::new(
                ErrorCode::RenderingFailure,
                "could not create the uniform buffers",
            ));
        };
        let bindings = guarded(device, || {
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("katana frame"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES as u64),
                    },
                    count: None,
                }],
            });
            let bind = |buffer: &wgpu::Buffer, label: &'static str| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(label),
                    layout: &layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    }],
                })
            };
            let scene = bind(&scene_uniforms, "katana scene frame");
            let cloud = bind(&cloud_uniforms, "katana cloud frame");
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("katana frame"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            (scene, cloud, pipeline_layout)
        });
        let Ok((scene_bindings, cloud_bindings, pipeline_layout)) = bindings else {
            return Err(Error::new(
                ErrorCode::RenderingFailure,
                "could not create the shader bindings",
            ));
        };

        let mut pipelines: [Option<wgpu::RenderPipeline>; PROGRAM_COUNT] = Default::default();
        let mut no_write_pipelines: [Option<wgpu::RenderPipeline>; PROGRAM_COUNT] =
            Default::default();
        let expansion = self.expansion;
        let sample_count = self.sample_count;
        let not_made = |program: Program, message: String| {
            Error::new(
                ErrorCode::RenderingFailure,
                format!(
                    "the {} ({}) pipeline could not be created ({DESCRIPTION}: {message})",
                    to_string(program),
                    to_string(expansion),
                ),
            )
        };
        for program in ALL_PROGRAMS {
            // Checked by naga first, so that a shader that does not compile
            // comes back with naga's own message (shader_compiler).
            let source = compiled_program(program)?;
            let module = guarded(device, || {
                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(to_string(program)),
                    source: wgpu::ShaderSource::Wgsl(source.into()),
                })
            })
            .map_err(|message| not_made(program, message))?;
            for depth_write in [true, false] {
                if !depth_write && program == Program::CloudPoints {
                    continue; // a cloud is drawn after the scene, writing depth
                }
                let pipeline = guarded(device, || {
                    make_pipeline(
                        device,
                        &pipeline_layout,
                        &module,
                        program,
                        depth_write,
                        target,
                        sample_count,
                    )
                })
                .map_err(|message| not_made(program, message))?;
                let slot = if depth_write {
                    &mut pipelines[program as usize]
                } else {
                    &mut no_write_pipelines[program as usize]
                };
                *slot = Some(pipeline);
            }
        }
        self.resources = Some(Resources {
            device: device.clone(),
            queue: queue.clone(),
            scene_uniforms,
            cloud_uniforms,
            scene_bindings,
            cloud_bindings,
            pipelines,
            no_write_pipelines,
        });
        for layer in &mut self.layers {
            layer.dirty = true;
        }
        self.cloud_dirty = !self.cloud_data.points.is_empty();
        Ok(())
    }

    /// Drops every GPU resource (the device is going away, or the target
    /// format changed). The packed scene is kept and uploaded again after
    /// initialise().
    pub fn release_resources(&mut self) {
        self.resources = None;
        for layer in &mut self.layers {
            layer.release_gpu();
        }
        self.cloud = None;
        self.cloud_dirty = !self.cloud_data.points.is_empty();
    }

    pub fn initialised(&self) -> bool {
        self.resources.is_some()
    }

    /// How lines and points are widened since initialise().
    pub fn expansion(&self) -> Expansion {
        self.expansion
    }

    /// The samples per pixel the pipelines were made for.
    pub fn sample_count(&self) -> u32 {
        self.sample_count
    }

    /// Packs `list` for the GPU as the one layer of the scene, writing depth;
    /// it is uploaded by the next frame.
    pub fn set_draw_list(&mut self, list: &DrawList) {
        let only = [LayerSource {
            list: Some(list),
            depth_write: true,
        }];
        self.set_layers(&only, None);
    }

    /// Packs every layer, in drawing order, against `origin` - by default the
    /// centre of their joint bounds; a host zoomed deep far from that passes
    /// its pivot (scene_origin, "The origin follows a deep zoom") - uploaded
    /// by the next frame. A non-finite origin is taken as none.
    pub fn set_layers(&mut self, layers: &[LayerSource<'_>], origin: Option<Vec3>) {
        // Each layer bounded once: for the joint box, and then for its packing.
        let mut bounds = vec![AABB::default(); layers.len()];
        let mut joint = AABB::default();
        for (layer, bound) in layers.iter().zip(bounds.iter_mut()) {
            if let Some(list) = layer.list {
                *bound = list.bounds();
                if !bound.empty() {
                    joint.expand_point(bound.min);
                    joint.expand_point(bound.max);
                }
            }
        }
        self.origin = match origin {
            Some(origin) if origin.is_finite() => origin,
            _ => choose_scene_origin(joint),
        };
        // Kept, not rebuilt, so a layer's buffers are reused by the next upload.
        self.layers.resize_with(layers.len(), GpuLayer::default);
        for ((layer, source), bound) in self.layers.iter_mut().zip(layers).zip(&bounds) {
            layer.depth_write = source.depth_write;
            if let Some(list) = source.list {
                pack_draw_list_with_bounds(list, self.origin, *bound, &mut layer.data);
            } else {
                layer.data.clear();
                layer.data.origin = self.origin;
            }
            layer.dirty = true;
        }
    }

    /// What the layers are packed against.
    pub fn origin(&self) -> Vec3 {
        self.origin
    }

    /// Repacks layer `index` alone, against the origin the last set_layers
    /// chose, keeping its depth rule: for a layer that changed without the
    /// rest - a selection, edges recoloured by their fade. Its content should
    /// lie within the scene set_layers was given (a selection within the
    /// drawing), or its offsets from the origin lose precision. An index past
    /// the layers is ignored.
    pub fn update_layer(&mut self, index: usize, list: &DrawList) {
        let origin = self.origin;
        let Some(layer) = self.layers.get_mut(index) else {
            return;
        };
        pack_draw_list_with_origin(list, origin, &mut layer.data);
        layer.dirty = true;
    }

    /// Takes an already packed scene as the one layer (a test that fixes the
    /// origin).
    pub fn set_scene(&mut self, scene: GpuSceneData) {
        self.origin = scene.origin;
        self.layers.resize_with(1, GpuLayer::default);
        let layer = &mut self.layers[0];
        layer.data = scene;
        layer.depth_write = true;
        layer.dirty = true;
    }

    /// The first layer as packed (an empty scene when there is none).
    pub fn scene(&self) -> &GpuSceneData {
        match self.layers.first() {
            Some(layer) => &layer.data,
            None => &EMPTY_SCENE,
        }
    }

    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// The world box of every layer together: what a view frames.
    pub fn scene_bounds(&self) -> AABB {
        let mut bounds = AABB::default();
        for layer in &self.layers {
            if !layer.data.bounds.empty() {
                bounds.expand_point(layer.data.bounds.min);
                bounds.expand_point(layer.data.bounds.max);
            }
        }
        bounds
    }

    /// Point clouds, drawn after the scene as round sprites.
    pub fn set_point_cloud(&mut self, cloud: PointCloudData) {
        self.cloud_data = cloud;
        self.cloud_dirty = true;
    }

    pub fn clear_point_cloud(&mut self) {
        self.cloud_data.clear();
        self.cloud_dirty = true;
    }

    /// How many frames have uploaded the scene: the proof, for a test, that a
    /// frame which changed only the camera uploaded nothing.
    pub fn upload_count(&self) -> usize {
        self.uploads
    }

    /// The first half of a frame, for a pass someone else begins: the upload
    /// if the scene changed, and the uniforms for a target of `width` x
    /// `height` pixels, written to the queue so that they land before the
    /// next submission. The camera's viewport must be that size; a camera
    /// with no viewport size fails with InvalidArgument (drawing it would
    /// show a wrong framing, as the Rasterizer argues). The counts of what is
    /// drawn are draw()'s.
    pub fn prepare(
        &mut self,
        camera: &Camera,
        settings: &FrameSettings,
        width: u32,
        height: u32,
    ) -> Result<GpuFrameStats> {
        let Some(resources) = self.resources.as_ref() else {
            return Err(Error::new(
                ErrorCode::InvalidState,
                "the GPU renderer is not initialised",
            ));
        };
        if width == 0 || height == 0 {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "render target is empty",
            ));
        }
        if i64::from(camera.viewport_width()) != i64::from(width)
            || i64::from(camera.viewport_height()) != i64::from(height)
        {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!(
                    "camera viewport {}x{} does not match the render target {width}x{height}",
                    camera.viewport_width(),
                    camera.viewport_height()
                ),
            ));
        }

        let mut stats = GpuFrameStats::default();
        for layer in &mut self.layers {
            if !layer.dirty {
                continue;
            }
            let s = &layer.data;
            let ok = upload(
                resources,
                &mut layer.vertices,
                wgpu::BufferUsages::VERTEX,
                bytes_of(&s.vertices),
            ) && upload(
                resources,
                &mut layer.indices,
                wgpu::BufferUsages::INDEX,
                bytes_of(&s.triangle_indices),
            ) && upload(
                resources,
                &mut layer.lines,
                wgpu::BufferUsages::VERTEX,
                bytes_of(&s.lines),
            ) && upload(
                resources,
                &mut layer.points,
                wgpu::BufferUsages::VERTEX,
                bytes_of(&s.points),
            );
            if !ok {
                return Err(Error::new(
                    ErrorCode::RenderingFailure,
                    format!("the GPU refused a scene buffer of {} bytes", s.byte_size()),
                ));
            }
            layer.dirty = false;
            stats.uploaded = true;
            stats.uploaded_bytes += s.byte_size();
        }
        if stats.uploaded {
            self.uploads += 1;
        }
        if self.cloud_dirty {
            let bytes = bytes_of(&self.cloud_data.points);
            if !upload(
                resources,
                &mut self.cloud,
                wgpu::BufferUsages::VERTEX,
                bytes,
            ) {
                return Err(Error::new(
                    ErrorCode::RenderingFailure,
                    format!("the GPU refused a point cloud of {} bytes", bytes.len()),
                ));
            }
            self.cloud_dirty = false;
            stats.uploaded_bytes += bytes.len();
        }

        let scene_uniforms = uniforms_for(camera, self.origin, settings, width, height);
        resources
            .queue
            .write_buffer(&resources.scene_uniforms, 0, &scene_uniforms.to_bytes());
        if self.draws_cloud() {
            let cloud_uniforms =
                uniforms_for(camera, self.cloud_data.origin, settings, width, height);
            resources
                .queue
                .write_buffer(&resources.cloud_uniforms, 0, &cloud_uniforms.to_bytes());
        }
        Ok(stats)
    }

    fn draws_cloud(&self) -> bool {
        !self.cloud_data.points.is_empty() && self.cloud.is_some()
    }

    /// The second half of a frame: records the scene's draws into `pass`, a
    /// pass on a target of the formats initialise() was given, and adds what
    /// it drew to `stats`. Lines and points after the fills they lie on, as
    /// the software path orders them, and blended over them; each layer whole
    /// before the next, as the cad layer's render_layers draws them. Nothing
    /// is drawn before initialise().
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, stats: &mut GpuFrameStats) {
        let Some(resources) = self.resources.as_ref() else {
            return;
        };
        pass.set_bind_group(0, &resources.scene_bindings, &[]);
        for layer in &self.layers {
            let pipelines = if layer.depth_write {
                &resources.pipelines
            } else {
                &resources.no_write_pipelines
            };
            let s = &layer.data;
            if !s.triangle_indices.is_empty()
                && let (Some(vertices), Some(indices), Some(pipeline)) = (
                    &layer.vertices,
                    &layer.indices,
                    &pipelines[Program::Triangles as usize],
                )
            {
                pass.set_pipeline(pipeline);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..s.triangle_indices.len() as u32, 0, 0..1);
                stats.triangles += s.triangle_count();
            }
            if !s.lines.is_empty()
                && let (Some(buffer), Some(pipeline)) =
                    (&layer.lines, &pipelines[Program::Lines as usize])
            {
                draw_quads(pass, pipeline, buffer, s.lines.len());
                stats.lines += s.lines.len();
            }
            if !s.points.is_empty()
                && let (Some(buffer), Some(pipeline)) =
                    (&layer.points, &pipelines[Program::Points as usize])
            {
                draw_quads(pass, pipeline, buffer, s.points.len());
                stats.points += s.points.len();
            }
        }
        if self.draws_cloud()
            && let (Some(buffer), Some(pipeline)) = (
                &self.cloud,
                &resources.pipelines[Program::CloudPoints as usize],
            )
        {
            pass.set_bind_group(0, &resources.cloud_bindings, &[]);
            draw_quads(pass, pipeline, buffer, self.cloud_data.points.len());
            stats.cloud_points = self.cloud_data.points.len();
        }
    }

    /// Records one frame into `encoder`: the upload if the scene changed, the
    /// uniforms, and a render pass on `target` that clears it and draws.
    /// prepare()'s checks and failures, and draw()'s order. The pass's
    /// viewport and scissor are the whole target, wgpu's default for a pass.
    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: RenderTarget<'_>,
        camera: &Camera,
        settings: &FrameSettings,
    ) -> Result<GpuFrameStats> {
        let mut stats = self.prepare(camera, settings, target.width, target.height)?;
        let background = settings.background;
        let clear = wgpu::Color {
            r: f64::from(red_of(background)) / 255.0,
            g: f64::from(green_of(background)) / 255.0,
            b: f64::from(blue_of(background)) / 255.0,
            a: f64::from(alpha_of(background)) / 255.0,
        };
        let color_attachments = [Some(wgpu::RenderPassColorAttachment {
            view: target.color,
            depth_slice: None,
            resolve_target: target.resolve,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(clear),
                store: wgpu::StoreOp::Store,
            },
        })];
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("katana scene"),
            color_attachments: &color_attachments,
            // Depth clears to 0: the FAR end of a reversed-Z buffer.
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: target.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: target.timestamp_writes,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.draw(&mut pass, &mut stats);
        Ok(stats)
    }
}

// Through instancing every line and every point is one instance of a
// six-vertex quad.
fn draw_quads(
    pass: &mut wgpu::RenderPass<'_>,
    pipeline: &wgpu::RenderPipeline,
    buffer: &wgpu::Buffer,
    count: usize,
) {
    pass.set_pipeline(pipeline);
    pass.set_vertex_buffer(0, buffer.slice(..));
    pass.draw(0..6, 0..count as u32);
}

#[allow(
    clippy::too_many_arguments,
    reason = "The C++ sets each of these on its QRhiGraphicsPipeline in turn; one argument a setting."
)]
fn make_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    program: Program,
    depth_write: bool,
    target: TargetFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let (bias, blend) = if program == Program::Triangles {
        (
            wgpu::DepthBiasState {
                constant: FILL_CONSTANT_BIAS,
                slope_scale: FILL_SLOPE_BIAS,
                clamp: 0.0,
            },
            None,
        )
    } else {
        // Antialiased coverage goes out as alpha.
        (
            wgpu::DepthBiasState::default(),
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
            }),
        )
    };
    let buffers = [Some(layout_for(program))];
    let targets = [Some(wgpu::ColorTargetState {
        format: target.color,
        blend,
        write_mask: wgpu::ColorWrites::ALL,
    })];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(to_string(program)),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(VERTEX_ENTRY),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..wgpu::PrimitiveState::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: target.depth,
            depth_write_enabled: Some(depth_write),
            // Reversed Z: nearer is GREATER. Strictly greater, so of two
            // primitives at exactly the same depth the first drawn wins, as
            // the software path's strict test does.
            depth_compare: Some(wgpu::CompareFunction::Greater),
            stencil: wgpu::StencilState::default(),
            bias,
        }),
        multisample: wgpu::MultisampleState {
            count: sample_count,
            ..wgpu::MultisampleState::default()
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(FRAGMENT_ENTRY),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

// Uploads `bytes` into `buffer`, replacing it by a larger one when it has
// outgrown it: an edited scene of the same size or smaller reuses the
// allocation. Returns false when the GPU refused it.
fn upload(
    resources: &Resources,
    buffer: &mut Option<wgpu::Buffer>,
    usage: wgpu::BufferUsages,
    bytes: &[u8],
) -> bool {
    if bytes.is_empty() {
        return true; // nothing to draw; the draw call is skipped too
    }
    let size = bytes.len() as wgpu::BufferAddress;
    if buffer
        .as_ref()
        .is_none_or(|existing| existing.size() < size)
    {
        *buffer = None;
        let made = guarded(&resources.device, || {
            resources.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("katana scene buffer"),
                size,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        match made {
            Ok(made) => *buffer = Some(made),
            Err(_) => return false,
        }
    }
    if let Some(buffer) = buffer.as_ref() {
        resources.queue.write_buffer(buffer, 0, bytes);
    }
    true
}

fn uniforms_for(
    camera: &Camera,
    origin: Vec3,
    settings: &FrameSettings,
    width: u32,
    height: u32,
) -> FrameUniforms {
    // WebGPU's clip space on every backend: y up, depth in [0, 1] (ClipSpace).
    let projection = ProjectionOptions {
        infinite_far: settings.infinite_far,
        clip: ClipSpace::default(),
    };
    let clip = relative_view_projection(camera, origin, &projection);
    let width = f64::from(width);
    let height = f64::from(height);

    let mut light = settings.light_direction;
    let light_length = light.length();
    let lit = settings.lighting == LightingMode::PerPixel
        && light_length.is_finite()
        && light_length > 1.0e-9;
    if lit {
        light /= light_length;
    }
    let ambient = if settings.ambient.is_finite() {
        cpp::clamp(settings.ambient, 0.0, 1.0)
    } else {
        0.35
    };

    // In double, then rounded.
    let eye = camera.eye() - origin;
    let perspective = camera.projection() == Projection::Perspective;
    let forward = camera.forward();

    let factor =
        if settings.vertical_exaggeration.is_finite() && settings.vertical_exaggeration > 0.0 {
            settings.vertical_exaggeration
        } else {
            1.0
        };
    let datum = if settings.exaggeration_datum.is_finite() {
        settings.exaggeration_datum
    } else {
        0.0
    };
    let ratio = if settings.pixel_ratio.is_finite() && settings.pixel_ratio > 0.0 {
        settings.pixel_ratio
    } else {
        1.0
    };
    let cloud_size = if settings.cloud_point_size.is_finite() && settings.cloud_point_size > 0.0 {
        settings.cloud_point_size
    } else {
        2.0
    };

    // The mark pull (MARK_PULL): per pixel of reach, a pixel's worth of world
    // at the mark's depth - in perspective a fraction of that depth, the
    // pixel's angular size; in orthographic a fixed length - and the clip z
    // of what the mark is pulled towards: the eye, or the view direction.
    // The projection mixes no x or y into z (no off-centre frusta), so the
    // eye's clip position is (0, 0, z, 0) and so is the direction's.
    let per_pixel = if perspective {
        2.0 * (camera.field_of_view() * 0.5).tan() / height
    } else {
        camera.orthographic_height() / height
    };
    let target = if perspective {
        clip * Vec4::new(eye.x, eye.y, eye.z, 1.0)
    } else {
        clip * Vec4::new(forward.x, forward.y, forward.z, 0.0)
    };

    FrameUniforms {
        mvp: to_float_column_major(clip),
        viewport: float4(width, height, 1.0 / width, 1.0 / height),
        light: float4(light.x, light.y, light.z, ambient),
        eye_rel: float4(eye.x, eye.y, eye.z, if perspective { 1.0 } else { 0.0 }),
        forward_mode: float4(forward.x, forward.y, forward.z, if lit { 1.0 } else { 0.0 }),
        params: float4(factor, datum - origin.z, ratio, cloud_size),
        mark_pull: float4(MARK_PULL * per_pixel, target.z, 0.0, 0.0),
    }
}
