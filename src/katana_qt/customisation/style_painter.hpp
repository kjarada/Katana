#pragma once

// Painting a cad::StyleDrawing with QPainter: the one place a 12d
// definition's strokes, dots, texts and pens become pixels.
//
// The viewport, the plot and every preview and thumbnail paint through these
// functions, so none of them can drift from the others. They used to be
// ViewportWidget members, which is why a preview drew something else: it
// could not reach them.
//
// Everything the caller decides comes in through a StylePaintTarget - the
// model-to-device transform, the ENTITY PEN (its colour, width and cap: a
// preview that guessed its own pen showed dashes one pen-width shorter than
// the plot, whose square caps grew every dash by that much) and whether this
// is paper, where the paper colour rule (cad::paperColour) turns a white 12d
// pen black. Nothing here reads a widget.

#include <string>

#include <QFont>
#include <QPen>

#include "katana/cad/plot.hpp"
#include "katana/cad/style_drawing.hpp"
#include "katana/cad/view_transform.hpp"

class QPainter;

namespace katana::qt {

struct StylePaintTarget {
    // Model units to device pixels.
    katana::cad::ViewTransform view{};
    // The entity's own pen. An empty 12d pen ("view_colour") is exactly this;
    // a named one changes only its colour.
    QPen entityPen{};
    // Null on screen. On paper, the plot's settings: a 12d pen follows their
    // paper colour rule. The entity pen's own colour is the caller's to have
    // converted already, since the caller chose it.
    const katana::cad::PlotSettings* paper = nullptr;
    // Every stroke in the entity pen, 12d pens ignored - the selection
    // highlight, which must read as one colour whatever the definition says.
    bool entityPenOnly = false;
};

// Style texts at or above this many pixels are drawn at it: the same ceiling
// the viewport's plain text has (ViewportWidget::drawText), for the same
// reason - a font asked for at a size of hundreds of thousands of pixels, one
// zoom step from a survey's extent, makes the raster engine allocate glyphs
// larger than any screen, and a glyph taller than the view reads the same at
// any height above it.
inline constexpr double kMaximumStyleTextPixels = 2000.0;
// Style texts smaller than this are not drawn: unreadable, and the commonest
// cost of a zoomed-out drawing full of labelled linestyles.
inline constexpr double kMinimumStyleTextPixels = 3.0;

// The pen a 12d `colour` names, on the entity's pen: its colour from 12d's
// standard names (archive12d::standardColour), through the paper colour rule
// when `paper` is given, with the entity pen's width, cap, style and alpha
// kept. An empty name, or one the standard names do not know, is the entity
// pen unchanged rather than a guess.
[[nodiscard]] QPen stylePenFor(const QPen& entityPen, const std::string& pen,
                               const katana::cad::PlotSettings* paper);

void paintStyleText(QPainter& painter, const katana::cad::StyleTextMark& text,
                    const StylePaintTarget& target);

// The model-space box a style text is painted over, measured with the font
// paintStyleText would choose (from `base`), its justification, width factor
// and angle. cad::drawnExtent cannot know a font and over-estimates on
// purpose, which is right for culling but shrinks a labelled symbol to a
// third of its thumbnail; a picture fitted to what is painted uses this.
// Ignores the pixel clamps, which depend on a view.
[[nodiscard]] katana::geometry::Box2 styleTextExtent(const katana::cad::StyleTextMark& text,
                                                     const QFont& base);
// Every stroke's points and every text's styleTextExtent.
[[nodiscard]] katana::geometry::Box2 paintedExtent(const katana::cad::StyleDrawing& drawing,
                                                   const QFont& base);

// Strokes, then texts. A one-point stroke is a 12d `dot`, painted as a round
// dot of the pen's width whatever the pen's cap: a flat cap draws a
// zero-length line as nothing. The painter's pen is left as it was found.
void paintStyleDrawing(QPainter& painter, const katana::cad::StyleDrawing& drawing,
                       const StylePaintTarget& target);

} // namespace katana::qt
