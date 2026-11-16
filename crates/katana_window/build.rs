//! The Windows resource of katana.exe: its icon and its version information, as the C++ build's
//! katana.rc.in gave it (gone with the C++).
//!
//! The icon is what Explorer, the taskbar, Alt-Tab, the Start menu and Settings > Apps show for
//! the FILE, before any of the program has run; eframe's viewport icon covers the running window
//! but not the executable on disk. The version block is what the file's Properties > Details tab
//! shows, what an installer compares to decide whether to replace a file, and what a crash report
//! names. Every value has one source: the version is project(Katana VERSION ...) of the
//! checkout's CMakeLists.txt (as katana_update's build.rs reads it), the publisher
//! KATANA_PUBLISHER of cmake/KatanaPackaging.cmake (the name the packages are signed under; the
//! variable of that name in cargo's environment, as a -D was, overrides it), the icon
//! resources/katana.ico.
//!
//! The script is written into cargo's output folder and compiled by the toolchain's resource
//! compiler (`windres` of MSYS2's binutils; `llvm-windres` where there is only LLVM's, as in
//! CLANGARM64; the WINDRES variable names another), into an object linked into katana alone:
//! the developer tools beside it are not the product. On any other platform nothing is done.
//! A Windows build without a resource compiler fails here, naming it, rather than shipping a
//! program with Windows' generic icon. REJECTED: the embed-resource and winresource crates, a
//! new dependency for what is one command; and committing a compiled .res, which would hold a
//! version that the CMakeLists.txt could move away from.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let manifest = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    );
    let checkout = manifest.join("../..");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));

    let cmake = read(&checkout.join("CMakeLists.txt"));
    let version = project_version(&cmake)
        .unwrap_or_else(|| panic!("CMakeLists.txt has no project(Katana VERSION ...)"));
    let numbers = version_numbers(version)
        .unwrap_or_else(|| panic!("the version {version} is not MAJOR.MINOR.PATCH"));
    let publisher = configured_publisher().unwrap_or_else(|| {
        let packaging = read(&checkout.join("cmake/KatanaPackaging.cmake"));
        packaging_publisher(&packaging)
            .unwrap_or_else(|| panic!("cmake/KatanaPackaging.cmake sets no KATANA_PUBLISHER"))
    });
    let icon = checkout.join("resources/katana.ico");
    println!("cargo:rerun-if-changed={}", icon.display());
    let icon = rc_path(&icon);

    let script = out.join("katana.rc");
    let text = resource_script(&icon, version, numbers, &publisher);
    std::fs::write(&script, text)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", script.display()));

    let object = out.join("katana_resource.o");
    let compiler = resource_compiler();
    let status = Command::new(&compiler)
        .arg("--input")
        .arg(&script)
        .arg("--output-format=coff")
        .arg("--output")
        .arg(&object)
        .status()
        .unwrap_or_else(|error| {
            panic!(
                "the resource compiler {compiler} could not be run ({error}): katana.exe needs it \
                 for its icon and version (MSYS2's binutils has windres; WINDRES names another)"
            )
        });
    assert!(
        status.success(),
        "{compiler} failed on {} ({status})",
        script.display()
    );
    println!("cargo:rustc-link-arg-bin=katana={}", object.display());
}

fn read(path: &Path) -> String {
    println!("cargo:rerun-if-changed={}", path.display());
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

// The resource compiler: WINDRES when set, else windres when it runs, else llvm-windres.
fn resource_compiler() -> String {
    println!("cargo:rerun-if-env-changed=WINDRES");
    if let Ok(named) = std::env::var("WINDRES")
        && !named.is_empty()
    {
        return named;
    }
    let runs = |name: &str| {
        Command::new(name)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    };
    if runs("windres") {
        String::from("windres")
    } else {
        String::from("llvm-windres")
    }
}

// KATANA_PUBLISHER from cargo's environment, as a configure's -D set it; None when unset or empty.
fn configured_publisher() -> Option<String> {
    println!("cargo:rerun-if-env-changed=KATANA_PUBLISHER");
    std::env::var("KATANA_PUBLISHER")
        .ok()
        .filter(|name| !name.is_empty())
}

// The quoted value of set(KATANA_PUBLISHER "...").
fn packaging_publisher(text: &str) -> Option<String> {
    const OPENING: &str = "set(KATANA_PUBLISHER \"";
    let start = text.find(OPENING)? + OPENING.len();
    let rest = &text[start..];
    Some(String::from(&rest[..rest.find('"')?]))
}

// The word after VERSION inside project(Katana ...), as katana_update's build.rs reads it.
fn project_version(text: &str) -> Option<&str> {
    const OPENING: &str = "project(Katana";
    let start = text.find(OPENING)? + OPENING.len();
    let rest = &text[start..];
    let arguments = &rest[..rest.find(')')?];
    let mut words = arguments.split_ascii_whitespace();
    while let Some(word) = words.next() {
        if word == "VERSION" {
            return words.next();
        }
    }
    None
}

// MAJOR.MINOR.PATCH as the three numbers of a VERSIONINFO (each a 16-bit word there).
fn version_numbers(version: &str) -> Option<[u16; 3]> {
    let mut parts = version.split('.');
    let numbers = [
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ];
    parts.next().is_none().then_some(numbers)
}

// A path as a resource script's string takes it: '/' between the names (windres reads a '\' as
// an escape), and a '"' never (a checkout's path has none).
fn rc_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

// A string of a resource script: '"' doubled, as the language escapes it.
fn rc_text(text: &str) -> String {
    text.replace('"', "\"\"")
}

// The script, as katana.rc.in was. Windows takes the first icon resource of a file, which is why
// the icon has the lowest id. 0409 is US English and 04b0 Unicode (the Translation 0x409, 1200).
fn resource_script(icon: &str, version: &str, numbers: [u16; 3], publisher: &str) -> String {
    let [major, minor, patch] = numbers;
    let publisher = rc_text(publisher);
    format!(
        r#"#include <winver.h>

1 ICON "{icon}"

VS_VERSION_INFO VERSIONINFO
FILEVERSION     {major},{minor},{patch},0
PRODUCTVERSION  {major},{minor},{patch},0
FILEFLAGSMASK   VS_FFI_FILEFLAGSMASK
FILEFLAGS       0
FILEOS          VOS_NT_WINDOWS32
FILETYPE        VFT_APP
FILESUBTYPE     VFT2_UNKNOWN
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName",      "{publisher}"
            VALUE "FileDescription",  "Katana - survey and CAD platform"
            VALUE "FileVersion",      "{version}"
            VALUE "InternalName",     "katana"
            VALUE "LegalCopyright",   "Copyright (C) {publisher}"
            VALUE "OriginalFilename", "katana.exe"
            VALUE "ProductName",      "Katana"
            VALUE "ProductVersion",   "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    )
}
