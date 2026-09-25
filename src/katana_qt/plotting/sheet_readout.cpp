#include "plotting/sheet_readout.hpp"

#include <cmath>

#include "katana/cad/plot.hpp"
#include "katana/cad/plotting/viewport_edits.hpp"

namespace katana::qt {

namespace plotting = katana::cad::plotting;
using katana::geometry::Point2;
using plotting::ViewportKind;

namespace {

QString kindLabel(ViewportKind kind)
{
    switch (kind) {
    case ViewportKind::Plan: return QStringLiteral("Plan");
    case ViewportKind::LongSection: return QStringLiteral("Long section");
    case ViewportKind::CrossSections: return QStringLiteral("Cross sections");
    case ViewportKind::Model3D: return QStringLiteral("3D snapshot");
    case ViewportKind::Legend: return QStringLiteral("Legend");
    case ViewportKind::Notes: return QStringLiteral("Notes");
    case ViewportKind::Image: return QStringLiteral("Image");
    case ViewportKind::KeyPlan: return QStringLiteral("Key plan");
    }
    return {};
}

QString ratio(double scale)
{
    return QString("1:%1").arg(QString::number(scale, 'f', scale == std::floor(scale) ? 0 : 2));
}

} // namespace

SheetCursorReadout sheetCursorReadout(const plotting::Sheet& sheet, const Point2& paper,
                                      const PlanResolver& resolve)
{
    SheetCursorReadout readout;
    readout.onSheet = true;
    readout.paper = paper;
    const auto size = katana::cad::paperDimensions(sheet.paper, sheet.landscape);
    readout.onPaper = paper.x >= 0.0 && paper.y >= 0.0 && paper.x <= size.widthMm &&
                      paper.y <= size.heightMm;
    // Front to back, as a click picks.
    for (auto it = sheet.viewports.rbegin(); it != sheet.viewports.rend(); ++it) {
        const plotting::Viewport& viewport = *it;
        if (viewport.rect.empty() || !viewport.rect.contains(paper)) {
            continue;
        }
        readout.viewportId = viewport.id;
        readout.kind = viewport.kind;
        if (viewport.kind == ViewportKind::Plan || viewport.kind == ViewportKind::KeyPlan) {
            const ResolvedViewport at =
                resolve ? resolve(viewport) : ResolvedViewport{viewport.scale, viewport.centre};
            if (at.scale > 0.0 && std::isfinite(at.scale)) {
                readout.scale = ratio(at.scale).toStdString();
                readout.world = plotting::planPaperToWorld(viewport, at.scale, at.centre, paper);
            }
        } else if (viewport.kind == ViewportKind::LongSection ||
                   viewport.kind == ViewportKind::CrossSections) {
            readout.scale = plotting::scaleText(viewport);
        }
        break;
    }
    return readout;
}

QString readoutText(const SheetCursorReadout& readout)
{
    if (!readout.onSheet) {
        return {};
    }
    QString text = QString("X %1  Y %2 mm")
                       .arg(readout.paper.x, 0, 'f', 1)
                       .arg(readout.paper.y, 0, 'f', 1);
    if (!readout.viewportId.empty()) {
        text += QString("   %1 %2").arg(QString::fromStdString(readout.viewportId),
                                        kindLabel(readout.kind));
        if (!readout.scale.empty()) {
            text += ' ' + QString::fromStdString(readout.scale);
        }
    }
    if (readout.world) {
        text += QString("   E %1  N %2")
                    .arg(readout.world->x, 0, 'f', 3)
                    .arg(readout.world->y, 0, 'f', 3);
    }
    return text;
}

} // namespace katana::qt
