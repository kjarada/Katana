#include "sheet_painter.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <format>
#include <set>
#include <string_view>
#include <utility>

#include <QFileInfo>
#include <QFont>
#include <QFontMetricsF>
#include <QPageSize>
#include <QPainter>
#include <QPainterPath>
#include <QPdfWriter>
#include <QPen>
#include <QPolygonF>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/key_plan.hpp"
#include "katana/cad/plotting/legend.hpp"
#include "katana/cad/plotting/plan_grid.hpp"
#include "katana/cad/plotting/tables.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/math/numerics.hpp"
#include "katana/render/camera.hpp"
#include "katana/render/framebuffer.hpp"
#include "katana/render/rasterizer.hpp"
#include "plotting/legend_painter.hpp"
#include "plotting/sheet_tables.hpp"

namespace katana::qt {

namespace plotting = katana::cad::plotting;
using katana::geometry::Box2;
using katana::geometry::Point2;
using plotting::FrameRole;
using plotting::HorizontalJustify;
using plotting::VerticalJustify;
using plotting::Viewport;
using plotting::ViewportKind;

namespace {

// ---- paper -------------------------------------------------------------------

// Paper millimetres (Y up) onto the device (Y down), for one sheet.
class Paper {
  public:
    Paper(double heightMm, const SheetPaintOptions& options) : height_(heightMm), options_(options)
    {
    }
    [[nodiscard]] QPointF at(const Point2& p) const { return paperToDevice(p, height_, options_); }
    [[nodiscard]] QRectF at(const Box2& b) const { return paperToDevice(b, height_, options_); }
    [[nodiscard]] double mm(double millimetres) const
    {
        return millimetres * options_.pixelsPerMillimetre;
    }
    [[nodiscard]] double ppmm() const { return options_.pixelsPerMillimetre; }
    [[nodiscard]] const SheetPaintOptions& options() const { return options_; }

  private:
    double height_;
    const SheetPaintOptions& options_;
};

QColor qColour(const katana::entity::Color& c) { return QColor(c.r, c.g, c.b, c.a); }

const QColor kInk(0, 0, 0);
const QColor kGridInk(190, 190, 190);
const QColor kFaint(150, 150, 150);
// A key plan: the other sheets' outlines, and this sheet's own - "you are here".
const QColor kOutlineInk(200, 0, 30);
const QColor kOutlineFill(200, 0, 30, 18);
const QColor kHereInk(0, 80, 180);
const QColor kHereFill(0, 110, 230, 90);

// A pen of `widthMm` on paper, round-capped as the frame is drawn.
QPen paperPen(const Paper& paper, const QColor& colour, double widthMm,
              Qt::PenStyle style = Qt::SolidLine)
{
    QPen pen(colour, std::max(paper.mm(widthMm), 0.01), style, Qt::RoundCap, Qt::RoundJoin);
    return pen;
}

// A dashed pen: `pattern` in paper millimetres, flat-capped so that a dash is
// exactly as long as it says (Qt measures a pattern in pen widths).
QPen dashedPen(const Paper& paper, const QColor& colour, double widthMm,
               std::initializer_list<double> patternMm)
{
    QPen pen(colour, std::max(paper.mm(widthMm), 0.01), Qt::CustomDashLine, Qt::FlatCap,
             Qt::RoundJoin);
    QList<qreal> pattern;
    for (const double length : patternMm) {
        pattern << std::max(length / std::max(widthMm, 1e-3), 0.01);
    }
    pen.setDashPattern(pattern);
    return pen;
}

Point2 rotated(const Point2& v, double radians)
{
    const double c = std::cos(radians);
    const double s = std::sin(radians);
    return Point2(c * v.x - s * v.y, s * v.x + c * v.y);
}

// ---- text --------------------------------------------------------------------

// Text is set in a font at a reference pixel size and scaled to its exact cap
// height, so a 1.2 mm label is 1.2 mm at 150 dpi and at 600 - no font size is
// ever rounded to a whole device pixel.
constexpr int kReferencePixels = 200;
constexpr double kArialCapRatio = 0.716; // when a font reports no cap height

struct TextStyle {
    double capMm = 2.0;
    double xFactor = 1.0;
    HorizontalJustify horizontal = HorizontalJustify::Left;
    VerticalJustify vertical = VerticalJustify::Bottom;
    double angleDegrees = 0.0;
    double lineSpacingMm = 0.0; // 0: 1.5 x the cap height
    bool bold = false;
    QColor colour = kInk;
    QString face = QStringLiteral("Arial");
};

class TextSetter {
  public:
    TextSetter(QPainter& painter, const Paper& paper) : painter_(painter), paper_(paper) {}

    // The width of one line, in paper millimetres.
    [[nodiscard]] double widthMm(const QString& line, const TextStyle& style)
    {
        const Metrics& m = metrics(style);
        return m.metrics.horizontalAdvance(line) * scaleFor(m, style) * style.xFactor /
               paper_.ppmm();
    }

    // Draws `text` (lines split on '\n') anchored at `anchor`; later lines
    // step down, whatever the justification, as the frame's rule is.
    void draw(const Point2& anchor, const QString& text, const TextStyle& style,
              double squeeze = 1.0)
    {
        if (text.isEmpty()) {
            return;
        }
        const Metrics& m = metrics(style);
        const double k = scaleFor(m, style);
        const double cap = capOf(m);
        const double spacing =
            style.lineSpacingMm > 0.0 ? style.lineSpacingMm : 1.5 * style.capMm;
        painter_.save();
        painter_.setFont(m.font);
        painter_.setPen(style.colour);
        painter_.setBrush(Qt::NoBrush);
        painter_.translate(paper_.at(anchor));
        painter_.rotate(-style.angleDegrees);
        const QStringList lines = text.split(QLatin1Char('\n'));
        for (qsizetype i = 0; i < lines.size(); ++i) {
            const QString& line = lines[i];
            const double width = m.metrics.horizontalAdvance(line);
            double x = 0.0;
            if (style.horizontal == HorizontalJustify::Centre) {
                x = -0.5 * width;
            } else if (style.horizontal == HorizontalJustify::Right) {
                x = -width;
            }
            double y = 0.0;
            if (style.vertical == VerticalJustify::Middle) {
                y = 0.5 * cap;
            } else if (style.vertical == VerticalJustify::Top) {
                y = cap;
            }
            painter_.save();
            painter_.translate(0.0, paper_.mm(static_cast<double>(i) * spacing));
            painter_.scale(k * style.xFactor * squeeze, k);
            painter_.drawText(QPointF(x, y), line);
            painter_.restore();
        }
        painter_.restore();
    }

    // Words of `text` broken into lines no wider than `widthMm`.
    [[nodiscard]] QStringList wrap(const QString& text, const TextStyle& style, double widthMm)
    {
        QStringList out;
        for (const QString& paragraph : text.split(QLatin1Char('\n'))) {
            QString line;
            for (const QString& word : paragraph.split(QLatin1Char(' '), Qt::SkipEmptyParts)) {
                const QString candidate = line.isEmpty() ? word : line + QLatin1Char(' ') + word;
                if (!line.isEmpty() && this->widthMm(candidate, style) > widthMm) {
                    out << line;
                    line = word;
                } else {
                    line = candidate;
                }
            }
            out << line;
        }
        return out;
    }

  private:
    struct Metrics {
        QFont font;
        QFontMetricsF metrics;
    };

    const Metrics& metrics(const TextStyle& style)
    {
        const QString key = style.face + (style.bold ? QStringLiteral("|b") : QStringLiteral("|"));
        auto found = fonts_.find(key);
        if (found == fonts_.end()) {
            QFont font(style.face);
            font.setPixelSize(kReferencePixels);
            font.setBold(style.bold);
            font.setHintingPreference(QFont::PreferNoHinting);
            font.setKerning(true);
            QFontMetricsF metrics(font, painter_.device());
            found = fonts_.emplace(key, Metrics{font, metrics}).first;
        }
        return found->second;
    }
    static double capOf(const Metrics& m)
    {
        const double cap = m.metrics.capHeight();
        return cap > 0.0 ? cap : kArialCapRatio * kReferencePixels;
    }
    double scaleFor(const Metrics& m, const TextStyle& style) const
    {
        return paper_.mm(style.capMm) / capOf(m);
    }

