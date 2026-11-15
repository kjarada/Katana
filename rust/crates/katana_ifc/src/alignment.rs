//! Alignments -> IfcAlignment, business logic and geometry both.
//!
//! IFC 4.3 describes an alignment twice, and a reader may use either: the
//! BUSINESS LOGIC - IfcAlignmentHorizontal and IfcAlignmentVertical nesting
//! IfcAlignmentSegment, each carrying the design parameters a designer
//! states (start point and direction, radii, length; start distance, height
//! and grades) - and the GEOMETRY - an IfcCompositeCurve of IfcCurveSegment,
//! and over it an IfcGradientCurve. Katana holds neither: it holds PIs and
//! PVIs and solves them (geometry's alignment and profile). Both descriptions
//! are written from the SOLVED elements, one segment per element, so the two
//! agree segment for segment (ALB022, ALB023) and both agree with what Katana
//! draws.
//!
//! The encodings of each segment type follow IfcOpenShell's alignment API
//! (ifcopenshell.api.alignment, 0.8), the reference implementation the
//! buildingSMART validation service evaluates geometry with:
//!
//! ```text
//! LINE          IfcLine through the origin along +x, from 0 for L
//! CIRCULARARC   IfcCircle of |R| at the origin, from 0 for L signed by
//!               the turn (negative: clockwise)
//! CLOTHOID      IfcClothoid with A = sign(dk) sqrt(L / |dk|), from
//!               k0 / (dk / L) for L - the length at which the parent
//!               spiral's curvature k(s) = s dk / L is the segment's start
//!               curvature k0
//! CONSTANTGRADIENT  IfcLine, for the length along the grade
//! PARABOLICARC  IfcPolynomialCurve y = A + B x + C x^2 with B the start
//!               grade and C = (g2 - g1) / 2L, for its arc length
//! ```
//!
//! each positioned by an IfcAxis2Placement2D at the segment's start point in
//! the segment's start direction. Radii and curvatures are positive turning
//! left, as in Katana (spiral2) and in IFC.
//!
//! Each layout, and each curve, ends with a zero-length segment (ALB015,
//! ALS015) at the alignment's end, in its end direction.

use std::cmp::Ordering;

use katana_entity::model::Model;
use katana_entity::tables::Alignment;
use katana_geometry::alignment::{
    AlignmentElement, AlignmentElementKind, AlignmentShape, SolvedAlignment, kind_of,
    solve_alignment,
};
use katana_geometry::profile::{ProfileElementKind, SolvedProfile, solve_profile};
use katana_math::cpp;
use katana_math::libm::asinh;
use katana_math::numerics::{HALF_PI, TWO_PI, tolerance};
use katana_math::{Vec2, Vec3};

use crate::builder::{Builder, PropertyList};
use crate::classification::IfcClass;
use crate::step::step_real;
use crate::step_file::{Args, Id};
use crate::support::printf_fixed;

/// Two directions or two curvatures closer than this are the same for the
/// transition code. The solver closes an element to about 1e-12 rad; this is
/// a thousand times that, and still far below anything a joint could be
/// designed to.
const SAME_DIRECTION: f64 = 1e-9;
const SAME_CURVATURE: f64 = 1e-9;

#[derive(Clone, Copy, Debug)]
struct Piece {
    kind: AlignmentElementKind,
    start: Vec2,
    direction: f64,
    length: f64,
    /// signed, positive turning left
    start_curvature: f64,
    end_curvature: f64,
    end: Vec2,
    end_direction: f64,
    pi: usize,
}

impl Default for Piece {
    fn default() -> Self {
        Self {
            kind: AlignmentElementKind::Tangent,
            start: Vec2::default(),
            direction: 0.0,
            length: 0.0,
            start_curvature: 0.0,
            end_curvature: 0.0,
            end: Vec2::default(),
            end_direction: 0.0,
            pi: 0,
        }
    }
}

