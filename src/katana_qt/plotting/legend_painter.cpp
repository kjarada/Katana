#include "plotting/legend_painter.hpp"

#include <algorithm>
#include <cmath>

#include <QFont>
#include <QList>
#include <QPainter>
#include <QPainterPath>
#include <QPen>
#include <QPolygonF>

#include "customisation/style_painter.hpp"
#include "katana/cad/dashing.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/hatching.hpp"
#include "katana/cad/style_resolver.hpp"
#include "katana/cad/view_transform.hpp"
#include "plan_painter.hpp"

namespace katana::qt {

namespace {

using katana::cad::plotting::LegendEntry;
using katana::cad::plotting::LegendKind;
using katana::geometry::Box2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

// What the sample is drawn through: the cell's own little map, whose origin
// is the cell's centre and whose units are metres on the ground at the
// legend's scale, so a pattern or a symbol sized on the ground comes out the
// size it prints.
class Sample {
  public:
    Sample(QPainter& painter, const LegendEntry& entry, const QRectF& cell,
           const LegendSampleContext& context)
        : painter_(painter), entry_(entry), cell_(cell), context_(context),
          model_(context.model != nullptr ? *context.model : noModel()),
          library_(context.library != nullptr ? *context.library : noLibrary())
    {
        const double scale =
            context.scale > 0.0 && std::isfinite(context.scale) ? context.scale : 500.0;
        metresPerMillimetre_ = scale / 1000.0;
        view_.center = Point2(0.0, 0.0);
        view_.scale = context.pixelsPerMillimetre / metresPerMillimetre_;
        view_.resize(cell.width(), cell.height());
        const katana::entity::Color colour =
            context.plot != nullptr ? katana::cad::paperColour(entry.colour, *context.plot)
                                    : entry.colour;
        ink_ = QColor(colour.r, colour.g, colour.b, colour.a);
        // A solid area prints as the plan prints a fill (cad::paperFillColour):
        // in monochrome black or white by its lightness, not the pen's black.
        const katana::entity::Color area =
            context.plot != nullptr ? katana::cad::paperFillColour(entry.colour, *context.plot)
                                    : entry.colour;
        fillInk_ = QColor(area.r, area.g, area.b, area.a);
        // The plan painter's pen: the weight in paper millimetres, times the
        // plot style's line weight scale, as the plan beside it is drawn.
        lineScale_ = context.plot != nullptr ? context.plot->lineWeightScale : 1.0;
        penWidth_ = std::max(entry.lineWeight, 0.0) * lineScale_ * context.pixelsPerMillimetre;
        pen_ = QPen(ink_, penWidth_);
        target_.view = view_;
        target_.entityPen = pen_;
        target_.entityPen.setCapStyle(Qt::FlatCap);
        target_.paper = context.plot;
        target_.colours = context.colours;
    }

    bool paint()
    {
        switch (entry_.kind) {
        case LegendKind::Line:
            return line();
        case LegendKind::Symbol:
            return symbol();
        case LegendKind::Point:
            return point();
        case LegendKind::Area:
            return area();
        case LegendKind::Text:
            return text();
        }
        return false;
    }

  private:
    static const katana::entity::Model& noModel()
    {
        static const katana::entity::Model empty;
        return empty;
    }
    static const katana::entity::StyleLibrary& noLibrary()
    {
        static const katana::entity::StyleLibrary empty;
        return empty;
    }

    // Half the cell, in metres on the ground.
    [[nodiscard]] double halfWidth() const { return 0.5 * cell_.width() / view_.scale; }
    [[nodiscard]] double halfHeight() const { return 0.5 * cell_.height() / view_.scale; }
    [[nodiscard]] QPointF at(const Point2& world, const katana::cad::ViewTransform& view) const
    {
        const Point2 p = view.worldToScreen(world);
        return QPointF(p.x, p.y);
    }
    [[nodiscard]] QPointF at(const Point2& world) const { return at(world, view_); }

