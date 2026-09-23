#include "icons.hpp"

#include <QIconEngine>
#include <QLinearGradient>
#include <QPainter>
#include <QPainterPath>
#include <QPixmap>
#include <QPolygonF>

#include <cmath>

#include "theme.hpp"

namespace katana::qt {

namespace {

// The design grid. Every coordinate below is in these units; the painter is
// scaled so that 24 of them fill whatever rectangle Qt hands over.
constexpr double kGrid = 24.0;
// Stroke weight on that grid. 1.7 is the weight at which a 20 px toolbar icon
// still reads as a line drawing and a 16 px menu icon does not fill in; the
// common 2.0 of icon sets drawn for 24 px display closes up small counters
// (the label of Save, the inside of the magnet) when shown at 16.
constexpr double kStroke = 1.7;

// A pen and brush for each of the two tones, and the handful of marks every
// icon is built from. Keeping the marks here is what keeps the set
// consistent: a node is the same node on every draw tool.
class Ink {
  public:
    Ink(QPainter& painter, const QColor& neutral, const QColor& accent)
        : painter_(painter), neutral_(neutral), accent_(accent)
    {
    }

    void stroke(const QPainterPath& path, bool accent = false, double width = kStroke) const
    {
        painter_.setBrush(Qt::NoBrush);
        painter_.setPen(QPen(tone(accent), width, Qt::SolidLine, Qt::RoundCap, Qt::RoundJoin));
        painter_.drawPath(path);
    }
    void dashed(const QPainterPath& path, bool accent = false) const
    {
        QPen pen(tone(accent), kStroke, Qt::CustomDashLine, Qt::FlatCap, Qt::RoundJoin);
        pen.setDashPattern({1.6, 1.4});
        painter_.setBrush(Qt::NoBrush);
        painter_.setPen(pen);
        painter_.drawPath(path);
    }
    void fill(const QPainterPath& path, bool accent = false, int alpha = 255) const
    {
        QColor color = tone(accent);
        color.setAlpha(color.alpha() * alpha / 255);
        painter_.setPen(Qt::NoPen);
        painter_.setBrush(color);
        painter_.drawPath(path);
    }
    void line(double x1, double y1, double x2, double y2, bool accent = false,
              double width = kStroke) const
    {
        QPainterPath path(QPointF(x1, y1));
        path.lineTo(x2, y2);
        stroke(path, accent, width);
    }
    void dot(double x, double y, double radius, bool accent = false) const
    {
        QPainterPath path;
        path.addEllipse(QPointF(x, y), radius, radius);
        fill(path, accent);
    }
    // A vertex handle, as the viewport draws a grip: a small filled square.
    void node(double x, double y, bool accent = false) const
    {
        QPainterPath path;
        path.addRect(QRectF(x - 1.5, y - 1.5, 3.0, 3.0));
        fill(path, accent);
    }

  private:
    [[nodiscard]] const QColor& tone(bool accent) const { return accent ? accent_ : neutral_; }