fn piece_of(element: &AlignmentElement) -> Piece {
    let mut piece = Piece {
        kind: kind_of(element),
        length: element.length,
        pi: element.pi,
        ..Piece::default()
    };
    match &element.shape {
        AlignmentShape::Segment(line) => {
            piece.start = line.start;
            piece.end = line.end;
            piece.direction = line.delta().y.atan2(line.delta().x);
            piece.end_direction = piece.direction;
        }
        AlignmentShape::Arc(arc) => {
            let turn = if arc.sweep >= 0.0 { 1.0 } else { -1.0 };
            piece.start = arc.start_point();
            piece.end = arc.end_point();
            piece.direction = arc.start_angle + turn * HALF_PI;
            piece.end_direction = arc.end_angle() + turn * HALF_PI;
            piece.start_curvature = turn / arc.radius;
            piece.end_curvature = piece.start_curvature;
        }
        AlignmentShape::Spiral(spiral) => {
            piece.start = spiral.start;
            piece.end = spiral.end_point();
            piece.direction = spiral.start_direction;
            piece.end_direction = spiral.end_direction();
            piece.start_curvature = spiral.start_curvature;
            piece.end_curvature = spiral.end_curvature;
        }
    }
    piece
}

/// The difference of two directions, folded into (-pi, pi].
fn angle_between(a: f64, b: f64) -> f64 {
    cpp::remainder(a - b, TWO_PI).abs()
}

/// What one piece's end has in common with the next's start.
fn transition(
    end_direction: f64,
    end_curvature: f64,
    next_direction: f64,
    next_curvature: f64,
) -> &'static str {
    if angle_between(end_direction, next_direction) > SAME_DIRECTION {
        return "CONTINUOUS"; // a kink: a PI with no curve
    }
    if (end_curvature - next_curvature).abs() > SAME_CURVATURE * 1.0_f64.max(end_curvature.abs()) {
        return "CONTSAMEGRADIENT";
    }
    "CONTSAMEGRADIENTSAMECURVATURE"
}

fn horizontal_type(kind: AlignmentElementKind) -> &'static str {
    match kind {
        AlignmentElementKind::Tangent => "LINE",
        AlignmentElementKind::Spiral => "CLOTHOID",
        AlignmentElementKind::Arc => "CIRCULARARC",
    }
}

/// The name surveyors give the point where one kind of element meets
/// another: TS, SC, CS, ST at a spiral; PC and PT at a simple curve; PCC
/// between two arcs; PI at a kink.
fn horizontal_tag(
    before: Option<AlignmentElementKind>,
    after: Option<AlignmentElementKind>,
) -> String {
    let Some(before) = before else {
        return String::from("Start");
    };
    let Some(after) = after else {
        return String::from("End");
    };
    let code = |kind: AlignmentElementKind| match kind {
        AlignmentElementKind::Tangent => 'T',
        AlignmentElementKind::Spiral => 'S',
        AlignmentElementKind::Arc => 'C',
    };
    let pair: String = [code(before), code(after)].iter().collect();
    match pair.as_str() {
        "TS" | "SC" | "CS" | "ST" | "SS" => pair,
        "TC" => String::from("PC"),
        "CT" => String::from("PT"),
        "CC" => String::from("PCC"),
        _ => String::from("PI"),
    }
}

fn piece_name(piece: &Piece) -> String {
    let pi = piece.pi;
    match piece.kind {
        AlignmentElementKind::Tangent => format!("Tangent to PI {pi}"),
        AlignmentElementKind::Spiral => format!("Spiral at PI {pi}"),
        AlignmentElementKind::Arc => format!("Arc at PI {pi}"),
    }
}

/// `snprintf(buffer, size, "%.3f", station)`.
fn station_text(station: f64) -> String {
    printf_fixed(station, 3)
}

/// The length of y = A + B x + C x^2 over 0..L, in closed form: with
/// u = B + 2 C x, the integral of sqrt(1 + u^2) du / 2C is
/// (u sqrt(1 + u^2) + asinh u) / 4C. A grade change too small to divide by
/// is a straight line.
fn parabola_length(length: f64, g1: f64, g2: f64) -> f64 {
    if (g2 - g1).abs() < 1e-9 {
        return length * (1.0 + g1 * g1).sqrt();
    }
    let f = |u: f64| u * (1.0 + u * u).sqrt() + asinh(u);
    length * (f(g2) - f(g1)) / (2.0 * (g2 - g1))
}

/// The curvature of the profile curve in the (distance, height) plane, which
/// is what an IfcGradientCurve's segments join in.
fn profile_curvature(grade: f64, grade_rate: f64) -> f64 {
    grade_rate / (1.0 + grade * grade).powf(1.5)
}

struct AlignmentWriter<'b, 'a> {
    b: &'b mut Builder<'a>,
    alignment: &'b Alignment,
    key: String,
    placement: Id,
    composite: Id,
}

