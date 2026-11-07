//! The twin of tests/entity/test_geometry_blob.cpp: the binary geometry
//! encoding (docs/storage.md).
//!
//! Two jobs. A round trip is EXACT - a saved drawing must reload as the same
//! drawing, not one that agrees to fifteen digits. And the decoder runs on
//! bytes from disk, which may be damaged or written by another version, so
//! every malformed input must produce an error rather than a crash or a
//! silently wrong shape. GeometryBlobWireFormat pins the bytes themselves:
//! the Rust writes and reads them byte for byte as the C++ does.

#![allow(
    clippy::approx_constant,
    reason = "Preserve the decimal constants copied verbatim from the C++ oracle."
)]

pub mod support;

use katana_core::error::ErrorCode;
use katana_entity::entity::{to_string, type_of};
use katana_entity::geometry_blob::{
    BLOB_VERSION, BLOB_VERSION_ANNOTATION, BLOB_VERSION_SMART_LEADER, geometry_from_blob,
    geometry_to_blob,
};
use katana_entity::serialization::{geometry_from_json, geometry_to_json};
use katana_entity::{
    AnchorPoint, AnchorRef, ArrowHead, CalloutShape, DimensionGeometry, Geometry, LabelGeometry,
    LeaderGeometry, PointGeometry, TextGeometry,
};
use katana_geometry::{
    Arc2, Circle2, CurvePolyline2, CurveVertex, Ellipse2, Point2, Polyline2, Segment2, Spline2,
    Vec2,
};
use support::double_eq;

fn p(x: f64, y: f64) -> Point2 {
    Point2::new(x, y)
}

fn round_trip(geometry: &Geometry) -> Geometry {
    let blob = geometry_to_blob(geometry).unwrap_or_else(|error| panic!("{}", error.describe()));
    geometry_from_blob(&blob).unwrap_or_else(|error| panic!("{}", error.describe()))
}

fn polyline_of(vertices: usize, closed: bool) -> Polyline2 {
    Polyline2::new(
        (0..vertices)
            .map(|i| p(i as f64 * 1.7, (i as f64).sin() * 3.0))
            .collect(),
        closed,
    )
}

fn sample_curve_polyline() -> CurvePolyline2 {
    CurvePolyline2 {
        vertices: vec![
            CurveVertex {
                position: p(0.0, 0.0),
                bulge: 0.5,
                height: Some(12.25),
            },
            CurveVertex {
                position: p(5.0, 1.0),
                bulge: -0.0,
                height: None,
            },
            CurveVertex {
                position: p(7.0, 4.0),
                bulge: -1.5,
                height: Some(-0.0),
            },
        ],
        closed: true,
    }
}

fn sample_ellipse() -> Ellipse2 {
    Ellipse2 {
        center: p(3.0, 4.0),
        major_axis: Vec2::new(6.0, -2.0),
        ratio: 0.35,
        start_parameter: -0.75,
        sweep: 1.25,
    }
}

fn sample_spline() -> Spline2 {
    Spline2::through_points(vec![p(0.0, 0.0), p(2.0, 3.0), p(5.0, 1.0), p(8.0, 4.0)], 3).unwrap()
}

/// Little-endian bytes written by hand from geometry_blob.rs's description, so
/// a layout cannot drift with the writer and the reader agreeing.
#[derive(Default)]
struct HandWritten {
    bytes: Vec<u8>,
}

impl HandWritten {
    fn u8(&mut self, value: u8) -> &mut Self {
        self.bytes.push(value);
        self
    }
    fn le(&mut self, value: u64, width: usize) -> &mut Self {
        for i in 0..width {
            self.bytes.push(((value >> (8 * i)) & 0xFF) as u8);
        }
        self
    }
    fn u32(&mut self, value: u32) -> &mut Self {
        self.le(u64::from(value), 4)
    }
    fn u64(&mut self, value: u64) -> &mut Self {
        self.le(value, 8)
    }
    fn f64(&mut self, value: f64) -> &mut Self {
        self.le(value.to_bits(), 8)
    }
    fn text(&mut self, value: &str) -> &mut Self {
        self.u32(value.len() as u32);
        for &byte in value.as_bytes() {
            self.u8(byte);
        }
        self
    }
}