    QPainter& painter_;
    const Paper& paper_;
    std::map<QString, Metrics> fonts_;
};

// A white box behind a label drawn over a drawing, so it reads.
void knockOut(QPainter& painter, const Paper& paper, const Box2& box)
{
    painter.fillRect(paper.at(box), Qt::white);
}

// ---- the frame's legend glyphs -------------------------------------------------
//
// The frame names its legend symbols by style; the glyphs are the owner's
// app's, drawn in a -1..1 box (Y down, as it drew them) scaled by the
// symbol's size, with a stroke of 0.18 mm whatever the size.

void glyph(QPainter& p, const std::string& style, const QColor& ink, double stroke)
{
    std::string key;
    for (const char c : style) {
        key.push_back(static_cast<char>(std::tolower(static_cast<unsigned char>(c))));
    }
    QPen pen(ink, stroke, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin);
    p.setPen(pen);
    p.setBrush(Qt::NoBrush);
    const auto circle = [&](double cx, double cy, double r, bool fill = false) {
        p.setBrush(fill ? QBrush(ink) : QBrush(Qt::NoBrush));
        p.drawEllipse(QPointF(cx, cy), r, r);
        p.setBrush(Qt::NoBrush);
    };
    const auto rect = [&](double hw, double hh) { p.drawRect(QRectF(-hw, -hh, 2 * hw, 2 * hh)); };
    const auto line = [&](double x1, double y1, double x2, double y2) {
        p.drawLine(QPointF(x1, y1), QPointF(x2, y2));
    };
    const auto triangle = [&](std::array<QPointF, 3> points) {
        p.setBrush(ink);
        p.drawPolygon(points.data(), 3);
        p.setBrush(Qt::NoBrush);
    };
    const auto meter = [&] {
        rect(1, 1);
        const std::array<QPointF, 5> m{QPointF(-0.5, 0.5), QPointF(-0.5, -0.5), QPointF(0, 0.1),
                                       QPointF(0.5, -0.5), QPointF(0.5, 0.5)};
        p.drawPolyline(m.data(), 5);
    };
    if (key == "watr hydrant") {
        circle(0, 0, 1);
        line(-1, 0, 1, 0);
        line(0, -1, 0, 1);
    } else if (key == "watr stop valve") {
        triangle({QPointF(-1, -0.85), QPointF(-1, 0.85), QPointF(0, 0)});
        triangle({QPointF(1, -0.85), QPointF(1, 0.85), QPointF(0, 0)});
    } else if (key == "watr meter" || key == "gas meter") {
        meter();
    } else if (key == "watr air valve") {
        circle(0, 0, 0.55);
        line(0, -0.55, 0, -1);
        line(-0.5, -1, 0.5, -1);
    } else if (key == "sewr manhole cover") {
        rect(1, 1);
        circle(0, 0, 0.66);
    } else if (key == "comm telephone single concrete pit") {
        rect(1, 0.72);
    } else if (key == "comm telephone twin concrete pit") {
        rect(1, 0.72);
        line(0, -0.72, 0, 0.72);
    } else if (key == "comm telephone distribution pillar") {
        rect(0.62, 1);
        p.fillRect(QRectF(-0.62, -1, 1.24, 0.34), ink);
    } else if (key == "comm telephone large sump") {
        rect(1.15, 0.85);
        line(-1.15, 0.85, 1.15, -0.85);
    } else if (key == "elec cable junction box") {
        rect(1, 1);
        line(-1, 1, 1, -1);
    } else if (key == "elec pole - power and light") {
        circle(0, 0, 0.42, true);
        line(0, 0, 1.15, 0);
        line(1.15, -0.32, 1.15, 0.32);
        line(0, 0, -0.88, 0);
        circle(-1.12, 0, 0.24);
    } else if (key == "elec pole - light") {
        circle(0, 0, 0.42, true);
        line(0, 0, 1.08, 0);
        circle(1.32, 0, 0.26);
    } else if (key == "gas valve box") {
        rect(1, 1);
        triangle({QPointF(-0.6, -0.5), QPointF(-0.6, 0.5), QPointF(0, 0)});
        triangle({QPointF(0.6, -0.5), QPointF(0.6, 0.5), QPointF(0, 0)});
    } else if (key == "gas test point") {
        circle(0, 0, 1);
        line(-0.5, -0.4, 0.5, -0.4);
        line(0, -0.4, 0, 0.6);
    } else {
        rect(0.8, 0.8); // a symbol with no glyph: an open square, as the app drew it
    }
}

// ---- the frame -----------------------------------------------------------------

// The smallest cell of the frame holding `point`: the room a left-anchored
// field has to its right.
const plotting::FrameCell* cellHolding(const plotting::Frame& frame, const Point2& point)
{
    const plotting::FrameCell* best = nullptr;
    double bestArea = 0.0;
    for (const plotting::FrameCell& cell : frame.cells) {
        if (cell.id == "drawing_area" || !cell.rect.contains(point)) {
            continue;
        }
        const double area = cell.rect.width() * cell.rect.height();
        if (best == nullptr || area < bestArea) {
            best = &cell;
            bestArea = area;
        }
    }
    return best;
}

void paintFrame(QPainter& painter, const Paper& paper, TextSetter& text,
                const plotting::Frame& frame, const plotting::Sheet& sheet,
                const std::map<std::string, std::string, std::less<>>& fields,
                const SheetSource& source, SheetPaintStats& stats)
{
    const SheetPaintOptions& options = paper.options();
    const auto shown = [&](FrameRole role) {
        return role != FrameRole::Legend || sheet.frameLegend;
    };

    // Lines.
    for (const plotting::FramePolyline& line : frame.polylines) {
        if (!shown(line.role) || (!line.plots && !options.construction) || line.points.size() < 2) {
            continue;
        }
        QPolygonF polygon;
        for (const Point2& point : line.points) {
            polygon << paper.at(point);
        }
        painter.setPen(line.dashMm > 0.0 && line.gapMm > 0.0
                           ? dashedPen(paper, qColour(line.colour), line.weightMm,
                                       {line.dashMm, line.gapMm})
                           : paperPen(paper, qColour(line.colour), line.weightMm));
        painter.setBrush(Qt::NoBrush);
        if (line.closed) {
            painter.drawPolygon(polygon);
        } else {
            painter.drawPolyline(polygon);
        }
    }

    // Legend glyphs.
    if (sheet.frameLegend) {
        for (const plotting::FrameSymbol& symbol : frame.symbols) {
            painter.save();
            painter.translate(paper.at(symbol.at));
            painter.rotate(-symbol.rotationDegrees);
            const double size = paper.mm(std::max(symbol.sizeMm, 0.01));
            painter.scale(size, size);
            glyph(painter, symbol.style, qColour(symbol.colour), 0.18 / std::max(symbol.sizeMm, 0.01));
            painter.restore();
        }
    }

    // The logo, in the slot the original organisation's logo was taken out of.
    if (const plotting::FrameCell* cell = frame.cell("logo"); cell != nullptr) {
        if (!source.logo.isNull()) {
            const Box2 at = plotting::fitImage(cell->rect, source.logo.width(), source.logo.height());
            if (!at.empty()) {
                painter.save();
                painter.setRenderHint(QPainter::SmoothPixmapTransform, true);
                painter.drawImage(paper.at(at), source.logo);
                painter.restore();
                stats.logoDrawn = true;
            }
        } else if (options.slotHints) {
            TextStyle hint;
            hint.capMm = 2.0 * frame.scale;
            hint.horizontal = HorizontalJustify::Centre;
            hint.vertical = VerticalJustify::Middle;
            hint.colour = kFaint;
            text.draw(cell->rect.center(), QStringLiteral("YOUR LOGO"), hint);
        }
    }

    // Texts, fields filled.
    for (const plotting::FrameText& item : frame.texts) {
        if (!shown(item.role)) {
            continue;
        }
        TextStyle style;
        style.capMm = item.capHeightMm;
        style.xFactor = item.xFactor;
        style.horizontal = item.horizontal;
        style.vertical = item.vertical;
        style.angleDegrees = item.angleDegrees;
        style.lineSpacingMm = item.lineSpacingMm;
        style.bold = item.bold;
        style.colour = qColour(item.colour);
        style.face = QString::fromStdString(item.fontFace);

        QString content = item.fields.empty()
                              ? QString::fromStdString(item.content)
                              : QString::fromStdString(plotting::expandTemplate(item.content, fields));
        Point2 anchor = item.anchor;
        double room = 0.0; // the width the text must fit, when it has a cell

        if (!item.centredIn.empty()) {
            if (const plotting::FrameCell* cell = frame.cell(item.centredIn); cell != nullptr) {
                anchor = cell->rect.center();
                style.horizontal = HorizontalJustify::Centre;
                style.vertical = VerticalJustify::Middle;
                room = cell->rect.width() - 0.6 * frame.scale;
                content = content.trimmed();
            }
        } else if (item.role == FrameRole::Slot && item.horizontal == HorizontalJustify::Centre) {
            if (const plotting::FrameCell* cell = cellHolding(frame, anchor); cell != nullptr) {
                room = cell->rect.width() - 1.0 * frame.scale;
            }
        } else if (item.angleDegrees == 0.0 && item.horizontal == HorizontalJustify::Left) {
            if (const plotting::FrameCell* cell = cellHolding(frame, anchor); cell != nullptr) {
                room = cell->rect.max.x - anchor.x - 0.4 * frame.scale;
            }
        }

        if (content.trimmed().isEmpty()) {
            if (item.role == FrameRole::Slot && options.slotHints &&
                item.fields == std::vector<std::string>{"organisation"}) {
                TextStyle hint = style;
                hint.colour = kFaint;
                text.draw(anchor, QStringLiteral("ORGANISATION"), hint);
            }
            continue;
        }

        // The notes slot holds a paragraph: wrapped to its cell, as many
        // lines as fit above the cell's foot.
        if (item.role == FrameRole::Slot && item.horizontal == HorizontalJustify::Left &&
            room > 0.0) {
            QStringList lines = text.wrap(content, style, room);
            if (const plotting::FrameCell* cell = cellHolding(frame, anchor); cell != nullptr) {
                const double spacing = item.lineSpacingMm > 0.0 ? item.lineSpacingMm
                                                                : 1.5 * item.capHeightMm;
                const auto fit =
                    static_cast<qsizetype>(std::floor((anchor.y - cell->rect.min.y) / spacing)) + 1;
                if (lines.size() > fit) {
                    lines = lines.mid(0, std::max<qsizetype>(fit, 1));
                }
            }
            text.draw(anchor, lines.join(QLatin1Char('\n')), style);
            ++stats.frameTextsDrawn;
            continue;
        }

        // A single line wider than its room is squeezed to fit, as the app
        // did, rather than running over the next rule.
        double squeeze = 1.0;
        if (room > 0.0 && !content.contains(QLatin1Char('\n'))) {
            const double width = text.widthMm(content, style);
            if (width > room) {
                squeeze = room / width;
            }
        }
        text.draw(anchor, content, style, squeeze);
        ++stats.frameTextsDrawn;
    }
}

// ---- viewport furniture ----------------------------------------------------------

// A round number of metres at most `maximum`: 1, 2 or 5 times a power of ten.
double niceAtMost(double maximum)
{
    if (!(maximum > 0.0)) {
        return 0.0;
    }
    const double magnitude = std::pow(10.0, std::floor(std::log10(maximum)));
    for (const double multiple : {5.0, 2.0, 1.0}) {
        if (magnitude * multiple <= maximum * (1.0 + 1e-9)) {
            return magnitude * multiple;
        }
    }
    return magnitude;
}

// A round number at least `minimum`.
double niceAtLeast(double minimum)
{
    if (!(minimum > 0.0)) {
        return 1.0;
    }
    const double magnitude = std::pow(10.0, std::floor(std::log10(minimum)));
    for (const double multiple : {1.0, 2.0, 5.0, 10.0}) {
        if (magnitude * multiple >= minimum * (1.0 - 1e-9)) {
            return magnitude * multiple;
        }
    }
    return magnitude * 10.0;
}

QString number(double value, double step)
{
    int decimals = 0;
    if (step < 1.0) {
        decimals = std::clamp(static_cast<int>(std::ceil(-std::log10(step) - 1e-9)), 1, 3);
    }
    return QString::number(value, 'f', decimals);
}

QString metres(double length)
{
    return length >= 1000.0 && std::fmod(length, 1000.0) == 0.0
               ? QString("%1 km").arg(length / 1000.0)
               : QString("%1 m").arg(length);
}

// The paper a plan's furniture knocks out white - the scale bar, the north
// arrow, the title - and so what a coordinate grid's labels keep off. Each
// paint function knocks out exactly its box, so the two cannot disagree.
Box2 scaleBarBox(const Box2& rect, double scale)
{
    const double mmPerMetre = 1000.0 / scale;
    const double lengthMm = niceAtMost(std::min(50.0, 0.4 * rect.width()) / mmPerMetre) * mmPerMetre;
    const double right = rect.max.x - 4.0;
    const double base = rect.min.y + 5.0;
    return Box2(Point2(right - lengthMm - 3.0, base - 4.2), Point2(right + 3.0, base + 1.2 + 3.2));
}

constexpr double kNorthArrowRadius = 4.5;

Point2 northArrowCentre(const Box2& rect)
{
    return Point2(rect.max.x - kNorthArrowRadius - 3.5, rect.max.y - kNorthArrowRadius - 5.5);
}

Box2 northArrowBox(const Box2& rect)
{
    const Point2 centre = northArrowCentre(rect);
    return Box2(Point2(centre.x - kNorthArrowRadius - 1.0, centre.y - kNorthArrowRadius - 1.0),
                Point2(centre.x + kNorthArrowRadius + 1.0, centre.y + kNorthArrowRadius + 4.5));
}

void paintScaleBar(QPainter& painter, const Paper& paper, TextSetter& text, const Box2& rect,
                   double scale)
{
    const double mmPerMetre = 1000.0 / scale;
    const double maximumMm = std::min(50.0, 0.4 * rect.width());
    const double length = niceAtMost(maximumMm / mmPerMetre);
    if (!(length > 0.0)) {
        return;
    }
    const double lengthMm = length * mmPerMetre;
    const double first = length / std::pow(10.0, std::floor(std::log10(length)));
    const int divisions = std::lround(first) == 5 ? 5 : 4;
    const double barH = 1.2;
    const double right = rect.max.x - 4.0;
    const double left = right - lengthMm;
    const double base = rect.min.y + 5.0;

    TextStyle label;
    label.capMm = 1.6;
    label.xFactor = 0.9;
    label.horizontal = HorizontalJustify::Centre;
    knockOut(painter, paper, scaleBarBox(rect, scale));
    painter.setPen(paperPen(paper, kInk, 0.18));
    for (int i = 0; i < divisions; ++i) {
        const double x0 = left + lengthMm * i / divisions;
        const double x1 = left + lengthMm * (i + 1) / divisions;
        const QRectF cell = paper.at(Box2(Point2(x0, base), Point2(x1, base + barH)));
        painter.setBrush(i % 2 == 0 ? QBrush(kInk) : QBrush(Qt::white));
        painter.drawRect(cell);
    }
    painter.setBrush(Qt::NoBrush);
    text.draw(Point2(left, base + barH + 0.8), QStringLiteral("0"), label);
    text.draw(Point2(left + lengthMm / 2.0, base + barH + 0.8),
              QString::number(length / 2.0), label);
    text.draw(Point2(right, base + barH + 0.8), metres(length), label);
    label.vertical = VerticalJustify::Top;
    text.draw(Point2((left + right) / 2.0, base - 0.8),
              QString("SCALE 1:%1").arg(QString::number(scale, 'f', 0)), label);
}

// North, drawn where the world's +Y points on the paper.
void paintNorthArrow(QPainter& painter, const Paper& paper, TextSetter& text, const Box2& rect,
                     double rotation)
{
    const double radius = kNorthArrowRadius;
    const Point2 centre = northArrowCentre(rect);
    const double north = katana::math::kPi / 2.0 - rotation; // paper angle of world +Y
    const auto at = [&](double along, double across) {
        return paper.at(centre + rotated(Point2(along, across), north));
    };
    knockOut(painter, paper, northArrowBox(rect));
    painter.setPen(paperPen(paper, kInk, 0.25));
    painter.setBrush(Qt::NoBrush);
    painter.drawEllipse(paper.at(centre), paper.mm(radius), paper.mm(radius));
    const QPointF tip = at(radius + 0.8, 0.0);
    const QPointF tail = at(-radius + 0.8, 0.0);
    const QPointF left = at(-radius * 0.6, radius * 0.45);
    const QPointF right = at(-radius * 0.6, -radius * 0.45);
    painter.setBrush(kInk);
    painter.drawPolygon(QPolygonF{tip, left, tail});
    painter.setBrush(Qt::white);
    painter.drawPolygon(QPolygonF{tip, right, tail});
    painter.setBrush(Qt::NoBrush);
    TextStyle n;
    n.capMm = 2.5;
    n.bold = true;
    n.horizontal = HorizontalJustify::Centre;
    n.vertical = VerticalJustify::Middle;
    text.draw(centre + rotated(Point2(radius + 2.6, 0.0), north), QStringLiteral("N"), n);
}

TextStyle titleStyle()
{
    TextStyle style;
    style.capMm = 2.5;
    style.bold = true;
    style.xFactor = 0.9;
    return style;
}

// A coordinate grid's labels: small, narrow, in ink.
TextStyle gridLabelStyle()
{
    TextStyle style;
    style.capMm = 1.8;
    style.xFactor = 0.9;
    return style;
}

// What a view is titled: its own title, else the automatic one at `scale`.
QString viewTitle(const Viewport& viewport, double scale)
{
    if (!viewport.title.empty()) {
        return QString::fromStdString(viewport.title);
    }
    Viewport titled = viewport;
    titled.scale = scale;
    return QString::fromStdString(plotting::automaticTitle(titled));
}

// Where the title is set, and the box it knocks out; nothing when it is not
// drawn (no title, or no room for one).
struct TitlePlace {
    Point2 at;
    double widthMm = 0.0;
    Box2 box;
};

std::optional<TitlePlace> titlePlace(TextSetter& text, const Box2& rect, const QString& title)
{
    if (title.isEmpty()) {
        return std::nullopt;
    }
    const double width = std::min(text.widthMm(title, titleStyle()), rect.width() - 6.0);
    if (!(width > 0.0)) {
        return std::nullopt;
    }
    const Point2 at(rect.min.x + 3.0, rect.min.y + 3.5);
    return TitlePlace{at, width,
                      Box2(Point2(at.x - 1.0, at.y - 1.6), Point2(at.x + width + 1.0, at.y + 3.4))};
}

// The viewport's name and scale under its bottom-left corner, inside it.
void paintTitle(QPainter& painter, const Paper& paper, TextSetter& text, const Box2& rect,
                const QString& title)
{
    const auto place = titlePlace(text, rect, title);
    if (!place) {
        return;
    }
    const TextStyle style = titleStyle();
    const double width = place->widthMm;
    const Point2 at = place->at;
    knockOut(painter, paper, place->box);
    double squeeze = 1.0;
    if (const double full = text.widthMm(title, style); full > width) {
        squeeze = width / full;
    }
    text.draw(at, title, style, squeeze);
    painter.setPen(paperPen(paper, kInk, 0.35));
    painter.drawLine(paper.at(Point2(at.x, at.y - 0.9)), paper.at(Point2(at.x + width, at.y - 0.9)));
}

void paintMessage(TextSetter& text, const Box2& rect, const QString& message)
{
    TextStyle style;
    style.capMm = 2.0;
    style.horizontal = HorizontalJustify::Centre;
    style.vertical = VerticalJustify::Middle;
    style.colour = kFaint;
    text.draw(rect.center(), message, style);
}

// A table laid out by cad/plotting/tables.hpp - the register, the revisions -
// drawn exactly where the layout put each rule and text: shading, then
// rules, then text.
void paintTable(QPainter& painter, const Paper& paper, TextSetter& text,
                const plotting::TableLayout& table)
{
    const QColor shade(232, 232, 232);
    for (const Box2& box : table.shaded) {
        painter.fillRect(paper.at(box), shade);
    }
    for (const plotting::TableRule& rule : table.rules) {
        painter.setPen(paperPen(paper, kInk, rule.weightMm));
        painter.drawLine(paper.at(rule.from), paper.at(rule.to));
    }
    for (const plotting::TableText& item : table.texts) {
        TextStyle style;
        style.capMm = item.capMm;
        style.xFactor = item.xFactor;
        style.horizontal = item.horizontal;
        style.bold = item.bold;
        style.colour = item.muted ? kFaint : kInk;
        text.draw(item.anchor, QString::fromStdString(item.text), style, item.squeeze);
    }
}

// ---- sections ----------------------------------------------------------------------

// The level of `surface` at `station`, interpolated; nothing in a gap.
std::optional<double> levelAt(const katana::cad::SectionSurface& surface, double station)
{
    const auto& samples = surface.samples;
    const auto after = std::lower_bound(samples.begin(), samples.end(), station,
                                        [](const katana::cad::SectionSample& s, double at) {
                                            return s.station < at;
                                        });
    if (after == samples.end()) {
        return std::nullopt;
    }
    if (after->station == station || after == samples.begin()) {
        return after->station == station ? after->elevation : std::nullopt;
    }
    const auto before = std::prev(after);
    if (!before->elevation || !after->elevation) {
        return std::nullopt;
    }
    const double t = (station - before->station) / (after->station - before->station);
    return *before->elevation + t * (*after->elevation - *before->elevation);
}

bool isDesign(const katana::cad::SectionSurface& surface)
{
    return std::string_view(surface.name).starts_with("design");
}

const std::array<QColor, 5> kGroundInks{QColor(125, 80, 40), QColor(0, 125, 50),
                                        QColor(0, 90, 200), QColor(150, 0, 150),
                                        QColor(210, 120, 0)};
const QColor kDesignInk(210, 0, 0);

struct SectionAxes {
    double scale = 500.0;        // 1 : scale horizontally
    double exaggeration = 1.0;   // the vertical scale is scale / exaggeration
    Point2 centre{};             // (axis value, elevation) at the plot's middle
    bool autoCentre = false;
    double shift = 0.0;          // axis value = station + shift
    std::optional<std::pair<double, double>> range; // a long section's chainages
    bool cross = false;          // a cross section: the centreline at 0
    bool dataBand = false;       // a long section's levels under it
    const katana::cad::LayerOverrides* hidden = nullptr;
    QString caption;             // under a cross section: its chainage
};

void paintSection(QPainter& painter, const Paper& paper, TextSetter& text, const Box2& area,
                  const katana::cad::Section& section, const SectionAxes& axes)
{
    // Room for the levels on the left, the axis values (and a long
    // section's data band) below, and a caption under a cross section.
    const std::size_t bandRows =
        axes.dataBand ? std::min<std::size_t>(section.surfaces.size(), 2) + 1 : 0;
    const double rowH = 8.0;
    double bottom = axes.dataBand ? static_cast<double>(bandRows) * rowH : 4.0;
    if (!axes.caption.isEmpty()) {
        bottom += 4.5;
    }
    const double left = axes.dataBand ? 22.0 : 11.0;
    Box2 plot(Point2(area.min.x + left, area.min.y + bottom + 1.0),
              Point2(area.max.x - 2.0, area.max.y - 2.0));
    const bool banded = axes.dataBand && plot.height() >= 30.0;
    if (axes.dataBand && !banded) {
        plot.min.y = area.min.y + 5.0 + (axes.caption.isEmpty() ? 0.0 : 4.5);
    }
    if (!(plot.width() > 10.0) || !(plot.height() > 8.0)) {
        return;
    }

    const double hMm = 1000.0 / axes.scale;
    const double vMm = hMm * axes.exaggeration;
    Point2 centre = axes.centre;
    if (axes.autoCentre) {
        const Box2 extent = section.extent();
        if (!extent.empty()) {
            centre.y = extent.center().y;
            centre.x = axes.cross ? 0.0 : extent.center().x + axes.shift;
        }
        if (axes.range && axes.range->second > axes.range->first) {
            centre.x = 0.5 * (axes.range->first + axes.range->second);
        }
    }
    const auto X = [&](double value) { return plot.center().x + (value - centre.x) * hMm; };
    const auto Y = [&](double level) { return plot.center().y + (level - centre.y) * vMm; };
    const double firstValue = centre.x - 0.5 * plot.width() / hMm;
    const double lastValue = centre.x + 0.5 * plot.width() / hMm;
    const double lowLevel = centre.y - 0.5 * plot.height() / vMm;
    const double highLevel = centre.y + 0.5 * plot.height() / vMm;
    const double hStep = niceAtLeast(12.0 / hMm);
    const double vStep = niceAtLeast(7.0 / vMm);

    // Grid.
    const QRectF plotDevice = paper.at(plot);
    painter.save();
    painter.setClipRect(plotDevice, Qt::IntersectClip);
    painter.setPen(paperPen(paper, kGridInk, 0.13));
    int drawn = 0;
    for (double v = std::ceil(firstValue / hStep) * hStep; v <= lastValue && drawn < 400;
         v += hStep, ++drawn) {
        painter.drawLine(paper.at(Point2(X(v), plot.min.y)), paper.at(Point2(X(v), plot.max.y)));
    }
    drawn = 0;
    for (double e = std::ceil(lowLevel / vStep) * vStep; e <= highLevel && drawn < 400;
         e += vStep, ++drawn) {
        painter.drawLine(paper.at(Point2(plot.min.x, Y(e))), paper.at(Point2(plot.max.x, Y(e))));
    }

    // The centreline of a cross section.
    if (axes.cross && X(0.0) > plot.min.x && X(0.0) < plot.max.x) {
        painter.setPen(dashedPen(paper, kInk, 0.25, {6.0, 1.2, 0.6, 1.2}));
        painter.drawLine(paper.at(Point2(X(0.0), plot.min.y)), paper.at(Point2(X(0.0), plot.max.y)));
    }

    // Crossings: where the drawing's lines cut the section, at their level
    // when they carry one.
    TextStyle small;
    small.capMm = 1.4;
    small.xFactor = 0.9;
    for (const katana::cad::SectionCrossing& crossing : section.crossings) {
        if (axes.hidden != nullptr && axes.hidden->hides(crossing.layer)) {
            continue;
        }
        const double x = X(crossing.station + axes.shift);
        if (x <= plot.min.x || x >= plot.max.x) {
            continue;
        }
        painter.setPen(dashedPen(paper, kFaint, 0.18, {1.5, 1.0}));
        painter.drawLine(paper.at(Point2(x, plot.min.y)), paper.at(Point2(x, plot.max.y)));
        if (crossing.elevation) {
            painter.setPen(paperPen(paper, kInk, 0.25));
            painter.setBrush(Qt::white);
            painter.drawEllipse(paper.at(Point2(x, Y(*crossing.elevation))), paper.mm(0.8),
                                paper.mm(0.8));
            painter.setBrush(Qt::NoBrush);
        }
        TextStyle label = small;
        label.angleDegrees = 90.0;
        label.horizontal = HorizontalJustify::Right;
        text.draw(Point2(x - 0.4, plot.max.y - 1.0), QString::fromStdString(crossing.layer), label);
    }

    // Surfaces, broken at every gap.
    std::size_t ground = 0;
    for (const katana::cad::SectionSurface& surface : section.surfaces) {
        const bool design = isDesign(surface);
        painter.setPen(paperPen(paper, design ? kDesignInk : kGroundInks[ground % kGroundInks.size()],
                                design ? 0.5 : 0.35));
        if (!design) {
            ++ground;
        }
        QPolygonF run;
        const auto flush = [&] {
            if (run.size() > 1) {
                painter.drawPolyline(run);
            }
            run.clear();
        };
        for (const katana::cad::SectionSample& sample : surface.samples) {
            if (!sample.elevation) {
                flush();
                continue;
            }
            run << paper.at(Point2(X(sample.station + axes.shift), Y(*sample.elevation)));
        }
        flush();
    }
    painter.restore();

    // Frame and labels.
    painter.setPen(paperPen(paper, kInk, 0.25));
    painter.drawRect(plotDevice);
    TextStyle levels = small;
    levels.capMm = 1.5;
    levels.horizontal = HorizontalJustify::Right;
    levels.vertical = VerticalJustify::Middle;
    drawn = 0;
    for (double e = std::ceil(lowLevel / vStep) * vStep; e <= highLevel && drawn < 200;
         e += vStep, ++drawn) {
        text.draw(Point2(plot.min.x - 0.8, Y(e)), number(e, vStep), levels);
    }
    // The datum inside the plot's foot: the left band is only as wide as a level.
    TextStyle datum = small;
    datum.capMm = 1.5;
    const QString datumText = QString("DATUM RL %1").arg(lowLevel, 0, 'f', 2);
    const Point2 datumAt(plot.min.x + 1.0, plot.min.y + 1.0);
    knockOut(painter, paper, Box2(Point2(datumAt.x - 0.4, datumAt.y - 0.5),
                                  Point2(datumAt.x + text.widthMm(datumText, datum) + 0.4,
                                         datumAt.y + 2.0)));
    text.draw(datumAt, datumText, datum);

    // Surface key, top left inside the plot.
    {
        TextStyle key = small;
        key.capMm = 1.5;
        key.vertical = VerticalJustify::Middle;
        double y = plot.max.y - 2.5;
        double widest = 0.0;
        for (const auto& surface : section.surfaces) {
            widest = std::max(widest, text.widthMm(QString::fromStdString(surface.name), key));
        }
        if (!section.surfaces.empty()) {
            knockOut(painter, paper,
                     Box2(Point2(plot.min.x + 0.6, y - 3.2 * (section.surfaces.size() - 1) - 1.8),
                          Point2(plot.min.x + 9.5 + widest, plot.max.y - 0.6)));
        }
        ground = 0;
        for (const auto& surface : section.surfaces) {
            const bool design = isDesign(surface);
            painter.setPen(paperPen(paper, design ? kDesignInk
                                                  : kGroundInks[ground % kGroundInks.size()],
                                    design ? 0.5 : 0.35));
            if (!design) {
                ++ground;
            }
            painter.drawLine(paper.at(Point2(plot.min.x + 1.5, y)),
                             paper.at(Point2(plot.min.x + 7.0, y)));
            text.draw(Point2(plot.min.x + 8.0, y), QString::fromStdString(surface.name), key);
            y -= 3.2;
        }
    }

    // Axis values, or the data band: each series' level at every grid line,
    // and the chainage.
    TextStyle values = small;
    values.capMm = 1.5;
    values.horizontal = HorizontalJustify::Centre;
    values.vertical = VerticalJustify::Top;
    if (!banded) {
        drawn = 0;
        for (double v = std::ceil(firstValue / hStep) * hStep; v <= lastValue && drawn < 200;
             v += hStep, ++drawn) {
            text.draw(Point2(X(v), plot.min.y - 0.8), number(v, hStep), values);
        }
    } else {
        // Design first: the level a long section is read for.
        std::vector<const katana::cad::SectionSurface*> rows;
        for (const auto& surface : section.surfaces) {
            if (isDesign(surface)) {
                rows.push_back(&surface);
            }
        }
        for (const auto& surface : section.surfaces) {
            if (!isDesign(surface)) {
                rows.push_back(&surface);
            }
        }
        rows.resize(std::min<std::size_t>(rows.size(), 2));
        const double bandTop = plot.min.y - 1.0;
        const double bandLeft = area.min.x + 0.5;
        painter.setPen(paperPen(paper, kInk, 0.18));
        TextStyle head = small;
        head.capMm = 1.5;
        head.vertical = VerticalJustify::Middle;
        TextStyle cell = small;
        cell.capMm = 1.4;
        cell.angleDegrees = 90.0;
        cell.vertical = VerticalJustify::Middle;
        cell.horizontal = HorizontalJustify::Centre;
        for (std::size_t r = 0; r <= rows.size(); ++r) {
            const double top = bandTop - static_cast<double>(r) * rowH;
            const Box2 row(Point2(bandLeft, top - rowH), Point2(plot.max.x, top));
            painter.setPen(paperPen(paper, kInk, 0.18));
            painter.drawRect(paper.at(row));
            painter.drawLine(paper.at(Point2(plot.min.x, top)), paper.at(Point2(plot.min.x, top - rowH)));
            const bool chainage = r == rows.size();
            QString name = chainage ? QStringLiteral("CHAINAGE")
                                    : QString::fromStdString(rows[r]->name).toUpper();
            const double room = plot.min.x - bandLeft - 1.0;
            double squeeze = 1.0;
            if (const double w = text.widthMm(name, head); w > room) {
                squeeze = room / w;
            }
            text.draw(Point2(bandLeft + 0.6, top - rowH / 2.0), name, head, squeeze);
            drawn = 0;
            for (double v = std::ceil(firstValue / hStep) * hStep; v <= lastValue && drawn < 200;
                 v += hStep, ++drawn) {
                QString value;
                if (chainage) {
                    value = number(v, hStep);
                } else if (auto level = levelAt(*rows[r], v - axes.shift)) {
                    value = QString::number(*level, 'f', 3);
                }
                text.draw(Point2(X(v), top - rowH / 2.0), value, cell);
            }
        }
    }

    if (!axes.caption.isEmpty()) {
        TextStyle caption;
        caption.capMm = 2.0;
        caption.bold = true;
        caption.horizontal = HorizontalJustify::Centre;
        text.draw(Point2(plot.center().x, area.min.y + 1.2), axes.caption, caption);
    }
}

} // namespace

// ---- the painter ---------------------------------------------------------------------

class SheetPainter {
  public:
    SheetPainter(QPainter& painter, const plotting::SheetSet& set, std::size_t index,
                 const SheetSource& source, const SheetPaintOptions& options, SheetPaintCache& cache)
        : painter_(painter), set_(set), index_(index), source_(source), options_(options),
          cache_(cache)
    {
    }