impl<'b, 'a> AlignmentWriter<'b, 'a> {
    fn new(builder: &'b mut Builder<'a>, alignment: &'b Alignment) -> Self {
        Self {
            b: builder,
            alignment,
            key: format!("alignment/{}", alignment.name),
            placement: 0,
            composite: 0,
        }
    }

    fn write(&mut self, horizontal: &SolvedAlignment, profile: Option<&SolvedProfile>) {
        let identity = self.b.identity3();
        self.placement = self
            .b
            .file()
            .add("IfcLocalPlacement", Args::new().null().reference(identity));

        // ---- horizontal ------------------------------------------------------
        let pieces: Vec<Piece> = horizontal.elements().iter().map(piece_of).collect();
        let mut curve_segments = Vec::new();
        let mut segments = Vec::new();
        for (i, piece) in pieces.iter().enumerate() {
            let before = if i == 0 {
                None
            } else {
                Some(pieces[i - 1].kind)
            };
            let after = pieces.get(i + 1).map(|next| next.kind);
            let code = match pieces.get(i + 1) {
                Some(next) => transition(
                    piece.end_direction,
                    piece.end_curvature,
                    next.direction,
                    next.start_curvature,
                ),
                None => transition(
                    piece.end_direction,
                    piece.end_curvature,
                    piece.end_direction,
                    0.0,
                ),
            };
            let curve_segment = self.horizontal_curve_segment(piece, code);
            curve_segments.push(curve_segment);
            let start_tag = horizontal_tag(before, Some(piece.kind));
            let end_tag = horizontal_tag(Some(piece.kind), after);
            let segment = self.horizontal_segment(
                piece,
                &start_tag,
                &end_tag,
                curve_segment,
                &piece_name(piece),
                i,
            );
            segments.push(segment);
        }
        // The zero-length segment that closes the layout, at the end in the
        // end direction. (solve_alignment answers at least one element.)
        if let Some(final_piece) = pieces.last() {
            let last = Piece {
                start: final_piece.end,
                direction: final_piece.end_direction,
                end: final_piece.end,
                end_direction: final_piece.end_direction,
                ..Piece::default()
            };
            let curve_segment = self.horizontal_curve_segment(&last, "DISCONTINUOUS");
            curve_segments.push(curve_segment);
            let segment =
                self.horizontal_segment(&last, "End", "End", curve_segment, "End", pieces.len());
            segments.push(segment);
        }
        self.composite = self.b.file().add(
            "IfcCompositeCurve",
            Args::new().refs(&curve_segments).boolean(false),
        );

        // ---- vertical ----------------------------------------------------------
        let mut vertical_segments = Vec::new();
        let mut gradient: Id = 0;
        if let Some(profile) = profile {
            let mut gradient_segments = Vec::new();
            self.write_profile(
                horizontal,
                profile,
                &mut gradient_segments,
                &mut vertical_segments,
            );
            gradient = self.b.file().add(
                "IfcGradientCurve",
                Args::new()
                    .refs(&gradient_segments)
                    .boolean(false)
                    .reference(self.composite)
                    .null(),
            );
        }

        // ---- the alignment ---------------------------------------------------------
        let mut representations = Vec::new();
        let axis_context = self.b.axis_context();
        if gradient == 0 {
            representations.push(
                self.b
                    .shape(axis_context, "Axis", "Curve2D", &[self.composite]),
            );
        } else {
            representations.push(self.b.shape(
                axis_context,
                "FootPrint",
                "Curve2D",
                &[self.composite],
            ));
            representations.push(self.b.shape(axis_context, "Axis", "Curve3D", &[gradient]));
        }
        let shape = self.b.product_shape(&representations);
        let guid = self.b.guid(&self.key);
        let ifc_alignment = self.b.file().add(
            "IfcAlignment",
            Args::new()
                .string(&guid)
                .null()
                .string(&self.alignment.name)
                .string_or_null(&self.alignment.description)
                .null()
                .reference(self.placement)
                .reference(shape)
                .null(),
        );
        *self
            .b
            .report()
            .classes
            .entry(String::from("IfcAlignment"))
            .or_insert(0) += 1;
        self.b.tally(
            &format!("alignment {}", self.alignment.name),
            &IfcClass::new("IfcAlignment", "", ""),
            "",
            if gradient != 0 {
                "its PIs and its profile's PVIs"
            } else {
                "its PIs"
            },
        );
        let horizontal_guid = self.b.guid(&format!("{}/horizontal", self.key));
        let layout_h = self.b.file().add(
            "IfcAlignmentHorizontal",
            Args::new()
                .string(&horizontal_guid)
                .null()
                .null()
                .null()
                .null()
                .null()
                .null(),
        );
        let mut layouts = vec![layout_h];
        let mut layout_v: Id = 0;
        if gradient != 0 {
            let vertical_guid = self.b.guid(&format!("{}/vertical", self.key));
            layout_v = self.b.file().add(
                "IfcAlignmentVertical",
                Args::new()
                    .string(&vertical_guid)
                    .null()
                    .null()
                    .null()
                    .null()
                    .null()
                    .null(),
            );
            layouts.push(layout_v);
        }
        self.nest(&format!("{}/layouts", self.key), ifc_alignment, &layouts);
        self.nest(
            &format!("{}/horizontal/segments", self.key),
            layout_h,
            &segments,
        );
        if layout_v != 0 {
            self.nest(
                &format!("{}/vertical/segments", self.key),
                layout_v,
                &vertical_segments,
            );
        }
        self.write_referents(ifc_alignment, horizontal, profile);
        self.write_properties(ifc_alignment, horizontal, profile);
        self.b.aggregate_in_project(ifc_alignment);
        self.b.report().alignments += 1;
    }