mod geometry_blob {
    use super::*;

    #[test]
    fn every_geometry_kind_survives_a_round_trip_exactly() {
        let samples = [
            Geometry::Point(PointGeometry {
                position: p(1.5, -2.25),
            }),
            Geometry::Line(Segment2::new(p(0.0, 0.0), p(123.456, -789.012))),
            Geometry::Arc(Arc2::new(p(3.0, 4.0), 25.0, 0.5, -1.25)),
            Geometry::Polyline(polyline_of(9, false)),
            Geometry::Polyline(polyline_of(4, true)),
            Geometry::Circle(Circle2::new(p(-10.0, 10.0), 7.5)),
            Geometry::Text(TextGeometry::new(
                p(2.0, 3.0),
                "Lot 42",
                2.5,
                0.7853981633974483,
            )),
            Geometry::Dimension(DimensionGeometry::new(
                p(0.0, 0.0),
                p(10.0, 0.0),
                1.5,
                "10.00 m",
            )),
            Geometry::CurvePolyline(sample_curve_polyline()),
            Geometry::Ellipse(sample_ellipse()),
            Geometry::Spline(sample_spline()),
        ];
        for original in &samples {
            let back = round_trip(original);
            // == on these is member-wise and exact, which is the point.
            assert_eq!(&back, original, "kind {}", original.index());
            assert_eq!(back.index(), original.index());
        }
    }

    #[test]
    fn doubles_come_back_bit_for_bit() {
        // Stored by bit pattern rather than through any decimal form, so values
        // a text format loses or mangles survive.
        let awkward = [
            0.0,
            -0.0,
            1.0 / 3.0,
            f64::MIN_POSITIVE,
            f64::from_bits(1), // denorm_min
            f64::MAX,
            -f64::MAX,
            f64::EPSILON,
            std::f64::consts::PI,
            1.0e-300,
            1.0e300,
        ];
        for value in awkward {
            let original = Geometry::Line(Segment2::new(p(value, -value), p(value * 0.5, value)));
            let Geometry::Line(segment) = round_trip(&original) else {
                panic!("not a line")
            };
            assert_eq!(segment.start.x.to_bits(), value.to_bits(), "value {value}");
        }

        // Negative zero specifically: it compares equal to zero but is a
        // different bit pattern, so a test using == alone would not notice
        // losing it.
        let negative_zero = Geometry::Point(PointGeometry {
            position: p(-0.0, 0.0),
        });
        let Geometry::Point(point) = round_trip(&negative_zero) else {
            panic!("not a point")
        };
        assert!(
            point.position.x.is_sign_negative(),
            "negative zero lost its sign"
        );
        assert!(!point.position.y.is_sign_negative());
    }

    #[test]
    fn non_finite_values_survive_rather_than_becoming_something_else() {
        // The model rejects non-finite geometry on the way in; the ENCODING
        // must not be the thing that quietly turns an infinity into a large
        // number.
        let original = Geometry::Line(Segment2::new(
            p(f64::INFINITY, f64::NEG_INFINITY),
            p(0.0, 0.0),
        ));
        let Geometry::Line(segment) = round_trip(&original) else {
            panic!("not a line")
        };
        assert!(segment.start.x.is_infinite());
        assert!(segment.start.x > 0.0);
        assert!(segment.start.y < 0.0);

        let with_nan = Geometry::Point(PointGeometry {
            position: p(f64::NAN, 1.0),
        });
        let Geometry::Point(point) = round_trip(&with_nan) else {
            panic!("not a point")
        };
        assert!(point.position.x.is_nan());
    }

    #[test]
    fn an_empty_polyline_and_a_single_vertex_one_both_round_trip() {
        for count in [0, 1, 2] {
            for closed in [false, true] {
                let original = Geometry::Polyline(polyline_of(count, closed));
                assert_eq!(
                    round_trip(&original),
                    original,
                    "{count} vertices, closed {closed}"
                );
            }
        }
    }

