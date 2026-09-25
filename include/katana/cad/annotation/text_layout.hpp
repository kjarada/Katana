#pragma once

// Text in its style, at a scale: the one layout every text, label and
// leader note goes through (docs/annotation.md, "Text").
//
// resolveTextStyle works out what a text is set in at one annotation scale
// - its model height from its own paper height, else its style's, else its
// stored model height; the face, width factor, slant and colour; the mask -
// and layoutText places its lines by the justification arithmetic of
// entity/text_block.hpp with MEASURED widths, turns a text that would read
// upside down (a readable style), and returns the runs, the block's
// rectangle and the mask as a Drawing.

#include <optional>
#include <string>
#include <string_view>

#include "katana/cad/annotation/drawing.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"

namespace katana::cad::annotation {

// What a text is set in, in model units for one scale.
struct TextAppearance {
    double height = 2.5; // model units
    TextFace face{};
    double widthFactor = 1.0;
    double oblique = 0.0;
    std::optional<katana::entity::Color> colour{}; // empty: the entity's pen
    bool mask = false;
    double maskMargin = 0.0; // model units
    bool readable = false;
    double lineSpacing = 1.0;
};

// The text style `name` names, or "Standard" when it is empty or gone (a
// renderer always has something to set text in), or nullptr when the model
// has no Standard either - which a model never lacks, but a caller that
// built one by hand might.
[[nodiscard]] const katana::entity::TextStyle* findTextStyle(const katana::entity::Model& model,
                                                             std::string_view name);

// A paper height (mm), a style and a fallback model height resolved at
// 1 : `scale`. The height is: `paperHeight` when positive, else the style's
// paper height when positive, else `modelHeight`. An empty style name is
// no style at all - the plain face, never turned, no mask - which is what
// every text written before styles existed is.
[[nodiscard]] TextAppearance resolveTextAppearance(const katana::entity::Model& model,
                                                   std::string_view styleName, double paperHeight,
                                                   double modelHeight, double scale);

// The appearance of a text entity at 1 : `scale`.
[[nodiscard]] TextAppearance resolveTextStyle(const katana::entity::Model& model,
                                              const katana::entity::TextGeometry& text,
                                              double scale);

// Whether the text's height depends on the scale: a paper height of its own,
// or a style with one.
[[nodiscard]] bool isPaperSized(const katana::entity::Model& model,
                                const katana::entity::TextGeometry& text);

// A text as it is made or edited at 1 : `scale`: a paper-sized one is given
// the model height it has at that scale (TextGeometry::height - what its box,
// picking and the spatial index go by); any other keeps its own. The one rule
// TEXT, MTEXT and TEXTEDIT and the Text and Multiline Text tools make a text
// by, so a typed text and a drawn one of the same style are the same size.
void fitModelHeight(const katana::entity::Model& model, double scale,
                    katana::entity::TextGeometry& text);

// A rotation that reads upside down - pointing from just past straight up to
// straight down, (90, 270] degrees - turned half a turn, and the
// justification mirrored so the block still covers the same place. The
// tolerance at 90 degrees is the angular one: a text exactly vertical reads
// from the right of the sheet, as drawing practice has it, and is left alone.
struct Readable {
    double rotation = 0.0;
    katana::entity::TextJustify justify = katana::entity::TextJustify::BottomLeft;
    bool turned = false;
};
[[nodiscard]] Readable readableRotation(double rotation, katana::entity::TextJustify justify);

// Lays out `text` (lines separated by '\n') at `position` in `appearance`.
// The Drawing holds one TextRun per line, the block's rectangle in
// textBoxes, and the mask (the block grown by the margin) when the
// appearance asks for one; its colour is the appearance's.
[[nodiscard]] Drawing layoutText(std::string_view text, const Point2& position, double rotation,
                                 katana::entity::TextJustify justify,
                                 const TextAppearance& appearance, const TextMeasure& measure);

// A text entity, laid out at 1 : `scale`.
[[nodiscard]] Drawing layoutTextEntity(const katana::entity::Model& model,
                                       const katana::entity::TextGeometry& text, double scale,
                                       const TextMeasure& measure);

} // namespace katana::cad::annotation
