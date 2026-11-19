//! Packed records as the bytes a GPU buffer is filled with.
//!
//! Each record says its own bytes: its fields in declaration order, four
//! bytes each, native-endian - what its #[repr(C)] layout holds in memory
//! (gpu_scene asserts the sizes), and so what the shaders' vertex layouts
//! read, as FrameUniforms::to_bytes says the uniform block's. The upload
//! writes them straight into the queue's staging memory (gpu_renderer's
//! upload), so each byte is copied once, as it was when the records' memory
//! was viewed as bytes in place. That view needed `unsafe` and a promise
//! that no record had padding; field by field there is nothing to promise.

use crate::gpu_scene::{GpuCloudPoint, GpuLine, GpuLineEnd, GpuPoint, GpuVertex};

/// A record of `N` bytes as a GPU buffer holds it.
pub(crate) trait GpuRecord<const N: usize>: Copy {
    fn gpu_bytes(&self) -> [u8; N];
}

// Four-byte words laid end to end. The callers' lengths are constants, so the
// assertion is decided when it is compiled.
fn words<const N: usize>(words: &[[u8; 4]]) -> [u8; N] {
    assert_eq!(words.len() * 4, N);
    let mut bytes = [0u8; N];
    for (slot, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(words) {
        *slot = *word;
    }
    bytes
}

impl GpuRecord<16> for GpuVertex {
    fn gpu_bytes(&self) -> [u8; 16] {
        words(&[
            self.x.to_ne_bytes(),
            self.y.to_ne_bytes(),
            self.z.to_ne_bytes(),
            self.color.to_ne_bytes(),
        ])
    }
}

fn line_end_words(end: &GpuLineEnd) -> [[u8; 4]; 5] {
    [
        end.x.to_ne_bytes(),
        end.y.to_ne_bytes(),
        end.z.to_ne_bytes(),
        end.color.to_ne_bytes(),
        end.width.to_ne_bytes(),
    ]
}

impl GpuRecord<40> for GpuLine {
    fn gpu_bytes(&self) -> [u8; 40] {
        let [a0, a1, a2, a3, a4] = line_end_words(&self.a);
        let [b0, b1, b2, b3, b4] = line_end_words(&self.b);
        words(&[a0, a1, a2, a3, a4, b0, b1, b2, b3, b4])
    }
}

impl GpuRecord<24> for GpuPoint {
    fn gpu_bytes(&self) -> [u8; 24] {
        words(&[
            self.x.to_ne_bytes(),
            self.y.to_ne_bytes(),
            self.z.to_ne_bytes(),
            self.color.to_ne_bytes(),
            self.size.to_ne_bytes(),
            self.unused.to_ne_bytes(),
        ])
    }
}

impl GpuRecord<16> for GpuCloudPoint {
    fn gpu_bytes(&self) -> [u8; 16] {
        words(&[
            self.x.to_ne_bytes(),
            self.y.to_ne_bytes(),
            self.z.to_ne_bytes(),
            self.color.to_ne_bytes(),
        ])
    }
}

impl GpuRecord<4> for u32 {
    fn gpu_bytes(&self) -> [u8; 4] {
        self.to_ne_bytes()
    }
}

// Every machine Katana runs on is little-endian, so the expected bytes are
// worked out little-endian: each float from its IEEE 754 single-precision
// bit pattern (1.0 is 0x3F800000, -2.0 is 0xC0000000, 0.5 is 0x3F000000 and
// so on), least significant byte first, and each colour the same way.
#[cfg(all(test, target_endian = "little"))]
mod tests {
    use super::GpuRecord;
    use crate::gpu_scene::{GpuCloudPoint, GpuLine, GpuLineEnd, GpuPoint, GpuVertex};

    #[test]
    fn a_vertex_is_its_position_then_its_colour_four_bytes_each_least_significant_first() {
        let vertex = GpuVertex {
            x: 1.0,
            y: -2.0,
            z: 0.5,
            color: 0x1122_3344,
        };
        assert_eq!(
            vertex.gpu_bytes(),
            [
                0x00, 0x00, 0x80, 0x3F, // x 1.0
                0x00, 0x00, 0x00, 0xC0, // y -2.0
                0x00, 0x00, 0x00, 0x3F, // z 0.5
                0x44, 0x33, 0x22, 0x11, // colour
            ]
        );
    }

    #[test]
    fn a_line_is_its_first_end_then_its_second_each_with_its_width_last() {
        let line = GpuLine {
            a: GpuLineEnd {
                x: 1.0,
                y: 2.0,
                z: 3.0,
                color: 0xAABB_CCDD,
                width: 1.5,
            },
            b: GpuLineEnd {
                x: -1.0,
                y: 0.0,
                z: 4.0,
                color: 0x0102_0304,
                width: 0.25,
            },
        };
        assert_eq!(
            line.gpu_bytes(),
            [
                0x00, 0x00, 0x80, 0x3F, // a.x 1.0
                0x00, 0x00, 0x00, 0x40, // a.y 2.0
                0x00, 0x00, 0x40, 0x40, // a.z 3.0
                0xDD, 0xCC, 0xBB, 0xAA, // a.colour
                0x00, 0x00, 0xC0, 0x3F, // a.width 1.5
                0x00, 0x00, 0x80, 0xBF, // b.x -1.0
                0x00, 0x00, 0x00, 0x00, // b.y 0.0
                0x00, 0x00, 0x80, 0x40, // b.z 4.0
                0x04, 0x03, 0x02, 0x01, // b.colour
                0x00, 0x00, 0x80, 0x3E, // b.width 0.25
            ]
        );
    }

    #[test]
    fn a_point_is_its_position_colour_size_and_the_unused_word_last() {
        let point = GpuPoint {
            x: 0.5,
            y: 1.0,
            z: 2.0,
            color: 0xFF00_0000,
            size: 3.0,
            unused: 0.0,
        };
        assert_eq!(
            point.gpu_bytes(),
            [
                0x00, 0x00, 0x00, 0x3F, // x 0.5
                0x00, 0x00, 0x80, 0x3F, // y 1.0
                0x00, 0x00, 0x00, 0x40, // z 2.0
                0x00, 0x00, 0x00, 0xFF, // colour
                0x00, 0x00, 0x40, 0x40, // size 3.0
                0x00, 0x00, 0x00, 0x00, // unused 0.0
            ]
        );
    }

    #[test]
    fn a_cloud_point_is_its_position_then_its_colour() {
        let point = GpuCloudPoint {
            x: 4.0,
            y: -0.5,
            z: 1.0,
            color: 0x00FF_00FF,
        };
        assert_eq!(
            point.gpu_bytes(),
            [
                0x00, 0x00, 0x80, 0x40, // x 4.0
                0x00, 0x00, 0x00, 0xBF, // y -0.5
                0x00, 0x00, 0x80, 0x3F, // z 1.0
                0xFF, 0x00, 0xFF, 0x00, // colour
            ]
        );
    }

    #[test]
    fn an_index_is_its_four_bytes_least_significant_first() {
        assert_eq!(0xDEAD_BEEFu32.gpu_bytes(), [0xEF, 0xBE, 0xAD, 0xDE]);
    }
}