    #[test]
    fn empty_strings_are_distinct_from_absent_ones() {
        // An empty text_override means "show the measured distance"; losing
        // the difference would change what the dimension displays.
        let blank = Geometry::Dimension(DimensionGeometry::new(p(0.0, 0.0), p(1.0, 0.0), 0.5, ""));
        let filled =
            Geometry::Dimension(DimensionGeometry::new(p(0.0, 0.0), p(1.0, 0.0), 0.5, "TYP"));
        assert_eq!(round_trip(&blank), blank);
        assert_eq!(round_trip(&filled), filled);
        assert_ne!(round_trip(&blank), round_trip(&filled));
    }

    #[test]
    fn multi_byte_text_survives() {
        let original = Geometry::Text(TextGeometry::new(
            p(0.0, 0.0),
            "Grundst\u{fc}ck 12 \u{2014} caf\u{e9}",
            3.0,
            0.0,
        ));
        assert_eq!(round_trip(&original), original);
    }

    // The C++ hands the WRITER CP1252 bytes and expects InvalidArgument; a
    // String cannot hold them. What the Rust has is the same rule at the
    // reader, which geometry_blob.hpp states for a string that is not valid
    // UTF-8: the same text stored as those bytes is refused, ParseFailure.
    #[test]
    fn text_that_is_not_valid_utf8_is_refused_rather_than_stored() {
        let mut stored = HandWritten::default();
        stored
            .u8(BLOB_VERSION)
            .u8(5)
            .f64(0.0)
            .f64(0.0)
            .f64(2.5)
            .f64(0.0)
            .u32(4);
        stored.u8(b'c').u8(b'a').u8(b'f').u8(0xE9); // CP1252, not UTF-8
        let blob = geometry_from_blob(&stored.bytes);
        assert_eq!(blob.unwrap_err().code, ErrorCode::ParseFailure);
    }

    // ---- hostile input ----------------------------------------------------------

    #[test]
    fn an_empty_or_truncated_blob_is_an_error_not_a_crash() {
        assert!(geometry_from_blob(&[]).is_err());

        let full = geometry_to_blob(&Geometry::Polyline(polyline_of(6, true))).unwrap();
        // Every proper prefix must be rejected. This is the loop that would
        // find an unchecked read.
        for length in 0..full.len() {
            let result = geometry_from_blob(&full[..length]);
            assert_eq!(
                result.unwrap_err().code,
                ErrorCode::ParseFailure,
                "prefix {length}"
            );
        }
        assert!(
            geometry_from_blob(&full).is_ok(),
            "the whole blob must still be fine"
        );
    }

    #[test]
    fn trailing_bytes_are_rejected_rather_than_ignored() {
        let mut blob = geometry_to_blob(&Geometry::Circle(Circle2::new(p(1.0, 2.0), 3.0))).unwrap();
        blob.push(0x00);
        let result = geometry_from_blob(&blob);
        assert_eq!(result.unwrap_err().code, ErrorCode::ParseFailure);
    }

    #[test]
    fn an_unknown_version_or_kind_is_reported_not_guessed_at() {
        let blob = geometry_to_blob(&Geometry::Circle(Circle2::new(p(1.0, 2.0), 3.0))).unwrap();

        let mut wrong_version = blob.clone();
        wrong_version[0] = 99;
        assert_eq!(
            geometry_from_blob(&wrong_version).unwrap_err().code,
            ErrorCode::ParseFailure
        );

        let mut wrong_kind = blob.clone();
        wrong_kind[1] = 77;
        assert_eq!(
            geometry_from_blob(&wrong_kind).unwrap_err().code,
            ErrorCode::ParseFailure
        );
    }

    #[test]
    fn an_implausible_length_is_rejected_before_anything_is_allocated() {
        // The classic decoder bug: a corrupt count field turned into a huge
        // allocation, or into a read past the end.
        let mut blob = geometry_to_blob(&Geometry::Polyline(polyline_of(3, false))).unwrap();
        // The vertex count sits right after the two header bytes.
        blob[2] = 0xFF;
        blob[3] = 0xFF;
        blob[4] = 0xFF;
        blob[5] = 0x7F;
        assert_eq!(
            geometry_from_blob(&blob).unwrap_err().code,
            ErrorCode::ParseFailure
        );
    }