    fn horizontal_curve_segment(&mut self, piece: &Piece, code: &str) -> Id {
        let mut start = 0.0;
        let mut length = piece.length;
        let parent = match piece.kind {
            AlignmentElementKind::Tangent => self.b.unit_line(),
            AlignmentElementKind::Arc => {
                let radius = 1.0 / piece.start_curvature.abs();
                let origin = self.b.origin2();
                let position = self.b.placement2(origin, 0.0);
                length = if piece.start_curvature >= 0.0 {
                    piece.length
                } else {
                    -piece.length
                };
                self.b
                    .file()
                    .add("IfcCircle", Args::new().reference(position).real(radius))
            }
            AlignmentElementKind::Spiral => {
                let change = piece.end_curvature - piece.start_curvature;
                let constant = (piece.length / change.abs()).sqrt().copysign(change);
                let origin = self.b.origin2();
                let position = self.b.placement2(origin, 0.0);
                start = piece.start_curvature * piece.length / change;
                self.b.file().add(
                    "IfcClothoid",
                    Args::new().reference(position).real(constant),
                )
            }
        };
        let local = self.b.local(piece.start);
        let location = self.b.local_point(local);
        let placement = self.b.placement2(location, piece.direction);
        self.b.file().add(
            "IfcCurveSegment",
            Args::new()
                .enumeration(code)
                .reference(placement)
                .typed("IfcLengthMeasure", &step_real(start))
                .typed("IfcLengthMeasure", &step_real(length))
                .reference(parent),
        )
    }

    fn horizontal_segment(
        &mut self,
        piece: &Piece,
        start_tag: &str,
        end_tag: &str,
        curve_segment: Id,
        name: &str,
        index: usize,
    ) -> Id {
        let radius = |curvature: f64| {
            if curvature == 0.0 {
                0.0
            } else {
                1.0 / curvature
            }
        };
        let local = self.b.local(piece.start);
        let start = self.b.local_point(local);
        let design = self.b.file().add(
            "IfcAlignmentHorizontalSegment",
            Args::new()
                .string(start_tag)
                .string(end_tag)
                .reference(start)
                .real(piece.direction)
                .real(radius(piece.start_curvature))
                .real(radius(piece.end_curvature))
                .real(piece.length)
                .null()
                .enumeration(horizontal_type(piece.kind)),
        );
        self.alignment_segment(
            &format!("{}/horizontal/{index}", self.key),
            name,
            curve_segment,
            design,
        )
    }

    fn alignment_segment(&mut self, key: &str, name: &str, curve_segment: Id, design: Id) -> Id {
        // Its own representation, the one curve segment it is (ALS004),
        // placed as the alignment is.
        let axis_context = self.b.axis_context();
        let representation = self
            .b
            .shape(axis_context, "Axis", "Segment", &[curve_segment]);
        let shape = self.b.product_shape(&[representation]);
        let guid = self.b.guid(key);
        self.b.file().add(
            "IfcAlignmentSegment",
            Args::new()
                .string(&guid)
                .null()
                .string(name)
                .null()
                .null()
                .reference(self.placement)
                .reference(shape)
                .reference(design),
        )
    }

