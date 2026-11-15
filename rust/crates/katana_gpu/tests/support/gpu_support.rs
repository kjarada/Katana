//! What the GPU tests share (tests/gpu/gpu_test_support.hpp): making a device
//! or saying why there is none, rendering the same scene on the software
//! rasteriser, and comparing the two images the way a GPU frame can be
//! compared - by coverage and by colour, within a tolerance, never bit for
//! bit.
//!
//! ONE DEVICE OF EACH KIND, made the first time a case asks for it and shared
//! by every case of the test binary: wgpu's device is Send + Sync, the cases
//! run on threads of one process, and a lavapipe device made once is cheaper
//! than one a case. Each case makes its own target and renderer on it
//! (OffscreenGpu::create_on). A device that cannot be made is remembered with
//! its reason, so every case skips with the same sentence.
//!
//! A case SKIPS only when its device is not there (ErrorCode::Unsupported, as
//! offscreen_gpu documents); any other failure to make the target - a
//! pipeline that will not build - fails the case. The C++'s makeGpu skipped
//! on every failure, which hid a broken pipeline as a missing device.
//!
//! One owner (katana_gpu's tests); included read-only elsewhere, as the
//! window's GPU twins do:
//! `#[path = "../../../katana_gpu/tests/support/gpu_support.rs"] pub mod gpu_support;`

use katana_core::task_pool::TaskPool;
use katana_core::{Error, ErrorCode};
use katana_gpu::{EnumName, GpuContext, GpuDevice, OffscreenGpu, OffscreenOptions};
use katana_render::{
    Camera, DrawList, Framebuffer, Rasterizer, RenderOptions, Rgba, alpha_of, blue_of, green_of,
    red_of,
};
use std::sync::OnceLock;

pub type Image = Vec<Rgba>;

/// The devices every device case runs on, in the C++'s order.
pub const DEVICES: [GpuDevice; 2] = [GpuDevice::Hardware, GpuDevice::Software];

/// The C++ parameter's name: Hardware or Software.
pub fn device_label(device: GpuDevice) -> &'static str {
    match device {
        GpuDevice::Hardware => "Hardware",
        GpuDevice::Software => "Software",
    }
}

/// The shared device of that kind, or why there is none.
pub fn shared_context(device: GpuDevice) -> Result<&'static GpuContext, Error> {
    static HARDWARE: OnceLock<Result<GpuContext, Error>> = OnceLock::new();
    static SOFTWARE: OnceLock<Result<GpuContext, Error>> = OnceLock::new();
    let cell = match device {
        GpuDevice::Hardware => &HARDWARE,
        GpuDevice::Software => &SOFTWARE,
    };
    match cell.get_or_init(|| GpuContext::request(device)) {
        Ok(context) => Ok(context),
        Err(error) => Err(error.clone()),
    }
}

/// An offscreen target on the shared device of that kind, or why not: the
/// C++'s makeGpu, with `base` for the options other than the device.
pub fn make_gpu_with(
    width: i32,
    height: i32,
    device: GpuDevice,
    base: &OffscreenOptions,
) -> Result<OffscreenGpu, Error> {
    let options = OffscreenOptions { device, ..*base };
    OffscreenGpu::create_on(shared_context(device)?, width, height, &options)
}

pub fn make_gpu(width: i32, height: i32, device: GpuDevice) -> Result<OffscreenGpu, Error> {
    make_gpu_with(width, height, device, &OffscreenOptions::default())
}

/// The target a device case draws on, or None after printing why the device
/// is not here (GTEST_SKIP's line). Any other failure fails the case.
pub fn gpu_or_skip(width: i32, height: i32, device: GpuDevice) -> Option<OffscreenGpu> {
    match make_gpu(width, height, device) {
        Ok(gpu) => Some(gpu),
        Err(error) if error.code == ErrorCode::Unsupported => {
            eprintln!(
                "[  SKIPPED ] {}: {}",
                device_label(device),
                error.describe()
            );
            None
        }
        Err(error) => panic!(
            "{}: the target could not be made: {}",
            device_label(device),
            error.describe()
        ),
    }
}

/// The same list through the software rasteriser, single-threaded, at the
/// camera's viewport size; empty when it cannot draw.
pub fn cpu_render(list: &DrawList, camera: &Camera, background: Rgba) -> Image {
    let Ok(mut target) = Framebuffer::create(camera.viewport_width(), camera.viewport_height())
    else {
        return Vec::new();
    };
    let pool = TaskPool::new(0);
    let options = RenderOptions {
        background,
        pool: Some(&pool),
        ..RenderOptions::default()
    };
    let mut rasterizer = Rasterizer::default();
    if rasterizer
        .render(list, camera, &mut target, &options)
        .is_err()
    {
        return Vec::new();
    }
    target.color().to_vec()
}

