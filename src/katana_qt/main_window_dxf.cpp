// The window's DXF export with the Export Vector dialog's choices, through the
// native module (katana_dxf, docs/dxf.md) rather than GDAL. Its own file, so
// that main_window.cpp - where every feature meets - carries one-line hooks.
// A typed EXPORT or IMPORT of a .dxf is the executor's (dxf_verbs.hpp in
// katana_app), as every front end's is.

#include "main_window.hpp"

#include "format.hpp"

#include <QApplication>

#include "katana/cad/annotation/export_annotation.hpp"
#include "katana/dxf/writer.hpp"

namespace katana::qt {

namespace interop = katana::interop;

namespace {

QString nameOf(const std::filesystem::path& path)
{
    return QString::fromStdWString(path.filename().wstring());
}

} // namespace

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
