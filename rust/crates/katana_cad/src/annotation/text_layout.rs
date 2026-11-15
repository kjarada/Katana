//! Text in its style, at a scale: the one layout every text, label and leader
//! note goes through (include/katana/cad/annotation/text_layout.hpp;
//! docs/annotation.md, "Text").
//!
//! resolve_text_style works out what a text is set in at one annotation scale
//! (its model height from its own paper height, else its style's, else its
//! stored model height; the face, width factor, slant and colour; the mask),
//! and layout_text places its lines by the justification arithmetic of
//! entity's text_block with MEASURED widths, turns a text that would read
//! upside down (a readable style), and returns the runs, the block's
//! rectangle and the mask as a Drawing.

use katana_entity::annotation::{DEFAULT_TEXT_STYLE_NAME, annotation_model_size};
use katana_entity::text_block::{line_origin, text_block_extent, text_lines};
use katana_entity::{Color, Model, TextGeometry, TextJustify, TextStyle};
use katana_geometry::{Point2, Vec2};
use katana_math::cpp;
use katana_math::numerics::{PI, normalize_angle, tolerance};

use crate::annotation::drawing::{Drawing, TextFace, TextMeasure, TextRun};

/// What a text is set in, in model units for one scale.
#[derive(Clone, Debug, PartialEq)]
pub struct TextAppearance {
    /// Model units.
    pub height: f64,
    pub face: TextFace,
    pub width_factor: f64,
    pub oblique: f64,
    /// None: the entity's pen.
    pub colour: Option<Color>,
    pub mask: bool,
    /// Model units.
    pub mask_margin: f64,
    pub readable: bool,
    pub line_spacing: f64,
}

impl Default for TextAppearance {
    fn default() -> Self {
        Self {
            height: 2.5,
            face: TextFace::default(),
            width_factor: 1.0,
            oblique: 0.0,
            colour: None,
            mask: false,
            mask_margin: 0.0,
            readable: false,
            line_spacing: 1.0,
        }
    }
}

/// The text style `name` names, or "Standard" when it is empty or gone (a
/// renderer always has something to set text in), or None when the model has
/// no Standard either - which a model never lacks, but a caller that built
/// one by hand might.
pub fn find_text_style<'a>(model: &'a Model, name: &str) -> Option<&'a TextStyle> {
    if !name.is_empty()
        && let Some(style) = model.text_styles.find(name)
    {
        return Some(style);
    }
    model.text_styles.find(DEFAULT_TEXT_STYLE_NAME)
}

/// A paper height (mm), a style and a fallback model height resolved at
/// 1 : `scale`. The height is: `paper_height` when positive, else the style's
/// paper height when positive, else `model_height`. An empty style name is
/// no style at all - the plain face, never turned, no mask - which is what
/// every text written before styles existed is.
pub fn resolve_text_appearance(
    model: &Model,
    style_name: &str,
    paper_height: f64,
    model_height: f64,
    scale: f64,
) -> TextAppearance {
    let mut appearance = TextAppearance::default();
    let style = if style_name.is_empty() {
        None
    } else {
        find_text_style(model, style_name)
    };
    let style_paper = style.map_or(0.0, |style| style.paper_height);
    if paper_height > 0.0 {
        appearance.height = annotation_model_size(paper_height, scale);
    } else if style_paper > 0.0 {
        appearance.height = annotation_model_size(style_paper, scale);
    } else {
        appearance.height = model_height;
    }
    if let Some(style) = style {
        appearance.face = TextFace {
            family: style.font_family.clone(),
            bold: style.bold,
            italic: style.italic,
        };
        appearance.width_factor = style.width_factor;
        appearance.oblique = style.oblique;
        appearance.colour = style.color;
        appearance.mask = style.mask;
        appearance.mask_margin = annotation_model_size(style.mask_margin, scale);
        appearance.readable = style.readable;
        appearance.line_spacing = style.line_spacing;
    }
    appearance
}

/// The appearance of a text entity at 1 : `scale`.
pub fn resolve_text_style(model: &Model, text: &TextGeometry, scale: f64) -> TextAppearance {
    resolve_text_appearance(model, &text.style, text.paper_height, text.height, scale)
}

/// Whether the text's height depends on the scale: a paper height of its
/// own, or a style with one.
pub fn is_paper_sized(model: &Model, text: &TextGeometry) -> bool {
    if text.paper_height > 0.0 {
        return true;
    }
    if text.style.is_empty() {
        return false;
    }
    find_text_style(model, &text.style).is_some_and(|style| style.paper_height > 0.0)
}

/// A text as it is made or edited at 1 : `scale`: a paper-sized one is given
/// the model height it has at that scale (TextGeometry::height - what its
/// box, picking and the spatial index go by); any other keeps its own. The
/// one rule TEXT, MTEXT and TEXTEDIT and the Text and Multiline Text tools
/// make a text by, so a typed text and a drawn one of the same style are the
/// same size.
pub fn fit_model_height(model: &Model, scale: f64, text: &mut TextGeometry) {
    if is_paper_sized(model, text) {
        text.height = resolve_text_style(model, text, scale).height;
    }
}