/// r + g + b.
pub fn brightness(color: Rgba) -> i32 {
    i32::from(red_of(color)) + i32::from(green_of(color)) + i32::from(blue_of(color))
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Comparison {
    pub covered_a: usize,
    pub covered_b: usize,
    /// Pixels covered in one image and not the other.
    pub coverage_mismatches: usize,
    /// Pixels whose 3x3 neighbourhood is covered in BOTH images and holds no
    /// colour edge in the first (no neighbour more than 16 levels from the
    /// centre in any channel): away from every outline and from every border
    /// between two primitives, where multisampling blends and the two fill
    /// rules round differently.
    pub interior_pixels: usize,
    /// The largest difference in any one channel over the interior pixels.
    pub max_interior_difference: i32,
    /// Pixels (anywhere) differing by more than 2 in some channel.
    pub differing_pixels: usize,
}

fn channel_difference(p: Rgba, q: Rgba) -> i32 {
    let dr = (i32::from(red_of(p)) - i32::from(red_of(q))).abs();
    let dg = (i32::from(green_of(p)) - i32::from(green_of(q))).abs();
    let db = (i32::from(blue_of(p)) - i32::from(blue_of(q))).abs();
    dr.max(dg).max(db)
}

/// A pixel is "covered" when it is at least `threshold` brighter than the
/// background (brightness above).
pub fn compare(
    a: &[Rgba],
    b: &[Rgba],
    width: i32,
    height: i32,
    background: Rgba,
    threshold: i32,
) -> Comparison {
    let mut result = Comparison::default();
    let count = width as usize * height as usize;
    if a.len() != count || b.len() != count {
        // Nothing to compare is as bad as it gets.
        result.coverage_mismatches = count;
        return result;
    }
    let ground = brightness(background);
    let at = |x: i32, y: i32| y as usize * width as usize + x as usize;
    let covered_in =
        |image: &[Rgba], x: i32, y: i32| brightness(image[at(x, y)]) - ground >= threshold;
    for y in 0..height {
        for x in 0..width {
            let i = at(x, y);
            let in_a = covered_in(a, x, y);
            let in_b = covered_in(b, x, y);
            result.covered_a += usize::from(in_a);
            result.covered_b += usize::from(in_b);
            if in_a != in_b {
                result.coverage_mismatches += 1;
            }
            let difference = channel_difference(a[i], b[i]);
            if difference > 2 {
                result.differing_pixels += 1;
            }
            let mut interior = x > 0 && y > 0 && x + 1 < width && y + 1 < height;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if !interior {
                        break;
                    }
                    interior = covered_in(a, x + dx, y + dy)
                        && covered_in(b, x + dx, y + dy)
                        && channel_difference(a[i], a[at(x + dx, y + dy)]) <= 16;
                }
            }
            if interior {
                result.interior_pixels += 1;
                result.max_interior_difference = result.max_interior_difference.max(difference);
            }
        }
    }
    if std::env::var_os("KATANA_GPU_TEST_IMAGES").is_some_and(|directory| !directory.is_empty()) {
        println!("  compared: {}", describe(&result));
    }
    result
}

/// The comparison in one line, for failure messages.
pub fn describe(c: &Comparison) -> String {
    format!(
        "covered {} / {}, coverage mismatches {}, interior {} (max difference {}), differing {}",
        c.covered_a,
        c.covered_b,
        c.coverage_mismatches,
        c.interior_pixels,
        c.max_interior_difference,
        c.differing_pixels
    )
}

/// `what` for `device`: the name a saved image takes (the C++'s label()).
pub fn label(what: &str, device: GpuDevice) -> String {
    format!("{what}_{}", device.name())
}

// The PNG chunk check: CRC-32 (ISO 3309, the polynomial 0xEDB88320 reflected)
// over the chunk's type and data (PNG specification, section 5.3).
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn push_chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = png.len();
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    let crc = crc32(&png[start..]);
    png.extend_from_slice(&crc.to_be_bytes());
}

// An 8-bit RGBA PNG of `image`, its pixel data in stored (uncompressed)
// deflate blocks: a picture to look at needs no compression, and this keeps
// the tests free of an image crate. The zlib stream is RFC 1950 (header 0x78
// 0x01, Adler-32 at the end) around RFC 1951 stored blocks of at most 65535
// bytes.
fn png_of(image: &[Rgba], width: i32, height: i32) -> Vec<u8> {
    let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    for row in image.chunks(width as usize) {
        raw.push(0); // filter: none
        for &pixel in row {
            raw.extend_from_slice(&[
                red_of(pixel),
                green_of(pixel),
                blue_of(pixel),
                alpha_of(pixel),
            ]);
        }
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (index, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(index + 1 == blocks.len()));
        let length = block.len() as u16;
        zlib.extend_from_slice(&length.to_le_bytes());
        zlib.extend_from_slice(&(!length).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut low, mut high) = (1u32, 0u32);
    for &byte in &raw {
        low = (low + u32::from(byte)) % 65_521;
        high = (high + low) % 65_521;
    }
    zlib.extend_from_slice(&((high << 16) | low).to_be_bytes());

    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&(width as u32).to_be_bytes());
    header.extend_from_slice(&(height as u32).to_be_bytes());
    // 8 bits a channel, colour type 6 (RGBA), deflate, adaptive filtering,
    // no interlace.
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    push_chunk(&mut png, b"IHDR", &header);
    push_chunk(&mut png, b"IDAT", &zlib);
    push_chunk(&mut png, b"IEND", &[]);
    png
}

/// With KATANA_GPU_TEST_IMAGES set to a directory, saves `image` there as
/// <name>.png so a person can LOOK at what the test compared. A no-op
/// otherwise.
pub fn save_for_looking(name: &str, image: &[Rgba], width: i32, height: i32) {
    let Some(directory) = std::env::var_os("KATANA_GPU_TEST_IMAGES") else {
        return;
    };
    if directory.is_empty()
        || width <= 0
        || height <= 0
        || image.len() != width as usize * height as usize
    {
        return;
    }
    let directory = std::path::PathBuf::from(directory);
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    println!("  saved {name}.png");
    let _ = std::fs::write(
        directory.join(format!("{name}.png")),
        png_of(image, width, height),
    );
}
