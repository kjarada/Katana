#pragma once

// The plot frame: the sheet border, the title-block strip, its cells, the
// field slots and the text rules (docs/plotting.md).
//
// The one frame Katana ships is the owner's A3 landscape cross-section frame,
// measured item by item from their app and committed as data
// (resources/plot_frames/a3_landscape.json, embedded into the build). The
// organisation's branding was taken out before it was committed; its three
// cells are now slots the user fills: their own logo image, their
// organisation's name, and their notes.
//
// PAPER COORDINATES. Everything on a sheet - frame, cells, viewports - is in
// millimetres from the BOTTOM-LEFT corner of the paper, Y up: the frame's own
// convention, and the one model space uses, so a renderer flips Y once, at
// the device, and nothing in the model has two conventions.
//
// Other paper sizes use the same frame scaled uniformly (frameScaleFor): A1 is
// the A3 frame doubled, text, line weights and dashes included. A portrait
// sheet has no frame (docs/plotting.md says why).
//
// Everything here is data and arithmetic; the painter lives in the Qt layer.

#include <cstddef>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/plot.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad::plotting {

using katana::geometry::Box2;
using katana::geometry::Point2;

// The id a Sheet names the built-in frame by. An empty id means no frame.
inline constexpr std::string_view kBuiltInFrameId = "a3_landscape";

// What an item of the frame is for. A renderer draws them all the same way;
// the role is what lets it leave the construction guide off the paper, hide
// the legend block, or find the text a field fills.
enum class FrameRole {
    Border,       // the drawing border, the title-block box and its rules
    Underline,    // the dashed line a hand-written or field value sits on
    Construction, // the sheet-edge guide: shown while editing, NEVER plotted
    Legend,       // the utility legend block (optional on a sheet)
    Label,        // static text: "SCALE", "SHEET No.", "DATE:"
    Field,        // text carrying one or more {field} placeholders
    Stamp,        // the frame version and plot date under the title block
    Slot,         // a user slot the branding was removed from: organisation, notes
};

struct FramePolyline {
    std::vector<Point2> points;
    bool closed = false;
    double weightMm = 0.25;
    entity::Color colour{0, 0, 0, 255};
    // Dash and gap in paper millimetres; both 0 for a solid line.
    double dashMm = 0.0;
    double gapMm = 0.0;
    FrameRole role = FrameRole::Border;
    // False only for the construction guide, which is drawn on screen and
    // never on paper.
    bool plots = true;
};

enum class HorizontalJustify { Left, Centre, Right };
enum class VerticalJustify { Bottom, Middle, Top };

struct FrameText {
    // Where the text is drawn: the measured position with the text's offset
    // and raise already added (six texts carry one). For a multi-line text it
    // is the FIRST line's anchor; later lines step DOWN by lineSpacingMm
    // whatever the justification - the measured rule.
    Point2 anchor{};
    double capHeightMm = 1.0; // the height of a capital, not the em
    // Horizontal stretch about the anchor: nearly every label is 0.82, which
    // on Arial is exactly Arial Narrow's width. A renderer applies it as a
    // painter scale, never as a font stretch (which measures 89%, not 82%).
    double xFactor = 1.0;
    HorizontalJustify horizontal = HorizontalJustify::Left;
    VerticalJustify vertical = VerticalJustify::Bottom;
    double angleDegrees = 0.0; // counter-clockwise
    double lineSpacingMm = 1.5; // 1.5 x the cap height, as measured
    std::string fontFace = "Arial";
    bool bold = false;
    entity::Color colour{0, 0, 0, 255};
    // Static text with {field} placeholders (expandTemplate in sheet_set.hpp
    // fills them). Lines are separated by '\n'.
    std::string content;
    // The fields this text shows, in order of appearance; empty for a label.
    std::vector<std::string> fields;
    FrameRole role = FrameRole::Label;
    // The cell a derived value is centred in rather than drawn from its
    // anchor (scale, sheet number, sheet count); empty for every other text.
    std::string centredIn;
};

