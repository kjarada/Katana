// The window's IFC import and export, through the native module (katana_ifc,
// docs/ifc.md). Its own file, so that main_window.cpp - where every feature
// meets - carries one-line hooks. What a typed line means, which files an
// export names and the GlobalIds' namespace are ifc/front_end.hpp's, shared
// with katana_cli, so the two front ends write the same file from the same
// line; what only the window has - its surfaces, its selection, a person to
// ask - is added here.

#include "main_window.hpp"

#include "format.hpp"

#include <chrono>
#include <fstream>

#include <QApplication>
#include <QMessageBox>
#include <QPushButton>

#include "katana/cad/project_crs.hpp"
#include "katana/ifc/export.hpp"
#include "katana/ifc/front_end.hpp"
#include "katana/ifc/import.hpp"
#include "project_crs_dialog.hpp"

namespace katana::qt {

namespace interop = katana::interop;

namespace {

QString nameOf(const std::filesystem::path& path)
{
    return QString::fromStdWString(path.filename().wstring());
}

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

// "IfcKerb 1; IfcPipeSegment 6;" - the classes line both front ends print.
QString classesLine(const std::map<std::string, std::size_t>& classes)
{
    QString line;
    for (const auto& [name, count] : classes) {
        line += ' ' + qs(name) + ' ' + grouped(count) + ';';
    }
    return line;
}

} // namespace

// ---- import -------------------------------------------------------------------------

bool MainWindow::importIfcFile(const IfcImportRequest& request)
{
    const std::filesystem::path path = katana::ifc::pathFromUtf8(request.arguments.path);
    katana::ifc::ImportOptions options;
    options.importAlignments = request.alignments;
    options.importElements = request.elements;
    options.importSurfaces = request.surfaces;
    options.curveTolerance = request.curveTolerance;
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto imported = katana::ifc::readIfcFile(path, options);
    QApplication::restoreOverrideCursor();
    if (!imported.ok()) {
        logMessage(qs(imported.error().describe()), true);
        warnUser("Import failed", qs(imported.error().describe()));
        return false;
    }

    // LOCAL, as katana_cli has it: read again shifted by the extent's corner,
    // so that the one reader moves every kind of geometry alike. Otherwise
    // the question every import asks, as the DXF import asks it: survey
    // coordinates merged into a drawing near the origin make one of the two
    // a dot. The extent is everything the file brings, alignments and
    // surfaces too (IfcImport::bounds).
    bool shifted = false;
    if (request.arguments.local && !imported->bounds.empty()) {
        options.originShift = imported->bounds.min;
        shifted = true;
    } else if (!request.arguments.local) {
        const auto advice =
            interop::advisePlacement(document_.model().entities.bounds(), imported->bounds);
        if (advice.farApart && headless_) {
            logMessage(qs(advice.message) + " (kept: no one to ask).", true);
        } else if (advice.farApart) {
            QMessageBox box(this);
            box.setIcon(QMessageBox::Question);
            box.setWindowTitle("Far from the current drawing");
            box.setText(qs(advice.message) + ".");
            box.setInformativeText(
                "Shifting moves everything in the file as one piece so it sits beside the "
                "drawing; its shape and internal dimensions are unchanged.");
            QPushButton* shift = box.addButton("Shift Alongside", QMessageBox::AcceptRole);
            box.addButton("Keep Survey Coordinates", QMessageBox::DestructiveRole);
            QPushButton* cancel = box.addButton(QMessageBox::Cancel);
            box.setDefaultButton(shift);
            box.exec();
            if (box.clickedButton() == cancel) {
                logMessage("Import cancelled.");
                return false;
            }
            if (box.clickedButton() == shift) {
                options.originShift = advice.suggestedShift;
                shifted = true;
            } else {
                logMessage(qs(advice.message) + ".", true);
            }
        }
    }
    if (shifted) {
        QApplication::setOverrideCursor(Qt::WaitCursor);
        auto moved = katana::ifc::readIfcFile(path, options);
        QApplication::restoreOverrideCursor();
        if (!moved.ok()) {
            logMessage(qs(moved.error().describe()), true);
            return false;
        }
        imported = std::move(moved);
        logMessage(QString("Shifted the imported data by %1,%2 %3.")
                       .arg(options.originShift->x, 0, 'f', 3)
                       .arg(options.originShift->y, 0, 'f', 3)
                       .arg(request.arguments.local ? "to sit at the origin"
                                                    : "to sit beside the drawing"));
    }

    // Layers, alignments and entities: ONE transaction, one Ctrl+Z. The
    // counts first - the command takes the entities, and adds its renames to
    // the warnings.
    const std::size_t entities = imported->entities.size();
    const std::size_t alignments = imported->alignments.size();
    const std::size_t surfaces = imported->surfaces.size();
    if (auto command = katana::ifc::importCommand(*imported, document_.model())) {
        if (const auto status = document_.execute(std::move(command)); !status) {
            logMessage(qs(status.error().describe()), true);
            warnUser("Import failed", qs(status.error().describe()));
            return false;
        }
    }
    logMessage("Imported " + grouped(entities) + " entities, " + grouped(alignments) +
               " alignments and " + grouped(surfaces) + " surfaces from " + nameOf(path) + " (" +
               qs(imported->schema) + ")");
    logMessage("  " + grouped(imported->products) +
               " objects read: " + grouped(imported->productsImported) + " drawn, " +
               grouped(imported->productsAsPoints) + " as a point at their placement; " +
               grouped(imported->alignmentsAsPolylines) + " alignments as polylines");
    if (!imported->classes.empty()) {
        logMessage("  classes:" + classesLine(imported->classes));
    }

    // The file's coordinate system. Taken only when the project has none and
    // the data is where the file put it - shifted, it is in no system at all.
    // Its own undoable step (Document::setCoordinateSystem), after the import.
    const std::string& project = document_.metadata().coordinateSystem;
    const std::string& file = imported->coordinateSystem;
    if (!file.empty() && project.empty() && !shifted) {
        bool take = request.takeCoordinateSystem.value_or(false);
        if (!request.takeCoordinateSystem && !headless_) {
            QString named = qs(file);
            if (const auto described = katana::cad::describeCoordinateSystem(file)) {
                named += " (" + qs(described->name) + ")";
            }
            take = QMessageBox::question(
                       this, "Project Coordinate System",
                       "The file is in " + named +
                           ", and the project has no coordinate system.\n\nSet the project's to "
                           "the file's? Nothing is moved; the drawing is then known to be in it.",
                       QMessageBox::Yes | QMessageBox::No, QMessageBox::Yes) == QMessageBox::Yes;
        }
        if (take) {
            if (const auto status = document_.setCoordinateSystem(file, "IMPORT_CRS"); !status) {
                logMessage("  the file's coordinate system " + qs(file) +
                               " could not be set: " + qs(status.error().describe()),
                           true);
            } else {
                logMessage("  the project is now in " + qs(file) +
                           ", the file's coordinate system");
            }
        } else {
            logMessage("  the file is in " + qs(file) +
                       " and the project has no coordinate system: File > Project Coordinate "
                       "System or CRS SET " +
                       qs(file) + " sets it");
        }
    } else if (!file.empty() && !project.empty() && project != file) {
        logMessage("  the file is in " + qs(file) + " and the project in " + qs(project) +
                   "; nothing was reprojected");
    }
    for (const std::string& warning : imported->warnings) {
        logMessage("  " + qs(warning));
    }
    const auto& bounds = imported->bounds;
    if (!bounds.empty()) {
        logMessage(QString("  extent %1,%2 to %3,%4")
                       .arg(bounds.min.x, 0, 'f', 2)
                       .arg(bounds.min.y, 0, 'f', 2)
                       .arg(bounds.max.x, 0, 'f', 2)
                       .arg(bounds.max.y, 0, 'f', 2));
    }

    // Surfaces are session data, outside undo (interop/reference_data.hpp),
    // added after the transaction so that a refused import leaves none.
    for (auto& surface : imported->surfaces) {
        addSurface(surface.name, std::move(surface.surface));
    }
    views_->refreshAll();
    views_->zoomExtentsAll();
    return true;
}

katana::core::Result<QString> MainWindow::describeIfcFile(const std::filesystem::path& path)
{
    QApplication::setOverrideCursor(Qt::WaitCursor);
    const auto read = katana::ifc::readIfcFile(path);
    QApplication::restoreOverrideCursor();
    if (!read.ok()) {
        return read.error();
    }
    QString text = nameOf(path) + ": " + qs(read->schema) + ", " +
                   (read->coordinateSystem.empty() ? QString("no coordinate system")
                                                   : qs(read->coordinateSystem)) +
                   "\n";
    text += "  " + grouped(read->products) + " objects: " + grouped(read->productsImported) +
            " drawn, " + grouped(read->productsAsPoints) + " as a point at their placement\n";
    if (!read->classes.empty()) {
        text += "  classes:" + classesLine(read->classes) + "\n";
    }
    text += "  " + grouped(read->alignments.size()) + " alignments with a PI definition";
    for (const auto& alignment : read->alignments) {
        text += (&alignment == &read->alignments.front() ? ": " : ", ") + qs(alignment.name) +
                " (" + grouped(alignment.horizontal.pis.size()) + " PIs" +
                (alignment.vertical ? ", " + grouped(alignment.vertical->pvis.size()) + " PVIs"
                                    : QString()) +
                ")";
    }
    text += "; " + grouped(read->alignmentsAsPolylines) + " as polylines\n";
    text += "  " + grouped(read->surfaces.size()) + " surfaces";
    for (const auto& surface : read->surfaces) {
        text += (&surface == &read->surfaces.front() ? ": " : ", ") + qs(surface.name) + " (" +
                grouped(surface.surface.triangleCount()) + " triangles)";
    }
    text += "\n  " + grouped(read->layers.size()) + " layers, " + grouped(read->entities.size()) +
            " entities\n";
    if (!read->bounds.empty()) {
        text += QString("  extent %1,%2 to %3,%4\n")
                    .arg(read->bounds.min.x, 0, 'f', 2)
                    .arg(read->bounds.min.y, 0, 'f', 2)
                    .arg(read->bounds.max.x, 0, 'f', 2)
                    .arg(read->bounds.max.y, 0, 'f', 2);
    }
    for (const std::string& warning : read->warnings) {
        text += "  " + qs(warning) + "\n";
    }
    return text.trimmed();
}

// ---- export -------------------------------------------------------------------------

katana::core::Result<katana::ifc::IfcExport>
MainWindow::exportIfcFile(const IfcExportRequest& request, bool write)
{
    // The files the export names, read before anything is written.
    auto files = katana::ifc::readExportFiles(request.arguments);
    if (!files) {
        if (write) {
            logMessage(qs(files.error().describe()), true);
            warnUser("Export failed", qs(files.error().describe()));
        }
        return files.error();
    }
    katana::ifc::ExportInput input;
    const bool drawing = request.arguments.drawing && (request.entities || request.alignments);
    if (drawing) {
        input.model = &document_.model();
    }
    if (request.arguments.drawing && request.surfaces) {
        for (const cad::SceneSurface& item : sceneSurfaces_) {
            input.surfaces.push_back({item.name, item.surface});
        }
    }
    input.utilities = std::move(files->utilities);

    // The options the CLI sets, from the same metadata by the same
    // functions, so that an object's GlobalId is the same whichever front
    // end wrote it.
    const auto& metadata = document_.metadata();
    katana::ifc::ExportOptions options;
    options.projectName = metadata.name;
    options.projectDescription = metadata.description;
    options.applicationVersion = KATANA_VERSION;
    options.timestamp = katana::ifc::headerTimestamp(std::chrono::system_clock::now());
    options.guidNamespace = katana::ifc::guidNamespaceFor(metadata.name, metadata.createdUtc);
    options.rules = std::move(files->rules);
    options.georeference.name = metadata.coordinateSystem;
    if (!metadata.coordinateSystem.empty()) {
        if (const auto described =
                katana::cad::describeCoordinateSystem(metadata.coordinateSystem)) {
            options.georeference.description = described->name;
        }
    }
    options.exportEntities = drawing && request.entities;
    options.exportAlignments = drawing && request.alignments;
    if (options.exportEntities && request.selectedOnly) {
        options.entities = document_.selection().ids();
    }

    QApplication::setOverrideCursor(Qt::WaitCursor);
    const std::filesystem::path path = katana::ifc::pathFromUtf8(request.arguments.path);
    auto written = write ? katana::ifc::writeIfcFile(input, path, options)
                         : katana::ifc::writeIfc(input, options);
    QApplication::restoreOverrideCursor();
    if (!written) {
        if (write) {
            logMessage(qs(written.error().describe()), true);
            warnUser("Export failed", qs(written.error().describe()));
        }
        return written.error();
    }
    if (!write) {
        written->text.clear();
        return written;
    }
    logMessage("Exported " + grouped(written->entitiesWritten) + " entities, " +
               grouped(written->alignments) + " alignments, " + grouped(written->services) +
               " services (" + grouped(written->serviceSegments) + " segments, " +
               grouped(written->segmentsIn3d) + " in 3D, " + grouped(written->locatedPoints) +
               " located points) and " + grouped(written->surfaces) + " surfaces to " +
               nameOf(path) + " (IFC4X3_ADD2, " + grouped(written->instances) + " instances)");
    logMessage("  classes:" + classesLine(written->classes));
    for (const std::string& warning : written->warnings) {
        logMessage("  " + qs(warning));
    }
    return written;
}

// ---- the command line and the dialogs ----------------------------------------------

bool MainWindow::runIfcLine(const QString& verb, const QString& rest)
{
    const std::string argument = rest.toStdString();
    if (verb == "EXPORT") {
        const auto parsed = katana::ifc::parseExportArguments(argument);
        if (!parsed) {
            return false; // not an .ifc: the other exporters' line
        }
        if (!*parsed) {
            logMessage(qs(parsed->error().describe()), true);
            return true;
        }
        IfcExportRequest request;
        request.arguments = **parsed;
        request.entities = request.alignments = request.surfaces = request.arguments.drawing;
        (void)exportIfcFile(request, true);
        return true;
    }
    if (verb == "IMPORT") {
        const auto parsed = katana::ifc::parseImportArguments(argument);
        if (!parsed) {
            return false;
        }
        if (!*parsed) {
            logMessage(qs(parsed->error().describe()), true);
            return true;
        }
        IfcImportRequest request;
        request.arguments = **parsed;
        (void)importIfcFile(request);
        return true;
    }
    return false;
}

IfcExportContext MainWindow::ifcExportContext()
{
    IfcExportContext context;
    context.state = [this] {
        IfcExportState state;
        state.entities = document_.model().entities.size();
        state.selected = document_.selection().size();
        state.alignments = document_.model().alignments.size();
        state.surfaces = sceneSurfaces_.size();
        const std::string& code = document_.metadata().coordinateSystem;
        state.coordinateSystem = projectCrsLabel(document_);
        if (code.empty()) {
            state.coordinateSystem.clear();
        }
        state.georeferenced = code.starts_with("EPSG:");
        return state;
    };
    context.preview = [this](const IfcExportRequest& request) {
        return exportIfcFile(request, false);
    };
    context.run = [this](const IfcExportRequest& request) { return exportIfcFile(request, true); };
    context.saveDefaultRules = [this](const QString& path) -> katana::core::Status {
        std::ofstream file(katana::ifc::pathFromUtf8(path.toStdString()),
                           std::ios::binary | std::ios::trunc);
        file << katana::ifc::formatClassificationRules(katana::ifc::defaultClassificationRules());
        file.close();
        if (!file) {
            return katana::core::makeError(katana::core::ErrorCode::FileExportFailure,
                                           "the rules could not be written", path.toStdString());
        }
        logMessage("Wrote the default IFC classification rules to " + path);
        return {};
    };
    context.headless = [this] { return headless_; };
    return context;
}

IfcImportContext MainWindow::ifcImportContext()
{
    IfcImportContext context;
    context.describe = [this](const QString& path) {
        return describeIfcFile(katana::ifc::pathFromUtf8(path.toStdString()));
    };
    context.run = [this](const IfcImportRequest& request) { return importIfcFile(request); };
    context.headless = [this] { return headless_; };
    return context;
}

void MainWindow::showIfcExport(const QString& file)
{
    if (ifcExport_.isNull()) {
        ifcExport_ = new IfcExportDialog(ifcExportContext(), this);
    }
    if (!file.isEmpty()) {
        ifcExport_->setFile(file);
    }
    ifcExport_->show();
    ifcExport_->raise();
    ifcExport_->activateWindow();
}

void MainWindow::showIfcImport(const QString& file)
{
    if (ifcImport_.isNull()) {
        ifcImport_ = new IfcImportDialog(ifcImportContext(), this);
    }
    if (!file.isEmpty()) {
        ifcImport_->setFile(file);
    }
    ifcImport_->show();
    ifcImport_->raise();
    ifcImport_->activateWindow();
}

} // namespace katana::qt
