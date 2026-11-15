//! The `render` layer: the camera, draw lists and the software rasteriser.
//!
//! The port of src/katana_render and include/katana/render. It depends on
//! core (for the task pool), math and geometry, and on nothing above them: the
//! renderer consumes a DrawList and knows nothing of entities, the document or
//! a project (Rule 3). A GPU backend is a second implementation behind the same
//! DrawList and Camera interface, so nothing above this layer changes when it
//! lands.

#![allow(
    clippy::neg_cmp_op_on_partial_ord,
    reason = "Preserve the C++ oracle's negated floating-point comparisons, including NaN rejection and branch order."
)]

pub mod camera;
mod disjoint;
pub mod draw_list;
pub mod framebuffer;
pub mod rasterizer;
mod scalar;
mod simd;

pub use camera::{Camera, Projection, StandardView};
pub use draw_list::{DrawLine, DrawList, DrawPoint, DrawTriangle, VertexIndex};
pub use framebuffer::{
    Framebuffer, Rgba, TileRect, alpha_of, blue_of, green_of, red_of, rgba, rgba_with_alpha, shade,
};
pub use rasterizer::{Rasterizer, RenderOptions, RenderStats};
