//! Where the lines of a text go, relative to its justification point: the one
//! statement of the arithmetic, so that the bounding box the entity layer
//! estimates, the layout worked out with real font widths and the painters'
//! output cannot disagree about where a line sits (docs/annotation.md,
//! "Text").
//!
//! A block of n lines at height h is laid out in the text's own frame - x
//! along the text, y up the text, the justification point at the origin:
//!
//!   * each line's baseline is `pitch` below the one above, pitch being
//!     LINE_PITCH x line_spacing x h;
//!   * the block runs from the TOP of the first line (h above its baseline)
//!     down to the BASELINE of the last;
//!   * the column puts the origin at the left end, the middle or the right end
//!     of the WIDEST line, and each line is then aligned in the same way
//!     within that width.
//!
//! The entity layer has no font metrics, so its widths are the estimate of
//! 0.6 x h per character (APPROXIMATE_GLYPH_ASPECT); the painters pass
//! measured ones.

use katana_geometry::{Point2, Vec2};

use crate::entity::{TextGeometry, TextJustify};
use crate::scalar;

/// The distance between two baselines at line spacing 1, as a multiple of the
/// text height: the 5/3 that AutoCAD's MTEXT uses at "1.0x" spacing (the MTEXT
/// reference's LINESPACINGFACTOR, whose 1.0 is 1.66 times the text height).
pub const LINE_PITCH: f64 = 5.0 / 3.0;

/// Character cell width as a fraction of the text height, for a layout with
/// no font metrics.
pub const APPROXIMATE_GLYPH_ASPECT: f64 = 0.6;

/// The justification's column and row as 0, 1, 2 - left, centre, right and
/// bottom, middle, top - which is how the enumerators are numbered.
fn column_of(justify: TextJustify) -> u8 {
    justify as u8 % 3
}

fn row_of(justify: TextJustify) -> u8 {
    justify as u8 / 3
}

/// The lines of a text: split at '\n' only (a text is typed, not read from a
/// file, so a CR is a character), an empty text is one empty line, and a
/// trailing '\n' makes a trailing empty line.
pub fn text_lines(text: &str) -> Vec<&str> {
    text.split('\n').collect()
}

/// The number of Unicode characters in `line`: the estimate counts
/// characters, not bytes, so "Café" is four cells wide and not five.
pub fn character_count(line: &str) -> usize {
    // A UTF-8 continuation byte is 10xxxxxx; every other byte starts a character.
    line.bytes().filter(|byte| byte & 0xC0 != 0x80).count()
}

/// The estimated width of `line` at height 1.
pub fn estimated_width(line: &str) -> f64 {
    APPROXIMATE_GLYPH_ASPECT * character_count(line) as f64
}

/// A block's extent in its own frame (x along, y up, the justification point
/// at the origin), before rotation.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextBlockExtent {
    pub left: f64,
    pub right: f64,
    /// The last line's baseline.
    pub bottom: f64,
    /// The top of the first line.
    pub top: f64,
}

impl TextBlockExtent {
    pub fn width(&self) -> f64 {
        self.right - self.left
    }
    pub fn height(&self) -> f64 {
        self.top - self.bottom
    }
}

/// The block of `line_count` lines whose widest line is `width` model units,
/// at `height`.
pub fn text_block_extent(
    width: f64,
    line_count: usize,
    height: f64,
    line_spacing: f64,
    justify: TextJustify,
) -> TextBlockExtent {
    let lines = line_count.max(1);
    let pitch = LINE_PITCH * line_spacing * height;
    let block_height = height + pitch * (lines - 1) as f64;

    let left = match column_of(justify) {
        0 => 0.0,
        1 => -0.5 * width,
        _ => -width,
    };
    let bottom = match row_of(justify) {
        0 => 0.0,
        1 => -0.5 * block_height,
        _ => -block_height,
    };
    TextBlockExtent {
        left,
        right: left + width,
        bottom,
        top: bottom + block_height,
    }
}

/// Where line `index` (0 = the top one) of `line_width` begins, in the block's
/// own frame: the left end of its baseline, aligned within the block's width
/// by the justification's column.
pub fn line_origin(
    block: &TextBlockExtent,
    index: usize,
    line_width: f64,
    height: f64,
    line_spacing: f64,
    justify: TextJustify,
) -> Point2 {
    let pitch = LINE_PITCH * line_spacing * height;
    let baseline = block.top - height - pitch * index as f64;
    let x = match column_of(justify) {
        0 => block.left,
        1 => block.left + 0.5 * (block.width() - line_width),
        _ => block.right - line_width,
    };
    Point2::new(x, baseline)
}

/// The four corners of `text`'s block in the model, counter-clockwise from the
/// bottom left, with estimated widths at the text's own model height and line
/// spacing 1: what bounding_box and distance_to go by.
pub fn estimated_text_corners(text: &TextGeometry) -> [Point2; 4] {
    let lines = text_lines(&text.text);
    let mut widest = 0.0;
    for line in &lines {
        widest = scalar::max(widest, estimated_width(line));
    }
    let block = text_block_extent(
        widest * text.height,
        lines.len(),
        text.height,
        1.0,
        text.justify,
    );
    let at = |x: f64, y: f64| text.position + Vec2::new(x, y).rotated(text.rotation);
    [
        at(block.left, block.bottom),
        at(block.right, block.bottom),
        at(block.right, block.top),
        at(block.left, block.top),
    ]
}
