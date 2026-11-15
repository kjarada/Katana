//! What an annotation draws, as plain model-space primitives
//! (include/katana/cad/annotation/drawing.hpp; docs/annotation.md,
//! "Drawing").
//!
//! Text in its styles, labels, leaders and callouts are laid out HERE, in the
//! cad layer, into this value; a painter only strokes, fills and sets the
//! pieces. So the layout - where a centred line of a readable, rotated,
//! masked, three-line note goes; where a label ended up after the placer; how
//! big a leader's arrow is at 1 : 500 - is decided once, tested headlessly,
//! and the same on the screen, on a plot and on every sheet viewport, the way
//! dimension_draw already made the dimensions one definition.
//!
//! Everything is in MODEL units, worked out for one annotation scale: the
//! paper sizes have already been multiplied by scale / 1000.

use std::sync::Arc;

use katana_entity::Color;
use katana_entity::text_block::estimated_width;
use katana_geometry::{Box2, Point2, Vec2};
use katana_math::numerics::TWO_PI;

/// The face a run of text is set in: what a painter makes its font from and
/// what a TextMeasure measures.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextFace {
    /// Empty: the painter's plain-text face.
    pub family: String,
    pub bold: bool,
    pub italic: bool,
}

/// One line of text, placed.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    pub text: String,
    /// The left end of the line's baseline, before the oblique slant.
    pub origin: Point2,
    /// The text height in model units: the em size a painter sets the font
    /// at, as the plain text of a TextGeometry always was.
    pub height: f64,
    /// Radians, counter-clockwise.
    pub rotation: f64,
    /// Horizontal stretch of the face (1: as designed) and the slant of the
    /// verticals (radians, positive leaning right).
    pub width_factor: f64,
    pub oblique: f64,
    pub face: TextFace,
}

impl Default for TextRun {
    fn default() -> Self {
        Self {
            text: String::new(),
            origin: Point2::default(),
            height: 0.0,
            rotation: 0.0,
            width_factor: 1.0,
            oblique: 0.0,
            face: TextFace::default(),
        }
    }
}

/// How wide `line` is when set in `face` at height 1, before the width
/// factor: the painters measure with their fonts, the tests and the entity
/// layer estimate (APPROXIMATE_GLYPH_ASPECT per character). A layout is only
/// as exact as its measure; with the painter's, a centred line is centred on
/// the sheet to the font's own advance widths. Shared and cloned as the C++
/// std::function is copied: a layout's options carry one.
pub type TextMeasure = Arc<dyn Fn(&str, &TextFace) -> f64 + Send + Sync>;

/// The estimate: 0.6 per character, whatever the face.
pub fn estimated_measure() -> TextMeasure {
    Arc::new(|line: &str, _face: &TextFace| estimated_width(line))
}

/// The segments circle_polygon is chorded into when the caller has no reason
/// to choose: the C++ default argument.
pub const DEFAULT_CIRCLE_SEGMENTS: i32 = 48;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drawing {
    /// Filled with the medium's background before anything else is drawn:
    /// text masks, and the inside of a callout's frame.
    pub masks: Vec<Vec<Point2>>,
    /// Open polylines in the annotation's pen: leader lines, ticks, markers.
    pub strokes: Vec<Vec<Point2>>,
    /// Closed outlines in the pen: callout boxes and circles (chorded).
    pub outlines: Vec<Vec<Point2>>,
    /// Filled with the pen's colour: closed arrowheads, dots.
    pub fills: Vec<Vec<Point2>>,
    pub texts: Vec<TextRun>,
    /// A colour the annotation's style gives it, overriding the entity's
    /// resolved (ByLayer) pen colour; None keeps the entity's.
    pub colour: Option<Color>,
    /// Everything above.
    pub extent: Box2,
    /// The rotated rectangle each laid-out text block occupies (before any
    /// mask margin): what the label placer tests for overlap and what
    /// update_extent counts for text. Four corners, counter-clockwise.
    pub text_boxes: Vec<Vec<Point2>>,
}

impl Drawing {
    pub fn empty(&self) -> bool {
        self.masks.is_empty()
            && self.strokes.is_empty()
            && self.outlines.is_empty()
            && self.fills.is_empty()
            && self.texts.is_empty()
    }

    /// Recomputes `extent` from the pieces; text by its measured block, which
    /// the layout records as a mask-sized outline of its own (text_boxes).
    pub fn update_extent(&mut self) {
        let mut extent = Box2::default();
        for lists in [
            &self.masks,
            &self.strokes,
            &self.outlines,
            &self.fills,
            &self.text_boxes,
        ] {
            for list in lists {
                for &point in list {
                    extent.expand(point);
                }
            }
        }
        self.extent = extent;
    }

    /// Appends another drawing's pieces.
    pub fn append(&mut self, other: &Drawing) {
        self.masks.extend_from_slice(&other.masks);
        self.strokes.extend_from_slice(&other.strokes);
        self.outlines.extend_from_slice(&other.outlines);
        self.fills.extend_from_slice(&other.fills);
        self.texts.extend_from_slice(&other.texts);
        self.text_boxes.extend_from_slice(&other.text_boxes);
        if !other.extent.empty() {
            self.extent.expand(other.extent.min);
            self.extent.expand(other.extent.max);
        }
    }
}

/// A circle as a closed polyline of `segments` chords, at least 8
/// (DEFAULT_CIRCLE_SEGMENTS is the C++ default).
pub fn circle_polygon(centre: Point2, radius: f64, segments: i32) -> Vec<Point2> {
    let count = std::cmp::max(segments, 8);
    let mut points = Vec::with_capacity(count as usize);
    for i in 0..count {
        let angle = TWO_PI * f64::from(i) / f64::from(count);
        points.push(centre + Vec2::new(angle.cos(), angle.sin()) * radius);
    }
    points
}