    fn write_profile(
        &mut self,
        horizontal: &SolvedAlignment,
        profile: &SolvedProfile,
        gradient_segments: &mut Vec<Id>,
        segments: &mut Vec<Id>,
    ) {
        let elements = profile.elements();
        let origin_height = self.b.local_3d(Vec3::new(0.0, 0.0, 0.0)).z;
        for (i, element) in elements.iter().enumerate() {
            let curve = element.kind == ProfileElementKind::Curve;
            let along = element.start_station - horizontal.start_station();
            let height = element.start_elevation + origin_height;
            let rate = if curve {
                (element.end_grade - element.start_grade) / element.length
            } else {
                0.0
            };

            let code = match elements.get(i + 1) {
                Some(next) => {
                    let next_rate = if next.kind == ProfileElementKind::Curve {
                        (next.end_grade - next.start_grade) / next.length
                    } else {
                        0.0
                    };
                    transition(
                        element.end_grade.atan(),
                        profile_curvature(element.end_grade, 2.0 * rate / 2.0),
                        next.start_grade.atan(),
                        profile_curvature(next.start_grade, next_rate),
                    )
                }
                None => transition(
                    element.end_grade.atan(),
                    profile_curvature(element.end_grade, rate),
                    element.end_grade.atan(),
                    0.0,
                ),
            };
            let gradient_segment = self.vertical_curve_segment(
                along,
                height,
                element.start_grade,
                element.end_grade,
                element.length,
                curve,
                code,
            );
            gradient_segments.push(gradient_segment);

            let start_tag = if i == 0 {
                "Start"
            } else if curve {
                "PVC"
            } else if elements[i - 1].kind == ProfileElementKind::Curve {
                "PVT"
            } else {
                "PVI"
            };
            let end_tag = match elements.get(i + 1) {
                None => "End",
                Some(_) if curve => "PVT",
                Some(next) if next.kind == ProfileElementKind::Curve => "PVC",
                Some(_) => "PVI",
            };
            let mut design = Args::new();
            design
                .string(start_tag)
                .string(end_tag)
                .real(along)
                .real(element.length)
                .real(height)
                .real(element.start_grade)
                .real(element.end_grade);
            if curve {
                // Signed: negative on a crest, where the grade falls (ALB012).
                design
                    .real(element.length / (element.end_grade - element.start_grade))
                    .enumeration("PARABOLICARC");
            } else {
                design.null().enumeration("CONSTANTGRADIENT");
            }
            let parameters = self.b.file().add("IfcAlignmentVerticalSegment", &design);
            let name = if curve {
                format!("Vertical curve at PVI {}", element.pvi)
            } else {
                format!("Grade to PVI {}", element.pvi)
            };
            let segment = self.alignment_segment(
                &format!("{}/vertical/{i}", self.key),
                &name,
                gradient_segment,
                parameters,
            );
            segments.push(segment);
        }
        // The closing zero-length segment, at the profile's end on its last
        // grade. (solve_profile answers at least one element.)
        let Some(last) = elements.last() else {
            return;
        };
        let along = last.start_station + last.length - horizontal.start_station();
        let height = last.end_elevation() + origin_height;
        let gradient_segment = self.vertical_curve_segment(
            along,
            height,
            last.end_grade,
            last.end_grade,
            0.0,
            false,
            "DISCONTINUOUS",
        );
        gradient_segments.push(gradient_segment);
        let parameters = self.b.file().add(
            "IfcAlignmentVerticalSegment",
            Args::new()
                .string("End")
                .string("End")
                .real(along)
                .real(0.0)
                .real(height)
                .real(last.end_grade)
                .real(last.end_grade)
                .null()
                .enumeration("CONSTANTGRADIENT"),
        );
        let segment = self.alignment_segment(
            &format!("{}/vertical/{}", self.key, elements.len()),
            "End",
            gradient_segment,
            parameters,
        );
        segments.push(segment);
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "The parameters of the C++'s verticalCurveSegment, in its order."
    )]
    fn vertical_curve_segment(
        &mut self,
        along: f64,
        height: f64,
        g1: f64,
        g2: f64,
        length: f64,
        curve: bool,
        code: &str,
    ) -> Id {
        let (parent, curve_length) = if curve {
            // y = A + B x + C x^2, A the start height as IfcOpenShell writes
            // it (the placement moves the curve's start there in any case).
            let c = (g2 - g1) / (2.0 * length);
            let origin = self.b.origin2();
            let position = self.b.placement2(origin, 0.0);
            let parent = self.b.file().add(
                "IfcPolynomialCurve",
                Args::new()
                    .reference(position)
                    .reals(&[0.0, 1.0])
                    .reals(&[height, g1, c])
                    .null(),
            );
            (parent, parabola_length(length, g1, g2))
        } else {
            (self.b.unit_line(), length * (1.0 + g1 * g1).sqrt())
        };
        let location = self.b.local_point(Vec2::new(along, height));
        let placement = self.b.placement2(location, g1.atan());
        self.b.file().add(
            "IfcCurveSegment",
            Args::new()
                .enumeration(code)
                .reference(placement)
                .typed("IfcLengthMeasure", &step_real(0.0))
                .typed("IfcLengthMeasure", &step_real(curve_length))
                .reference(parent),
        )
    }

    fn nest(&mut self, key: &str, parent: Id, children: &[Id]) {
        let guid = self.b.guid(&format!("nests/{key}"));
        self.b.file().add(
            "IfcRelNests",
            Args::new()
                .string(&guid)
                .null()
                .null()
                .null()
                .reference(parent)
                .refs(children),
        );
    }

    /// A station referent at the start, at every key station of the
    /// horizontal and the profile, and at the end: where an alignment is
    /// labelled and set out from. Each is placed by its distance along the
    /// horizontal curve, with the profile's level as a vertical offset where
    /// there is one, and carries its station in Pset_Stationing.
    fn write_referents(
        &mut self,
        ifc_alignment: Id,
        horizontal: &SolvedAlignment,
        profile: Option<&SolvedProfile>,
    ) {
        let mut stations: Vec<(f64, String)> = Vec::new();
        let elements = horizontal.elements();
        for (i, element) in elements.iter().enumerate() {
            if i == 0 {
                stations.push((element.start_station, String::from("Start")));
            } else {
                stations.push((
                    element.start_station,
                    horizontal_tag(Some(kind_of(&elements[i - 1])), Some(kind_of(element))),
                ));
            }
        }
        stations.push((horizontal.end_station(), String::from("End")));
        if let Some(profile) = profile {
            let profile_elements = profile.elements();
            for (i, element) in profile_elements.iter().enumerate().skip(1) {
                let curve = element.kind == ProfileElementKind::Curve;
                let after_curve = profile_elements[i - 1].kind == ProfileElementKind::Curve;
                let tag = if curve {
                    "PVC"
                } else if after_curve {
                    "PVT"
                } else {
                    "PVI"
                };
                stations.push((element.start_station, String::from(tag)));
            }
        }
        // std::stable_sort by `a.first < b.first`: ties keep their order.
        stations.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));

        let mut referents = Vec::new();
        let mut previous = f64::NEG_INFINITY;
        let origin_height = self.b.local_3d(Vec3::new(0.0, 0.0, 0.0)).z;
        for (station, tag) in &stations {
            let station = *station;
            if station - previous < tolerance::COORDINATE || !horizontal.contains_station(station) {
                continue;
            }
            previous = station;
            let along = station - horizontal.start_station();
            let at = horizontal
                .point_at_station(station)
                .expect("a station the alignment contains has a point");
            let direction = horizontal
                .direction_at_station(station)
                .expect("a station the alignment contains has a direction");
            let level = match profile {
                Some(profile) if profile.contains_station(station) => Some(
                    profile
                        .elevation_at(station)
                        .expect("a station the profile contains has a level")
                        + origin_height,
                ),
                _ => None,
            };
            let mut expression = Args::new();
            expression
                .typed("IfcLengthMeasure", &step_real(along))
                .null();
            match level {
                Some(level) => expression.real(level),
                None => expression.null(),
            };
            expression.null().reference(self.composite);
            let distance = self
                .b
                .file()
                .add("IfcPointByDistanceExpression", &expression);
            let relative = self.b.file().add(
                "IfcAxis2PlacementLinear",
                Args::new().reference(distance).null().null(),
            );
            let local = self.b.local(at);
            let cartesian_point = self.b.file().add(
                "IfcCartesianPoint",
                Args::new().reals(&[local.x, local.y, level.unwrap_or(0.0)]),
            );
            let up = self.b.direction_3d(0.0, 0.0, 1.0);
            let ahead = self.b.direction_3d(direction.cos(), direction.sin(), 0.0);
            let cartesian = self.b.file().add(
                "IfcAxis2Placement3D",
                Args::new()
                    .reference(cartesian_point)
                    .reference(up)
                    .reference(ahead),
            );
            let placement = self.b.file().add(
                "IfcLinearPlacement",
                Args::new().null().reference(relative).reference(cartesian),
            );
            let name = format!("{tag} {}", station_text(station));
            let guid = self
                .b
                .guid(&format!("{}/station/{}", self.key, station_text(station)));
            let referent = self.b.file().add(
                "IfcReferent",
                Args::new()
                    .string(&guid)
                    .null()
                    .string(&name)
                    .null()
                    .null()
                    .reference(placement)
                    .null()
                    .enumeration("STATION"),
            );
            *self
                .b
                .report()
                .classes
                .entry(String::from("IfcReferent"))
                .or_insert(0) += 1;
            self.b.tally(
                &format!("alignment {}", self.alignment.name),
                &IfcClass::new("IfcReferent", "STATION", ""),
                "",
                "a key station",
            );
            let mut stationing = PropertyList::new();
            stationing.length("Station", Some(station));
            let pset_key = format!("{}/station/{}", self.key, station_text(station));
            let set = self
                .b
                .property_set(&pset_key, "Pset_Stationing", &stationing);
            self.b.defines(&pset_key, set, &[referent]);
            referents.push(referent);
        }
        self.nest(
            &format!("{}/referents", self.key),
            ifc_alignment,
            &referents,
        );
    }

    fn write_properties(
        &mut self,
        ifc_alignment: Id,
        horizontal: &SolvedAlignment,
        profile: Option<&SolvedProfile>,
    ) {
        let mut list = PropertyList::new();
        list.length("StartStation", Some(horizontal.start_station()))
            .length("EndStation", Some(horizontal.end_station()))
            .positive_length("HorizontalLength", Some(horizontal.length()))
            .integer(
                "PointsOfIntersection",
                Some(self.alignment.horizontal.pis.len() as i64),
            );
        if let Some(profile) = profile {
            let pvis = self
                .alignment
                .vertical
                .as_ref()
                .map_or(0, |vertical| vertical.pvis.len());
            list.length("ProfileStartStation", Some(profile.start_station()))
                .length("ProfileEndStation", Some(profile.end_station()))
                .integer("PointsOfVerticalIntersection", Some(pvis as i64));
        }
        let set = self.b.property_set(&self.key, "Katana_Alignment", &list);
        self.b.defines(&self.key, set, &[ifc_alignment]);
    }
}

