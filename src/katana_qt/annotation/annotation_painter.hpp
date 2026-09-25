#pragma once

// Painting a cad::annotation::Drawing (docs/annotation.md, "Drawing").
//
// The layout - where every line of text, leader and callout goes - was done
// headlessly in katana_cad; this only strokes, fills and sets the pieces
// through the plan painter's transform. Kept out of plan_painter.cpp so the
// annotation code lives in one directory and the painter gains only the
// calls into it.

#include <functional>
#include <map>
#include <tuple>

#include <QColor>
#include <QFont>
#include <QPen>
#include <QPointF>
#include <QString>

#include "katana/cad/annotation/drawing.hpp"

class QPainter;

namespace katana::qt {

// The faces annotation text is set in, one QFont per face at a reference
// size, and their measures: a font made by family name resolves the family
// through the font database, which per text per frame was measurable
// (plan_painter.cpp, fontFor). One per paint.
class AnnotationFonts {
  public:
    explicit AnnotationFonts(QString defaultFamily) : defaultFamily_(std::move(defaultFamily)) {}

    // The font for `face` at kReferencePixels.
    const QFont& font(const katana::cad::annotation::TextFace& face);
    // What layoutText measures with: the font's own advance widths, per unit
    // of height (the em size), so a centred line is centred on the sheet.
    [[nodiscard]] katana::cad::annotation::TextMeasure measure();

    // The size the fonts are made at; a run is set by scaling it, so its
    // height is exact to the fraction of a device pixel on screen and paper
    // alike, and its measure is the font's at a size where the metrics are
    // not rounded coarsely.
    static constexpr int kReferencePixels = 100;

  private:
    QString defaultFamily_;
    std::map<std::tuple<std::string, bool, bool>, QFont> fonts_;
};

struct AnnotationPaintTarget {
    // Model to the painter's coordinates.
    std::function<QPointF(const katana::geometry::Point2&)> toDevice;
    // Device pixels per model unit: the view scale.
    double pixelsPerUnit = 1.0;
    // The pen the annotation is drawn in (its entity's resolved pen), and
    // the colour a style's own colour becomes on this medium (white prints
    // black on paper): called only when the drawing has a colour.
    QPen pen;
    std::function<QColor(const katana::entity::Color&)> colourOf;
    // What a mask is painted in: the paper, or the view's ground.
    QColor background = Qt::white;
    // Text smaller than this on the device is a stroke along its baseline,
    // as plain text below three pixels is: legible it is not, and a glyph
    // there is noise.
    double minimumTextPixels = 3.0;
};

// Paints `drawing`: masks, then strokes, outlines and fills in the pen (or
// the drawing's own colour), then the text.
void paintAnnotationDrawing(QPainter& painter, const katana::cad::annotation::Drawing& drawing,
                            const AnnotationPaintTarget& target, AnnotationFonts& fonts);

} // namespace katana::qt