    QPainter& painter_;
    QColor neutral_;
    QColor accent_;
};

QPainterPath polyline(std::initializer_list<QPointF> points, bool closed = false)
{
    QPainterPath path;
    bool first = true;
    for (const QPointF& point : points) {
        if (first) {
            path.moveTo(point);
            first = false;
        } else {
            path.lineTo(point);
        }
    }
    if (closed) {
        path.closeSubpath();
    }
    return path;
}

QPainterPath rectangle(double x, double y, double w, double h, double radius = 0.0)
{
    QPainterPath path;
    if (radius > 0.0) {
        path.addRoundedRect(QRectF(x, y, w, h), radius, radius);
    } else {
        path.addRect(QRectF(x, y, w, h));
    }
    return path;
}

QPainterPath circle(double x, double y, double radius)
{
    QPainterPath path;
    path.addEllipse(QPointF(x, y), radius, radius);
    return path;
}

// The floppy disk shared by Save and Save As.
void drawDisk(const Ink& ink, bool withLabel)
{
    ink.stroke(polyline({{4, 4}, {17, 4}, {20, 7}, {20, 20}, {4, 20}}, true));
    ink.stroke(polyline({{8, 4}, {8, 9}, {15, 9}, {15, 4}}));
    if (withLabel) {
        ink.stroke(polyline({{7, 20}, {7, 13.5}, {17, 13.5}, {17, 20}}), true);
    }
}

// The tray shared by Import and Export.
void drawTray(const Ink& ink) { ink.stroke(polyline({{4, 14}, {4, 20}, {20, 20}, {20, 14}})); }

// The three kinds of GIS data, drawn in the lower-left 15 units so that an
// arrow fits down the right-hand side. Neutral: the data is the object, the
// arrow is what the command does to it.
void drawVectorGlyph(const Ink& ink)
{
    ink.stroke(polyline({{3, 20}, {3, 11}, {9, 7.5}, {14.5, 12.5}, {12, 20}}, true));
    for (const QPointF& p :
         {QPointF(3, 20), QPointF(3, 11), QPointF(9, 7.5), QPointF(14.5, 12.5), QPointF(12, 20)}) {
        ink.node(p.x(), p.y());
    }
}

void drawRasterGlyph(const Ink& ink)
{
    ink.stroke(rectangle(3, 8, 12, 12));
    ink.line(7, 8, 7, 20, false, 1.1);
    ink.line(11, 8, 11, 20, false, 1.1);
    ink.line(3, 12, 15, 12, false, 1.1);
    ink.line(3, 16, 15, 16, false, 1.1);
    ink.fill(rectangle(3, 8, 4, 4), false, 110);
    ink.fill(rectangle(7, 12, 4, 4), false, 110);
    ink.fill(rectangle(11, 16, 4, 4), false, 110);
}

void drawCloudGlyph(const Ink& ink)
{
    for (const QPointF& p : {QPointF(4, 19.5), QPointF(8, 20), QPointF(12.5, 19), QPointF(5.5, 15.5),
                             QPointF(10, 15.5), QPointF(14, 14.5), QPointF(4, 11.5),
                             QPointF(8.5, 11), QPointF(12, 9)}) {
        ink.dot(p.x(), p.y(), 1.3);
    }
}

// The down arrow of an import and the up arrow of an export, down the right.
void drawArrowIn(const Ink& ink)
{
    ink.line(18.5, 3, 18.5, 12.5, true);
    ink.stroke(polyline({{15.5, 9.5}, {18.5, 12.5}, {21.5, 9.5}}), true);
}

void drawArrowOut(const Ink& ink)
{
    ink.line(18.5, 12.5, 18.5, 3, true);
    ink.stroke(polyline({{15.5, 6}, {18.5, 3}, {21.5, 6}}), true);
}

// ---- Survey marks --------------------------------------------------------------------
//
// A survey point: a ring with a dot at its centre, as a mark is drawn on a
// plan. In the accent for a point the command creates.
void drawMark(const Ink& ink, double x, double y, bool accent)
{
    ink.stroke(circle(x, y, 2.2), accent, 1.3);
    ink.dot(x, y, 0.8, accent);
}

// A control or traverse station: a filled triangle, the usual plan symbol.
void drawStation(const Ink& ink, double x, double y)
{
    ink.fill(polyline({{x, y - 2.8}, {x + 2.5, y + 1.6}, {x - 2.5, y + 1.6}}, true));
}

// Grid north above a mark: a thin line from `from` up to `to` with its head.
void drawNorth(const Ink& ink, double x, double from, double to)
{
    ink.line(x, from, x, to, false, 1.1);
    ink.stroke(polyline({{x - 1.7, to + 2.5}, {x, to}, {x + 1.7, to + 2.5}}), false, 1.1);
}

// The curved arrow of Undo; Redo is its mirror image.
void drawTurn(QPainter& painter, const Ink& ink, bool mirrored)
{
    painter.save();
    if (mirrored) {
        painter.translate(kGrid, 0.0);
        painter.scale(-1.0, 1.0);
    }
    QPainterPath sweep(QPointF(5.5, 9));
    sweep.lineTo(14, 9);
    sweep.cubicTo(17.5, 9, 19.5, 11.5, 19.5, 14.2);
    sweep.cubicTo(19.5, 17.2, 17.2, 19.5, 14, 19.5);
    sweep.lineTo(10, 19.5);
    ink.stroke(sweep);
    ink.stroke(polyline({{9.5, 5}, {5.5, 9}, {9.5, 13}}), true);
    painter.restore();
}

} // namespace

const std::vector<Icon>& allIcons()
{
    static const std::vector<Icon> icons{
        Icon::New,          Icon::Open,
        Icon::Save,         Icon::SaveAs,
        Icon::Import,       Icon::Export,
        Icon::Plot,         Icon::Undo,
        Icon::Redo,         Icon::Erase,
        Icon::SelectAll,    Icon::Select,
        Icon::Point,        Icon::Line,
        Icon::Polyline,     Icon::Rectangle,
        Icon::Circle,       Icon::Arc,
        Icon::Move,         Icon::Copy,
        Icon::ZoomExtents,  Icon::Grid,
        Icon::Snap,         Icon::Layers,
        Icon::Properties,   Icon::SurfaceFromCloud,
        Icon::SurfaceFromRaster, Icon::SurfaceFromDrawing,
        Icon::Section,      Icon::SectionAlignment,
        Icon::CorridorQuantities, Icon::CorridorSurface,
        Icon::ImportVector, Icon::ImportRaster,
        Icon::ImportPointCloud, Icon::ExportPointCloud,
        Icon::ExportDem,    Icon::ConvertCopc,
        Icon::DatasetInfo,
        Icon::Minimise,     Icon::Float,
        Icon::Dock,         Icon::Maximise,
        Icon::Restore,      Icon::Close,
        Icon::CommandLine,  Icon::ReferenceData,
        Icon::ViewPlan,     Icon::View3D,
        Icon::ViewSection,  Icon::ViewElevation,
        Icon::ViewLayersFiltered,
        Icon::LayerNew,     Icon::LayerNewChild,
        Icon::Rename,       Icon::ZoomTo,
        Icon::Help,         Icon::About,
        // Survey
        Icon::SurveyInverse, Icon::SurveyForward,
        Icon::SurveyArea,   Icon::SurveyAngle,
        Icon::SurveyTraverse, Icon::SurveyLevelBook,
        Icon::SurveyConverter,
        Icon::SurveyImport, Icon::SurveyExport,
        Icon::SurveyPointManager, Icon::SurveyPointReport,
    };
    return icons;
}

void paintIcon(QPainter& painter, Icon which, const QRectF& rect, const QColor& neutral,
               const QColor& accent)
{
    painter.save();
    painter.setRenderHint(QPainter::Antialiasing, true);
    painter.translate(rect.topLeft());
    painter.scale(rect.width() / kGrid, rect.height() / kGrid);
    const Ink ink(painter, neutral, accent);

    switch (which) {
    case Icon::New:
        ink.stroke(polyline({{6, 3}, {14, 3}, {19, 8}, {19, 21}, {6, 21}}, true));
        ink.stroke(polyline({{14, 3}, {14, 8}, {19, 8}}));
        ink.line(12.5, 12, 12.5, 18, true);
        ink.line(9.5, 15, 15.5, 15, true);
        break;
    case Icon::Open:
        ink.stroke(polyline({{3, 19}, {3, 6}, {9, 6}, {11, 8}, {19, 8}, {19, 11}}));
        ink.stroke(polyline({{3, 19}, {6.5, 11}, {22, 11}, {18.5, 19}}, true), true);
        break;
    case Icon::Save:
        drawDisk(ink, true);
        break;
    case Icon::SaveAs:
        drawDisk(ink, false);
        ink.fill(polyline({{10.5, 21.5}, {11.5, 17.5}, {18.5, 10.5}, {21.5, 13.5}, {14.5, 20.5}},
                          true),
                 true);
        break;
    case Icon::Import:
        drawTray(ink);
        ink.line(12, 3, 12, 15, true);
        ink.stroke(polyline({{8, 11}, {12, 15}, {16, 11}}), true);
        break;
    case Icon::Export:
        drawTray(ink);
        ink.line(12, 16, 12, 4, true);
        ink.stroke(polyline({{8, 8}, {12, 4}, {16, 8}}), true);
        break;
    case Icon::Plot:
        ink.stroke(polyline({{7, 9}, {7, 4}, {17, 4}, {17, 9}}));
        ink.stroke(polyline({{7, 17}, {4, 17}, {4, 9}, {20, 9}, {20, 17}, {17, 17}}));
        ink.stroke(rectangle(7, 14, 10, 7), true);
        ink.line(9.5, 17.5, 14.5, 17.5, true);
        break;
    case Icon::Undo:
        drawTurn(painter, ink, false);
        break;
    case Icon::Redo:
        drawTurn(painter, ink, true);
        break;
    case Icon::Erase:
        ink.line(4, 7, 20, 7);
        ink.stroke(polyline({{9, 7}, {9, 4.5}, {15, 4.5}, {15, 7}}));
        ink.stroke(polyline({{6, 7}, {7, 21}, {17, 21}, {18, 7}}));
        ink.line(10, 11, 10, 17, true);
        ink.line(14, 11, 14, 17, true);
        break;
    case Icon::SelectAll:
        ink.dashed(rectangle(3.5, 3.5, 17, 17), true);
        ink.stroke(rectangle(7, 8, 5.5, 4.5));
        ink.stroke(circle(15, 15.5, 2.6));
        break;
    case Icon::Select: {
        const QPainterPath arrow = polyline(
            {{6, 3}, {6, 18}, {10, 14.5}, {12.8, 20.5}, {15.2, 19.4}, {12.5, 13.5}, {18, 13.5}},
            true);
        ink.fill(arrow, true);
        ink.stroke(arrow);
        break;
    }
    case Icon::Point:
        ink.line(12, 3, 12, 8);
        ink.line(12, 16, 12, 21);
        ink.line(3, 12, 8, 12);
        ink.line(16, 12, 21, 12);
        ink.dot(12, 12, 2.6, true);
        break;
    case Icon::Line:
        ink.line(5, 19, 19, 5, true);
        ink.node(5, 19);
        ink.node(19, 5);
        break;
    case Icon::Polyline:
        ink.stroke(polyline({{4, 18}, {9, 8}, {15, 15}, {20, 5}}), true);
        for (const QPointF& p : {QPointF(4, 18), QPointF(9, 8), QPointF(15, 15), QPointF(20, 5)}) {
            ink.node(p.x(), p.y());
        }
        break;
    case Icon::Rectangle:
        ink.stroke(rectangle(4, 6, 16, 12), true);
        for (const QPointF& p : {QPointF(4, 6), QPointF(20, 6), QPointF(20, 18), QPointF(4, 18)}) {
            ink.node(p.x(), p.y());
        }
        break;
    case Icon::Circle:
        // The radius goes off at 45 degrees. Drawn horizontally it made the
        // icon read as a minus sign in a ring.
        ink.stroke(circle(12, 12, 8), true);
        ink.line(12, 12, 17.66, 6.34, false, 1.1);
        ink.node(12, 12);
        ink.node(17.66, 6.34);
        break;
    case Icon::Arc: {
        QPainterPath arc(QPointF(4, 18));
        arc.quadTo(12, -1, 20, 18);
        ink.stroke(arc, true);
        ink.node(4, 18);
        ink.node(12, 8.5);
        ink.node(20, 18);
        break;
    }
    case Icon::Move:
        ink.line(12, 4, 12, 20);
        ink.line(4, 12, 20, 12);
        ink.stroke(polyline({{9, 6}, {12, 3}, {15, 6}}), true);
        ink.stroke(polyline({{9, 18}, {12, 21}, {15, 18}}), true);
        ink.stroke(polyline({{6, 9}, {3, 12}, {6, 15}}), true);
        ink.stroke(polyline({{18, 9}, {21, 12}, {18, 15}}), true);
        break;
    case Icon::Copy:
        ink.stroke(polyline({{8.5, 15}, {4, 15}, {4, 4}, {15, 4}, {15, 8.5}}));
        ink.stroke(rectangle(9, 9, 11, 11), true);
        break;
    case Icon::ZoomExtents:
        ink.stroke(polyline({{3, 8}, {3, 3}, {8, 3}}));
        ink.stroke(polyline({{16, 3}, {21, 3}, {21, 8}}));
        ink.stroke(polyline({{21, 16}, {21, 21}, {16, 21}}));
        ink.stroke(polyline({{8, 21}, {3, 21}, {3, 16}}));
        ink.stroke(rectangle(8, 8, 8, 8), true);
        break;
    case Icon::Grid:
        ink.stroke(rectangle(3, 3, 18, 18));
        ink.line(9, 3, 9, 21, false, 1.1);
        ink.line(15, 3, 15, 21, false, 1.1);
        ink.line(3, 9, 21, 9, false, 1.1);
        ink.line(3, 15, 21, 15, false, 1.1);
        ink.dot(15, 9, 2.2, true);
        break;
    case Icon::Snap: {
        // A horseshoe magnet, drawn as one thick stroke so the bend is a true
        // semicircle, with the pole tips in the accent.
        QPainterPath magnet(QPointF(7.5, 4));
        magnet.lineTo(7.5, 13.5);
        magnet.cubicTo(7.5, 19.5, 16.5, 19.5, 16.5, 13.5);
        magnet.lineTo(16.5, 4);
        QPen body(neutral, 4.2, Qt::SolidLine, Qt::FlatCap, Qt::RoundJoin);
        painter.setBrush(Qt::NoBrush);
        painter.setPen(body);
        painter.drawPath(magnet);
        QPen tip(accent, 4.2, Qt::SolidLine, Qt::FlatCap);
        painter.setPen(tip);
        painter.drawLine(QPointF(7.5, 4), QPointF(7.5, 7.5));
        painter.drawLine(QPointF(16.5, 4), QPointF(16.5, 7.5));
        break;
    }
    case Icon::Layers:
        ink.stroke(polyline({{3, 16}, {12, 21}, {21, 16}}));
        ink.stroke(polyline({{3, 12}, {12, 17}, {21, 12}}));
        ink.stroke(polyline({{12, 3}, {21, 8}, {12, 13}, {3, 8}}, true), true);
        break;
    case Icon::Properties:
        ink.line(4, 7, 20, 7);
        ink.line(4, 12, 20, 12);
        ink.line(4, 17, 20, 17);
        ink.dot(9, 7, 2.3, true);
        ink.dot(15.5, 12, 2.3, true);
        ink.dot(7.5, 17, 2.3, true);
        break;
    case Icon::SurfaceFromCloud:
        for (const QPointF& p : {QPointF(5, 7), QPointF(9.5, 4.5), QPointF(14.5, 7.5),
                                 QPointF(19, 5), QPointF(7.5, 11), QPointF(12, 10),
                                 QPointF(17.5, 11.5)}) {
            ink.dot(p.x(), p.y(), 1.15, true);
        }
        ink.stroke(polyline({{3, 20.5}, {8, 15.5}, {13, 18.5}, {21, 14}}));
        break;
    case Icon::SurfaceFromRaster:
        ink.stroke(rectangle(3, 3, 10, 10));
        ink.line(8, 3, 8, 13, false, 1.1);
        ink.line(3, 8, 13, 8, false, 1.1);
        ink.fill(rectangle(3, 3, 5, 5), false, 110);
        ink.fill(rectangle(8, 8, 5, 5), false, 110);
        ink.stroke(polyline({{8, 21}, {14.5, 12}, {17.5, 16.5}, {19, 14.5}, {22, 21}}, true), true);
        break;
    case Icon::SurfaceFromDrawing:
        ink.stroke(polyline({{3, 19}, {10, 6}, {21, 9}, {21, 20}, {15, 17}}, true), true);
        ink.stroke(polyline({{10, 6}, {15, 17}, {21, 9}}), true);
        for (const QPointF& p : {QPointF(3, 19), QPointF(10, 6), QPointF(21, 9), QPointF(21, 20),
                                 QPointF(15, 17)}) {
            ink.dot(p.x(), p.y(), 1.5);
        }
        break;
    case Icon::Section:
        ink.stroke(polyline({{3, 15}, {8, 11}, {12, 14}, {17, 8}, {21, 11}}));
        ink.line(3, 20, 21, 20, false, 1.1);
        ink.dashed(polyline({{12, 3.5}, {12, 20}}), true);
        break;
    case Icon::SectionAlignment: {
        QPainterPath alignment(QPointF(3, 20));
        alignment.cubicTo(10, 19, 9, 11, 13, 9);
        alignment.cubicTo(16, 7.5, 18, 6, 21, 4);
        ink.stroke(alignment);
        ink.line(7.5, 8.5, 16.5, 14.5, true);
        ink.line(6.7, 9.7, 8.3, 7.3, true);
        ink.line(15.7, 15.7, 17.3, 13.3, true);
        break;
    }
    case Icon::CorridorQuantities: {
        // Ground falling left to right, the road template cut into it, and the
        // cut between them shaded - which is the quantity.
        const QPainterPath cut =
            polyline({{5.5, 8.4}, {8.5, 15}, {15.5, 15}, {18.5, 11.3}}, true);
        ink.fill(cut, true, 70);
        ink.line(3, 7.8, 21, 11.9);
        ink.stroke(polyline({{5.5, 8.4}, {8.5, 15}, {15.5, 15}, {18.5, 11.3}}), true);
        ink.line(3, 20, 21, 20, false, 1.1);
        break;
    }
    case Icon::CorridorSurface: {
        QPainterPath left(QPointF(4, 21));
        left.quadTo(5, 10, 15, 3);
        QPainterPath right(QPointF(11, 21));
        right.quadTo(12, 13, 21, 7);
        ink.stroke(left);
        ink.stroke(right);
        ink.line(4.5, 17, 11.4, 17.6, true, 1.3);
        ink.line(6.6, 11.6, 13.4, 13.4, true, 1.3);
        ink.line(10.6, 6.6, 17, 9.6, true, 1.3);
        break;
    }
    case Icon::ImportVector:
        drawVectorGlyph(ink);
        drawArrowIn(ink);
        break;
    case Icon::ImportRaster:
        drawRasterGlyph(ink);
        drawArrowIn(ink);
        break;
    case Icon::ImportPointCloud:
        drawCloudGlyph(ink);
        drawArrowIn(ink);
        break;
    case Icon::ExportPointCloud:
        drawCloudGlyph(ink);
        drawArrowOut(ink);
        break;
    case Icon::ExportDem:
        // A surface's profile over the grid it becomes: the accent is the
        // surface, since that is what is being written out.
        drawRasterGlyph(ink);
        ink.stroke(polyline({{3, 7}, {7.5, 3.5}, {11, 6}, {15, 2.5}}), true);
        drawArrowOut(ink);
        break;
    case Icon::ConvertCopc:
        // An octree seen from above: a square quartered, one quarter quartered
        // again, points in the cells - the structure COPC writes into the file.
        ink.stroke(rectangle(3, 3, 18, 18));
        ink.line(12, 3, 12, 21, false, 1.1);
        ink.line(3, 12, 21, 12, false, 1.1);
        ink.line(16.5, 12, 16.5, 21, true, 1.1);
        ink.line(12, 16.5, 21, 16.5, true, 1.1);
        for (const QPointF& p : {QPointF(7.5, 7.5), QPointF(16.5, 7.5), QPointF(7.5, 16.5),
                                 QPointF(14.25, 14.25), QPointF(18.75, 14.25),
                                 QPointF(14.25, 18.75), QPointF(18.75, 18.75)}) {
            ink.dot(p.x(), p.y(), 1.2, true);
        }
        break;
    case Icon::DatasetInfo:
        // A file's page, and the "i" of information beside it rather than on
        // it, so the two never cross at 16 px.
        ink.stroke(rectangle(3, 3, 10, 13));
        ink.line(5.5, 7, 10.5, 7, false, 1.1);
        ink.line(5.5, 10, 10.5, 10, false, 1.1);
        ink.line(5.5, 13, 10.5, 13, false, 1.1);
        ink.stroke(circle(17.5, 17.5, 4), true);
        ink.dot(17.5, 15.6, 0.9, true);
        ink.line(17.5, 17.6, 17.5, 19.8, true, 1.4);
        break;
    // ---- window buttons: neutral, and heavier than the toolbar set, because
    // they are shown at 14 px on a title bar where 1.7 units is under a pixel.
    case Icon::Minimise:
        ink.line(6, 17, 18, 17, false, 2.2);
        break;
    case Icon::Maximise:
        ink.stroke(rectangle(5, 5, 14, 14, 1.0), false, 2.0);
        ink.fill(rectangle(5, 5, 14, 3));
        break;
    case Icon::Restore:
        // Two windows, the front one whole and the back one showing only
        // where it is not covered - the convention every desktop uses.
        ink.stroke(polyline({{9, 8.5}, {9, 4}, {20, 4}, {20, 15}, {16, 15}}), false, 2.0);
        ink.stroke(rectangle(4, 9, 12, 11, 1.0), false, 2.0);
        ink.fill(rectangle(4, 9, 12, 2.6));
        break;
    case Icon::Close:
        ink.line(6.5, 6.5, 17.5, 17.5, false, 2.2);
        ink.line(17.5, 6.5, 6.5, 17.5, false, 2.2);
        break;
    case Icon::Float:
        // The main window open at one corner, and an arrow leaving through it.
        // Dock is the same box with the arrow coming back in, so the pair
        // reads as one control in two states.
        ink.stroke(polyline({{11, 4}, {4, 4}, {4, 20}, {20, 20}, {20, 13}}), false, 2.0);
        ink.line(10, 14, 20, 4, false, 2.0);
        ink.stroke(polyline({{14, 4}, {20, 4}, {20, 10}}), false, 2.0);
        break;
    case Icon::Dock:
        ink.stroke(polyline({{11, 4}, {4, 4}, {4, 20}, {20, 20}, {20, 13}}), false, 2.0);
        ink.line(20, 4, 10, 14, false, 2.0);
        ink.stroke(polyline({{10, 8}, {10, 14}, {16, 14}}), false, 2.0);
        break;
    // ---- panels
    case Icon::CommandLine:
        ink.stroke(rectangle(3, 5, 18, 14, 2.0));
        ink.stroke(polyline({{7, 9.5}, {10, 12}, {7, 14.5}}), true);
        ink.line(12, 14.5, 17, 14.5, true);
        break;
    case Icon::ReferenceData:
        // An image and a scatter of points: the two kinds of reference data.
        ink.stroke(rectangle(3, 3, 13, 11));
        ink.stroke(polyline({{3.8, 12}, {7, 8}, {10, 10.5}, {12.5, 8.5}, {15.2, 11.5}}), true);
        for (const QPointF& p : {QPointF(12, 18.5), QPointF(15.5, 17), QPointF(19, 18.5),
                                 QPointF(16.5, 20.8), QPointF(20.2, 14.8), QPointF(13, 21.5)}) {
            ink.dot(p.x(), p.y(), 1.2);
        }
        break;
    // ---- what a view shows
    case Icon::ViewPlan:
        ink.stroke(rectangle(3, 3, 18, 18, 1.5));
        ink.stroke(polyline({{6.5, 16.5}, {10, 9}, {14.5, 13}, {17.5, 7}}), true);
        for (const QPointF& p :
             {QPointF(6.5, 16.5), QPointF(10, 9), QPointF(14.5, 13), QPointF(17.5, 7)}) {
            ink.node(p.x(), p.y());
        }
        break;
    case Icon::View3D: {
        const QPainterPath top = polyline({{12, 3}, {20, 7.5}, {12, 12}, {4, 7.5}}, true);
        ink.fill(top, true, 90);
        ink.stroke(polyline({{12, 3}, {20, 7.5}, {20, 16.5}, {12, 21}, {4, 16.5}, {4, 7.5}}, true));
        ink.line(12, 12, 12, 21);
        ink.stroke(top, true);
        break;
    }
    case Icon::ViewSection:
        // A profile on its axes: a section is a measured graph, which is what
        // tells it apart from the Section command's cut through the ground.
        ink.stroke(polyline({{4, 3.5}, {4, 20}, {20.5, 20}}));
        for (const double x : {8.5, 12.5, 16.5}) {
            ink.line(x, 20, x, 18.3, false, 1.1);
        }
        ink.stroke(polyline({{4, 14}, {8.5, 10}, {12.5, 13}, {16.5, 7}, {20.5, 10}}), true);
        break;
    case Icon::ViewElevation:
        // A building's front: the architectural meaning of an elevation.
        ink.line(2.5, 20, 21.5, 20);
        ink.stroke(polyline({{5, 20}, {5, 10}, {12, 4}, {19, 10}, {19, 20}}));
        ink.stroke(rectangle(9.5, 13, 5, 7), true);
        break;
    case Icon::ViewLayersFiltered:
        // The Layers glyph moved left, and the funnel everyone reads as
        // "filtered" in the accent: this view is not showing everything.
        ink.stroke(polyline({{2, 16}, {8.5, 20}, {15, 16}}));
        ink.stroke(polyline({{2, 12}, {8.5, 16}, {15, 12}}));
        ink.stroke(polyline({{8.5, 4.5}, {15, 8.2}, {8.5, 12}, {2, 8.2}}, true));
        ink.fill(polyline({{14.5, 2.5}, {22.5, 2.5}, {19.8, 6.5}, {19.8, 10.5}, {17.2, 11.8},
                           {17.2, 6.5}},
                          true),
                 true);
        break;
    // ---- panel tools
    case Icon::LayerNew:
        ink.stroke(polyline({{2.5, 15.5}, {9.5, 19.5}, {16.5, 15.5}}));
        ink.stroke(polyline({{9.5, 7.5}, {16.5, 11.5}, {9.5, 15.5}, {2.5, 11.5}}, true));
        ink.line(19, 2.5, 19, 9.5, true);
        ink.line(15.5, 6, 22.5, 6, true);
        break;
    case Icon::LayerNewChild:
        ink.stroke(rectangle(3, 3, 9, 5, 1.0));
        ink.stroke(polyline({{7, 8}, {7, 17}, {11, 17}}));
        ink.stroke(rectangle(11, 14.5, 10, 5, 1.0), true);
        ink.line(18.5, 3, 18.5, 10, true);
        ink.line(15, 6.5, 22, 6.5, true);
        break;
    case Icon::Rename:
        // A name field with the text cursor in it.
        ink.stroke(rectangle(2.5, 7, 19, 10, 1.5));
        ink.line(6, 12, 11, 12);
        ink.line(15, 4.5, 15, 19.5, true);
        ink.line(13, 4.5, 17, 4.5, true);
        ink.line(13, 19.5, 17, 19.5, true);
        break;
    case Icon::ZoomTo:
        ink.stroke(circle(10, 10, 6.5));
        ink.line(14.8, 14.8, 20.5, 20.5, false, 2.6);
        ink.stroke(rectangle(7.5, 7.5, 5, 5), true);
        break;
    case Icon::Help: {
        ink.stroke(circle(12, 12, 9));
        QPainterPath mark(QPointF(9, 9.6));
        mark.cubicTo(9, 5.6, 15, 5.6, 15, 9.4);
        mark.cubicTo(15, 11.6, 12, 11.8, 12, 14.4);
        ink.stroke(mark, true);
        ink.dot(12, 17.6, 1.2, true);
        break;
    }
    case Icon::About:
        ink.stroke(circle(12, 12, 9));
        ink.dot(12, 7.6, 1.25, true);
        ink.line(12, 11.2, 12, 17.4, true);
        break;
    // ---- Survey ------------------------------------------------------------------------
    case Icon::SurveyInverse: {
        // Two marks, north at the first, and what the inverse measures: the
        // line between them and its azimuth, swept clockwise from north.
        drawNorth(ink, 6, 15.8, 4);
        // (6, 18) to (19, 7): unit vector (13, -11) / 17.03, trimmed by the
        // marks' 2.2 radius at each end.
        ink.line(7.68, 16.58, 17.32, 8.42, true);
        QPainterPath sweep;
        // Radius 10 about (6, 18): large enough to read as the angle at 20 px.
        const QRectF round(-4, 8, 20, 20);
        sweep.arcMoveTo(round, 90.0);
        // From north (90 degrees in Qt's anticlockwise sense) to the line,
        // atan2(11, 13) = 40.2 degrees: 49.8 degrees clockwise.
        sweep.arcTo(round, 90.0, -49.8);
        ink.stroke(sweep, true, 1.3);
        drawMark(ink, 6, 18, false);
        drawMark(ink, 19, 7, false);
        break;
    }
    case Icon::SurveyForward:
        // From a known mark along a direction to a NEW point, which is the
        // accent: what the command adds to the drawing.
        drawNorth(ink, 5, 16.8, 4);
        // (5, 19) to (18, 7): unit u = (13, -12) / 17.69, trimmed by 2.2 and
        // 2.4. The arrowhead at the tip T = (16.24, 8.63), its wings at
        // T - 3u +- 2.2 n with n = (0.678, 0.735) across the line, says the
        // direction runs OUT to the new point - which the inverse's has not.
        ink.line(6.62, 17.51, 16.24, 8.63, true);
        ink.stroke(polyline({{12.55, 9.04}, {16.24, 8.63}, {15.53, 12.28}}), true);
        drawMark(ink, 5, 19, false);
        drawMark(ink, 18, 7, true);
        break;
    case Icon::SurveyArea: {
        const QPainterPath parcel =
            polyline({{3, 19}, {5, 6}, {14, 3}, {21, 9}, {18, 20}}, true);
        ink.fill(parcel, true, 130);
        ink.stroke(parcel);
        for (const QPointF& p :
             {QPointF(3, 19), QPointF(5, 6), QPointF(14, 3), QPointF(21, 9), QPointF(18, 20)}) {
            ink.node(p.x(), p.y());
        }
        break;
    }
    case Icon::SurveyAngle: {
        // Two rays from a vertex, the angle between them, and the degree mark.
        ink.line(4, 19, 21, 19);
        ink.line(4, 19, 14, 5.4);
        QPainterPath sweep;
        const QRectF round(-5, 10, 18, 18); // radius 9 about (4, 19)
        sweep.arcMoveTo(round, 0.0);
        // The second ray's direction, atan2(13.6, 10) = 53.7 degrees.
        sweep.arcTo(round, 0.0, 53.7);
        ink.stroke(sweep, true);
        ink.stroke(circle(18, 8, 2), true, 1.3);
        break;
    }
    case Icon::SurveyTraverse:
        // The legs, which the adjustment corrects, over the stations.
        ink.stroke(polyline({{4, 18}, {10, 7}, {15, 15}, {20, 5}}), true);
        for (const QPointF& p : {QPointF(4, 18), QPointF(10, 7), QPointF(15, 15), QPointF(20, 5)}) {
            drawStation(ink, p.x(), p.y());
        }
        break;
    case Icon::SurveyLevelBook:
        // A level on its tripod sighting a staff: the sight line is the
        // reading the book records.
        ink.stroke(rectangle(2.5, 7.5, 7, 3.5, 0.8));
        ink.line(6, 11, 3, 21, false, 1.3);
        ink.line(6, 11, 9, 21, false, 1.3);
        // Solid: at 20 px a dashed sight line this short is a single dash.
        ink.line(10, 9.25, 15.5, 9.25, true);
        ink.stroke(rectangle(16, 3, 5, 18));
        for (const double y : {6.0, 10.0, 14.0, 18.0}) {
            ink.fill(rectangle(16, y, 2.5, 2), false, 160);
        }
        break;
    case Icon::SurveyConverter: {
        // A globe and a grid, and the conversion between them both ways.
        ink.stroke(circle(7.5, 7.5, 5));
        ink.line(2.5, 7.5, 12.5, 7.5, false, 1.1);
        QPainterPath meridian;
        meridian.addEllipse(QPointF(7.5, 7.5), 2.2, 5);
        ink.stroke(meridian, false, 1.1);
        ink.stroke(rectangle(13, 13, 8, 8));
        ink.line(17, 13, 17, 21, false, 1.1);
        ink.line(13, 17, 21, 17, false, 1.1);
        QPainterPath there(QPointF(14.5, 4.5));
        there.quadTo(18.5, 4.5, 18.5, 10.5);
        ink.stroke(there, true);
        ink.stroke(polyline({{16.5, 8.5}, {18.5, 10.8}, {20.5, 8.5}}), true);
        QPainterPath back(QPointF(10.5, 19.5));
        back.quadTo(5.5, 19.5, 5.5, 13.5);
        ink.stroke(back, true);
        ink.stroke(polyline({{3.5, 15.5}, {5.5, 13.2}, {7.5, 15.5}}), true);
        break;
    }
    case Icon::SurveyImport:
        // A file's rows, and the arrow INTO the drawing to the new point it
        // makes (the accent), beside a point already there.
        ink.stroke(rectangle(2.5, 3, 8.5, 12, 0.8));
        for (const double y : {6.5, 9.0, 11.5}) {
            ink.line(4.5, y, 9, y, false, 1.1);
        }
        ink.line(12.5, 9, 17.6, 9, true);
        ink.stroke(polyline({{15.3, 6.8}, {17.8, 9}, {15.3, 11.2}}), true);
        drawMark(ink, 20.2, 9, true);
        drawMark(ink, 15.5, 18.5, false);
        break;
    case Icon::SurveyExport:
        // A point of the drawing, and the arrow OUT to the file it writes
        // (the file's rows are the accent).
        drawMark(ink, 3.8, 9, false);
        drawMark(ink, 8.5, 18.5, false);
        ink.line(6.4, 9, 11.3, 9, true);
        ink.stroke(polyline({{9, 6.8}, {11.5, 9}, {9, 11.2}}), true);
        ink.stroke(rectangle(13, 3, 8.5, 12, 0.8));
        for (const double y : {6.5, 9.0, 11.5}) {
            ink.line(15, y, 19.5, y, true, 1.1);
        }
        break;
    case Icon::SurveyPointManager:
        // A table of points: a header row, a column of marks (the accent),
        // and rows of values.
        ink.stroke(rectangle(2.5, 3.5, 19, 17, 0.8));
        ink.line(2.5, 8, 21.5, 8, false, 1.1);
        ink.line(9, 3.5, 9, 20.5, false, 1.1);
        for (const double y : {11.2, 14.7, 18.2}) {
            ink.dot(5.7, y, 1.1, true);
            ink.line(11, y, 19.5, y, false, 1.1);
        }
        break;
    case Icon::SurveyPointReport:
        // A page listing points: a mark heading it, then lines of text.
        ink.stroke(rectangle(4, 2.5, 16, 19, 0.8));
        drawMark(ink, 8, 7, true);
        ink.line(12, 6, 17.5, 6, false, 1.1);
        ink.line(12, 8.5, 16, 8.5, false, 1.1);
        for (const double y : {12.5, 15.5, 18.5}) {
            ink.line(6.5, y, 17.5, y, false, 1.1);
        }
        break;
    }
    painter.restore();
}

namespace {

class PaintedIconEngine final : public QIconEngine {
  public:
    explicit PaintedIconEngine(Icon which) : which_(which) {}

