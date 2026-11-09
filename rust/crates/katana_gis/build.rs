//! Where the linker finds GDAL, and the native code beside the crate.
//!
//! MSYS2's GCC searches its own lib/ (libgdal.dll.a) by itself; the
//! conda-forge prefix of WSL and of the Linux and macOS release jobs is in no
//! default search path, so it is named from KATANA_TOOLCHAIN, which
//! tools/build_visible.sh sets there (docs/rust.md, "WSL").
//!
//! Every native/*.c and native/*.cpp is compiled with cc, so a later file -
//! the algorithm shim, the HTTP check - is added to the folder and nothing
//! here changes. native/ffi_check.c is no code: its `_Static_assert`s hold the
//! sizes, offsets, constants and function types the ffi declarations copy
//! (src/ffi/cpl.rs), so compiling it is the check.
//!
//! The compiler: with KATANA_TOOLCHAIN set on Linux, the prefix's own
//! `<arch>-conda-linux-gnu-gcc` and `-c++` (and its `ar`), unless CC, CXX or
//! AR names another, so that libstdc++ and the C++ ABI are those of the GDAL
//! the crate links; WSL has no system compiler at all. Otherwise cc's default
//! (MSYS2's gcc and g++ first on PATH on Windows; on macOS the prefix's
//! clang is not named, and cc's default stands).

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=native");
    for variable in ["KATANA_TOOLCHAIN", "CC", "CXX", "AR"] {
        println!("cargo:rerun-if-env-changed={variable}");
    }
    let prefix = std::env::var("KATANA_TOOLCHAIN")
        .ok()
        .filter(|prefix| !prefix.is_empty());
    if let Some(prefix) = &prefix {
        println!("cargo:rustc-link-search=native={prefix}/lib");
    }

    let (c_files, cpp_files) = native_files(Path::new("native"));
    if !c_files.is_empty() {
        compile(&c_files, false, prefix.as_deref(), "katana_gis_native_c");
    }
    if !cpp_files.is_empty() {
        compile(&cpp_files, true, prefix.as_deref(), "katana_gis_native_cpp");
    }
}

/// The C and the C++ files of the folder, each list in name order.
fn native_files(folder: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut c_files = Vec::new();
    let mut cpp_files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            let path = entry.path();
            match path.extension().and_then(|extension| extension.to_str()) {
                Some("c") => c_files.push(path),
                Some("cpp") => cpp_files.push(path),
                _ => {}
            }
        }
    }
    c_files.sort();
    cpp_files.sort();
    (c_files, cpp_files)
}

fn compile(files: &[PathBuf], cpp: bool, prefix: Option<&str>, library: &str) {
    let mut build = cc::Build::new();
    // GNU C, as the C++ is GNU C++ (g++ defines _GNU_SOURCE): GDAL's headers
    // and the platform's are read as the C++ reads them.
    build.cpp(cpp).std(if cpp { "c++17" } else { "gnu11" });
    for file in files {
        println!("cargo:rerun-if-changed={}", file.display());
        build.file(file);
    }
    if let Some(prefix) = prefix {
        let prefix = Path::new(prefix);
        build.include(prefix.join("include"));
        if let Some(tools) = toolchain_tools(prefix) {
            let variable = if cpp { "CXX" } else { "CC" };
            if !is_set(variable) {
                build.compiler(if cpp { &tools.cpp } else { &tools.c });
            }
            if !is_set("AR") && tools.archiver.exists() {
                build.archiver(&tools.archiver);
            }
        }
    }
    build.compile(library);
}

struct Tools {
    c: PathBuf,
    cpp: PathBuf,
    archiver: PathBuf,
}

/// The prefix's compilers, on Linux: conda-forge names them for the target,
/// `x86_64-conda-linux-gnu-gcc` in WSL.
fn toolchain_tools(prefix: &Path) -> Option<Tools> {
    if std::env::var("CARGO_CFG_TARGET_OS").ok()? != "linux" {
        return None;
    }
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").ok()?;
    let tool = |name: &str| {
        prefix
            .join("bin")
            .join(format!("{arch}-conda-linux-gnu-{name}"))
    };
    Some(Tools {
        c: tool("gcc"),
        cpp: tool("c++"),
        archiver: tool("ar"),
    })
}

fn is_set(variable: &str) -> bool {
    std::env::var_os(variable).is_some_and(|value| !value.is_empty())
}