    #[test]
    fn a_corrupt_string_length_is_rejected() {
        let mut blob = geometry_to_blob(&Geometry::Text(TextGeometry::new(
            p(0.0, 0.0),
            "abc",
            2.5,
            0.0,
        )))
        .unwrap();
        // The string length is the last 4 bytes before the text itself.
        let length_at = blob.len() - 3 - 4;
        blob[length_at] = 0xFF;
        blob[length_at + 1] = 0xFF;
        assert_eq!(
            geometry_from_blob(&blob).unwrap_err().code,
            ErrorCode::ParseFailure
        );
    }

    #[test]
    fn every_single_byte_corruption_is_either_rejected_or_decodes_to_something_valid() {
        // Fuzzing by hand: flip each byte of a real blob and require that the
        // decoder never crashes and never reads out of bounds. It is allowed to
        // succeed, but it must not do anything else.
        let original = geometry_to_blob(&Geometry::Polyline(polyline_of(5, true))).unwrap();
        let mut rejected = 0;
        for i in 0..original.len() {
            for pattern in [0x00_u8, 0xFF, 0x7F, 0x80] {
                let mut damaged = original.clone();
                damaged[i] = pattern;
                if let Err(error) = geometry_from_blob(&damaged) {
                    rejected += 1;
                    assert_eq!(error.code, ErrorCode::ParseFailure, "byte {i}");
                }
            }
        }
        // A run where nothing was ever rejected would mean the checks are not
        // reachable, i.e. the test proves nothing.
        assert!(rejected > 0);
    }

    // ---- against the JSON path it replaces --------------------------------------

    #[test]
    fn agrees_with_the_json_encoding_it_replaces() {
        // Both encodings must describe the same geometry, or a project written
        // by one build and read by another would differ.
        let samples = [
            Geometry::Point(PointGeometry {
                position: p(1.5, -2.25),
            }),
            Geometry::Line(Segment2::new(p(0.0, 0.0), p(123.456, -789.012))),
            Geometry::Arc(Arc2::new(p(3.0, 4.0), 25.0, 0.5, -1.25)),
            Geometry::Polyline(polyline_of(7, true)),
            Geometry::Circle(Circle2::new(p(-10.0, 10.0), 7.5)),
            Geometry::Text(TextGeometry::new(p(2.0, 3.0), "Lot 42", 2.5, 0.5)),
            Geometry::Dimension(DimensionGeometry::new(p(0.0, 0.0), p(10.0, 0.0), 1.5, "")),
            Geometry::CurvePolyline(sample_curve_polyline()),
            Geometry::Ellipse(sample_ellipse()),
            Geometry::Spline(sample_spline()),
        ];
        for original in &samples {
            let json = geometry_to_json(original).unwrap();
            let via_json =
                geometry_from_json(&json).unwrap_or_else(|error| panic!("{}", error.describe()));
            let via_blob = round_trip(original);
            assert_eq!(via_blob, via_json, "kind {}", original.index());
        }
    }

    #[test]
    fn is_substantially_smaller_than_the_json_it_replaces() {
        // Not the point of the change - the parse cost is - but a format that
        // was BIGGER would be a sign the layout is wrong.
        let polyline = Geometry::Polyline(polyline_of(8, false));
        let blob = geometry_to_blob(&polyline).unwrap();
        let json = geometry_to_json(&polyline).unwrap();
        assert!(
            blob.len() < json.len(),
            "blob {} json {}",
            blob.len(),
            json.len()
        );
        // 8 vertices = 128 bytes of doubles, plus a 7-byte header.
        assert_eq!(blob.len(), 2 + 4 + 1 + 8 * 16);
    }
}

mod geometry_blob_wire_format {
    use super::*;

