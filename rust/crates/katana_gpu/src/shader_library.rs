//! Where the GPU renderer's shaders come from (docs/gpu.md, "Shaders";
//! shader_library.hpp).
//!
//! THE C++ carried two hand-written sets of the same stages: HLSL compiled at
//! run time for Direct3D 11 on Windows, and Vulkan-style GLSL baked to SPIR-V
//! by Qt's qsb at build time for Linux and macOS, behind a ShaderLibrary
//! interface (with SerializedShaderLibrary for the baked blobs).
//!
//! THE PORT has one set, WGSL, written by hand from the GLSL function for
//! function (src/shaders/*.wgsl), embedded in the program and compiled by wgpu
//! when a pipeline's module is made - naga translates it to SPIR-V, HLSL or
//! MSL for the backend. So there is one library, no serialized blob and no
//! second compiler: the cases that compared two libraries or two compilers
//! have no twin (rust/parity.toml). shader_compiler validates every module
//! with naga once per process, so a mistake comes back as a Status with
//! naga's own message rather than as wgpu's uncaptured error.
//!
//! Rejected: naga's GLSL front end over the existing files (a GLSL 450 subset
//! with no geometry stage, behind a further wgpu feature), and SPIR-V
//! precompiled at build time (glslang in the toolchain and an artefact to
//! keep beside the source).
//!
//! WGSL has no include, so a program's module is its pieces joined in the
//! order the GLSL included them: frame.wgsl (the uniform block and the
//! exaggeration), then quad.wgsl (the quad builders) for the programs that
//! widen lines or points, then the program's own stages. Every module has the
//! entry points VERTEX_ENTRY and FRAGMENT_ENTRY.

use crate::names::EnumName;

/// The four things the renderer draws, one pipeline each (two for those a
/// layer that writes no depth can hold).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Program {
    /// Filled, optionally lit per pixel from their face normal.
    Triangles,
    /// Screen-space quads with analytic antialiasing.
    Lines,
    /// The draw list's square markers, as sprites.
    Points,
    /// Round sprites of a fixed screen size, for point clouds.
    CloudPoints,
}

pub const ALL_PROGRAMS: [Program; 4] = [
    Program::Triangles,
    Program::Lines,
    Program::Points,
    Program::CloudPoints,
];

impl EnumName for Program {
    fn name(self) -> &'static str {
        match self {
            Program::Triangles => "triangles",
            Program::Lines => "lines",
            Program::Points => "points",
            Program::CloudPoints => "cloud points",
        }
    }
}

/// How a line or a point becomes the quad that is drawn (docs/gpu.md, "Lines
/// and points").
///
/// The C++ had two ways: a geometry shader turning each line or point into a
/// four-vertex strip (its default, the cheaper on the owner's scene), and
/// instancing, where each line or point is an instance of six vertices widened
/// by the vertex shader (for a device with no geometry stage, as Metal has
/// none). wgpu has no geometry stage on any backend, so instancing is the one
/// way: the variant is kept so that the renderer can say how it draws, in the
/// C++'s words. What it costs: with no line on screen the geometry shader took
/// 2.0 ms against instancing's 5.1 on the owner's scene; with the lines on
/// screen both took 12.5 (docs/gpu.md, "Measurements").
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Expansion {
    #[default]
    Instanced,
}

pub const ALL_EXPANSIONS: [Expansion; 1] = [Expansion::Instanced];

impl EnumName for Expansion {
    fn name(self) -> &'static str {
        match self {
            Expansion::Instanced => "instanced",
        }
    }
}

/// The vertex stage's entry point in every module.
pub const VERTEX_ENTRY: &str = "vs_main";
/// The fragment stage's entry point in every module.
pub const FRAGMENT_ENTRY: &str = "fs_main";

/// For logs and the status line: which kind of library this is.
pub const DESCRIPTION: &str = "WGSL compiled by wgpu";

const TRIANGLES: &str = concat!(
    include_str!("shaders/frame.wgsl"),
    include_str!("shaders/triangles.wgsl"),
);
const LINES: &str = concat!(
    include_str!("shaders/frame.wgsl"),
    include_str!("shaders/quad.wgsl"),
    include_str!("shaders/lines.wgsl"),
);
const POINTS: &str = concat!(
    include_str!("shaders/frame.wgsl"),
    include_str!("shaders/quad.wgsl"),
    include_str!("shaders/points.wgsl"),
);
const CLOUD_POINTS: &str = concat!(
    include_str!("shaders/frame.wgsl"),
    include_str!("shaders/quad.wgsl"),
    include_str!("shaders/cloud.wgsl"),
);

/// The whole WGSL module `program` is drawn with: the twin of the C++'s
/// hlslSource, which answered one stage at a time where a WGSL module holds
/// both.
pub fn wgsl_source(program: Program) -> &'static str {
    match program {
        Program::Triangles => TRIANGLES,
        Program::Lines => LINES,
        Program::Points => POINTS,
        Program::CloudPoints => CLOUD_POINTS,
    }
}