    SheetPaintStats run();

  private:
    void paintViewport(const Viewport& viewport, TextSetter& text, const Paper& paper);
    bool paintPlanViewport(const Viewport& viewport, TextSetter& text, const Paper& paper,
                           double& scaleUsed);
    bool paintSections(const Viewport& viewport, TextSetter& text, const Paper& paper);
    bool paintSnapshot(const Viewport& viewport, const Paper& paper);
    void paintLegend(const Viewport& viewport, TextSetter& text, const Paper& paper);
    void paintNotes(const Viewport& viewport, TextSetter& text, const Paper& paper);
    bool paintImage(const Viewport& viewport, const Paper& paper);
    void paintTableViewport(const Viewport& viewport, TextSetter& text, const Paper& paper);
    void paintMarks(const Viewport& viewport, const ResolvedViewport& at, TextSetter& text,
                    const Paper& paper);
    // The coordinate grid (plan_grid.hpp): worked out once per viewport, its
    // strokes under the marks and its labels over them.
    std::optional<plotting::PlanGrid> planGridFor(const Viewport& viewport,
                                                  const ResolvedViewport& at, TextSetter& text);
    void paintGridStrokes(const plotting::PlanGrid& grid, const Paper& paper);
    void paintGridLabels(const plotting::PlanGrid& grid, TextSetter& text, const Paper& paper);
    // A key plan's outlines, live (key_plan.hpp).
    void paintKeyPlan(const Viewport& viewport, const ResolvedViewport& at, TextSetter& text,
                      const Paper& paper);
    // Decides an automatic plan's placement for the key plans, as it is drawn.
    [[nodiscard]] plotting::PlanPlacer placer() const
    {
        return [this](const Viewport& viewport) {
            const ResolvedViewport at = resolvePlanViewport(viewport, source_);
            return plotting::PlanPlacement{at.scale, at.centre};
        };
    }
    void problem(const Viewport& viewport, std::string what)
    {
        stats_.problems.push_back(viewport.id + ": " + std::move(what));
    }