    #[test]
    fn the_kind_byte_is_pinned_to_the_variant_order() {
        // THE test that makes the "append only" rule enforceable rather than a
        // comment. geometry_to_blob writes `geometry.index()` as the on-disk
        // kind byte, so reordering or inserting an alternative reinterprets
        // every project ever saved; two kinds of EQUAL payload size swap with
        // no complaint at all.
        let pinned: [(u8, &str, Geometry); 12] = [
            (
                0,
                "Point",
                Geometry::Point(PointGeometry {
                    position: p(1.0, 2.0),
                }),
            ),
            (
                1,
                "Line",
                Geometry::Line(Segment2::new(p(0.0, 0.0), p(1.0, 1.0))),
            ),
            (
                2,
                "Arc",
                Geometry::Arc(Arc2::new(p(0.0, 0.0), 1.0, 0.0, 1.0)),
            ),
            (3, "Polyline", Geometry::Polyline(polyline_of(3, false))),
            (
                4,
                "Circle",
                Geometry::Circle(Circle2::new(p(0.0, 0.0), 1.0)),
            ),
            (
                5,
                "Text",
                Geometry::Text(TextGeometry::new(p(0.0, 0.0), "x", 2.5, 0.0)),
            ),
            (
                6,
                "Dimension",
                Geometry::Dimension(DimensionGeometry::new(p(0.0, 0.0), p(1.0, 0.0), 0.0, "")),
            ),
            (
                7,
                "Label",
                Geometry::Label(LabelGeometry {
                    target: 1,
                    style: String::from("S"),
                    ..LabelGeometry::default()
                }),
            ),
            (
                8,
                "Leader",
                Geometry::Leader(LeaderGeometry {
                    vertices: vec![p(0.0, 0.0), p(1.0, 1.0)],
                    ..LeaderGeometry::default()
                }),
            ),
            // The drawing system's, appended after the annotation system's.
            (
                9,
                "CurvePolyline",
                Geometry::CurvePolyline(CurvePolyline2::from_points(
                    &[p(0.0, 0.0), p(1.0, 1.0)],
                    false,
                )),
            ),
            (10, "Ellipse", Geometry::Ellipse(Ellipse2::default())),
            (
                11,
                "Spline",
                Geometry::Spline(
                    Spline2::from_control_points(vec![p(0.0, 0.0), p(1.0, 1.0)], 1).unwrap(),
                ),
            ),
        ];
        assert_eq!(
            pinned.len(),
            katana_entity::entity::GEOMETRY_KIND_COUNT,
            "a geometry kind was added; append it here and confirm it went on the END"
        );

        for (kind, name, geometry) in &pinned {
            assert_eq!(
                geometry.index(),
                usize::from(*kind),
                "{name} moved in the variant"
            );
            assert_eq!(
                type_of(geometry) as u8,
                *kind,
                "{name}: EntityType and the variant have drifted apart"
            );
            assert_eq!(to_string(type_of(geometry)), *name);

            let blob = geometry_to_blob(geometry).unwrap();
            assert!(blob.len() >= 2, "{name}");
            // The kinds version 1 had are written in it; those added since
            // exist only in version 2.
            assert_eq!(
                blob[0],
                if *kind <= 6 {
                    BLOB_VERSION
                } else {
                    BLOB_VERSION_ANNOTATION
                },
                "{name}"
            );
            assert_eq!(
                blob[1], *kind,
                "{name}: the on-disk kind byte changed, which reinterprets every project already saved"
            );
        }
    }

    #[test]
    fn a_curve_polylines_layout_is_pinned() {
        // Written by hand from geometry_blob.rs's description: version 2, kind
        // 9, u32 count, u8 closed, then x, y, bulge and height a vertex, a NaN
        // height for none.
        let mut stored: Vec<u8> = vec![0x02, 0x09, 0x02, 0x00, 0x00, 0x00, 0x00];
        for value in [1.0, 2.0, 0.5, f64::NAN, 3.0, 4.0, 0.0, 7.25] {
            stored.extend_from_slice(&value.to_bits().to_le_bytes());
        }
        // The NaN is the canonical quiet one, 0x7FF8000000000000.
        assert_eq!(stored[7 + 3 * 8 + 7], 0x7F);
        assert_eq!(stored[7 + 3 * 8 + 6], 0xF8);

        let decoded =
            geometry_from_blob(&stored).unwrap_or_else(|error| panic!("{}", error.describe()));
        let Geometry::CurvePolyline(polyline) = &decoded else {
            panic!("kind 9 no longer means CurvePolyline")
        };
        assert_eq!(polyline.vertices.len(), 2);
        assert!(!polyline.closed);
        assert_eq!(polyline.vertices[0].position, p(1.0, 2.0));
        assert_eq!(polyline.vertices[0].bulge, 0.5);
        assert!(polyline.vertices[0].height.is_none());
        assert_eq!(polyline.vertices[1].height, Some(7.25));

        let written = geometry_to_blob(&decoded).unwrap();
        assert_eq!(written, stored, "the on-disk layout changed");

        // A drawing kind in a version-1 blob is refused, not guessed at.
        let mut old = stored.clone();
        old[0] = 0x01;
        assert!(geometry_from_blob(&old).is_err());
    }

