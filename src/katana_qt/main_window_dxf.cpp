// The window's DXF import and export, through the native module
// (katana_dxf, docs/dxf.md) rather than GDAL. Its own file, so that
// main_window.cpp - where every feature meets - carries one-line hooks.

#include "main_window.hpp"

#include "format.hpp"

#include <QApplication>
#include <QMessageBox>
#include <QPushButton>

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

void MainWindow::importDxfFile(const std::filesystem::path& path, bool local)
{
    katana::dxf::ImportOptions options;
    options.sourceName = path.filename().string();
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto imported = katana::dxf::readDxfFile(path, options);
    // LOCAL: read again with the shift, so the one reader moves every kind of
    // geometry alike, as katana_cli's does.
    if (imported.ok() && local && !imported->bounds.empty()) {
        options.originShift = katana::geometry::Vec2(imported->bounds.min.x, imported->bounds.min.y);
        imported = katana::dxf::readDxfFile(path, options);
    }
    QApplication::restoreOverrideCursor();
    if (!imported.ok()) {
        logMessage(QString::fromStdString(imported.error().describe()), true);
        warnUser("Import failed", QString::fromStdString(imported.error().describe()));
        return;
    }
    if (local && options.originShift) {
        logLocalShift(*options.originShift);
    }

    // The question every import asks: a DXF of survey data sits at survey
    // coordinates, and merged into a drawing near the origin one of the two
    // becomes a dot. With LOCAL the place is chosen already.
    const auto advice =
        interop::advisePlacement(document_.model().entities.bounds(), imported->bounds);
    const bool ask = advice.farApart && !local;
    if (ask && headless_) {
        logMessage(QString::fromStdString(advice.message) + " (kept: no one to ask).", true);
    } else if (ask) {
        QMessageBox box(this);
        box.setIcon(QMessageBox::Question);
        box.setWindowTitle("Far from the current drawing");
        box.setText(QString::fromStdString(advice.message) + ".");
        box.setInformativeText(
            "Shifting moves everything in the file as one piece so it sits beside the drawing; "
            "its shape and internal dimensions are unchanged.");
        QPushButton* shift = box.addButton("Shift Alongside", QMessageBox::AcceptRole);
        box.addButton("Keep Survey Coordinates", QMessageBox::DestructiveRole);
        QPushButton* cancel = box.addButton(QMessageBox::Cancel);
        box.setDefaultButton(shift);
        box.exec();
        if (box.clickedButton() == cancel) {
            logMessage("Import cancelled.");
            return;
        }
        if (box.clickedButton() == shift) {
            options.originShift = advice.suggestedShift;
            QApplication::setOverrideCursor(Qt::WaitCursor);
            auto shifted = katana::dxf::readDxfFile(path, options);
            QApplication::restoreOverrideCursor();
            if (!shifted.ok()) {
                logMessage(QString::fromStdString(shifted.error().describe()), true);
                return;
            }
            imported = std::move(shifted);
            logMessage(QString("Shifted the imported data by %1,%2 to sit beside the drawing.")
                           .arg(advice.suggestedShift.x, 0, 'f', 3)
                           .arg(advice.suggestedShift.y, 0, 'f', 3));
        } else {
            logMessage(QString::fromStdString(advice.message) + ".", true);
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
