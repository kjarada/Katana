//! Where the linker finds GDAL. MSYS2's GCC searches its own lib/ (libgdal.dll.a)
//! by itself; the conda-forge prefix of WSL and of the Linux and macOS release
//! jobs is in no default search path, so it is named from KATANA_TOOLCHAIN,
//! which tools/build_visible.sh sets there (docs/rust.md, "WSL").

fn main() {
    println!("cargo:rerun-if-env-changed=KATANA_TOOLCHAIN");
    if let Ok(prefix) = std::env::var("KATANA_TOOLCHAIN")
        && !prefix.is_empty()
    {
        println!("cargo:rustc-link-search=native={prefix}/lib");
    }
}