pub(crate) fn export_alignments(builder: &mut Builder<'_>, model: &Model) {
    model.alignments.for_each(|alignment| {
        let horizontal = match solve_alignment(&alignment.horizontal) {
            Ok(horizontal) => horizontal,
            Err(error) => {
                builder.warn(format!(
                    "alignment \"{}\" not written: {}",
                    alignment.name,
                    error.describe()
                ));
                return;
            }
        };
        let mut profile: Option<SolvedProfile> = None;
        if let Some(vertical) = &alignment.vertical {
            match solve_profile(vertical) {
                Err(error) => builder.warn(format!(
                    "the profile of alignment \"{}\" is not written: {}",
                    alignment.name,
                    error.describe()
                )),
                Ok(solved)
                    if solved.start_station()
                        < horizontal.start_station() - tolerance::COORDINATE
                        || solved.end_station()
                            > horizontal.end_station() + tolerance::COORDINATE =>
                {
                    // IFC measures a profile along its alignment; a part
                    // beyond the alignment has nowhere to be.
                    builder.warn(format!(
                        "the profile of alignment \"{}\" ({} to {}) runs beyond the alignment \
                         ({} to {}) and is not written",
                        alignment.name,
                        station_text(solved.start_station()),
                        station_text(solved.end_station()),
                        station_text(horizontal.start_station()),
                        station_text(horizontal.end_station())
                    ));
                }
                Ok(solved) => profile = Some(solved),
            }
        }
        AlignmentWriter::new(builder, alignment).write(&horizontal, profile.as_ref());
    });
}