    // A line of the entry along `path`: what the plan draws it with.
    void stroke(const Polyline2& path)
    {
        const katana::cad::ResolvedLinetype pattern =
            katana::cad::resolveLinePattern(model_, library_, entry_.linetype, entry_.symbol);
        if (pattern.kind == katana::cad::LinetypeKind::LibraryDefinition &&
            pattern.definition != nullptr) {
            const katana::cad::StyleDrawing drawing =
                katana::cad::styleDrawing(*pattern.definition, path, metresPerMillimetre_);
            if (!drawing.empty()) {
                paintStyleDrawing(painter_, drawing, target_);
                return;
            }
        }
        QPen pen = pen_;
        if (pattern.kind == katana::cad::LinetypeKind::ModelLinetype &&
            pattern.linetype != nullptr) {
            katana::cad::DashOptions dash;
            dash.viewScale = view_.scale;
            const auto dashes =
                katana::cad::qtDashPattern(*pattern.linetype, dash, std::max(penWidth_, 0.01));
            if (!dashes.empty()) {
                pen.setDashPattern(QList<qreal>(dashes.begin(), dashes.end()));
            }
        }
        QPolygonF polygon;
        for (const Point2& vertex : path.vertices) {
            polygon << at(vertex);
        }
        painter_.setPen(pen);
        painter_.setBrush(Qt::NoBrush);
        if (path.closed) {
            painter_.drawPolygon(polygon);
        } else {
            painter_.drawPolyline(polygon);
        }
    }

    // The symbol the entry names, as the plan stamps it; `view` places it.
    [[nodiscard]] katana::cad::StyleDrawing symbolAt(const Point2& where) const
    {
        return katana::cad::pointSymbolDrawing(
            library_, entry_.symbol, where, entry_.symbolSize, 0.0, metresPerMillimetre_,
            kPointMarkerPaperMillimetres * metresPerMillimetre_);
    }

    bool line()
    {
        // The whole cell's width, so a pattern shows as much of itself as
        // ten millimetres of the plan would.
        const double half = halfWidth();
        const Polyline2 path{{Point2(-half, 0.0), Point2(half, 0.0)}, false};
        stroke(path);
        // A style's symbol sits at every vertex of a line (decision D8).
        if (!entry_.symbol.empty()) {
            for (const Point2& end : path.vertices) {
                paintStyleDrawing(painter_, symbolAt(end), target_);
            }
        }
        return true;
    }

    bool symbol()
    {
        const katana::cad::StyleDrawing drawing = symbolAt(Point2(0.0, 0.0));
        if (drawing.empty()) {
            return false;
        }
        // At its plotted size, unless that overflows the cell: then as large
        // as fits, centred, the pen's width kept.
        const Box2 extent = paintedExtent(drawing, QFont(QStringLiteral("Arial")));
        StylePaintTarget fitted = target_;
        if (!extent.empty()) {
            double shrink = 1.0;
            if (extent.width() > 2.0 * halfWidth()) {
                shrink = std::min(shrink, 2.0 * halfWidth() / extent.width());
            }
            if (extent.height() > 2.0 * halfHeight()) {
                shrink = std::min(shrink, 2.0 * halfHeight() / extent.height());
            }
            fitted.view.scale = view_.scale * shrink;
            fitted.view.center = extent.center();
        }
        paintStyleDrawing(painter_, drawing, fitted);
        return true;
    }

    bool point()
    {
        // The plain mark: a cross 2 mm across (kPointMarkerPaperMillimetres).
        const double r = kPointMarkerPaperMillimetres * context_.pixelsPerMillimetre;
        const QPointF centre = at(Point2(0.0, 0.0));
        painter_.setPen(pen_);
        painter_.drawLine(centre + QPointF(-r, 0.0), centre + QPointF(r, 0.0));
        painter_.drawLine(centre + QPointF(0.0, -r), centre + QPointF(0.0, r));
        return true;
    }