    #[test]
    fn an_ellipses_layout_is_pinned() {
        // Version 2, kind 10: centre, major axis, ratio, start, sweep.
        let mut stored = HandWritten::default();
        stored
            .u8(0x02)
            .u8(0x0A)
            .f64(5.0)
            .f64(6.0)
            .f64(4.0)
            .f64(3.0)
            .f64(0.5)
            .f64(1.0)
            .f64(3.0);
        let ellipse = Ellipse2 {
            center: p(5.0, 6.0),
            major_axis: Vec2::new(4.0, 3.0),
            ratio: 0.5,
            start_parameter: 1.0,
            sweep: 3.0,
        };

        let decoded = geometry_from_blob(&stored.bytes)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(
            matches!(decoded, Geometry::Ellipse(_)),
            "kind 10 no longer means Ellipse"
        );
        assert_eq!(decoded, Geometry::Ellipse(ellipse));
        let written = geometry_to_blob(&Geometry::Ellipse(ellipse)).unwrap();
        assert_eq!(written, stored.bytes, "the on-disk layout changed");
    }

    #[test]
    fn a_splines_layout_is_pinned() {
        // Version 2, kind 11: u8 degree, then control points, knots, weights
        // and fit points, each list u32-counted. A rational one, so the weights
        // list is not empty and its place in the order is pinned.
        let spline = Spline2 {
            degree: 1,
            control_points: vec![p(0.0, 0.0), p(2.0, 1.0)],
            knots: vec![0.0, 0.0, 1.0, 1.0],
            weights: vec![1.0, 2.0],
            fit_points: vec![p(7.0, 8.0)],
        };
        let mut stored = HandWritten::default();
        stored.u8(0x02).u8(0x0B).u8(1);
        stored.u32(2).f64(0.0).f64(0.0).f64(2.0).f64(1.0);
        stored.u32(4).f64(0.0).f64(0.0).f64(1.0).f64(1.0);
        stored.u32(2).f64(1.0).f64(2.0);
        stored.u32(1).f64(7.0).f64(8.0);

        let decoded = geometry_from_blob(&stored.bytes)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(
            matches!(decoded, Geometry::Spline(_)),
            "kind 11 no longer means Spline"
        );
        assert_eq!(decoded, Geometry::Spline(spline.clone()));
        let written = geometry_to_blob(&Geometry::Spline(spline)).unwrap();
        assert_eq!(written, stored.bytes, "the on-disk layout changed");

        // A drawing kind in a version-1 blob is refused, not guessed at.
        let mut old = stored.bytes.clone();
        old[0] = 0x01;
        assert!(geometry_from_blob(&old).is_err());
    }

    #[test]
    fn a_smart_leaders_version_three_layout_is_pinned() {
        // Version 3, kind 8: the version-2 leader with the anchor's parameter
        // after its index, then the fields flag and the label style.
        let leader = LeaderGeometry {
            vertices: vec![p(1.0, 2.0), p(3.0, 4.0)],
            text: String::from("IL {prop.invert}"),
            arrow: ArrowHead::Open,
            callout: CalloutShape::Box,
            style: String::from("Notes"),
            paper_height: 2.0,
            arrow_size: 3.0,
            landing: 1.5,
            tip_ref: AnchorRef {
                entity: 42,
                point: AnchorPoint::Along,
                index: 3,
                parameter: 0.25,
            },
            fields: true,
            label_style: String::from("Pipe"),
            ..LeaderGeometry::default()
        };
        let mut stored = HandWritten::default();
        stored.u8(0x03).u8(0x08);
        stored.u32(2).f64(1.0).f64(2.0).f64(3.0).f64(4.0);
        stored.text("IL {prop.invert}").text("Notes");
        stored.u8(3).u8(1); // ArrowHead::Open, CalloutShape::Box
        stored.f64(2.0).f64(3.0).f64(1.5);
        stored.u64(42).u8(7).u32(3).f64(0.25); // entity, Along, index, parameter
        stored.u8(1).text("Pipe");

        let decoded = geometry_from_blob(&stored.bytes)
            .unwrap_or_else(|error| panic!("{}", error.describe()));
        assert!(
            matches!(decoded, Geometry::Leader(_)),
            "kind 8 no longer means Leader"
        );
        assert_eq!(decoded, Geometry::Leader(leader.clone()));
        let written = geometry_to_blob(&Geometry::Leader(leader)).unwrap();
        assert_eq!(written, stored.bytes, "the on-disk layout changed");
    }

