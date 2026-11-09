//! What the pointcloud layer compiles and links: native/pdal_shim.cpp, PDAL's
//! primitives behind a C ABI (src/ffi.rs), and native/ffi_check.c, whose
//! static assertions hold the GDAL declarations of src/ffi.rs to GDAL's own
//! headers; then PDAL's C++ library and GDAL.
//!
//! Where the linker finds them, as katana_gis's and katana_geodesy's build
//! scripts say: MSYS2's GCC searches its own lib/ by itself; the conda-forge
//! prefix of WSL and of the Linux and macOS release jobs is in no default
//! search path, so it is named from KATANA_TOOLCHAIN, which
//! tools/build_visible.sh sets there (docs/rust.md, "WSL").
//!
//! Which compiler: with KATANA_TOOLCHAIN set, the prefix's own
//! `<arch>-conda-linux-gnu-c++` (and `-gcc` for C, `-ar` to archive), unless
//! CXX, CC or AR names another, so that libstdc++ and the C++ ABI are those
//! of the PDAL and GDAL linked; otherwise cc's default, MSYS2's g++ first on
//! PATH on Windows. tools/build_visible.sh puts only the prefix's build tools
//! on PATH in WSL, so cc's default there would be the distribution's compiler,
//! built against another libstdc++. A prefix whose compilers are named
//! otherwise (macOS's conda-forge clang) is left to cc's default for now: the
//! name is used only when that file is there.

use std::path::PathBuf;

const SHIM: &str = "native/pdal_shim.cpp";
const CHECK: &str = "native/ffi_check.c";

fn main() {
    println!("cargo:rerun-if-changed={SHIM}");
    println!("cargo:rerun-if-changed={CHECK}");
    for variable in ["KATANA_TOOLCHAIN", "CXX", "CC", "AR"] {
        println!("cargo:rerun-if-env-changed={variable}");
    }

    let prefix = std::env::var("KATANA_TOOLCHAIN")
        .ok()
        .filter(|prefix| !prefix.is_empty());
    if let Some(prefix) = &prefix {
        println!("cargo:rustc-link-search=native={prefix}/lib");
    }
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    // The prefix's tool of that name, when the prefix has one.
    let prefix_tool = |tool: &str| {
        let prefix = prefix.as_ref()?;
        let path = PathBuf::from(format!("{prefix}/bin/{arch}-conda-linux-gnu-{tool}"));
        path.is_file().then_some(path)
    };
    let unset = |variable: &str| std::env::var_os(variable).is_none();

    let configure = |build: &mut cc::Build, compiler: &str, compiler_variable: &str| {
        if unset(compiler_variable)
            && let Some(path) = prefix_tool(compiler)
        {
            build.compiler(path);
            if unset("AR")
                && let Some(archiver) = prefix_tool("ar")
            {
                build.archiver(archiver);
            }
        }
        if let Some(prefix) = &prefix {
            build.include(format!("{prefix}/include"));
        }
    };

    // PDAL 2.10's headers are C++17. Every compiler the port builds with
    // (GCC on Windows and Linux, clang on macOS) reads the GNU spelling.
    let mut shim = cc::Build::new();
    shim.cpp(true).flag("-std=c++17").file(SHIM);
    configure(&mut shim, "c++", "CXX");
    shim.compile("katana_pdal_shim");

    let mut check = cc::Build::new();
    check.file(CHECK);
    configure(&mut check, "gcc", "CC");
    check.compile("katana_pointcloud_ffi_check");

    // After the shim's archive, which needs them; cc has already named the
    // C++ runtime (stdc++).
    println!("cargo:rustc-link-lib=pdalcpp");
    println!("cargo:rustc-link-lib=gdal");
}
