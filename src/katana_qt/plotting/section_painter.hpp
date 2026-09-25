#pragma once

// Sections on paper (docs/plotting.md, "Section smarts"): a LongSection or
// CrossSections viewport drawn, and its automatic scale chosen.
//
// What a section decides with numbers - its automatic scale and
// exaggeration, its label steps, its layout, cut and fill, each crossing's
// note and where every label goes - is decided headless in
// cad/plotting/section_fit.hpp and section_annotation.hpp; this measures the
// text and draws what they decide:
//
//   the plot     cut and fill shaded between the design and the ground (light
//                red where the ground stands above the design, light green
//                where it is below), split where the two cross; the grid at
//                steps chosen from the measured labels; the series; a
//                chainage range's ends; all clipped to the plot.
//   inside it    the datum, the surface key, a cross section's design and
//                ground levels (and the cut or fill) at its centreline, and
//                each crossing's note ("WATER RL 24.10 D 1.20") stood up
//                beside its line with its offset under the marker - placed
//                clear of each other, the nearest the middle first, and
//                dropped (counted) rather than drawn over another.
//   outside it   the levels up the left, the axis values or a long section's
//                data band (design, ground, CUT/FILL, chainage), a cross
//                section's "CH" caption - none outside the viewport.
//
// The sheet painter hands its paper mapping, pens and text setter in through
// SectionCanvas, so every colour and line weight still passes through the
// one place that styles them (paperPen, dashedPen, the TextSetter).

#include <cstddef>
#include <functional>
#include <initializer_list>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include <QBrush>
#include <QColor>
#include <QPen>
#include <QPointF>
#include <QRectF>
#include <QString>

#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/plotting/frame.hpp"
#include "katana/cad/plotting/section_fit.hpp"
#include "katana/cad/plotting/sheet_set.hpp"
#include "katana/cad/section.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"

class QPainter;

namespace katana::qt {

// A label's look: the sheet painter's TextStyle, less its colour (a
// section's words are all in the ink).
struct SectionTextStyle {
    double capMm = 1.4;
    double xFactor = 0.9;
    katana::cad::plotting::HorizontalJustify horizontal =
        katana::cad::plotting::HorizontalJustify::Left;
    katana::cad::plotting::VerticalJustify vertical = katana::cad::plotting::VerticalJustify::Bottom;
    double angleDegrees = 0.0;
    double lineSpacingMm = 0.0; // 0: 1.5 x the cap height
    bool bold = false;
};

// What a section is drawn with: the sheet painter's paper (millimetres from
// the bottom-left, Y up, onto the device), its pens and fills, and its text
// setter. Implemented in sheet_painter.cpp over paperPen, dashedPen and the
// TextSetter.
class SectionCanvas {
  public:
    virtual ~SectionCanvas() = default;
    [[nodiscard]] virtual QPainter& painter() = 0;
    [[nodiscard]] virtual QPointF at(const katana::geometry::Point2& paper) const = 0;
    [[nodiscard]] virtual QRectF at(const katana::geometry::Box2& paper) const = 0;
    // Paper millimetres in device units.
    [[nodiscard]] virtual double mm(double millimetres) const = 0;
    [[nodiscard]] virtual QPen pen(const QColor& colour, double widthMm) const = 0;
    [[nodiscard]] virtual QPen dashedPen(const QColor& colour, double widthMm,
                                         std::initializer_list<double> patternMm) const = 0;
    [[nodiscard]] virtual QBrush fill(const QColor& colour) const = 0;
    [[nodiscard]] virtual double textWidthMm(const QString& line, const SectionTextStyle& style) = 0;
    virtual void text(const katana::geometry::Point2& anchor, const QString& text,
                      const SectionTextStyle& style, double squeeze = 1.0) = 0;
    // A white box behind a label drawn over the drawing.
    virtual void knockOut(const katana::geometry::Box2& paper) = 0;
};

// The cut sections a viewport draws, from the sheet painter's cache.
struct SectionCuts {
    // The long section along the viewport's alignment and the chainage its
    // station 0 is at; null with `failure` said.
    std::function<const katana::cad::Section*(std::string& failure, double& start)> longSection;
    // The cross section at `chainage`, `halfWidth` each side.
    std::function<const katana::cad::Section*(double chainage, double halfWidth,
                                              std::string& failure)>
        crossSection;
    // The alignment's design profile's level at `chainage`; nothing without
    // a profile.
    std::function<std::optional<double>(double chainage)> profileLevel;
};

// How a section viewport's drawing went.
struct SectionPaintResult {
    bool drawn = false;
    std::vector<std::string> problems; // "no alignment chosen", a cut's failure
    std::size_t notesDropped = 0;      // SheetPaintStats::sectionNotesDropped
};

// Draws LongSection or CrossSections `viewport` into its rectangle less the
// title's strip, at its own scale and exaggeration (the sheet painter has
// already put an automatic one's there). `model` gives crossings their own
// levels.
[[nodiscard]] SectionPaintResult paintSectionViewport(SectionCanvas& canvas,
                                                      const katana::cad::plotting::Viewport& viewport,
                                                      const SectionCuts& cuts,
                                                      const katana::entity::Model* model);

// The scale and exaggeration section `viewport` is drawn at: its own, or with
// autoScale those plotting::fitSection chooses for its plot - laid out as
// paintSectionViewport lays it out, around its measured level labels - for
// its chainage range (the whole alignment without one) or its cross
// sections' full width, and the level range of what it shows. A viewport
// that cannot be cut keeps its own.
[[nodiscard]] katana::cad::plotting::SectionFit
fitSectionViewport(SectionCanvas& canvas, const katana::cad::plotting::Viewport& viewport,
                   const SectionCuts& cuts, const katana::entity::Model* model);

} // namespace katana::qt
