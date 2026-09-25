#include "plotting/section_painter.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>

#include <QPainter>
#include <QPolygonF>
#include <QStringList>

#include "katana/cad/plotting/section_annotation.hpp"

namespace katana::qt {

namespace plotting = katana::cad::plotting;

using katana::geometry::Box2;
using katana::geometry::Point2;
using plotting::HorizontalJustify;
using plotting::VerticalJustify;
using plotting::Viewport;
using plotting::ViewportKind;

namespace {

constexpr double kInfinity = std::numeric_limits<double>::infinity();

// The sheet painter's inks (sheet_painter.cpp): the frame's black, the
// grid's light grey, a faint grey for construction.
const QColor kInk(0, 0, 0);
const QColor kGridInk(190, 190, 190);
const QColor kFaint(150, 150, 150);

const std::array<QColor, 5> kGroundInks{QColor(125, 80, 40), QColor(0, 125, 50),
                                        QColor(0, 90, 200), QColor(150, 0, 150),
                                        QColor(210, 120, 0)};
const QColor kDesignInk(210, 0, 0);
// Cut and fill, shaded between the design and the ground: light red where the
// ground stands above the design and is dug away, light green where the
// design stands above the ground and is built up.
const QColor kCutShade(250, 200, 200);
const QColor kFillShade(200, 236, 200);

// A long section with no levels to fit is drawn as long sections usually
// are: H 1:500 V 1:50.
constexpr double kLongSectionExaggeration = 10.0;

QString qText(const std::string& text) { return QString::fromStdString(text); }

// `value` as a label on a grid of `step` (plotting::stepText).
QString number(double value, double step) { return qText(plotting::stepText(value, step)); }

// The pen of each series, in the order they were cut: the design red and
// heavier, each ground a colour of its own.
std::vector<QPen> seriesPens(const SectionCanvas& canvas, const katana::cad::Section& section)
{
    std::vector<QPen> pens;
    std::size_t ground = 0;
    for (const katana::cad::SectionSurface& surface : section.surfaces) {
        const bool design = plotting::isDesignSeries(surface.name);
        pens.push_back(canvas.pen(design ? kDesignInk : kGroundInks[ground % kGroundInks.size()],
                                  design ? 0.5 : 0.35));
        ground += design ? 0 : 1;
    }
    return pens;
}

// A row of a long section's data band: a series' levels, the cut or fill
// between the design and the ground, or - neither - the chainage.
struct BandRow {
    const katana::cad::SectionSurface* series = nullptr;
    bool cutFill = false;
};

// The band's rows, top down: the design first - the level a long section is
// read for - then the ground, two series at most; the cut or fill when there
// are both; the chainage last.
std::vector<BandRow> bandRows(const katana::cad::Section& section)
{
    std::vector<BandRow> rows;
    for (const bool design : {true, false}) {
        for (const katana::cad::SectionSurface& surface : section.surfaces) {
            if (plotting::isDesignSeries(surface.name) == design) {
                rows.push_back({&surface, false});
            }
        }
    }
    rows.resize(std::min<std::size_t>(rows.size(), 2));
    if (plotting::designSeries(section) != nullptr && plotting::groundSeries(section) != nullptr) {
        rows.push_back({nullptr, true});
    }
    rows.push_back({});
    return rows;
}

// The level labels up a section's left side.
SectionTextStyle levelLabelStyle()
{
    SectionTextStyle style;
    style.capMm = 1.5;
    style.horizontal = HorizontalJustify::Right;
    style.vertical = VerticalJustify::Middle;
    return style;
}

// A line of text drawn in the box placeLabels gave it: `turned` stands it up
// from the box's foot, reading upwards; otherwise it sits on the box's base.
void drawInBox(SectionCanvas& canvas, const Box2& box, const QString& line, SectionTextStyle style,
               bool turned)
{
    style.horizontal = HorizontalJustify::Left;
    if (turned) {
        style.angleDegrees = 90.0;
        style.vertical = VerticalJustify::Middle;
        canvas.text(Point2(box.center().x, box.min.y), line, style);
    } else {
        style.angleDegrees = 0.0;
        style.vertical = VerticalJustify::Bottom;
        canvas.text(box.min, line, style);
    }
}

// `levels` widened to take in `level` too.
std::optional<std::pair<double, double>> widened(std::optional<std::pair<double, double>> levels,
                                                 std::optional<double> level)
{
    if (!level) {
        return levels;
    }
    if (!levels) {
        return std::pair{*level, *level};
    }
    return std::pair{std::min(levels->first, *level), std::max(levels->second, *level)};
}

// How one section is drawn in its box.
struct SectionAxes {
    double scale = 500.0;      // 1 : scale horizontally
    double exaggeration = 1.0; // the vertical scale is scale / exaggeration
    Point2 centre{};           // (axis value, elevation) at the plot's middle
    bool autoCentre = false;
    double shift = 0.0; // axis value = station + shift
    // A long section's chainages: exactly these are shown, ends labelled.
    std::optional<std::pair<double, double>> range;
    bool cross = false;    // a cross section: the centreline at 0
    bool dataBand = false; // a long section's levels under it
    const katana::cad::LayerOverrides* hidden = nullptr;
    QString caption; // under a cross section: its chainage
    // Gives the crossings their own levels (their entities' heights).
    const katana::entity::Model* model = nullptr;
    // A cross section's design level at its centreline from the alignment's
    // profile: read there when no design series gives one.
    std::optional<double> profileLevel;
};

// A label placed with the others (plotting::placeLabels), and how to draw it
// in the place it gets.
struct Placed {
    plotting::LabelCandidate candidate;
    // The place index given is into the candidate's boxes followed by these.
    std::function<void(std::size_t place)> draw;
    bool note = false; // counted when dropped
    // Places tried only once every label has had its own: a last resort
    // that must not take a place another label could have had.
    std::vector<Box2> fallback{};
};

// Places `labels` in `bounds` clear of `obstacles` and draws each that has a
// place; returns how many notes had none.
std::size_t placeAndDraw(const std::vector<Placed>& labels, const Box2& bounds,
                         const std::vector<Box2>& obstacles, double gapMm)
{
    std::vector<plotting::LabelCandidate> candidates;
    candidates.reserve(labels.size());
    for (const Placed& label : labels) {
        candidates.push_back(label.candidate);
    }
    std::vector<std::optional<std::size_t>> places =
        plotting::placeLabels(candidates, bounds, obstacles, gapMm);
    // The last resorts, in the same order, around everything placed.
    std::vector<Box2> taken = obstacles;
    for (std::size_t i = 0; i < labels.size(); ++i) {
        if (places[i]) {
            taken.push_back(candidates[i].boxes[*places[i]]);
        }
    }
    std::vector<std::size_t> waiting;
    std::vector<plotting::LabelCandidate> lastResorts;
    for (std::size_t i = 0; i < labels.size(); ++i) {
        if (!places[i] && !labels[i].fallback.empty()) {
            waiting.push_back(i);
            lastResorts.push_back({labels[i].fallback, candidates[i].priority});
        }
    }
    const auto resorted = plotting::placeLabels(lastResorts, bounds, taken, gapMm);
    for (std::size_t w = 0; w < waiting.size(); ++w) {
        if (resorted[w]) {
            places[waiting[w]] = candidates[waiting[w]].boxes.size() + *resorted[w];
        }
    }

    std::size_t dropped = 0;
    for (std::size_t i = 0; i < labels.size(); ++i) {
        if (places[i]) {
            labels[i].draw(*places[i]);
        } else if (labels[i].note) {
            ++dropped;
        }
    }
    return dropped;
}

// Draws `section` in `area`; returns how many of its notes it had no room
// for (SheetPaintStats::sectionNotesDropped).
std::size_t paintSection(SectionCanvas& canvas, const Box2& area, const katana::cad::Section& section,
                         const SectionAxes& axes)
{
    QPainter& painter = canvas.painter();
    const katana::cad::SectionSurface* design = plotting::designSeries(section);
    const katana::cad::SectionSurface* ground = plotting::groundSeries(section);
    const std::vector<BandRow> rows = axes.dataBand ? bandRows(section) : std::vector<BandRow>{};
    // The stations shown: a long section's range and nothing past it, else
    // all of it.
    const double fromStation = axes.range ? axes.range->first - axes.shift : -kInfinity;
    const double toStation = axes.range ? axes.range->second - axes.shift : kInfinity;
    const auto levels = widened(
        plotting::levelRange(section, fromStation, toStation, axes.model, axes.hidden),
        axes.profileLevel);

    const double hMm = 1000.0 / axes.scale;
    const double vMm = hMm * axes.exaggeration;
    Point2 centre = axes.centre;
    if (axes.autoCentre) {
        if (levels) {
            centre.y = 0.5 * (levels->first + levels->second);
        }
        if (axes.range) {
            centre.x = 0.5 * (axes.range->first + axes.range->second);
        } else if (axes.cross) {
            centre.x = 0.0;
        } else if (const Box2 extent = section.extent(); !extent.empty()) {
            centre.x = extent.center().x + axes.shift;
        }
    }

    // The plot, laid out around the widest level label: the levels' step
    // depends only on the plot's height, so it is known before the width is.
    const SectionTextStyle levelStyle = levelLabelStyle();
    plotting::SectionLayoutRequest request;
    request.area = area;
    request.bandRows = rows.size();
    request.caption = !axes.caption.isEmpty();
    plotting::SectionLayout layout = plotting::sectionPlotLayout(request);
    if (layout.plot.empty()) {
        return 0;
    }
    const double midY = layout.plot.center().y;
    const double lowLevel = centre.y - 0.5 * layout.plot.height() / vMm;
    const double highLevel = centre.y + 0.5 * layout.plot.height() / vMm;
    // Level labels are stacked up the side: their height sets their step,
    // 7 mm apart at the least so the grid stays open.
    const double vStep = plotting::labelStep(vMm, 7.0, 1.0, [&](double) { return levelStyle.capMm; });
    const std::vector<double> levelValues = plotting::gridValues(lowLevel, highLevel, vStep, 200);
    request.levelLabelMm = 0.0;
    for (const double level : levelValues) {
        request.levelLabelMm =
            std::max(request.levelLabelMm, canvas.textWidthMm(number(level, vStep), levelStyle));
    }
    layout = plotting::sectionPlotLayout(request);
    if (layout.plot.empty()) {
        return 0;
    }
    Box2 plot = layout.plot;
    const bool banded = layout.banded;
    const double midX = plot.center().x;
    const auto X = [&](double value) { return midX + (value - centre.x) * hMm; };
    const auto Y = [&](double level) { return midY + (level - centre.y) * vMm; };
    // A chainage range is shown exactly: the plot ends where it does.
    if (axes.range) {
        plot.min.x = std::max(plot.min.x, X(axes.range->first));
        plot.max.x = std::min(plot.max.x, X(axes.range->second));
        if (!(plot.width() > 1.0)) {
            return 0; // the range is off the plot
        }
    }
    const double firstValue = centre.x + (plot.min.x - midX) / hMm;
    const double lastValue = centre.x + (plot.max.x - midX) / hMm;

    SectionTextStyle small;
    SectionTextStyle values = small;
    values.capMm = 1.5;
    const SectionTextStyle cell = small; // a band value, turned across its row

    // The step along: a band's values are turned across it, so they need
    // only their height; the axis values need their width.
    const double hStep =
        banded ? plotting::labelStep(hMm, 10.0, 1.0, [&](double) { return cell.capMm; })
               : plotting::labelStep(hMm, 10.0, 2.0, [&](double step) {
                     double widest = 0.0;
                     for (const double v : plotting::gridValues(firstValue, lastValue, step, 200)) {
                         widest = std::max(widest, canvas.textWidthMm(number(v, step), values));
                     }
                     return widest;
                 });
    const std::vector<double> alongValues = plotting::gridValues(firstValue, lastValue, hStep, 400);
    const std::vector<QPen> pens = seriesPens(canvas, section);

    // ---- under the plot's clip: shading, grid, lines ----
    const QRectF plotDevice = canvas.at(plot);
    painter.save();
    painter.setClipRect(plotDevice, Qt::IntersectClip);

    // Cut and fill, under everything else.
    if (design != nullptr && ground != nullptr) {
        painter.setPen(Qt::NoPen);
        for (const plotting::EarthworkRegion& region :
             plotting::earthworkRegions(*design, *ground, fromStation, toStation)) {
            QPolygonF polygon;
            for (const Point2& p : region.outline) {
                polygon << canvas.at(Point2(X(p.x + axes.shift), Y(p.y)));
            }
            painter.setBrush(
                canvas.fill(region.kind == plotting::Earthwork::Cut ? kCutShade : kFillShade));
            painter.drawPolygon(polygon);
        }
        painter.setBrush(Qt::NoBrush);
    }

    painter.setPen(canvas.pen(kGridInk, 0.13));
    for (const double v : alongValues) {
        painter.drawLine(canvas.at(Point2(X(v), plot.min.y)), canvas.at(Point2(X(v), plot.max.y)));
    }
    for (const double e : levelValues) {
        painter.drawLine(canvas.at(Point2(plot.min.x, Y(e))), canvas.at(Point2(plot.max.x, Y(e))));
    }

    // The centreline of a cross section.
    const bool centreline = axes.cross && X(0.0) > plot.min.x && X(0.0) < plot.max.x;
    if (centreline) {
        painter.setPen(canvas.dashedPen(kInk, 0.25, {6.0, 1.2, 0.6, 1.2}));
        painter.drawLine(canvas.at(Point2(X(0.0), plot.min.y)), canvas.at(Point2(X(0.0), plot.max.y)));
    }

    // Crossings: where the drawing's lines cut the section, marked at their
    // level when they have one; their notes are placed below with the rest.
    struct Crossing {
        double x = 0.0;
        std::optional<double> y; // the marker, when it is inside the plot
        double along = 0.0;      // chainage or offset
        plotting::CrossingNote note;
    };
    std::vector<Crossing> crossings;
    for (const katana::cad::SectionCrossing& crossing : section.crossings) {
        if (axes.hidden != nullptr && axes.hidden->hides(crossing.layer)) {
            continue;
        }
        Crossing at;
        at.along = crossing.station + axes.shift;
        at.x = X(at.along);
        if (at.x <= plot.min.x || at.x >= plot.max.x) {
            continue;
        }
        at.note = plotting::crossingNote(section, crossing, axes.model);
        painter.setPen(canvas.dashedPen(kFaint, 0.18, {1.5, 1.0}));
        painter.drawLine(canvas.at(Point2(at.x, plot.min.y)), canvas.at(Point2(at.x, plot.max.y)));
        crossings.push_back(std::move(at));
    }

    // Surfaces, broken at every gap.
    for (std::size_t s = 0; s < section.surfaces.size(); ++s) {
        painter.setPen(pens[s]);
        QPolygonF run;
        const auto flush = [&] {
            if (run.size() > 1) {
                painter.drawPolyline(run);
            }
            run.clear();
        };
        for (const katana::cad::SectionSample& sample : section.surfaces[s].samples) {
            if (!sample.elevation) {
                flush();
                continue;
            }
            run << canvas.at(Point2(X(sample.station + axes.shift), Y(*sample.elevation)));
        }
        flush();
    }

    // The crossings' markers, over the series they sit on or under.
    for (Crossing& at : crossings) {
        if (!at.note.level) {
            continue;
        }
        const double y = Y(*at.note.level);
        painter.setPen(canvas.pen(kInk, 0.25));
        painter.setBrush(Qt::white);
        painter.drawEllipse(canvas.at(Point2(at.x, y)), canvas.mm(0.8), canvas.mm(0.8));
        painter.setBrush(Qt::NoBrush);
        if (y > plot.min.y && y < plot.max.y) {
            at.y = y;
        }
    }
    painter.restore();

    // The plot's frame; a range's ends heavier, as where the section stops.
    painter.setPen(canvas.pen(kInk, 0.25));
    painter.setBrush(Qt::NoBrush);
    painter.drawRect(plotDevice);
    std::vector<std::pair<double, double>> ends; // (chainage, paper x) of the range's ends in view
    if (axes.range) {
        painter.setPen(canvas.pen(kInk, 0.35));
        for (const double end : {axes.range->first, axes.range->second}) {
            const double x = X(end);
            if (x >= plot.min.x - 1e-6 && x <= plot.max.x + 1e-6) {
                painter.drawLine(canvas.at(Point2(x, plot.min.y)), canvas.at(Point2(x, plot.max.y)));
                ends.emplace_back(end, x);
            }
        }
    }

    // ---- labels inside the plot -------------------------------------------------------
    // The datum and the surface key, the centreline's levels, and each
    // crossing's note and offset: placed so that none covers another, the
    // nearest the middle kept first.
    std::vector<Placed> inside;
    // A note is never written over another crossing's line or the
    // centreline, where it would seem to name that one: such places are not
    // offered. Its own line is the one at `own`.
    std::vector<double> linesAt;
    for (const Crossing& at : crossings) {
        linesAt.push_back(at.x);
    }
    if (centreline) {
        linesAt.push_back(X(0.0));
    }
    const auto onAnotherLine = [&linesAt](const Box2& place, double own) {
        constexpr double clear = 0.25;
        return std::any_of(linesAt.begin(), linesAt.end(), [&](double x) {
            return x != own && place.min.x < x + clear && place.max.x > x - clear;
        });
    };

    {
        SectionTextStyle datum = small;
        datum.capMm = 1.5;
        const QString datumText = QString("DATUM RL %1").arg(lowLevel, 0, 'f', 2);
        const double w = canvas.textWidthMm(datumText, datum);
        const Box2 box(Point2(plot.min.x + 1.0, plot.min.y + 1.0),
                       Point2(plot.min.x + 1.0 + w, plot.min.y + 1.0 + datum.capMm));
        inside.push_back({{{box}, -3.0}, [&canvas, box, datumText, datum](std::size_t) {
                              canvas.knockOut(box.inflated(0.4));
                              drawInBox(canvas, box, datumText, datum, false);
                          }});
    }

    // The surface key, top left.
    if (!section.surfaces.empty()) {
        SectionTextStyle key = small;
        key.capMm = 1.5;
        double widest = 0.0;
        for (const auto& surface : section.surfaces) {
            widest = std::max(widest, canvas.textWidthMm(qText(surface.name), key));
        }
        const double pitch = 3.2;
        const double count = static_cast<double>(section.surfaces.size());
        const Box2 box(Point2(plot.min.x + 0.6, plot.max.y - 2.5 - pitch * (count - 1.0) - 1.8),
                       Point2(plot.min.x + 9.5 + widest, plot.max.y - 0.6));
        inside.push_back({{{box}, -2.0}, [&, box, key, pitch](std::size_t) {
                              canvas.knockOut(box);
                              SectionTextStyle line = key;
                              line.vertical = VerticalJustify::Middle;
                              double y = plot.max.y - 2.5;
                              for (std::size_t s = 0; s < section.surfaces.size(); ++s) {
                                  painter.setPen(pens[s]);
                                  painter.drawLine(canvas.at(Point2(plot.min.x + 1.5, y)),
                                                   canvas.at(Point2(plot.min.x + 7.0, y)));
                                  canvas.text(Point2(plot.min.x + 8.0, y),
                                              qText(section.surfaces[s].name), line);
                                  y -= pitch;
                              }
                          }});
    }

    // A cross section's design and ground levels at the centreline, and the
    // cut or fill between them. The design is the design series', else the
    // alignment's profile's.
    if (centreline) {
        const double station = -axes.shift;
        std::optional<double> designLevel =
            design != nullptr ? plotting::levelAt(*design, station) : std::nullopt;
        const bool designSeriesLevel = designLevel.has_value();
        if (!designLevel) {
            designLevel = axes.profileLevel;
        }
        const auto groundLevel =
            ground != nullptr ? plotting::levelAt(*ground, station) : std::nullopt;
        const auto seriesPen = [&](const katana::cad::SectionSurface* series) {
            return pens[static_cast<std::size_t>(series - section.surfaces.data())];
        };
        QStringList lines;
        if (designLevel) {
            const QString name =
                designSeriesLevel ? qText(design->name).toUpper() : QStringLiteral("DESIGN");
            lines << QString("%1 RL %2").arg(name).arg(*designLevel, 0, 'f', 3);
        }
        if (groundLevel) {
            lines << QString("%1 RL %2").arg(qText(ground->name).toUpper()).arg(*groundLevel, 0, 'f', 3);
        }
        if (designLevel && groundLevel && std::abs(*designLevel - *groundLevel) >= 0.0005) {
            // CUT where the ground is above the design, FILL where below.
            const double designLessGround = *designLevel - *groundLevel;
            lines << QString("%1 %2")
                         .arg(designLessGround > 0.0 ? "FILL" : "CUT")
                         .arg(std::abs(designLessGround), 0, 'f', 3);
        }
        if (!lines.isEmpty()) {
            // A tick across the centreline at each level read, in its series' pen.
            const double x0 = X(0.0);
            const auto tick = [&](const QPen& pen, std::optional<double> level) {
                if (!level || !(Y(*level) > plot.min.y) || !(Y(*level) < plot.max.y)) {
                    return;
                }
                painter.setPen(pen);
                painter.drawLine(canvas.at(Point2(x0 - 1.0, Y(*level))),
                                 canvas.at(Point2(x0 + 1.0, Y(*level))));
            };
            if (designLevel) {
                tick(designSeriesLevel ? seriesPen(design) : canvas.pen(kDesignInk, 0.5), designLevel);
            }
            if (ground != nullptr) {
                tick(seriesPen(ground), groundLevel);
            }
            SectionTextStyle note = small;
            note.lineSpacingMm = 2.3;
            double w = 0.0;
            for (const QString& line : lines) {
                w = std::max(w, canvas.textWidthMm(line, note));
            }
            const double h = note.lineSpacingMm * static_cast<double>(lines.size() - 1) + note.capMm;
            // Beside the centreline just above the higher level, else at the
            // plot's top, else at its foot; right of it first.
            double above = plot.min.y + 1.0;
            for (const auto& level : {designLevel, groundLevel}) {
                if (level) {
                    above = std::max(above, Y(*level) + 1.2);
                }
            }
            std::vector<Box2> clear;
            std::vector<Box2> across;
            for (const double base : {above, plot.max.y - 1.0 - h, plot.min.y + 1.0}) {
                for (const Box2& place : {Box2(Point2(x0 + 0.8, base), Point2(x0 + 0.8 + w, base + h)),
                                          Box2(Point2(x0 - 0.8 - w, base), Point2(x0 - 0.8, base + h))}) {
                    (onAnotherLine(place, x0) ? across : clear).push_back(place);
                }
            }
            // Clear of the crossings' lines first. Among services close
            // either side there may be no such place. Level text written
            // across a line, masked, cannot be taken for that line's note
            // (those stand up along their lines), so it goes across one
            // rather than leave the centreline unlabelled - as a last resort,
            // once the crossings' notes have had their places.
            std::vector<Box2> boxes = clear;
            boxes.insert(boxes.end(), across.begin(), across.end());
            inside.push_back({{clear, -1.0},
                              [&canvas, boxes, lines, note](std::size_t k) {
                                  const Box2& box = boxes[k];
                                  canvas.knockOut(box.inflated(0.3));
                                  SectionTextStyle first = note;
                                  first.vertical = VerticalJustify::Top;
                                  canvas.text(Point2(box.min.x, box.max.y),
                                              lines.join(QLatin1Char('\n')), first);
                              },
                              true,
                              across});
        }
    }

    // Each crossing: its note stood up beside its line, above or below its
    // marker (or from the plot's top or foot without one), in full or as its
    // layer alone - staggered through those places, the nearest the middle
    // placed first; its chainage or offset under the marker.
    const double middle = axes.cross ? X(0.0) : plot.center().x;
    SectionTextStyle offsetStyle = small;
    offsetStyle.capMm = 1.3;
    for (const Crossing& at : crossings) {
        const double priority = std::abs(at.x - middle);
        const double cap = small.capMm;
        std::vector<Box2> boxes;
        std::vector<QString> texts;
        QStringList forms{qText(at.note.text)};
        if (at.note.shortText != at.note.text) {
            forms << qText(at.note.shortText);
        }
        for (const QString& form : forms) {
            const double w = canvas.textWidthMm(form, small);
            for (const bool high : {true, false}) {
                for (const double side : {-1.0, 1.0}) {
                    const double x0 = side < 0.0 ? at.x - 0.4 - cap : at.x + 0.4;
                    double foot = 0.0;
                    if (at.y) {
                        foot = high ? *at.y + 1.2 : *at.y - 1.2 - w;
                    } else {
                        foot = high ? plot.max.y - 1.0 - w : plot.min.y + 1.0;
                    }
                    const Box2 place(Point2(x0, foot), Point2(x0 + cap, foot + w));
                    if (!onAnotherLine(place, at.x)) {
                        boxes.push_back(place);
                        texts.push_back(form);
                    }
                }
            }
        }
        inside.push_back({{boxes, priority},
                          [&canvas, boxes, texts, small](std::size_t k) {
                              // Masked, so the note reads where it crosses a series or a shade.
                              canvas.knockOut(boxes[k].inflated(0.15));
                              drawInBox(canvas, boxes[k], texts[k], small, true);
                          },
                          true});
        if (at.y) {
            const double along = std::abs(at.along) < 0.005 ? 0.0 : at.along;
            const QString offset = axes.cross ? QString::number(along, 'f', 2)
                                              : QString("CH %1").arg(along, 0, 'f', 3);
            const double w = canvas.textWidthMm(offset, offsetStyle);
            const std::vector<Box2> places{
                Box2(Point2(at.x - w / 2.0, *at.y - 1.2 - offsetStyle.capMm),
                     Point2(at.x + w / 2.0, *at.y - 1.2)),
                Box2(Point2(at.x - w / 2.0, *at.y + 1.2),
                     Point2(at.x + w / 2.0, *at.y + 1.2 + offsetStyle.capMm))};
            inside.push_back({{places, priority + 1e-6},
                              [&canvas, places, offset, offsetStyle](std::size_t k) {
                                  canvas.knockOut(places[k].inflated(0.2));
                                  drawInBox(canvas, places[k], offset, offsetStyle, false);
                              }});
        }
    }
    const std::size_t dropped = placeAndDraw(inside, plot.inflated(-0.3), {}, 0.3);

    // ---- labels outside the plot --------------------------------------------------------
    // Levels up the left, and under the plot the axis values - or the data
    // band - and a cross section's caption; none outside the viewport.
    std::vector<Placed> outside;
    std::vector<Box2> fixed;
    for (const double e : levelValues) {
        const QString label = number(e, vStep);
        const double w = canvas.textWidthMm(label, levelStyle);
        const Box2 box(Point2(plot.min.x - 0.8 - w, Y(e) - levelStyle.capMm / 2.0),
                       Point2(plot.min.x - 0.8, Y(e) + levelStyle.capMm / 2.0));
        outside.push_back({{{box}, -1.0}, [&canvas, box, label, levelStyle](std::size_t) {
                               drawInBox(canvas, box, label, levelStyle, false);
                           }});
    }
    if (!axes.caption.isEmpty()) {
        SectionTextStyle caption;
        caption.capMm = 2.0;
        caption.xFactor = 1.0;
        caption.bold = true;
        caption.horizontal = HorizontalJustify::Centre;
        const double room = area.width() - 2.0;
        double squeeze = 1.0;
        if (const double w = canvas.textWidthMm(axes.caption, caption); w > room && room > 0.0) {
            squeeze = room / w;
        }
        const double w = canvas.textWidthMm(axes.caption, caption) * squeeze;
        const double x =
            std::clamp(plot.center().x, area.min.x + 1.0 + w / 2.0, area.max.x - 1.0 - w / 2.0);
        canvas.text(Point2(x, area.min.y + 1.2), axes.caption, caption, squeeze);
        fixed.emplace_back(Point2(x - w / 2.0, area.min.y + 1.2),
                           Point2(x + w / 2.0, area.min.y + 1.2 + caption.capMm));
    }

    if (!banded) {
        // Axis values; a range's ends in full, reading inwards from them.
        const auto axisLabel = [&](const QString& label, double x, int align, double priority) {
            const double w = canvas.textWidthMm(label, values);
            const double left = align < 0 ? x : (align > 0 ? x - w : x - w / 2.0);
            const Box2 box(Point2(left, plot.min.y - 0.8 - values.capMm),
                           Point2(left + w, plot.min.y - 0.8));
            outside.push_back({{{box}, priority}, [&canvas, box, label, values](std::size_t) {
                                   drawInBox(canvas, box, label, values, false);
                               }});
        };
        for (const auto& [value, x] : ends) {
            axisLabel(QString::number(value, 'f', 3), x, value == axes.range->first ? -1 : 1, -2.0);
        }
        for (const double v : alongValues) {
            axisLabel(number(v, hStep), X(v), 0, 0.0);
        }
    }
    placeAndDraw(outside, area, fixed, 0.3);

    if (!banded) {
        return dropped;
    }

    // ---- the data band: each row's value at every column that has room -----------------
    const double rowH = plotting::kSectionBandRowMm;
    const double bandTop = plot.min.y - 1.0;
    const double bandBottom = bandTop - rowH * static_cast<double>(rows.size());
    const double bandLeft = plot.min.x - layout.leftMm + 0.5;
    SectionTextStyle head = small;
    head.capMm = 1.5;
    head.vertical = VerticalJustify::Middle;
    for (std::size_t r = 0; r < rows.size(); ++r) {
        const double top = bandTop - static_cast<double>(r) * rowH;
        painter.setPen(canvas.pen(kInk, 0.18));
        painter.drawRect(canvas.at(Box2(Point2(bandLeft, top - rowH), Point2(plot.max.x, top))));
        painter.drawLine(canvas.at(Point2(plot.min.x, top)), canvas.at(Point2(plot.min.x, top - rowH)));
        const QString name = rows[r].cutFill            ? QStringLiteral("CUT/FILL")
                             : rows[r].series == nullptr ? QStringLiteral("CHAINAGE")
                                                         : qText(rows[r].series->name).toUpper();
        const double room = plot.min.x - bandLeft - 1.0;
        double squeeze = 1.0;
        if (const double w = canvas.textWidthMm(name, head); w > room && room > 0.0) {
            squeeze = room / w;
        }
        canvas.text(Point2(bandLeft + 0.6, top - rowH / 2.0), name, head, squeeze);
    }
    // The columns: a range's ends first, written in full just inside them,
    // then the grid's; a column too near one already placed is left out.
    struct Column {
        double value = 0.0;
        QString chainage;
    };
    std::vector<Column> columns;
    std::vector<plotting::LabelCandidate> places;
    const double half = cell.capMm / 2.0 + 0.1;
    for (const auto& [value, x] : ends) {
        const bool start = value == axes.range->first;
        const double left = start ? x + 0.4 : x - 0.4 - 2.0 * half;
        columns.push_back({value, QString::number(value, 'f', 3)});
        places.push_back({{Box2(Point2(left, bandBottom), Point2(left + 2.0 * half, bandTop))}, -1.0});
    }
    for (const double v : alongValues) {
        columns.push_back({v, number(v, hStep)});
        places.push_back({{Box2(Point2(X(v) - half, bandBottom), Point2(X(v) + half, bandTop))}, 0.0});
    }
    const auto placedColumns = plotting::placeLabels(
        places, Box2(Point2(plot.min.x, bandBottom), Point2(plot.max.x, bandTop)), {}, 0.6);
    SectionTextStyle turned = cell;
    turned.angleDegrees = 90.0;
    turned.vertical = VerticalJustify::Middle;
    turned.horizontal = HorizontalJustify::Centre;
    for (std::size_t c = 0; c < columns.size(); ++c) {
        if (!placedColumns[c]) {
            continue;
        }
        const double x = places[c].boxes.front().center().x;
        const double station = columns[c].value - axes.shift;
        for (std::size_t r = 0; r < rows.size(); ++r) {
            QString value;
            if (rows[r].cutFill) {
                if (const auto difference = plotting::cutFillAt(*design, *ground, station)) {
                    value = qText(plotting::cutFillText(*difference));
                }
            } else if (rows[r].series == nullptr) {
                value = columns[c].chainage;
            } else if (const auto level = plotting::levelAt(*rows[r].series, station)) {
                value = QString::number(*level, 'f', 3);
            }
            if (value.isEmpty()) {
                continue;
            }
            // Turned across its row, and never taller than the row.
            const double room = rowH - 1.2;
            double squeeze = 1.0;
            if (const double w = canvas.textWidthMm(value, turned); w > room) {
                squeeze = room / w;
            }
            const double top = bandTop - static_cast<double>(r) * rowH;
            canvas.text(Point2(x, top - rowH / 2.0), value, turned, squeeze);
        }
    }
    return dropped;
}

} // namespace

SectionPaintResult paintSectionViewport(SectionCanvas& canvas, const Viewport& viewport,
                                        const SectionCuts& cuts, const katana::entity::Model* model)
{
    SectionPaintResult result;
    const plotting::ViewportSource& from = viewport.source;
    if (from.alignment.empty()) {
        result.problems.emplace_back("no alignment chosen");
        return result;
    }
    SectionAxes axes;
    axes.scale = viewport.scale > 0.0 ? viewport.scale : 500.0;
    axes.exaggeration = viewport.verticalExaggeration > 0.0 ? viewport.verticalExaggeration : 1.0;
    axes.centre = viewport.centre;
    axes.autoCentre = viewport.autoCentre;
    axes.hidden = &viewport.hiddenLayers;
    axes.model = model;
    const Box2 area = plotting::sectionDrawingArea(viewport.rect);

    if (viewport.kind == ViewportKind::LongSection) {
        std::string failure;
        double start = 0.0;
        const katana::cad::Section* section = cuts.longSection(failure, start);
        if (section == nullptr) {
            result.problems.push_back(failure);
            return result;
        }
        axes.shift = start;
        // The chainages asked for, else the whole alignment: exactly those
        // are shown, and their ends labelled.
        axes.range = from.chainageTo > from.chainageFrom
                         ? std::pair{from.chainageFrom, from.chainageTo}
                         : std::pair{start, start + section->length};
        axes.dataBand = true;
        result.notesDropped += paintSection(canvas, area, *section, axes);
        result.drawn = true;
        return result;
    }

    // Cross sections: the chainages asked for, one row each.
    const std::vector<double> stations = plotting::viewportStations(from);
    if (stations.empty()) {
        result.problems.emplace_back("no chainage to cut a cross section at");
        return result;
    }
    const double halfWidth = plotting::viewportHalfWidth(from);
    axes.cross = true;
    const double rowHeight = area.height() / static_cast<double>(stations.size());
    for (std::size_t i = 0; i < stations.size(); ++i) {
        std::string failure;
        const katana::cad::Section* section = cuts.crossSection(stations[i], halfWidth, failure);
        if (section == nullptr) {
            result.problems.push_back(failure);
            continue;
        }
        const double top = area.max.y - static_cast<double>(i) * rowHeight;
        const Box2 row(Point2(area.min.x, top - rowHeight), Point2(area.max.x, top));
        SectionAxes rowAxes = axes;
        rowAxes.shift = -halfWidth;
        rowAxes.caption = QString("CH %1").arg(stations[i], 0, 'f', 3);
        rowAxes.profileLevel = cuts.profileLevel ? cuts.profileLevel(stations[i]) : std::nullopt;
        result.notesDropped += paintSection(canvas, row, *section, rowAxes);
        result.drawn = true;
    }
    return result;
}

plotting::SectionFit fitSectionViewport(SectionCanvas& canvas, const Viewport& viewport,
                                        const SectionCuts& cuts, const katana::entity::Model* model)
{
    plotting::SectionFit fit{viewport.scale,
                             viewport.verticalExaggeration > 0.0 ? viewport.verticalExaggeration : 1.0};
    const plotting::ViewportSource& from = viewport.source;
    if (!viewport.autoScale || viewport.rect.empty() || from.alignment.empty()) {
        return fit;
    }
    const Box2 area = plotting::sectionDrawingArea(viewport.rect);
    const std::optional<double> centreAlong =
        viewport.autoCentre ? std::nullopt : std::optional<double>(viewport.centre.x);
    const std::optional<double> centreLevel =
        viewport.autoCentre ? std::nullopt : std::optional<double>(viewport.centre.y);
    // The level column, as wide as the widest level written to a tenth: near
    // enough the painter's own for a fit that leaves a tenth of the plot spare.
    const SectionTextStyle levelStyle = levelLabelStyle();
    const auto levelColumn = [&](const std::pair<double, double>& levels) {
        return std::max(canvas.textWidthMm(number(levels.first, 0.1), levelStyle),
                        canvas.textWidthMm(number(levels.second, 0.1), levelStyle));
    };

    plotting::SectionFitRequest request;
    plotting::SectionLayoutRequest layout;
    if (viewport.kind == ViewportKind::LongSection) {
        std::string failure;
        double start = 0.0;
        const katana::cad::Section* section = cuts.longSection(failure, start);
        if (section == nullptr) {
            return fit;
        }
        // The range asked for, else the whole alignment.
        const bool ranged = from.chainageTo > from.chainageFrom;
        const double first = ranged ? from.chainageFrom : start;
        const double last = ranged ? from.chainageTo : start + section->length;
        const auto levels = plotting::levelRange(*section, first - start, last - start, model,
                                                 &viewport.hiddenLayers);
        layout = {area, bandRows(*section).size(), false, levels ? levelColumn(*levels) : 0.0};
        request.spanM = plotting::sectionSpan(first, last, centreAlong);
        request.depthM =
            levels ? plotting::sectionSpan(levels->first, levels->second, centreLevel) : 0.0;
        request.flatExaggeration = kLongSectionExaggeration;
    } else {
        // Every row at one scale: each as wide as the viewport, the deepest
        // setting the exaggeration.
        const std::vector<double> stations = plotting::viewportStations(from);
        if (stations.empty()) {
            return fit;
        }
        const double halfWidth = plotting::viewportHalfWidth(from);
        double column = 0.0;
        for (const double chainage : stations) {
            std::string failure;
            const katana::cad::Section* section = cuts.crossSection(chainage, halfWidth, failure);
            if (section == nullptr) {
                continue;
            }
            const auto levels =
                widened(plotting::levelRange(*section, -kInfinity, kInfinity, model,
                                             &viewport.hiddenLayers),
                        cuts.profileLevel ? cuts.profileLevel(chainage) : std::nullopt);
            if (levels) {
                request.depthM = std::max(
                    request.depthM, plotting::sectionSpan(levels->first, levels->second, centreLevel));
                column = std::max(column, levelColumn(*levels));
            }
        }
        const double rowHeight = area.height() / static_cast<double>(stations.size());
        layout = {Box2(area.min, Point2(area.max.x, area.min.y + rowHeight)), 0, true, column};
        request.spanM = plotting::sectionSpan(-halfWidth, halfWidth, centreAlong);
    }
    const plotting::SectionLayout plot = plotting::sectionPlotLayout(layout);
    request.plotWidthMm = plot.plot.width();
    request.plotHeightMm = plot.plot.height();
    if (auto fitted = plotting::fitSection(request)) {
        return *fitted;
    }
    return fit;
}

} // namespace katana::qt
