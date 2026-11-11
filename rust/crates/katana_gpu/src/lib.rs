//! The `gpu` layer: the 3D view's GPU renderer, the twin of the software
//! rasteriser behind the same seam (a `DrawList` and a `Camera`).
//!
//! The port of src/katana_qt/gpu (docs/gpu.md), from QRhi to wgpu: Vulkan on
//! Linux (lavapipe where there is no GPU), Direct3D 12 on Windows (WARP as its
//! software device), Metal on macOS. Its layer is the C++'s: core, math,
//! geometry, render and pointcloud, never a Document (tools/check_layering.cmake).
//!
//! ```text
//! DrawList --pack_draw_list--> GpuSceneData --GpuRenderer--> wgpu (a texture or a pass)
//! Camera ------- relative_view_projection (double) -----> one uniform block
//! ```
//!
//! What changed with the move, each recorded where it is decided:
//!
//! * the shaders are WGSL modules (shader_library), validated by naga
//!   (shader_compiler), and widen lines and points by instancing alone: wgpu
//!   has no geometry stage, so the C++'s default expansion has no twin;
//! * the backends' names (renderer_choice, GpuBackend);
//! * a device can be shared (offscreen_gpu, GpuContext): the tests make one of
//!   each kind per process, and the window hands its own to the renderer.
//!
//! wgpu's types cross into the window as QRhi's crossed into the C++ window;
//! both crates name the workspace's one wgpu, and nothing re-exports it.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection and branch order."
)]

pub mod gpu_renderer;
pub mod gpu_scene;
pub mod names;
pub mod offscreen_gpu;
mod plain;
pub mod renderer_choice;
pub mod scene_origin;
pub mod shader_compiler;
pub mod shader_library;

// gpu/simd: the packing kernels (pack_kernels.hpp, pack_avx2.cpp,
// pack_neon.cpp), private to the packing that dispatches to them.
mod simd {
    pub(crate) mod pack_kernels;

    #[cfg(target_arch = "x86_64")]
    #[allow(
        unsafe_code,
        reason = "The AVX2 kernel is a target-feature function of unaligned loads and stores, as the C++ kernel is; pack_kernels calls it only on a CPU that has AVX2."
    )]
    mod pack_avx2;
    #[cfg(target_arch = "aarch64")]
    #[allow(
        unsafe_code,
        reason = "The NEON kernel is a target-feature function of loads and stores, as the C++ kernel is; pack_kernels calls it only on a CPU that has NEON."
    )]
    mod pack_neon;
}

pub use gpu_renderer::{
    FrameSettings, GpuFrameStats, GpuRenderer, LayerSource, LightingMode, RenderTarget,
    TargetFormat,
};
pub use gpu_scene::{
    GpuCloudPoint, GpuLine, GpuLineEnd, GpuPoint, GpuSceneData, GpuVertex, PointCloudData,
    pack_draw_list, pack_draw_list_with_bounds, pack_draw_list_with_origin,
    pack_engine_point_cloud, pack_point_cloud,
};
pub use names::{EnumName, to_string};
pub use offscreen_gpu::{GpuContext, GpuDevice, OffscreenGpu, OffscreenOptions};
pub use renderer_choice::{
    GpuBackend, RendererDecision, RendererEnvironment, RendererKind, choose_renderer,
    current_renderer_environment,
};
pub use scene_origin::{
    ClipSpace, ORIGIN_ERROR_PIXELS, ProjectionOptions, choose_scene_origin, origin_error_pixels,
    relative_position, relative_view_matrix, relative_view_projection, reversed_z_projection,
    to_float_column_major,
};
pub use shader_compiler::{compile_wgsl, compiled_program, precompile_shaders};
pub use shader_library::{ALL_EXPANSIONS, ALL_PROGRAMS, Expansion, Program, wgsl_source};
