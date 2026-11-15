//! Twin of tests/terrain/test_tin_codec.cpp: a TIN surface as bytes and back -
//! exact, checked on the way in, and a newer version refused.

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "Preserve decimal constants and expected values copied verbatim from the C++ oracle."
)]
#![allow(
    clippy::module_inception,
    reason = "Inner module names identify the C++ suites for the parity checker."
)]

pub mod support;

use katana_core::ErrorCode;
use katana_geometry::Point3;
use katana_terrain::{TIN_CODEC_VERSION, TinSurface, decode_tin, encode_tin, encoded_tin_bytes};
use support::*;

/// Two triangles over the square (0,0)-(10,10), counter-clockwise, with the
/// diagonal 0-2 constrained in both (edge 2 of the first runs 2 -> 0, edge 0
/// of the second runs 0 -> 2) and the first's bottom edge 0 -> 1 too: masks
/// 0b101 and 0b001.
fn square_with_masks(corners: &[Point3]) -> TinSurface {
    TinSurface::create(
        corners.to_vec(),
        vec![[0, 1, 2], [0, 2, 3]],
        vec![0b101, 0b001],
    )
    .unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn bits_of(points: &[Point3]) -> Vec<u64> {
    points
        .iter()
        .flat_map(|point| [point.x.to_bits(), point.y.to_bits(), point.z.to_bits()])
        .collect()
}

/// Little-endian u32 written into `bytes` at `at`.
fn put_u32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn plain_square() -> Vec<Point3> {
    vec![
        p3(0.0, 0.0, 0.0),
        p3(10.0, 0.0, 0.0),
        p3(10.0, 10.0, 1.0),
        p3(0.0, 10.0, 1.0),
    ]
}

fn code_of(bytes: &[u8]) -> Option<ErrorCode> {
    decode_tin(bytes).err().map(|error| error.code)
}

mod tin_codec {
    use super::*;

    #[test]
    fn a_surface_comes_back_bit_for_bit_with_its_constrained_edges() {
        // Values chosen because a text form would lose them: 0.1 has no finite
        // binary expansion, a negative zero compares equal to zero, a value of
        // 1e-300 and one past 2^53 need every bit, and a survey coordinate in
        // the hundreds of thousands carries its millimetres in the low bits.
        let corners = vec![
            p3(0.0, -0.0, 0.1),
            p3(10.0, 1e-300, 101.25),
            p3(10.0, 10.0, 9007199254740993.0),
            p3(0.0, 10.0, 554321.987654321),
        ];
        let surface = square_with_masks(&corners);
        assert_eq!(surface.triangle_count(), 2);

        let bytes = encode_tin(&surface);
        // 24 + 24 x 4 vertices + 13 x 2 triangles (tin_codec's layout).
        assert_eq!(bytes.len(), 24 + 24 * 4 + 13 * 2);
        assert_eq!(encoded_tin_bytes(4, 2), bytes.len() as u64);

        let back = decode_tin(&bytes).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(back == surface, "vertices, triangles, adjacency and masks");
        assert_eq!(
            bits_of(back.vertices()),
            bits_of(&corners),
            "every coordinate's bit pattern"
        );
        assert_eq!(*back.constrained_edges(), vec![0b101u8, 0b001]);
        assert!(
            back.is_edge_constrained(1, 0),
            "the diagonal, from the second triangle"
        );
    }

    #[test]
    fn a_built_surface_with_breaklines_round_trips_exactly() {
        let mut random = Random::new(11);
        let built = build(&pyramid_input(20.0, 5.0, 40, &mut random));
        assert!(!built.surface.empty());
        let back = decode_tin(&encode_tin(&built.surface))
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(back == built.surface);
        assert_eq!(bits_of(back.vertices()), bits_of(built.surface.vertices()));
        // The pyramid's four ridges are breaklines, so some edge is constrained.
        assert!(back.constrained_edges().iter().any(|&mask| mask != 0));
        // Answers the way the original does: the apex is 5 m up, by construction.
        double_eq(back.elevation_at_xy(10.0, 10.0).unwrap_or(-1.0), 5.0);
    }

    #[test]
    fn the_empty_surface_is_the_header_alone() {
        let bytes = encode_tin(&TinSurface::new());
        assert_eq!(bytes.len(), 24);
        let back = decode_tin(&bytes).unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(back.empty());
    }

    #[test]
    fn a_newer_version_is_refused_as_unsupported() {
        let mut bytes = encode_tin(&square_with_masks(&plain_square()));
        put_u32(&mut bytes, 4, TIN_CODEC_VERSION + 1);
        assert_eq!(code_of(&bytes), Some(ErrorCode::Unsupported));
    }

    #[test]
    fn damaged_bytes_are_refused_not_read() {
        let good = encode_tin(&square_with_masks(&plain_square()));

        // Nothing at all, and a header cut short.
        assert_eq!(code_of(&[]), Some(ErrorCode::ParseFailure));
        assert_eq!(code_of(&good[..10]), Some(ErrorCode::ParseFailure));
        // Not the magic.
        let mut magic = good.clone();
        magic[0] = b'X';
        assert_eq!(code_of(&magic), Some(ErrorCode::ParseFailure));
        // Version 0, which nothing writes.
        let mut zero = good.clone();
        put_u32(&mut zero, 4, 0);
        assert_eq!(code_of(&zero), Some(ErrorCode::ParseFailure));
        // A byte short, and a byte over.
        assert_eq!(
            code_of(&good[..good.len() - 1]),
            Some(ErrorCode::ParseFailure)
        );
        let mut over = good.clone();
        over.push(0);
        assert_eq!(code_of(&over), Some(ErrorCode::ParseFailure));
        // A vertex count of 2^63, which a reader that allocated first would try
        // to: refused from the size alone.
        let mut huge = good.clone();
        for i in 0..8 {
            huge[8 + i] = if i == 7 { 0x80 } else { 0x00 };
        }
        assert_eq!(code_of(&huge), Some(ErrorCode::ParseFailure));
    }

    #[test]
    fn an_invalid_triangulation_is_refused_by_the_surfaces_own_checks() {
        let good = encode_tin(&square_with_masks(&plain_square()));
        // The first triangle's first index (just after the four vertices) made 9:
        // there are four vertices.
        let mut out_of_range = good.clone();
        put_u32(&mut out_of_range, 24 + 24 * 4, 9);
        assert_eq!(code_of(&out_of_range), Some(ErrorCode::InvalidArgument));

        // The first vertex's x made a NaN: a surface holds finite coordinates.
        let mut not_finite = good.clone();
        not_finite[24..32].copy_from_slice(&f64::NAN.to_bits().to_le_bytes());
        assert_eq!(code_of(&not_finite), Some(ErrorCode::InvalidArgument));

        // A mask with a bit beyond the three edges.
        let mut mask = good.clone();
        let last = mask.len() - 1;
        mask[last] = 0x08;
        assert_eq!(code_of(&mask), Some(ErrorCode::InvalidArgument));
    }
}
