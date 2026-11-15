//! The GPU layer's twins (tests/gpu): one module per C++ test file, the gtest
//! suites as modules inside. test_gpu_scene_view.cpp is the 3D view's, and
//! is twinned with it in the window.
//!
//! The arithmetic, packing, renderer choice and shader cases need no device
//! and always run. Every device case runs its body on the Hardware device and
//! then the Software one (the C++'s TEST_P over GpuDevice), on devices made
//! once and shared by the whole binary (support/gpu_support.rs), and skips a
//! device that is not there with the C++'s sentence: in WSL there is no
//! hardware Vulkan device, so Hardware skips and Software runs on lavapipe.
//! No case needs a display: wgpu renders into a texture.
//!
//! Not twinned, each because its subject has no counterpart under wgpu
//! (rust/parity.toml): GpuRender.TheGeometryShaderAndInstancingDrawTheSameLinesPointsAndCloud
//! (wgpu has no geometry stage), GpuRender.SerializedShadersDrawWhatTheRuntimeShadersDraw
//! and SerializedShaderLibrary.ReportsABlobThatDoesNotDeserialize (there are no
//! serialized shaders), GpuRender.BytecodeCompiledHereDrawsWhatQRhiCompilingTheSourceDraws
//! (there is no second compiler).

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]

#[path = "../support/gpu_support.rs"]
pub mod gpu_support;
// The step-1 render tests' support: gtest's comparisons, GCC's mt19937_64
// and uniform_real_distribution, and the SIMD level switch.
#[path = "../../../katana_render/tests/support/mod.rs"]
pub mod render_support;

mod test_gpu_layers;
mod test_gpu_render;
mod test_gpu_scene;
mod test_renderer_choice;
mod test_scene_origin;