    const katana::cad::Section* longSection(const std::string& alignment, std::string& failure,
                                            double& start);
    const katana::cad::Section* crossSection(const std::string& alignment, double chainage,
                                             double halfWidth, std::string& failure);
    [[nodiscard]] std::vector<katana::cad::SectionSurfaceInput> surfaceInputs() const;

    QPainter& painter_;
    const plotting::SheetSet& set_;
    std::size_t index_;
    const SheetSource& source_;
    const SheetPaintOptions& options_;
    SheetPaintCache& cache_;
    SheetPaintStats stats_;
};

std::vector<katana::cad::SectionSurfaceInput> SheetPainter::surfaceInputs() const
{
    std::vector<katana::cad::SectionSurfaceInput> inputs;
    for (const katana::cad::SceneSurface& surface : source_.surfaces) {
        if (surface.visible && surface.surface != nullptr) {
            inputs.push_back({surface.name, surface.surface});
        }
    }
    return inputs;
}

const katana::cad::Section* SheetPainter::longSection(const std::string& name,
                                                      std::string& failure, double& start)
{
    const std::string key = "L|" + name;
    auto& entry = cache_.sections_[key];
    if (entry.revision != source_.revision || (!entry.section && entry.failure.empty())) {
        entry = {};
        entry.revision = source_.revision;
        const katana::entity::Model* model = source_.plan.model;
        const katana::entity::Alignment* alignment =
            model != nullptr ? model->alignments.find(name) : nullptr;
        if (alignment == nullptr) {
            entry.failure = "no alignment named " + name;
        } else if (auto solved = katana::geometry::solveAlignment(alignment->horizontal); !solved) {
            entry.failure = solved.error().describe();
        } else {
            entry.startChainage = solved->startStation();
            const auto inputs = surfaceInputs();
            if (inputs.empty() && !alignment->vertical) {
                entry.failure = "no surface to cut and no design profile";
            } else {
                const auto line = solved->toPolyline(0.01);
                katana::cad::SectionOptions options;
                options.interval = std::max(line.length() / 2000.0, 0.1);
                options.spatialIndex = source_.plan.index;
                auto section = katana::cad::extractSection(line, inputs, model, options);
                if (section && alignment->vertical) {
                    if (auto profile = katana::geometry::solveProfile(*alignment->vertical)) {
                        if (auto status =
                                katana::cad::appendDesignProfile(*section, *profile, "design " + name);
                            !status) {
                            entry.failure = status.error().describe();
                        }
                    }
                }
                if (!section) {
                    entry.failure = section.error().describe();
                } else if (entry.failure.empty()) {
                    entry.section = std::move(*section);
                }
            }
        }
    }
    failure = entry.failure;
    start = entry.startChainage;
    return entry.section ? &*entry.section : nullptr;
}

const katana::cad::Section* SheetPainter::crossSection(const std::string& name, double chainage,
                                                       double halfWidth, std::string& failure)
{
    const std::string key = std::format("X|{}|{:.6f}|{:.6f}", name, chainage, halfWidth);
    auto& entry = cache_.sections_[key];
    if (entry.revision != source_.revision || (!entry.section && entry.failure.empty())) {
        entry = {};
        entry.revision = source_.revision;
        const katana::entity::Model* model = source_.plan.model;
        const katana::entity::Alignment* alignment =
            model != nullptr ? model->alignments.find(name) : nullptr;
        const auto inputs = surfaceInputs();
        if (alignment == nullptr) {
            entry.failure = "no alignment named " + name;
        } else if (inputs.empty()) {
            entry.failure = "no surface to cut: build one (Terrain > Surface From ...)";
        } else if (auto solved = katana::geometry::solveAlignment(alignment->horizontal); !solved) {
            entry.failure = solved.error().describe();
        } else {
            const auto left = solved->pointAtStationOffset(chainage, halfWidth);
            const auto right = solved->pointAtStationOffset(chainage, -halfWidth);
            if (!left || !right) {
                entry.failure = std::format("chainage {:.3f} is off the alignment", chainage);
            } else {
                katana::geometry::Polyline2 across;
                across.vertices = {*left, *right};
                katana::cad::SectionOptions options;
                options.interval = std::max(halfWidth / 100.0, 0.02);
                options.spatialIndex = source_.plan.index;
                auto section = katana::cad::extractSection(across, inputs, model, options);
                if (!section) {
                    entry.failure = section.error().describe();
                } else {
                    entry.section = std::move(*section);
                }
            }
        }
    }
    failure = entry.failure;
    return entry.section ? &*entry.section : nullptr;
}

bool SheetPainter::paintPlanViewport(const Viewport& viewport, TextSetter& text, const Paper& paper,
                                     double& scaleUsed)
{
    if (source_.plan.model == nullptr) {
        problem(viewport, "no drawing to show");
        return false;
    }
    const ResolvedViewport at = resolvePlanViewport(viewport, source_);
    scaleUsed = at.scale;
    const QRectF device = paper.at(viewport.rect);
    PlanFrame frame;
    frame.transform.center = at.centre;
    frame.transform.scale = std::clamp(paper.ppmm() * 1000.0 / at.scale,
                                       katana::cad::ViewTransform::kMinimumScale,
                                       katana::cad::ViewTransform::kMaximumScale);
    frame.transform.resize(device.width(), device.height());
    frame.origin = device.topLeft();
    frame.rotation = -viewport.rotation;
    frame.clip = true;
    frame.layers = &viewport.hiddenLayers;
    PlanPaintOptions plan;
    plan.medium = options_.medium;
    plan.pixelsPerMillimetre = paper.ppmm();
    plan.plot = &options_.plot;
    plan.rasterDpiCap = options_.rasterDpiCap; // imagery capped as the 3D snapshot is
    plan.fontFamily = QStringLiteral("Arial");
    const PlanPaintStats drawn = paintPlan(painter_, source_.plan, frame, plan, cache_.plan());
    stats_.planEntitiesDrawn += drawn.entitiesDrawn;

    if (viewport.kind == ViewportKind::KeyPlan) {
        // The drawing faded, so the sheets over it read first.
        painter_.fillRect(device, QColor(255, 255, 255, 165));
    }
    const std::optional<plotting::PlanGrid> grid = planGridFor(viewport, at, text);
    if (grid) {
        paintGridStrokes(*grid, paper);
    }
    if (viewport.kind == ViewportKind::KeyPlan) {
        paintKeyPlan(viewport, at, text, paper);
    }
    paintMarks(viewport, at, text, paper);
    if (grid) {
        paintGridLabels(*grid, text, paper);
    }
    return true;
}

std::optional<plotting::PlanGrid> SheetPainter::planGridFor(const Viewport& viewport,
                                                            const ResolvedViewport& at,
                                                            TextSetter& text)
{
    if (viewport.gridStyle == plotting::GridStyle::None) {
        return std::nullopt;
    }
    const TextStyle style = gridLabelStyle();
    plotting::PlanGridOptions options;
    options.labelCapMm = style.capMm;
    options.labelWidthMm = [&text, &style](std::string_view label) {
        return text.widthMm(QString::fromUtf8(label.data(), static_cast<qsizetype>(label.size())),
                            style);
    };
    // The furniture paintViewport draws over the plan, which the labels keep off.
    if (viewport.northArrow) {
        options.keepOut.push_back(northArrowBox(viewport.rect));
    }
    if (viewport.scaleBar) {
        options.keepOut.push_back(scaleBarBox(viewport.rect, at.scale));
    }
    if (const auto title = titlePlace(text, viewport.rect, viewTitle(viewport, at.scale))) {
        options.keepOut.push_back(title->box);
    }
    auto grid = plotting::planGrid(viewport, {at.scale, at.centre}, options);
    if (!grid) {
        problem(viewport, grid.error().message);
        return std::nullopt;
    }
    return std::move(*grid);
}

void SheetPainter::paintGridStrokes(const plotting::PlanGrid& grid, const Paper& paper)
{
    // Lines light and fine, under everything; crosses and ticks in ink, as
    // they are few and must be found.
    painter_.setPen(grid.style == plotting::GridStyle::Lines ? paperPen(paper, kGridInk, 0.13)
                                                             : paperPen(paper, kInk, 0.18));
    painter_.setBrush(Qt::NoBrush);
    for (const plotting::GridSegment& stroke : grid.strokes) {
        painter_.drawLine(paper.at(stroke.from), paper.at(stroke.to));
    }
}

void SheetPainter::paintGridLabels(const plotting::PlanGrid& grid, TextSetter& text,
                                   const Paper& paper)
{
    TextStyle style = gridLabelStyle();
    for (const plotting::GridLabel& label : grid.labels) {
        knockOut(painter_, paper, label.box);
        style.angleDegrees = label.angleDegrees;
        style.horizontal = label.horizontal;
        style.vertical = label.vertical;
        text.draw(label.anchor, QString::fromStdString(label.text), style);
    }
}

void SheetPainter::paintKeyPlan(const Viewport& viewport, const ResolvedViewport& at,
                                TextSetter& text, const Paper& paper)
{
    const plotting::PlanPlacement placed{at.scale, at.centre};
    const auto outlines = plotting::keyPlanOutlines(set_, index_, placer());
    const auto onPaper = [&](const plotting::KeyPlanOutline& outline) {
        std::vector<Point2> points;
        for (const Point2& corner : outline.corners) {
            points.push_back(plotting::planWorldToPaper(viewport, placed, corner));
        }
        return points;
    };
    // The other sheets first, then this sheet's over them in a fill of its
    // own - "you are here" - so no neighbour's outline hides it.
    for (const bool current : {false, true}) {
        for (const plotting::KeyPlanOutline& outline : outlines) {
            if (outline.current != current) {
                continue;
            }
            QPolygonF ring;
            for (const Point2& point : onPaper(outline)) {
                ring << paper.at(point);
            }
            painter_.setPen(paperPen(paper, current ? kHereInk : kOutlineInk, current ? 0.5 : 0.35));
            painter_.setBrush(current ? kHereFill : kOutlineFill);
            painter_.drawPolygon(ring);
        }
    }
    painter_.setBrush(Qt::NoBrush);
    // The numbers after every fill, so none is covered; each sized to its
    // outline, so a small sheet's number stays inside it.
    for (const plotting::KeyPlanOutline& outline : outlines) {
        if (!outline.labelled || outline.label.empty()) {
            continue;
        }
        Box2 extent;
        for (const Point2& point : onPaper(outline)) {
            extent.expand(point);
        }
        TextStyle style;
        style.capMm = std::clamp(0.4 * std::min(extent.width(), extent.height()), 1.2, 3.0);
        style.bold = true;
        style.colour = outline.current ? kHereInk : kOutlineInk;
        style.horizontal = HorizontalJustify::Centre;
        style.vertical = VerticalJustify::Middle;
        text.draw(extent.center(), QString::fromStdString(outline.label), style);
    }
}

void SheetPainter::paintMarks(const Viewport& viewport, const ResolvedViewport& at,
                              TextSetter& text, const Paper& paper)
{
    if (viewport.marks.empty()) {
        return;
    }
    const double mmPerMetre = 1000.0 / at.scale;
    const Point2 middle = viewport.rect.center();
    const auto onPaper = [&](const Point2& world) {
        return middle + rotated(world - at.centre, -viewport.rotation) * mmPerMetre;
    };
    painter_.save();
    painter_.setClipRect(paper.at(viewport.rect), Qt::IntersectClip);
    for (const plotting::WorldMark& mark : viewport.marks) {
        // A key plan draws its sheets' outlines live (paintKeyPlan); the ones
        // stored when it was made go stale as the sheets change.
        if (mark.points.size() < 2 || (viewport.kind == ViewportKind::KeyPlan &&
                                       mark.kind == plotting::WorldMark::Kind::SheetOutline)) {
            continue;
        }
        QPolygonF line;
        std::vector<Point2> points;
        for (const Point2& world : mark.points) {
            points.push_back(onPaper(world));
            line << paper.at(points.back());
        }
        const QString label = QString::fromStdString(plotting::markLabel(set_, mark));
        if (mark.kind == plotting::WorldMark::Kind::SheetOutline) {
            const QColor ink(200, 0, 30);
            painter_.setPen(paperPen(paper, ink, 0.35));
            painter_.setBrush(QColor(200, 0, 30, 18));
            painter_.drawPolygon(line);
            painter_.setBrush(Qt::NoBrush);
            Point2 centroid;
            for (const Point2& p : points) {
                centroid = centroid + p;
            }
            centroid = centroid * (1.0 / static_cast<double>(points.size()));
            TextStyle style;
            style.capMm = 3.0;
            style.bold = true;
            style.colour = ink;
            style.horizontal = HorizontalJustify::Centre;
            style.vertical = VerticalJustify::Middle;
            text.draw(centroid, label, style);
            continue;
        }
        painter_.setPen(dashedPen(paper, kInk, 0.5, {8.0, 1.5, 1.0, 1.5}));
        painter_.drawPolyline(line);
        // The label along the line, turned to read from the bottom or right.
        const Point2 a = points.front();
        const Point2 b = points[1];
        double angle = std::atan2(b.y - a.y, b.x - a.x) * katana::math::kRadToDeg;
        if (angle > 90.0) {
            angle -= 180.0;
        } else if (angle <= -90.0) {
            angle += 180.0;
        }
        TextStyle style;
        style.capMm = 2.0;
        style.bold = true;
        style.angleDegrees = angle;
        style.horizontal = HorizontalJustify::Centre;
        const Point2 mid = (a + b) * 0.5;
        const Point2 lift = rotated(Point2(0.0, 1.0), angle * katana::math::kDegToRad);
        text.draw(mid + lift, label, style);
    }
    painter_.restore();
}

bool SheetPainter::paintSections(const Viewport& viewport, TextSetter& text, const Paper& paper)
{
    const plotting::ViewportSource& from = viewport.source;
    if (from.alignment.empty()) {
        problem(viewport, "no alignment chosen");
        return false;
    }
    SectionAxes axes;
    axes.scale = viewport.scale;
    axes.exaggeration = viewport.verticalExaggeration > 0.0 ? viewport.verticalExaggeration : 1.0;
    axes.centre = viewport.centre;
    axes.autoCentre = viewport.autoCentre;
    axes.hidden = &viewport.hiddenLayers;
    // Leave the title's corner to the title.
    Box2 area = viewport.rect;
    area.min.y += 7.0;

    if (viewport.kind == ViewportKind::LongSection) {
        std::string failure;
        double start = 0.0;
        const katana::cad::Section* section = longSection(from.alignment, failure, start);
        if (section == nullptr) {
            problem(viewport, failure);
            return false;
        }
        axes.shift = start;
        if (from.chainageTo > from.chainageFrom) {
            axes.range = std::pair{from.chainageFrom, from.chainageTo};
        }
        axes.dataBand = true;
        paintSection(painter_, paper, text, area, *section, axes);
        return true;
    }

    // Cross sections: the chainages asked for, one row each.
    std::vector<double> stations = from.stations;
    if (stations.empty() && from.sectionInterval > 0.0 && from.chainageTo > from.chainageFrom) {
        for (double at = from.chainageFrom; at <= from.chainageTo + 1e-9 && stations.size() < 64;
             at += from.sectionInterval) {
            stations.push_back(at);
        }
    }
    if (stations.empty()) {
        problem(viewport, "no chainage to cut a cross section at");
        return false;
    }
    const double halfWidth = from.sectionHalfWidth > 0.0 ? from.sectionHalfWidth : 20.0;
    axes.cross = true;
    const double rowHeight = area.height() / static_cast<double>(stations.size());
    bool any = false;
    for (std::size_t i = 0; i < stations.size(); ++i) {
        std::string failure;
        const katana::cad::Section* section =
            crossSection(from.alignment, stations[i], halfWidth, failure);
        if (section == nullptr) {
            problem(viewport, failure);
            continue;
        }
        const double top = area.max.y - static_cast<double>(i) * rowHeight;
        const Box2 row(Point2(area.min.x, top - rowHeight), Point2(area.max.x, top));
        SectionAxes rowAxes = axes;
        rowAxes.shift = -halfWidth;
        rowAxes.caption = QString("CH %1").arg(stations[i], 0, 'f', 3);
        paintSection(painter_, paper, text, row, *section, rowAxes);
        any = true;
    }
    return any;
}

bool SheetPainter::paintSnapshot(const Viewport& viewport, const Paper& paper)
{
    if (source_.document == nullptr) {
        problem(viewport, "no 3D scene to show");
        return false;
    }
    const double dpi = std::min(options_.rasterDpiCap, paper.ppmm() * 25.4);
    int width = std::max(static_cast<int>(viewport.rect.width() * dpi / 25.4), 16);
    int height = std::max(static_cast<int>(viewport.rect.height() * dpi / 25.4), 16);
    const double pixels = static_cast<double>(width) * height;
    if (pixels > static_cast<double>(options_.rasterPixelCap)) {
        const double shrink = std::sqrt(static_cast<double>(options_.rasterPixelCap) / pixels);
        width = std::max(static_cast<int>(width * shrink), 16);
        height = std::max(static_cast<int>(height * shrink), 16);
    }
    const std::string key = std::format("{}|{}x{}|{:.4f}|{:.4f}", viewport.id, width, height,
                                        viewport.tiltDegrees, viewport.rotation);
    auto& entry = cache_.snapshots_[key];
    if (entry.image.isNull() || entry.revision != source_.revision) {
        katana::cad::SceneOptions scene;
        scene.drawGrid = false;
        katana::cad::SceneBuilder builder;
        katana::cad::SceneLayers layers;
        static const std::vector<katana::cad::SceneMesh> kNoMeshes;
        const auto& meshes = source_.plan.meshes != nullptr ? *source_.plan.meshes : kNoMeshes;
        builder.buildTerrain(source_.surfaces, meshes, scene, layers);
        builder.buildEntities(*source_.document, source_.surfaces, scene, layers);
        if (layers.bounds.empty()) {
            problem(viewport, "the 3D scene is empty");
            return false;
        }
        katana::render::Camera camera;
        camera.setViewportSize(width, height);
        camera.setOrientation(-katana::math::kPi / 4.0 + viewport.rotation,
                              std::clamp(viewport.tiltDegrees, 1.0, 89.0) * katana::math::kDegToRad);
        camera.frame(layers.bounds);
        katana::render::Framebuffer framebuffer;
        if (auto status = framebuffer.resize(width, height); !status) {
            problem(viewport, status.error().describe());
            return false;
        }
        katana::render::Rasterizer rasterizer;
        katana::render::RenderOptions render;
        render.background = katana::render::rgba(255, 255, 255);
        if (auto rendered = katana::cad::renderLayers(layers, camera, rasterizer, framebuffer, render);
            !rendered) {
            problem(viewport, rendered.error().describe());
            return false;
        }
        entry.image = QImage(reinterpret_cast<const uchar*>(framebuffer.color().data()), width,
                             height, width * static_cast<int>(sizeof(katana::render::Rgba)),
                             QImage::Format_ARGB32)
                          .copy();
        entry.revision = source_.revision;
    }
    painter_.save();
    painter_.setRenderHint(QPainter::SmoothPixmapTransform, true);
    painter_.drawImage(paper.at(viewport.rect), entry.image);
    painter_.restore();
    return true;
}

void SheetPainter::paintLegend(const Viewport& viewport, TextSetter& text, const Paper& paper)
{
    const katana::entity::Model* model = source_.plan.model;
    TextStyle head;
    head.capMm = 2.5;
    head.bold = true;
    const Box2& r = viewport.rect;
    text.draw(Point2(r.min.x + 2.5, r.max.y - 5.0), QStringLiteral("LEGEND"), head);
    painter_.setPen(paperPen(paper, kInk, 0.35));
    painter_.drawLine(paper.at(Point2(r.min.x + 2.5, r.max.y - 6.0)),
                      paper.at(Point2(r.min.x + 2.5 + text.widthMm("LEGEND", head), r.max.y - 6.0)));
    if (model == nullptr) {
        return;
    }
    // What the plans show (cad/plotting/legend.hpp), each automatic plan at
    // the window this painter draws it at, so the legend lists what the plan
    // beside it drew.
    const auto legend = gatherLegend(set_, index_, viewport, source_);
    if (!legend) {
        problem(viewport, legend.error().describe());
        return;
    }
    if (legend->entries.empty() && options_.slotHints) {
        paintMessage(text, r, QStringLiteral("Nothing to list: the plans show nothing"));
    }
    // Labels in capitals, as the frame letters; columns as wide as the
    // widest needs, flowed to fit (plotting::layoutLegend).
    TextStyle label;
    label.capMm = 1.8;
    label.xFactor = 0.9;
    label.vertical = VerticalJustify::Middle;
    std::vector<QString> names;
    std::vector<double> widths;
    for (const plotting::LegendEntry& entry : legend->entries) {
        names.push_back(QString::fromStdString(entry.label).toUpper());
        widths.push_back(text.widthMm(names.back(), label));
    }
    const plotting::LegendLayout layout = plotting::layoutLegend(r, widths);
    LegendSampleContext samples;
    samples.model = model;
    samples.library = source_.plan.library;
    samples.plot = &options_.plot;
    samples.pixelsPerMillimetre = paper.ppmm();
    samples.scale = legend->scale > 0.0 ? legend->scale : viewport.scale;
    for (const plotting::LegendCell& cell : layout.cells) {
        paintLegendSample(painter_, legend->entries[cell.entry], paper.at(cell.sample), samples);
        double squeeze = 1.0;
        if (const double w = widths[cell.entry]; w > cell.labelRoom && cell.labelRoom > 0.0) {
            squeeze = std::max(cell.labelRoom / w, 0.5);
        }
        text.draw(cell.label, names[cell.entry], label, squeeze);
    }
    if (layout.moreAt) {
        TextStyle more = label;
        more.colour = kFaint;
        text.draw(*layout.moreAt, QString("+%1 more").arg(layout.more), more);
    }
}

void SheetPainter::paintNotes(const Viewport& viewport, TextSetter& text, const Paper& paper)
{
    TextStyle head;
    head.capMm = 2.5;
    head.bold = true;
    const Box2& r = viewport.rect;
    text.draw(Point2(r.min.x + 2.5, r.max.y - 5.0), QStringLiteral("NOTES"), head);
    painter_.setPen(paperPen(paper, kInk, 0.35));
    painter_.drawLine(paper.at(Point2(r.min.x + 2.5, r.max.y - 6.0)),
                      paper.at(Point2(r.min.x + 2.5 + text.widthMm("NOTES", head), r.max.y - 6.0)));
    TextStyle body;
    body.capMm = 2.0;
    body.xFactor = 0.9;
    body.vertical = VerticalJustify::Top;
    const QStringList lines =
        text.wrap(QString::fromStdString(viewport.text), body, std::max(r.width() - 5.0, 5.0));
    const double spacing = 1.6 * body.capMm;
    const auto fit = static_cast<qsizetype>(std::floor((r.height() - 10.0) / spacing));
    text.draw(Point2(r.min.x + 2.5, r.max.y - 9.0),
              lines.mid(0, std::max<qsizetype>(fit, 0)).join(QLatin1Char('\n')),
              TextStyle{body.capMm, body.xFactor, body.horizontal, body.vertical, 0.0, spacing});
}

bool SheetPainter::paintImage(const Viewport& viewport, const Paper& paper)
{
    if (viewport.text.empty()) {
        problem(viewport, "no image chosen");
        return false;
    }
    const std::filesystem::path path = source_.assets / viewport.text;
    const std::string key = path.string();
    auto found = cache_.images_.find(key);
    if (found == cache_.images_.end()) {
        found = cache_.images_.emplace(key, QImage(QString::fromStdString(key))).first;
    }
    const QImage& image = found->second;
    if (image.isNull()) {
        problem(viewport, "cannot read the image " + viewport.text);
        return false;
    }
    const Box2 at = plotting::fitImage(viewport.rect, image.width(), image.height());
    painter_.save();
    painter_.setRenderHint(QPainter::SmoothPixmapTransform, true);
    painter_.drawImage(paper.at(at), image);
    painter_.restore();
    return true;
}

void SheetPainter::paintTableViewport(const Viewport& viewport, TextSetter& text,
                                      const Paper& paper)
{
    // Laid out with this painter's own font, so what the layout measured is
    // what is drawn.
    const plotting::TextWidth measure = [&text](std::string_view line, double capMm, bool bold) {
        TextStyle style;
        style.capMm = capMm;
        style.bold = bold;
        return text.widthMm(QString::fromUtf8(line.data(), static_cast<qsizetype>(line.size())),
                            style);
    };
    // The register reports each sheet's scale as its title block does, an
    // automatic one decided first.
    const plotting::TableLayout table =
        viewport.kind == ViewportKind::SheetIndex
            ? plotting::layoutViewportTable(resolvedSheetSet(set_, source_), index_, viewport,
                                            measure)
            : plotting::layoutViewportTable(set_, index_, viewport, measure);
    paintTable(painter_, paper, text, table);
    if (table.rowsHidden > 0) {
        problem(viewport, std::format("{} of {} rows do not fit; make the view larger",
                                      table.rowsHidden, table.rowsShown + table.rowsHidden));
    }
    if (options_.slotHints && viewport.kind == ViewportKind::Revisions && table.rowsShown == 0 &&
        table.rowsHidden == 0) {
        // On screen only: where the rows of an empty table come from.
        const QString hint = QStringLiteral("No revisions yet: add them under Title Block");
        TextStyle style;
        style.capMm = 2.0;
        style.horizontal = HorizontalJustify::Centre;
        style.vertical = VerticalJustify::Middle;
        style.colour = kFaint;
        const double half = text.widthMm(hint, style) / 2.0 + 1.0;
        const Point2 at = viewport.rect.center();
        knockOut(painter_, paper, Box2(Point2(at.x - half, at.y - 2.0), Point2(at.x + half, at.y + 2.0)));
        text.draw(at, hint, style);
    }
}

void SheetPainter::paintViewport(const Viewport& viewport, TextSetter& text, const Paper& paper)
{
    const QRectF device = paper.at(viewport.rect);
    painter_.save();
    painter_.setClipRect(device, Qt::IntersectClip);
    painter_.fillRect(device, Qt::white);

    bool drawn = false;
    double scale = viewport.scale;
    switch (viewport.kind) {
    case ViewportKind::Plan:
    case ViewportKind::KeyPlan:
        drawn = paintPlanViewport(viewport, text, paper, scale);
        break;
    case ViewportKind::LongSection:
    case ViewportKind::CrossSections:
        drawn = paintSections(viewport, text, paper);
        break;
    case ViewportKind::Model3D:
        drawn = paintSnapshot(viewport, paper);
        break;
    case ViewportKind::Legend:
        paintLegend(viewport, text, paper);
        drawn = true;
        break;
    case ViewportKind::Notes:
        paintNotes(viewport, text, paper);
        drawn = true;
        break;
    case ViewportKind::Image:
        drawn = paintImage(viewport, paper);
        break;
    case ViewportKind::SheetIndex:
    case ViewportKind::Revisions:
        paintTableViewport(viewport, text, paper);
        drawn = true;
        break;
    }
    if (!drawn && options_.slotHints && !stats_.problems.empty()) {
        const std::string& last = stats_.problems.back();
        paintMessage(text, viewport.rect,
                     QString::fromStdString(last.substr(last.find(": ") + 2)));
    }

    const bool planLike =
        viewport.kind == ViewportKind::Plan || viewport.kind == ViewportKind::KeyPlan;
    if (drawn && planLike && viewport.northArrow) {
        paintNorthArrow(painter_, paper, text, viewport.rect, viewport.rotation);
    }
    if (drawn && planLike && viewport.scaleBar) {
        paintScaleBar(painter_, paper, text, viewport.rect, scale);
    }
    // A table's title is its heading, drawn by the table.
    if (viewport.kind != ViewportKind::Legend && viewport.kind != ViewportKind::Notes &&
        viewport.kind != ViewportKind::Image && viewport.kind != ViewportKind::SheetIndex &&
        viewport.kind != ViewportKind::Revisions) {
        paintTitle(painter_, paper, text, viewport.rect, viewTitle(viewport, scale));
    }
    painter_.restore();

    painter_.setPen(paperPen(paper, kInk, 0.25));
    painter_.setBrush(Qt::NoBrush);
    painter_.drawRect(device);
    if (drawn) {
        ++stats_.viewportsDrawn;
    }
}

SheetPaintStats SheetPainter::run()
{
    if (index_ >= set_.sheets.size()) {
        return stats_;
    }
    const plotting::Sheet& sheet = set_.sheets[index_];
    const katana::cad::PaperDimensions dimensions =
        katana::cad::paperDimensions(sheet.paper, sheet.landscape);
    const Paper paper(dimensions.heightMm, options_);

    painter_.save();
    painter_.setRenderHint(QPainter::Antialiasing, true);
    painter_.setRenderHint(QPainter::TextAntialiasing, true);
    painter_.fillRect(paper.at(Box2(Point2(0.0, 0.0), Point2(dimensions.widthMm, dimensions.heightMm))),
                      Qt::white);
    TextSetter text(painter_, paper);

    // An automatic scale is decided here, and the title block reports the
    // scale that was drawn.
    plotting::SheetSet drawnSet = set_;
    plotting::Sheet& drawnSheet = drawnSet.sheets[index_];
    for (Viewport& viewport : drawnSheet.viewports) {
        if ((viewport.kind == ViewportKind::Plan || viewport.kind == ViewportKind::KeyPlan) &&
            (viewport.autoScale || viewport.autoCentre)) {
            const ResolvedViewport at = resolvePlanViewport(viewport, source_, set_, index_);
            viewport.scale = at.scale;
            viewport.centre = at.centre;
            viewport.autoScale = false;
            viewport.autoCentre = false;
        }
    }

    // The panels under the frame, back to front, as the app drew them.
    for (const Viewport& viewport : drawnSheet.viewports) {
        if (!viewport.rect.empty()) {
            paintViewport(viewport, text, paper);
        }
    }

    if (!sheet.frame.empty()) {
        const std::string key = std::format("{}|{}|{}", sheet.frame, static_cast<int>(sheet.paper),
                                            sheet.landscape);
        auto found = cache_.frames_.find(key);
        if (found == cache_.frames_.end()) {
            found = cache_.frames_
                        .emplace(key, plotting::frameFor(sheet.frame, sheet.paper, sheet.landscape))
                        .first;
        }
        if (found->second) {
            const auto fields = plotting::resolveFields(drawnSet, index_, source_.fields);
            paintFrame(painter_, paper, text, *found->second, sheet, fields, source_, stats_);
        } else if (sheet.landscape) {
            stats_.problems.push_back("frame: " + found->second.error().describe());
        }
    }
    painter_.restore();
    return stats_;
}

// ---- public -----------------------------------------------------------------------------

void SheetPaintCache::clear()
{
    plan_.clear();
    sections_.clear();
    snapshots_.clear();
    images_.clear();
    frames_.clear();
}

QPointF paperToDevice(const Point2& paper, double paperHeightMm, const SheetPaintOptions& options)
{
    return options.origin + QPointF(paper.x * options.pixelsPerMillimetre,
                                    (paperHeightMm - paper.y) * options.pixelsPerMillimetre);
}

QRectF paperToDevice(const Box2& paper, double paperHeightMm, const SheetPaintOptions& options)
{
    return QRectF(paperToDevice(Point2(paper.min.x, paper.max.y), paperHeightMm, options),
                  paperToDevice(Point2(paper.max.x, paper.min.y), paperHeightMm, options));
}

Point2 deviceToPaper(const QPointF& device, double paperHeightMm, const SheetPaintOptions& options)
{
    const QPointF d = device - options.origin;
    return Point2(d.x() / options.pixelsPerMillimetre,
                  paperHeightMm - d.y() / options.pixelsPerMillimetre);
}

std::optional<plotting::Frame> sheetFrame(const plotting::Sheet& sheet)
{
    if (sheet.frame.empty() || !sheet.landscape) {
        return std::nullopt;
    }
    auto frame = plotting::frameFor(sheet.frame, sheet.paper, sheet.landscape);
    if (!frame) {
        return std::nullopt;
    }
    return std::move(*frame);
}

ResolvedViewport resolvePlanViewport(const Viewport& viewport, const SheetSource& source)
{
    ResolvedViewport at{viewport.scale, viewport.centre};
    if ((!viewport.autoScale && !viewport.autoCentre) || source.plan.model == nullptr ||
        viewport.rect.empty()) {
        return at;
    }
    // What the viewport shows: its stretch of an alignment, or the drawing.
    std::vector<Point2> points;
    const plotting::ViewportSource& from = viewport.source;
    if (!from.alignment.empty()) {
        if (const auto* alignment = source.plan.model->alignments.find(from.alignment)) {
            if (auto solved = katana::geometry::solveAlignment(alignment->horizontal)) {
                double a = from.chainageFrom;
                double b = from.chainageTo;
                if (!(b > a)) {
                    a = solved->startStation();
                    b = solved->endStation();
                }
                for (int i = 0; i <= 64; ++i) {
                    if (auto p = solved->pointAtStation(a + (b - a) * i / 64.0)) {
                        points.push_back(*p);
                    }
                }
            }
        }
    }
    if (points.empty()) {
        const Box2 box = planDrawnBounds(source.plan, viewport.hiddenLayers, {});
        if (box.empty()) {
            return at;
        }
        points = {box.min, box.max, Point2(box.min.x, box.max.y), Point2(box.max.x, box.min.y)};
    }
    if (viewport.autoCentre) {
        Box2 box;
        for (const Point2& p : points) {
            box.expand(rotated(p, -viewport.rotation));
        }
        at.centre = rotated(box.center(), viewport.rotation);
    }
    if (viewport.autoScale) {
        double halfW = 0.0;
        double halfH = 0.0;
        for (const Point2& p : points) {
            const Point2 d = rotated(p - at.centre, -viewport.rotation);
            halfW = std::max(halfW, std::abs(d.x));
            halfH = std::max(halfH, std::abs(d.y));
        }
        // 4% to spare, so the drawing does not touch the viewport's edge.
        const double needed = std::max(2.0 * halfW * 1000.0 / viewport.rect.width(),
                                       2.0 * halfH * 1000.0 / viewport.rect.height()) *
                              1.04;
        if (needed > 0.0) {
            if (auto scale = katana::cad::sheetScaleAtLeast(needed)) {
                at.scale = *scale;
            }
        }
    }
    return at;
}

ResolvedViewport resolvePlanViewport(const Viewport& viewport, const SheetSource& source,
                                     const plotting::SheetSet& set, std::size_t sheetIndex)
{
    if (viewport.kind != ViewportKind::KeyPlan || (!viewport.autoScale && !viewport.autoCentre) ||
        source.plan.model == nullptr || viewport.rect.empty()) {
        return resolvePlanViewport(viewport, source);
    }
    const auto outlines =
        plotting::keyPlanOutlines(set, sheetIndex, [&source](const Viewport& plan) {
            const ResolvedViewport at = resolvePlanViewport(plan, source);
            return plotting::PlanPlacement{at.scale, at.centre};
        });
    // The drawing is only the fallback for a set with no plan to frame.
    const plotting::PlanPlacement fitted = plotting::fitKeyPlan(
        viewport, outlines,
        outlines.empty() ? planDrawnBounds(source.plan, viewport.hiddenLayers, {}) : Box2{});
    return {fitted.scale, fitted.centre};
}

SheetPaintStats paintSheet(QPainter& painter, const plotting::SheetSet& set, std::size_t index,
                           const SheetSource& source, const SheetPaintOptions& options,
                           SheetPaintCache& cache)
{
    return SheetPainter(painter, set, index, source, options, cache).run();
}

katana::core::Status plotSheetsToPdf(const QString& path, const plotting::SheetSet& set,
                                     std::span<const std::size_t> indices,
                                     const SheetSource& source, double dpi,
                                     SheetPaintCache& cache, const QString& title,
                                     std::vector<std::string>* problems)
{
    using katana::core::ErrorCode;
    using katana::core::makeError;
    std::vector<std::size_t> pages(indices.begin(), indices.end());
    if (pages.empty()) {
        for (std::size_t i = 0; i < set.sheets.size(); ++i) {
            pages.push_back(i);
        }
    }
    if (pages.empty()) {
        return makeError(ErrorCode::InvalidArgument, "there are no sheets to plot");
    }
    for (const std::size_t page : pages) {
        if (page >= set.sheets.size()) {
            return makeError(ErrorCode::InvalidArgument, "no sheet at that index",
                             std::to_string(page));
        }
    }
    if (!(dpi > 0.0) || !std::isfinite(dpi)) {
        return makeError(ErrorCode::InvalidArgument, "the resolution must be positive");
    }

    const PdfResolution resolution = pdfResolutionFor(dpi);
    const auto pageSize = [&](std::size_t index) {
        const plotting::Sheet& sheet = set.sheets[index];
        const katana::cad::PaperDimensions paper =
            katana::cad::paperDimensions(sheet.paper, sheet.landscape);
        return QPageSize(QSizeF(paper.widthMm, paper.heightMm), QPageSize::Millimeter);
    };
    QPdfWriter writer(path);
    writer.setResolution(resolution.resolution);
    writer.setTitle(title.isEmpty() ? QFileInfo(path).completeBaseName() : title);
    writer.setCreator(QStringLiteral("Katana"));
    writer.setPageSize(pageSize(pages.front()));
    writer.setPageMargins(QMarginsF(0.0, 0.0, 0.0, 0.0));
    QPainter painter(&writer);
    if (!painter.isActive()) {
        return makeError(ErrorCode::FileExportFailure, "could not open the PDF for writing",
                         path.toStdString());
    }
    SheetPaintOptions options;
    options.medium = PlanMedium::Paper;
    options.pixelsPerMillimetre = katana::cad::millimetresToPixels(1.0, dpi);
    options.plot.dpi = dpi;
    for (std::size_t n = 0; n < pages.size(); ++n) {
        if (n > 0) {
            writer.setPageSize(pageSize(pages[n]));
            if (!writer.newPage()) {
                return makeError(ErrorCode::FileExportFailure, "could not start a page",
                                 path.toStdString());
            }
        }
        painter.save();
        if (resolution.scale != 1.0) {
            painter.scale(resolution.scale, resolution.scale);
        }
        const SheetPaintStats stats = paintSheet(painter, set, pages[n], source, options, cache);
        painter.restore();
        if (problems != nullptr) {
            for (const std::string& line : stats.problems) {
                problems->push_back(set.sheets[pages[n]].name + " - " + line);
            }
        }
    }
    if (!painter.end()) {
        return makeError(ErrorCode::FileExportFailure, "could not finish the PDF",
                         path.toStdString());
    }
    return {};
}

} // namespace katana::qt
