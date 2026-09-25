#pragma once

// Where the lines of a text go, relative to its justification point: the one
// statement of the arithmetic, so that the bounding box the entity layer
// estimates, the layout cad/annotation/text_layout.hpp works out with real
// font widths and the painters' output cannot disagree about where a line
// sits (docs/annotation.md, "Text").
//
// A block of n lines at height h is laid out in the text's own frame - x
// along the text, y up the text, the justification point at the origin:
//
//   * each line's baseline is `pitch` below the one above, pitch being
//     kLinePitch x lineSpacing x h (lineSpacing is the text style's factor);
//   * the block runs from the TOP of the first line (h above its baseline)
//     down to the BASELINE of the last - the bottom row of the nine points is
//     the last baseline, so a single line justified BottomLeft is exactly
//     the "left end of the baseline" every text had before justification;
//   * the column puts the origin at the left end, the middle or the right end
//     of the WIDEST line, and each line is then aligned in the same way
//     within that width (a centred block has centred lines).
//
// The entity layer has no font metrics, so its widths are the estimate of
// 0.6 x h per character that boundingBox has always used (kApproximateGlyphAspect);
// the painters pass measured ones.

#include <array>
#include <cstddef>
#include <string_view>
#include <vector>

#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::entity {

// The distance between two baselines at line spacing 1, as a multiple of the
// text height: the 5/3 that AutoCAD's MTEXT uses at "1.0x" spacing (the
// MTEXT reference's LINESPACINGFACTOR, whose 1.0 is 1.66 times the text
// height), so a note set here and in a DWG breaks at the same pitch.
inline constexpr double kLinePitch = 5.0 / 3.0;

// Character cell width as a fraction of the text height, for a layout with
// no font metrics. What boundingBox has estimated with since Phase 05.
inline constexpr double kApproximateGlyphAspect = 0.6;

// The lines of a text: split at '\n' only (a text is typed, not read from a
// file, so a CR is a character), an empty text is one empty line, and a
// trailing '\n' makes a trailing empty line - what was typed is what is laid.
[[nodiscard]] std::vector<std::string_view> textLines(std::string_view text);

// The number of Unicode characters in `line` (well-formed UTF-8 assumed, as
// everything in the model is): the estimate counts characters, not bytes, so
// "Café" is four cells wide and not five.
[[nodiscard]] std::size_t characterCount(std::string_view line);

// The estimated width of `line` at height 1: kApproximateGlyphAspect per
// character.
[[nodiscard]] double estimatedWidth(std::string_view line);

// A block's extent in its own frame (x along, y up, the justification point
// at the origin), before rotation.
struct TextBlockExtent {
    double left = 0.0;
    double right = 0.0;
    double bottom = 0.0; // the last line's baseline
    double top = 0.0;    // the top of the first line

    [[nodiscard]] double width() const { return right - left; }
    [[nodiscard]] double height() const { return top - bottom; }
};

// The block of `lineCount` lines whose widest line is `width` model units, at
// `height`, laid out as the file comment says.
[[nodiscard]] TextBlockExtent textBlockExtent(double width, std::size_t lineCount, double height,
                                              double lineSpacing, TextJustify justify);

// Where line `index` (0 = the top one) of `lineWidth` begins, in the block's
// own frame: the left end of its baseline, aligned within the block's width
// by the justification's column.
[[nodiscard]] katana::geometry::Point2 lineOrigin(const TextBlockExtent& block, std::size_t index,
                                                  double lineWidth, double height,
                                                  double lineSpacing, TextJustify justify);

// The four corners of `text`'s block in the model, counter-clockwise from the
// bottom left, with estimated widths at the text's own model height and
// line spacing 1: what boundingBox and distanceTo go by.
[[nodiscard]] std::array<katana::geometry::Point2, 4> estimatedTextCorners(const TextGeometry& text);

} // namespace katana::entity
