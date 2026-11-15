//! Packed arrays as the bytes a GPU buffer is filled with.
//!
//! The C++ hands QRhi a pointer to its vectors and their size in bytes; this
//! is the same view, written once, so that an upload copies nothing first. It
//! is sound only for records whose every byte is a field's (no padding): the
//! types it is implemented for are #[repr(C)] records of four-byte fields,
//! whose sizes gpu_scene asserts.

#![allow(
    unsafe_code,
    reason = "Viewing plain #[repr(C)] records as bytes is the one way to hand them to a GPU buffer without copying them."
)]

use crate::gpu_scene::{GpuCloudPoint, GpuLine, GpuPoint, GpuVertex};

/// A type every byte of which is initialised data.
///
/// # Safety
///
/// Implemented only for #[repr(C)] records of `f32` and `u32` fields (and
/// for `u32` itself), which have no padding.
pub(crate) unsafe trait Plain: Copy {}

// SAFETY: four f32/u32 fields, #[repr(C)], 16 bytes (asserted in gpu_scene).
unsafe impl Plain for GpuVertex {}
// SAFETY: two #[repr(C)] GpuLineEnd records of five four-byte fields, 40 bytes.
unsafe impl Plain for GpuLine {}
// SAFETY: six four-byte fields, #[repr(C)], 24 bytes.
unsafe impl Plain for GpuPoint {}
// SAFETY: four four-byte fields, #[repr(C)], 16 bytes.
unsafe impl Plain for GpuCloudPoint {}
// SAFETY: an integer has no padding.
unsafe impl Plain for u32 {}

/// The bytes of `items`, in memory order: native-endian, which is what the
/// GPU reads on every machine Katana runs on (all little-endian).
pub(crate) fn bytes_of<T: Plain>(items: &[T]) -> &[u8] {
    // SAFETY: T is Plain, so every one of the size_of_val(items) bytes from
    // items' start is initialised; u8 needs no alignment, and the view
    // borrows `items` for as long as it lives.
    unsafe { std::slice::from_raw_parts(items.as_ptr().cast::<u8>(), std::mem::size_of_val(items)) }
}