    bool area()
    {
        // A swatch a little inside the cell, so its outline's weight shows.
        const double inset = 0.3 * metresPerMillimetre_;
        const double w = std::max(halfWidth() - inset, 0.0);
        const double h = std::max(halfHeight() - inset, 0.0);
        const Polyline2 swatch{{Point2(-w, -h), Point2(w, -h), Point2(w, h), Point2(-w, h)}, true};
        if (const katana::entity::HatchPattern* pattern =
                model_.hatchPatterns.find(entry_.hatchPattern);
            pattern != nullptr && !pattern->drawsNothing()) {
            katana::cad::HatchOptions options;
            options.viewScale = view_.scale;
            QPolygonF polygon;
            for (const Point2& vertex : swatch.vertices) {
                polygon << at(vertex);
            }
            switch (katana::cad::hatchDrawing(*pattern, options)) {
            case katana::cad::HatchDrawing::None:
                break;
            case katana::cad::HatchDrawing::Solid: {
                QPainterPath path;
                path.addPolygon(polygon);
                path.closeSubpath();
                painter_.fillPath(path, fillInk_);
                break;
            }
            case katana::cad::HatchDrawing::Lines:
                // The finest standard pen, as the plan hatches on paper.
                painter_.setPen(QPen(ink_, kHatchLinePaperMillimetres * lineScale_ *
                                               context_.pixelsPerMillimetre));
                for (const katana::geometry::Segment2& hatch :
                     katana::cad::hatchSegments(swatch, *pattern)) {
                    painter_.drawLine(at(hatch.start), at(hatch.end));
                }
                break;
            }
        }
        stroke(swatch);
        return true;
    }

    bool text()
    {
        // Two millimetres of capitals, or what the cell's height allows.
        const double capMm = std::min(2.0, 0.8 * cell_.height() / context_.pixelsPerMillimetre);
        QFont font(QStringLiteral("Arial"));
        font.setPixelSize(std::max(1, static_cast<int>(std::lround(
                                          capMm * context_.pixelsPerMillimetre / 0.716))));
        painter_.setFont(font);
        painter_.setPen(ink_);
        const QPointF left = at(Point2(-halfWidth(), 0.0));
        painter_.drawText(left + QPointF(0.0, 0.5 * capMm * context_.pixelsPerMillimetre),
                          QStringLiteral("Abc"));
        return true;
    }

    QPainter& painter_;
    const LegendEntry& entry_;
    QRectF cell_;
    const LegendSampleContext& context_;
    const katana::entity::Model& model_;
    const katana::entity::StyleLibrary& library_;
    double metresPerMillimetre_ = 0.5;
    katana::cad::ViewTransform view_{};
    QColor ink_;
    QColor fillInk_; // a solid area's colour on paper
    double lineScale_ = 1.0; // the plot style's line weight scale
    double penWidth_ = 0.0;
    QPen pen_;
    StylePaintTarget target_{};
};

} // namespace

katana::core::Result<katana::cad::plotting::Legend>
gatherLegend(const katana::cad::plotting::SheetSet& set, std::size_t sheetIndex,
             const katana::cad::plotting::Viewport& legend, const SheetSource& source)
{
    namespace plotting = katana::cad::plotting;
    if (source.plan.model == nullptr) {
        return katana::core::makeError(katana::core::ErrorCode::InvalidArgument,
                                       "no drawing to list");
    }
    plotting::LegendOptions options;
    options.window = [&source](const plotting::Viewport& plan) {
        const ResolvedViewport at = resolvePlanViewport(plan, source);
        return plotting::PlanWindow{at.scale, at.centre};
    };
    options.index = source.plan.index;
    options.codes = source.document != nullptr ? &source.document->surveyMap() : nullptr;
    return plotting::computeLegend(*source.plan.model, set, sheetIndex, legend.legendScope,
                                   options);
}

bool paintLegendSample(QPainter& painter, const katana::cad::plotting::LegendEntry& entry,
                       const QRectF& cell, const LegendSampleContext& context)
{
    if (!(cell.width() > 0.0) || !(cell.height() > 0.0) || !(context.pixelsPerMillimetre > 0.0)) {
        return false;
    }
    painter.save();
    painter.translate(cell.topLeft());
    // A pattern's strokes may reach above and below its line; they stop half
    // a row's gap past the cell, where the next row begins.
    const double spill = 0.45 * context.pixelsPerMillimetre;
    painter.setClipRect(QRectF(0.0, -spill, cell.width(), cell.height() + 2.0 * spill),
                        Qt::IntersectClip);
    const bool drawn = Sample(painter, entry, cell, context).paint();
    painter.restore();
    return drawn;
}

} // namespace katana::qt
