// The window's DXF import and export, through the native module
// (katana_dxf, docs/dxf.md) rather than GDAL. Its own file, so that
// main_window.cpp - where every feature meets - carries one-line hooks.

#include "main_window.hpp"

#include "format.hpp"
#include "import_placement.hpp"

#include <QApplication>

#include "katana/cad/annotation/export_annotation.hpp"
#include "katana/dxf/import_command.hpp"
#include "katana/dxf/reader.hpp"
#include "katana/dxf/writer.hpp"

namespace katana::qt {

namespace interop = katana::interop;

namespace {

QString nameOf(const std::filesystem::path& path)
{
    return QString::fromStdWString(path.filename().wstring());
}

} // namespace

void MainWindow::importDxfFile(const std::filesystem::path& path,
                               const katana::cad::ImportPlacement& placement)
{
    katana::dxf::ImportOptions options;
    options.sourceName = path.filename().string();
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto imported = katana::dxf::readDxfFile(path, options);
    QApplication::restoreOverrideCursor();
    if (!imported.ok()) {
        logMessage(QString::fromStdString(imported.error().describe()), true);
        warnUser("Import failed", QString::fromStdString(imported.error().describe()));
        return;
    }

    // Where it lands, as every import decides it: a DXF of survey data sits
    // at survey coordinates, and merged into a drawing near the origin one of
    // the two becomes a dot. A move is read again with the shift, so the one
    // reader moves every kind of geometry alike, as katana_cli's does.
    const PlacementDecision placed = placeImport(placement, imported->bounds);
    if (placed.cancelled) {
        logMessage("Import cancelled.");
        return;
    }
    if (placed.shift) {
        options.originShift = placed.shift;
        QApplication::setOverrideCursor(Qt::WaitCursor);
        imported = katana::dxf::readDxfFile(path, options);
        QApplication::restoreOverrideCursor();
        if (!imported.ok()) {
            logMessage(QString::fromStdString(imported.error().describe()), true);
            return;
        }
    }

    // Linetypes, layers and entities: ONE transaction, one Ctrl+Z.
    const std::size_t created = imported->entities.size();
    if (auto command = katana::dxf::importCommand(*imported, document_.model())) {
        if (const auto status = document_.execute(std::move(command)); !status) {
            logMessage(QString::fromStdString(status.error().describe()), true);
            warnUser("Import failed", QString::fromStdString(status.error().describe()));
            return;
        }
    }

    QString summary = "Imported " + grouped(created) + " entities from " + nameOf(path) + " (DXF";
    if (!imported->release.empty()) {
        summary += " " + QString::fromStdString(imported->release);
    }
    logMessage(summary + ")");
    for (const auto& tally : imported->tally) {
        logMessage(QString("  %1: %2 read, %3 imported")
                       .arg(QString::fromStdString(tally.kind))
                       .arg(grouped(tally.read))
                       .arg(grouped(tally.imported)),
                   tally.imported == 0 && tally.read != 0);
    }
    for (const std::string& warning : imported->warnings) {
        logMessage("  " + QString::fromStdString(warning));
    }
    const auto& bounds = imported->bounds;
    if (!bounds.empty()) {
        logMessage(QString("  extent %1,%2 to %3,%4")
                       .arg(bounds.min.x, 0, 'f', 2)
                       .arg(bounds.min.y, 0, 'f', 2)
                       .arg(bounds.max.x, 0, 'f', 2)
                       .arg(bounds.max.y, 0, 'f', 2));
    }
    views_->refreshAll();
    views_->zoomExtentsAll();
}

bool MainWindow::exportDxfFile(const std::filesystem::path& path,
                               const interop::VectorExportOptions& options)
{
    katana::dxf::ExportOptions dxfOptions;
    dxfOptions.entities = options.entities;
    dxfOptions.layers = options.layers;
    dxfOptions.originShift = options.originShift;
    // Annotation at the drawing's annotation scale, labels and every
    // dimension kind drawn out as the plan view draws them
    // (cad/annotation/export_annotation.hpp); katana_cli's EXPORT does the
    // same, so both write the same file.
    dxfOptions.annotationScale = document_.annotationScale();
    QApplication::setOverrideCursor(Qt::WaitCursor);
    const auto drawn = katana::cad::annotation::drawAnnotationForExport(document_.model(),
                                                                        dxfOptions.annotationScale);
    dxfOptions.drawn = &drawn;
    const auto written = katana::dxf::writeDxfFile(document_.model(), path, dxfOptions);
    QApplication::restoreOverrideCursor();
    if (!written.ok()) {
        logMessage(QString::fromStdString(written.error().describe()), true);
        warnUser("Export failed", QString::fromStdString(written.error().describe()));
        return false;
    }
    logMessage("Exported " + grouped(written->entitiesWritten) + " entities and " +
               grouped(written->layersWritten) + " layers to " + nameOf(path) + " (DXF R2000)");
    for (const std::string& warning : written->warnings) {
        logMessage("  " + QString::fromStdString(warning));
    }
    return true;
}

} // namespace katana::qt
