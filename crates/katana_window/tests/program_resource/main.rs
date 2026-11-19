//! katana.exe carries its icon and its version as Windows resources (build.rs), so Explorer, the
//! taskbar, the Start menu and Settings > Apps show Katana's icon for the file and its Properties
//! name the product and the version. Read from the built program's bytes, without a PE parser:
//! a resource compiler copies each image of the .ico verbatim into an RT_ICON resource, and a
//! VERSIONINFO block holds its strings in UTF-16. Windows only: elsewhere a program has no such
//! resource.

#![cfg(windows)]

use std::path::PathBuf;

fn checkout() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn program() -> Vec<u8> {
    std::fs::read(env!("CARGO_BIN_EXE_katana")).expect("the built katana.exe is read")
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|window| window == needle)
}

/// `text` as UTF-16, little-endian, as a VERSIONINFO block's strings are held.
fn utf16(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// The images of an .ico file, each as its directory entry places it (ICONDIR: reserved, type 1,
/// count; then 16-byte ICONDIRENTRYs whose last two fields are the image's size and offset).
fn icon_images(ico: &[u8]) -> Vec<&[u8]> {
    let word = |at: usize| u16::from_le_bytes([ico[at], ico[at + 1]]);
    let dword = |at: usize| {
        u32::from_le_bytes([ico[at], ico[at + 1], ico[at + 2], ico[at + 3]]) as usize
    };
    assert_eq!((word(0), word(2)), (0, 1), "resources/katana.ico is an icon file");
    (0..usize::from(word(4)))
        .map(|index| {
            let entry = 6 + 16 * index;
            let (size, offset) = (dword(entry + 8), dword(entry + 12));
            &ico[offset..offset + size]
        })
        .collect()
}

// Every image of resources/katana.ico is in the program, so the file shows Katana's icon at
// every size Windows asks for (16 px in a list, 256 in a large view) rather than the generic one.
#[test]
fn katana_exe_carries_every_image_of_the_application_icon() {
    let ico = std::fs::read(checkout().join("resources/katana.ico")).expect("the icon is read");
    let images = icon_images(&ico);
    assert!(!images.is_empty(), "the icon file holds images");
    let program = program();
    for (index, image) in images.iter().enumerate() {
        assert!(
            contains(&program, image),
            "image {index} of resources/katana.ico ({} bytes) is not in katana.exe",
            image.len()
        );
    }
}

// The version block names the product and the version project(Katana VERSION ...) declares, the
// one place a version is written.
#[test]
fn katana_exe_names_the_product_and_the_projects_version() {
    let cmake = std::fs::read_to_string(checkout().join("CMakeLists.txt"))
        .expect("the checkout's CMakeLists.txt is read");
    let version = cmake
        .split("project(Katana")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .and_then(|arguments| {
            let mut words = arguments.split_ascii_whitespace();
            words.find(|word| *word == "VERSION")?;
            words.next()
        })
        .expect("CMakeLists.txt declares project(Katana VERSION ...)");
    let program = program();
    for text in [
        "ProductName",
        "Katana - survey and CAD platform",
        "ProductVersion",
        version,
        "katana.exe",
    ] {
        assert!(
            contains(&program, &utf16(text)),
            "katana.exe's version block has no {text:?}"
        );
    }
}

/// The Subsystem field of a PE image's optional header (Microsoft's "PE Format": e_lfanew at
/// 0x3C names the "PE\0\0" signature; the 20-byte COFF header follows it, then the optional
/// header, whose Subsystem is at offset 68 in both PE32 and PE32+).
fn pe_subsystem(image: &[u8]) -> u16 {
    let dword = |at: usize| {
        u32::from_le_bytes([image[at], image[at + 1], image[at + 2], image[at + 3]]) as usize
    };
    let signature = dword(0x3C);
    assert_eq!(&image[signature..signature + 4], b"PE\0\0", "a PE image");
    let subsystem = signature + 4 + 20 + 68;
    u16::from_le_bytes([image[subsystem], image[subsystem + 1]])
}

// katana.exe is a GUI program (IMAGE_SUBSYSTEM_WINDOWS_GUI, 2), so starting it opens no console
// window beside the main one (the owner, 2026-11-19: "when i open katana i get an additional
// cmd"). The test's own program, a console one (IMAGE_SUBSYSTEM_WINDOWS_CUI, 3), proves the
// field is read where it is; both values are the PE format's.
#[test]
fn katana_exe_is_a_windows_gui_program_and_opens_no_console() {
    assert_eq!(pe_subsystem(&program()), 2, "katana.exe's subsystem");
    let console = std::fs::read(std::env::current_exe().expect("the test's own path"))
        .expect("the test program is read");
    assert_eq!(pe_subsystem(&console), 3, "a console program reads as one: the parse is right");
}