// A drawing symbol of the legend block, by the name of its style.
struct FrameSymbol {
    std::string style;
    Point2 at{};
    double sizeMm = 1.0;
    double rotationDegrees = 0.0;
    entity::Color colour{0, 0, 0, 255};
};

// A named rectangle of the title block: "scale", "logo", "organisation",
// "notes", "sheet_number" ... docs/plotting.md lists all sixteen.
struct FrameCell {
    std::string id;
    Box2 rect;
};

struct FrameMargins {
    double left = 0.0;
    double right = 0.0;
    double top = 0.0;
    double bottom = 0.0;
};

struct Frame {
    std::string id;
    double widthMm = 0.0;
    double heightMm = 0.0;
    // The factor this frame was scaled by from the measured one: 1 for A3.
    double scale = 1.0;
    FrameMargins margins;
    // Where viewports go: the paper inside the margins.
    Box2 drawingArea;
    // The drawn border around the drawing area and the title-block box.
    Box2 border;
    Box2 titleBlock;
    // The construction guide at the sheet edge (never plotted).
    Box2 sheetEdge;
    std::vector<FramePolyline> polylines;
    std::vector<FrameText> texts;
    std::vector<FrameSymbol> symbols;
    std::vector<FrameCell> cells;

    // nullptr when the frame has no cell of that id.
    [[nodiscard]] const FrameCell* cell(std::string_view cellId) const;
    // Indices into `texts` of every text that shows `field`.
    [[nodiscard]] std::vector<std::size_t> textsShowing(std::string_view field) const;
};

// Parses a frame description in the committed format (see the JSON's own
// "_about"). ParseFailure for text that is not JSON or lacks a required
// member; InvalidArgument for a size or margin that makes no sheet.
[[nodiscard]] core::Result<Frame> parseFrame(std::string_view json);

// The built-in A3 landscape frame, parsed once on first use from the copy
// embedded in the build. A failure here is a defect in the committed data,
// and is reported rather than papered over.
[[nodiscard]] const core::Result<Frame>& builtInFrame();

// The uniform factor the A3 frame is scaled by to sit on `paper`: the smaller
// of width / 420 and height / 297, so it always fits. A1 is 2 exactly
// (594 / 297); A0 2.8310 (1189 / 420); A2 1.4141 (420 / 297); A4 0.7071
// (210 / 297). The slack of a millimetre or two the ISO roundings leave goes
// to the right and top margins. 0 for a portrait sheet, which has no frame.
[[nodiscard]] double frameScaleFor(PaperSize paper, bool landscape);

// `frame` scaled by `factor` about the paper origin: every coordinate, text
// height, line spacing, line weight, dash and symbol size. The paper becomes
// the given size, and the slack goes to the right and top margins.
[[nodiscard]] Frame scaledFrame(const Frame& frame, double factor, double paperWidthMm,
                                double paperHeightMm);

// The frame a sheet of this paper plots with: the built-in frame scaled to it.
// NotFound for an unknown id; InvalidArgument for a portrait sheet, which has
// no frame. Call only with a non-empty id - an empty one means "no frame".
[[nodiscard]] core::Result<Frame> frameFor(std::string_view frameId, PaperSize paper,
                                           bool landscape);

// Where an image of `imageWidth` x `imageHeight` (any unit, only the ratio
// matters) is drawn in `cell`: as large as fits inside the cell less
// `paddingMm` all round, its aspect ratio kept, centred. Empty when the image
// or the padded cell has no size. The logo slot is 52.96 x 12.0 mm; with the
// default 1 mm padding a 4:1 logo is drawn 40.0 x 10.0 mm.
[[nodiscard]] Box2 fitImage(const Box2& cell, double imageWidth, double imageHeight,
                            double paddingMm = 1.0);

} // namespace katana::cad::plotting
