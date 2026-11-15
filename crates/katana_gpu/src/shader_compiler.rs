//! The shader library's WGSL checked by naga, the compiler wgpu itself runs,
//! once per process (docs/gpu.md, "Shaders"; shader_compiler.hpp).
//!
//! The C++ compiled its HLSL to Direct3D bytecode itself, once per process,
//! for two reasons: QRhi compiled the source again for every new QRhi, and a
//! compile error came back from QRhi only as a line in the Qt log. Under wgpu
//! the first does not arise - every module is compiled when its pipeline's
//! module is made, from text the program carries - but the second does:
//! wgpu reports a module that does not compile as an uncaptured device error.
//! So every module is parsed and validated here first, once per process, and
//! a mistake comes back as a Status carrying naga's own message, which the
//! renderer returns before it asks wgpu for anything.
//!
//! The C++ keyed its cache by the source text, so that a changed source could
//! not be answered from a stale entry; the WGSL is fixed when the program is
//! built, so the cache is keyed by the program.

use crate::names::to_string;
use crate::shader_library::{ALL_PROGRAMS, Expansion, Program, wgsl_source};
use katana_core::{Error, ErrorCode, Result, Status};
use std::sync::OnceLock;
use wgpu::naga;

fn did_not_compile(message: &str) -> Error {
    Error::new(
        ErrorCode::RenderingFailure,
        format!(
            "the WGSL module did not compile: {}",
            message.trim_end_matches(['\n', '\0'])
        ),
    )
}

/// One WGSL module parsed and validated by naga, with no caching:
/// RenderingFailure with naga's message (the line and the name it stopped at)
/// when it does not compile.
pub fn compile_wgsl(source: &str) -> Status {
    if source.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "no WGSL source to compile",
        ));
    }
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|error| did_not_compile(&error.emit_to_string(source)))?;
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    );
    validator
        .validate(&module)
        .map_err(|error| did_not_compile(&error.emit_to_string(source)))?;
    Ok(())
}

// What compile_wgsl said of each program's module, the first time it was
// asked.
static CHECKED: [OnceLock<Status>; ALL_PROGRAMS.len()] =
    [const { OnceLock::new() }; ALL_PROGRAMS.len()];

/// The WGSL module of `program`, checked once per process: what the renderer
/// makes its pipelines from. Safe to call from any thread; two threads asking
/// at once wait for one check.
pub fn compiled_program(program: Program) -> Result<&'static str> {
    let source = wgsl_source(program);
    let checked = CHECKED[program as usize].get_or_init(|| {
        compile_wgsl(source)
            .map_err(|error| error.with_context(format!("the {} program", to_string(program))))
    });
    match checked {
        Ok(()) => Ok(source),
        Err(error) => Err(error.clone()),
    }
}

/// Checks every module the renderer draws with now, so the first view does
/// not wait for it. Safe to call from any thread, and more than once (the
/// second call finds everything checked). The expansion is the C++'s
/// argument; instancing, wgpu's one way, draws with every program.
pub fn precompile_shaders(_expansion: Expansion) -> Status {
    for program in ALL_PROGRAMS {
        compiled_program(program)?;
    }
    Ok(())
}