/// A rotation that reads upside down - pointing from just past straight up to
/// straight down, (90, 270] degrees - turned half a turn, and the
/// justification mirrored so the block still covers the same place. The
/// tolerance at 90 degrees is the angular one: a text exactly vertical reads
/// from the right of the sheet, as drawing practice has it, and is left
/// alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Readable {
    pub rotation: f64,
    pub justify: TextJustify,
    pub turned: bool,
}

impl Default for Readable {
    fn default() -> Self {
        Self {
            rotation: 0.0,
            justify: TextJustify::BottomLeft,
            turned: false,
        }
    }
}

// The justification at row * 3 + column, as the C++ casts the number back to
// the enumeration; the arithmetic below only makes 0 to 8.
fn justify_at(value: i32) -> TextJustify {
    match value {
        0 => TextJustify::BottomLeft,
        1 => TextJustify::BottomCentre,
        2 => TextJustify::BottomRight,
        3 => TextJustify::MiddleLeft,
        4 => TextJustify::MiddleCentre,
        5 => TextJustify::MiddleRight,
        6 => TextJustify::TopLeft,
        7 => TextJustify::TopCentre,
        _ => TextJustify::TopRight,
    }
}

pub fn readable_rotation(rotation: f64, justify: TextJustify) -> Readable {
    let angle = normalize_angle(rotation);
    let quarter = 0.5 * PI;
    let mut result = Readable {
        rotation,
        justify,
        turned: false,
    };
    if angle > quarter + tolerance::ANGULAR && angle <= 3.0 * quarter + tolerance::ANGULAR {
        result.rotation = rotation + PI;
        // Column 0 <-> 2 and row 0 <-> 2: the block turned about the point
        // covers the same ground when its anchor moves to the opposite corner.
        let value = justify as i32;
        let column = 2 - value % 3;
        let row = 2 - value / 3;
        result.justify = justify_at(row * 3 + column);
        result.turned = true;
    }
    result
}

/// Lays out `text` (lines separated by '\n') at `position` in `appearance`.
/// The Drawing holds one TextRun per line, the block's rectangle in
/// text_boxes, and the mask (the block grown by the margin) when the
/// appearance asks for one; its colour is the appearance's.
pub fn layout_text(
    text: &str,
    position: Point2,
    rotation: f64,
    justify: TextJustify,
    appearance: &TextAppearance,
    measure: &TextMeasure,
) -> Drawing {
    let mut drawing = Drawing {
        colour: appearance.colour,
        ..Drawing::default()
    };
    if !(appearance.height > 0.0) || !appearance.height.is_finite() {
        return drawing;
    }
    let mut rotation = rotation;
    let mut justify = justify;
    if appearance.readable {
        let readable = readable_rotation(rotation, justify);
        rotation = readable.rotation;
        justify = readable.justify;
    }
    let height = appearance.height;
    let lines = text_lines(text);
    let mut widths = Vec::with_capacity(lines.len());
    let mut widest = 0.0;
    for &line in &lines {
        let width = measure(line, &appearance.face) * height * appearance.width_factor;
        widths.push(width);
        widest = cpp::max(widest, width);
    }
    let block = text_block_extent(
        widest,
        lines.len(),
        height,
        appearance.line_spacing,
        justify,
    );
    let at = |x: f64, y: f64| position + Vec2::new(x, y).rotated(rotation);
    for (i, &line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let local = line_origin(
            &block,
            i,
            widths[i],
            height,
            appearance.line_spacing,
            justify,
        );
        drawing.texts.push(TextRun {
            text: String::from(line),
            origin: at(local.x, local.y),
            height,
            rotation,
            width_factor: appearance.width_factor,
            oblique: appearance.oblique,
            face: appearance.face.clone(),
        });
    }
    // The descenders hang a fifth of the height below the last baseline in a
    // typical face (the em box's descent); the box and the mask include them,
    // so a mask does not cut the tail off a g.
    let descent = 0.2 * height;
    drawing.text_boxes.push(vec![
        at(block.left, block.bottom - descent),
        at(block.right, block.bottom - descent),
        at(block.right, block.top),
        at(block.left, block.top),
    ]);
    if appearance.mask {
        let m = appearance.mask_margin;
        drawing.masks.push(vec![
            at(block.left - m, block.bottom - descent - m),
            at(block.right + m, block.bottom - descent - m),
            at(block.right + m, block.top + m),
            at(block.left - m, block.top + m),
        ]);
    }
    drawing.update_extent();
    drawing
}

/// A text entity, laid out at 1 : `scale`.
pub fn layout_text_entity(
    model: &Model,
    text: &TextGeometry,
    scale: f64,
    measure: &TextMeasure,
) -> Drawing {
    layout_text(
        &text.text,
        text.position,
        text.rotation,
        text.justify,
        &resolve_text_style(model, text, scale),
        measure,
    )
}