    #[test]
    fn an_explicit_leader_landing_appends_its_angle_in_version_four_only() {
        let mut leader = LeaderGeometry {
            vertices: vec![p(1.0, 2.0), p(3.0, 4.0)],
            text: String::from("Note"),
            landing_angle: Some(1.25),
            ..LeaderGeometry::default()
        };
        let mut stored = HandWritten::default();
        stored
            .u8(4)
            .u8(8)
            .u32(2)
            .f64(1.0)
            .f64(2.0)
            .f64(3.0)
            .f64(4.0);
        stored.text("Note").text("").u8(2).u8(0);
        stored.f64(0.0).f64(2.5).f64(2.5);
        stored.u64(0).u8(0).u32(0).f64(0.0).u8(0).text("").f64(1.25);
        let blob = geometry_to_blob(&Geometry::Leader(leader.clone())).unwrap();
        assert_eq!(blob, stored.bytes);
        let back = geometry_from_blob(&stored.bytes).unwrap();
        assert_eq!(back, Geometry::Leader(leader.clone()));
        for length in blob.len() - 8..blob.len() {
            assert!(geometry_from_blob(&blob[..length]).is_err());
        }
        leader.landing_angle = None;
        let old = geometry_to_blob(&Geometry::Leader(leader)).unwrap();
        assert_eq!(old[0], BLOB_VERSION_ANNOTATION);
    }

    #[test]
    fn the_drawing_kinds_are_read_in_version_three_as_in_version_two() {
        // The writer never puts them in version 3 (they hold no anchor), but a
        // reader of version 3 takes every kind version 2 had.
        for geometry in [
            Geometry::CurvePolyline(CurvePolyline2::from_points(
                &[p(0.0, 0.0), p(1.0, 1.0)],
                false,
            )),
            Geometry::Ellipse(Ellipse2::default()),
            Geometry::Spline(
                Spline2::from_control_points(vec![p(0.0, 0.0), p(1.0, 1.0)], 1).unwrap(),
            ),
        ] {
            let mut blob = geometry_to_blob(&geometry).unwrap();
            assert_eq!(blob[0], BLOB_VERSION_ANNOTATION);
            blob[0] = BLOB_VERSION_SMART_LEADER;
            let decoded =
                geometry_from_blob(&blob).unwrap_or_else(|error| panic!("{}", error.describe()));
            assert_eq!(decoded, geometry);
        }
    }

    #[test]
    fn stored_bytes_from_a_previous_build_still_decode() {
        // A blob captured by hand rather than produced by the current writer,
        // so this fails if the LAYOUT changes even when the writer and reader
        // change together. Circle at centre (1.5, 2.5) radius 4.0: version 1,
        // kind 4, then three little-endian IEEE-754 doubles.
        let stored: Vec<u8> = vec![
            0x01, 0x04, //
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF8, 0x3F, // 1.5
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x40, // 2.5
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x40, // 4.0
        ];
        let decoded =
            geometry_from_blob(&stored).unwrap_or_else(|error| panic!("{}", error.describe()));
        let Geometry::Circle(circle) = decoded else {
            panic!("the kind byte no longer means Circle")
        };
        double_eq(circle.center.x, 1.5);
        double_eq(circle.center.y, 2.5);
        double_eq(circle.radius, 4.0);

        // And the current writer still produces exactly those bytes.
        let written = geometry_to_blob(&Geometry::Circle(Circle2::new(p(1.5, 2.5), 4.0))).unwrap();
        assert_eq!(written, stored, "the on-disk layout changed");
    }
}