    void paint(QPainter* painter, const QRect& rect, QIcon::Mode mode, QIcon::State) override
    {
        // Disabled is the only mode drawn differently. Hover and checked are
        // shown by the button's background (see theme.cpp), so that one icon
        // serves a toolbar and a menu without the two disagreeing.
        const bool disabled = mode == QIcon::Disabled;
        paintIcon(*painter, which_, QRectF(rect),
                  disabled ? theme::textDisabled() : theme::text(),
                  disabled ? theme::textDisabled() : theme::accent());
    }

    QPixmap pixmap(const QSize& size, QIcon::Mode mode, QIcon::State state) override
    {
        QPixmap pixmap(size);
        pixmap.fill(Qt::transparent);
        QPainter painter(&pixmap);
        paint(&painter, QRect(QPoint(0, 0), size), mode, state);
        return pixmap;
    }

    [[nodiscard]] QIconEngine* clone() const override { return new PaintedIconEngine(which_); }

  private:
    Icon which_;
};

} // namespace

QIcon icon(Icon which) { return QIcon(new PaintedIconEngine(which)); }

// ---- the application icon -----------------------------------------------------------

QImage applicationIconImage(int size)
{
    QImage image(size, size, QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::transparent);
    QPainter painter(&image);
    painter.setRenderHint(QPainter::Antialiasing, true);
    // Drawn on a 256 grid: the largest size Windows asks for, so the numbers
    // below are pixels at that size and proportions at every other.
    painter.scale(size / 256.0, size / 256.0);

    // The tile. Slate, a little lighter at the top, with a hairline rim so it
    // holds its edge on a dark taskbar and a light desktop alike.
    QPainterPath tile;
    tile.addRoundedRect(QRectF(8, 8, 240, 240), 54, 54);
    QLinearGradient ground(0, 8, 0, 248);
    ground.setColorAt(0.0, QColor(0x2f, 0x37, 0x42));
    ground.setColorAt(1.0, QColor(0x14, 0x18, 0x1d));
    painter.fillPath(tile, ground);

    // A survey grid behind the blade, clipped to the tile: what the product
    // is for, said quietly. Dropped below 48 px, where it would only be noise.
    if (size >= 48) {
        painter.save();
        painter.setClipPath(tile);
        painter.setPen(QPen(QColor(255, 255, 255, 16), 2.0));
        for (int i = 1; i < 6; ++i) {
            const double at = 8.0 + 40.0 * i;
            painter.drawLine(QPointF(at, 8), QPointF(at, 248));
            painter.drawLine(QPointF(8, at), QPointF(248, at));
        }
        painter.restore();
    }
    painter.setPen(QPen(QColor(255, 255, 255, 38), 2.0));
    painter.setBrush(Qt::NoBrush);
    painter.drawPath(tile);

    // Below 64 px the sword is drawn LARGER within the tile and heavier. At
    // true proportions it is a few pixels wide on a taskbar and disappears;
    // an icon is a sign, not a scale drawing, and every icon set that ships
    // small sizes redraws them bolder for the same reason.
    const double boost = size >= 64 ? 1.0 : (size >= 32 ? 1.14 : 1.26);
    const double heft = size >= 64 ? 1.0 : (size >= 32 ? 1.25 : 1.55);
    painter.save();
    painter.setClipPath(tile);
    painter.translate(128.0, 128.0);
    painter.scale(boost, boost);
    painter.translate(-128.0, -128.0);

    // The blade: a sliver along a shallow curve from the guard to the point,
    // widest at the guard. Built from offsets either side of a quadratic
    // Bezier so the taper and the curve are each one number.
    const QPointF root(96, 166);
    const QPointF tip(214, 40);
    const QPointF bend(142, 92); // pulls the curve towards the back of the blade
    const auto onCurve = [&](double t) {
        const double u = 1.0 - t;
        return QPointF(u * u * root.x() + 2 * u * t * bend.x() + t * t * tip.x(),
                       u * u * root.y() + 2 * u * t * bend.y() + t * t * tip.y());
    };
    const auto normalAt = [&](double t) {
        const QPointF d = 2.0 * (1.0 - t) * (bend - root) + 2.0 * t * (tip - bend);
        const double length = std::hypot(d.x(), d.y());
        return QPointF(-d.y() / length, d.x() / length);
    };
    constexpr int kSamples = 28;
    QPolygonF edge;
    QPolygonF back;
    for (int i = 0; i <= kSamples; ++i) {
        const double t = static_cast<double>(i) / kSamples;
        // Full width for most of the length, then a quick run-out to the point.
        const double half = 9.0 * heft * (t < 0.82 ? 1.0 : (1.0 - t) / 0.18);
        edge << onCurve(t) + normalAt(t) * half;
        back << onCurve(t) - normalAt(t) * half;
    }
    QPainterPath blade;
    blade.addPolygon(edge);
    for (int i = kSamples; i >= 0; --i) {
        blade.lineTo(back[i]);
    }
    blade.closeSubpath();
    QLinearGradient steel(root, tip);
    steel.setColorAt(0.0, QColor(0xb4, 0xc0, 0xce));
    steel.setColorAt(1.0, QColor(0xf6, 0xf9, 0xfc));
    painter.fillPath(blade, steel);
    // The cutting edge, a brighter line along one side.
    painter.setPen(QPen(QColor(255, 255, 255, 210), 2.4, Qt::SolidLine, Qt::RoundCap));
    painter.drawPolyline(edge.mid(0, kSamples - 3));

    // The handle, in the one warm colour of the mark, continuing the blade's
    // own line BACK from the root. `along` is the unit tangent pointing root
    // to tip: with the normal n = (-dy, dx) / L, the tangent is (n.y, -n.x).
    // The first version had the sign wrong, so the handle ran UP the blade and
    // the guard ended up at the pommel - visible at once on the contact sheet,
    // which is what the sheet is for.
    const QPointF along(normalAt(0.0).y(), -normalAt(0.0).x());
    const QPointF pommel = root - along * 66.0;
    painter.setPen(QPen(QColor(0xe5, 0x48, 0x4d), 22.0 * heft, Qt::SolidLine, Qt::RoundCap));
    painter.drawLine(root - along * 10.0, pommel);
    // The wrap: dark diamonds across the handle. Only where there are pixels
    // enough to show them.
    if (size >= 64) {
        painter.setPen(QPen(QColor(0x5a, 0x16, 0x1b), 3.0, Qt::SolidLine, Qt::RoundCap));
        const QPointF across = normalAt(0.0) * 9.0;
        for (int i = 1; i <= 4; ++i) {
            const QPointF centre = root - along * (8.0 + 11.5 * i);
            painter.drawLine(centre - across - along * 5.0, centre + across + along * 5.0);
            painter.drawLine(centre - across + along * 5.0, centre + across - along * 5.0);
        }
    }
    // The guard, across the blade at its root.
    painter.setPen(QPen(QColor(0xd9, 0xb2, 0x4a), 10.0 * heft, Qt::SolidLine, Qt::RoundCap));
    painter.drawLine(root - normalAt(0.0) * 24.0, root + normalAt(0.0) * 24.0);
    painter.restore();
    return image;
}

QIcon applicationIcon()
{
    QIcon result;
    for (const int size : {16, 24, 32, 48, 64, 128, 256}) {
        result.addPixmap(QPixmap::fromImage(applicationIconImage(size)));
    }
    return result;
}

} // namespace katana::qt
