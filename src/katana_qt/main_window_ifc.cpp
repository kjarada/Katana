// The window's IFC import and export, through the native module (katana_ifc,
// docs/ifc.md). Its own file, so that main_window.cpp - where every feature
// meets - carries one-line hooks.
//
// ONE DOOR: every IFC action in the window is a line - IMPORT, EXPORT, INFO
// or IFC RULES - with ifc/front_end.hpp's grammar, which katana_cli reads
// too. A typed line comes here from runCommandLine; File > Import IFC and
// Export IFC write their line (formatImportLine, formatExportLine) and hand
// it to runIfcCommand, which echoes it as typed and runs it here; File >
// Import and a path given to the window do the same. So a dialog cannot do
// what a line cannot, an agent can do all a dialog can, and the reply - the
// key=value records katana_cli prints - is the same whoever asked. What only
// the window has - its surfaces, its selection, a person to ask - is added
// here.

#include "main_window.hpp"

#include "format.hpp"

#include <chrono>
#include <cstdio>
#include <format>

#include <QApplication>
#include <QMessageBox>
#include <QPlainTextEdit>
#include <QPushButton>

#include "katana/cad/project_crs.hpp"
#include "katana/ifc/export.hpp"
#include "katana/ifc/front_end.hpp"
#include "katana/ifc/import.hpp"
#include "project_crs_dialog.hpp"

namespace katana::qt {

namespace interop = katana::interop;

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

// The arguments of a line that names the file and nothing else: what INFO
// takes.
katana::ifc::ImportArguments fileAlone(const std::string& path)
{
    katana::ifc::ImportArguments bare;
    bare.path = path;
    return bare;
}

std::string fileNameOf(const std::string& utf8Path)
{
    const auto name = katana::ifc::pathFromUtf8(utf8Path).filename().u8string();
    return {name.begin(), name.end()};
}

} // namespace

// ---- import -------------------------------------------------------------------------

Result<std::string> MainWindow::importIfc(const katana::ifc::ImportArguments& arguments,
                                          IfcLineFrom from)
{
    const std::filesystem::path path = katana::ifc::pathFromUtf8(arguments.path);
    katana::ifc::ImportOptions options;
    options.importAlignments = arguments.alignments;
    options.importElements = arguments.elements;
    options.importSurfaces = arguments.surfaces;
    if (arguments.tolerance) {
        options.curveTolerance = *arguments.tolerance;
    }
    QApplication::setOverrideCursor(Qt::WaitCursor);
    auto imported = katana::ifc::readIfcFile(path, options);
    QApplication::restoreOverrideCursor();
    if (!imported.ok()) {
        return imported.error();
    }

    // LOCAL, as katana_cli has it: read again shifted by the extent's corner,
    // so that the one reader moves every kind of geometry alike. Otherwise
    // the question every import asks, as the DXF import asks it: survey
    // coordinates merged into a drawing near the origin make one of the two
    // a dot. The extent is everything the file brings, alignments and
    // surfaces too (IfcImport::bounds).
    std::vector<std::string> notes;
    bool shifted = false;
    if (arguments.local && !imported->bounds.empty()) {
        options.originShift = imported->bounds.min;
        shifted = true;
    } else if (!arguments.local) {
        // Weighed against everything the drawing has too - its alignments
        // and the session's surfaces are not entities, and a drawing of an
        // alignment alone is as far from a file as one of lines.
        katana::geometry::Box2 existing = document_.model().entities.bounds();
        document_.model().alignments.forEach([&](const katana::entity::Alignment& alignment) {
            for (const auto& pi : alignment.horizontal.pis) {
                existing.expand(pi.point);
            }
        });
        for (const cad::SceneSurface& item : sceneSurfaces_) {
            if (item.surface != nullptr && item.surface->vertexCount() > 0) {
                existing.expand(item.surface->bounds());
            }
        }
        const auto advice = interop::advisePlacement(existing, imported->bounds);
        if (advice.farApart && headless_) {
            notes.push_back(advice.message + " (kept: no one to ask)");
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
                // Declined, not failed: the dialog says it as a cancel.
                return makeError(ErrorCode::CommandRejected, "Import cancelled.");
            }
            if (box.clickedButton() == shift) {
                options.originShift = advice.suggestedShift;
                shifted = true;
            } else {
                notes.push_back(advice.message + " (kept)");
            }
        }
    }
    if (shifted) {
        QApplication::setOverrideCursor(Qt::WaitCursor);
        auto moved = katana::ifc::readIfcFile(path, options);
        QApplication::restoreOverrideCursor();
        if (!moved.ok()) {
            return moved.error();
        }
        imported = std::move(moved);
        notes.push_back(std::format(
            "shifted by {:.3f},{:.3f} {}", options.originShift->x, options.originShift->y,
            arguments.local ? "to sit at the origin" : "to sit beside the drawing"));
    }

    // Layers, alignments and entities: ONE transaction, one Ctrl+Z. The
    // count first - the command takes the entities - and the reply after it,
    // since it adds its renames to the warnings.
    const std::size_t entities = imported->entities.size();
    if (auto command = katana::ifc::importCommand(*imported, document_.model())) {
        if (const auto status = document_.execute(std::move(command)); !status) {
            return status.error();
        }
    }
    std::string reply =
        katana::ifc::formatImportReply(*imported, fileNameOf(arguments.path), entities);

    // The file's coordinate system. Taken only when the project has none and
    // the data is where the file put it - shifted, it is in no system at all.
    // Its own undoable step (Document::setCoordinateSystem), after the import.
    // TAKECRS and KEEPCRS answer; without either, File > Import asks the
    // person there, and a typed line - a script typed or pasted in - is
    // answered in the log as katana_cli answers it.
    const std::string& project = document_.metadata().coordinateSystem;
    const std::string& file = imported->coordinateSystem;
    if (!file.empty() && project.empty() && !shifted) {
        bool take = arguments.takeCoordinateSystem.value_or(false);
        if (!arguments.takeCoordinateSystem && from == IfcLineFrom::Menu && !headless_) {
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
                notes.push_back("the file's coordinate system " + file +
                                " could not be set: " + status.error().describe());
            } else {
                notes.push_back("the project is now in " + file + ", the file's coordinate system");
            }
        } else {
            notes.push_back("the file is in " + file +
                            " and the project has no coordinate system: File > Project "
                            "Coordinate System or CRS SET " +
                            file + " sets it");
        }
    } else if (!file.empty() && !project.empty() && project != file) {
        reply +=
            "\n" + katana::ifc::warningRecord("the file is in " + file + " and the project in " +
                                              project + "; nothing was reprojected");
    }
    for (const std::string& note : notes) {
        reply += "\n" + katana::ifc::noteRecord(note);
    }

    // Surfaces are session data, outside undo (interop/reference_data.hpp),
    // added after the transaction so that a refused import leaves none.
    for (auto& surface : imported->surfaces) {
        addSurface(surface.name, std::move(surface.surface));
    }
    views_->refreshAll();
    views_->zoomExtentsAll();
    return reply;
}

Result<QString> MainWindow::describeIfcFile(const std::filesystem::path& path)
{
    QApplication::setOverrideCursor(Qt::WaitCursor);
    const auto read = katana::ifc::readIfcFile(path);
    QApplication::restoreOverrideCursor();
    if (!read.ok()) {
        return read.error();
    }
    const auto name = path.filename().u8string();
    return qs(katana::ifc::formatDescription(*read, std::string(name.begin(), name.end())));
}

// ---- export -------------------------------------------------------------------------

Result<std::string> MainWindow::exportIfc(const katana::ifc::ExportArguments& arguments)
{
    // The files the export names, read before anything is written.
    auto files = katana::ifc::readExportFiles(arguments);
    if (!files) {
        return files.error();
    }
    katana::ifc::ExportInput input;
    if (arguments.entities || arguments.alignments) {
        input.model = &document_.model();
    }
    if (arguments.surfaces) {
        for (const cad::SceneSurface& item : sceneSurfaces_) {
            input.surfaces.push_back({item.name, item.surface});
        }
    }
    input.utilities = std::move(files->utilities);

    // The options katana_cli sets, from the same metadata by the same
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
    options.exportEntities = arguments.entities;
    options.exportAlignments = arguments.alignments;
    if (arguments.selected) {
        options.entities = document_.selection().ids();
        // An empty list is every entity (ExportOptions::entities): a
        // selection cleared since it was asked for must not become the
        // whole drawing. katana_cli refuses in the same words.
        if (options.entities.empty()) {
            return makeError(ErrorCode::InvalidState,
                             "SELECTED, and nothing is selected: select the entities to export, "
                             "or leave SELECTED out");
        }
    }

    QApplication::setOverrideCursor(Qt::WaitCursor);
    const std::filesystem::path path = katana::ifc::pathFromUtf8(arguments.path);
    auto written = arguments.preview ? katana::ifc::writeIfc(input, options)
                                     : katana::ifc::writeIfcFile(input, path, options);
    QApplication::restoreOverrideCursor();
    if (!written) {
        return written.error();
    }
    return katana::ifc::formatExportReply(*written, fileNameOf(arguments.path), arguments.preview);
}

// ---- the lines ---------------------------------------------------------------------

std::optional<Result<std::string>> MainWindow::runIfcLine(const QString& verb, const QString& rest,
                                                          IfcLineFrom from)
{
    const std::string argument = rest.toStdString();
    std::optional<Result<std::string>> reply;
    if (verb == "EXPORT") {
        const auto parsed = katana::ifc::parseExportArguments(argument);
        if (!parsed) {
            return std::nullopt; // not an .ifc: the other exporters' line
        }
        reply = *parsed ? exportIfc(**parsed) : Result<std::string>(parsed->error());
    } else if (verb == "IMPORT") {
        const auto parsed = katana::ifc::parseImportArguments(argument);
        if (!parsed) {
            return std::nullopt;
        }
        reply = *parsed ? importIfc(**parsed, from) : Result<std::string>(parsed->error());
    } else if (verb == "INFO") {
        const auto parsed = katana::ifc::parseImportArguments(argument);
        if (!parsed) {
            return std::nullopt; // not an .ifc: GIS's INFO
        }
        if (!*parsed) {
            reply = Result<std::string>(parsed->error());
        } else if (**parsed != fileAlone((*parsed)->path)) {
            reply = Result<std::string>(makeError(
                ErrorCode::InvalidArgument, "INFO <file.ifc> takes the file and nothing else"));
        } else {
            auto described = describeIfcFile(katana::ifc::pathFromUtf8((*parsed)->path));
            reply = described ? Result<std::string>(described->toStdString())
                              : Result<std::string>(described.error());
        }
    } else if (verb == "IFC") {
        const auto path = katana::ifc::parseRulesArguments(argument);
        if (!path) {
            return std::nullopt; // IFC <something else>: no verb of the window's
        }
        reply = *path ? katana::ifc::writeDefaultRules(**path) : Result<std::string>(path->error());
    } else {
        return std::nullopt;
    }
    if (*reply) {
        logMessage(qs(**reply));
    } else if (reply->error().code == ErrorCode::CommandRejected) {
        logMessage(qs(reply->error().message)); // a cancel, said as one
    } else {
        logMessage(qs(reply->error().describe()), true);
        if (from == IfcLineFrom::Menu) {
            warnUser(verb == "EXPORT" ? "Export failed" : "Import failed",
                     qs(reply->error().describe()));
        }
    }
    return reply;
}

Result<std::string> MainWindow::runIfcCommand(const QString& line, IfcLineFrom from)
{
    // Echoed as a typed line is, and run as runCommandLine runs a typed IFC
    // line - but never offered to a running tool first, since the line is
    // never an answer to one.
    commandLog_->appendPlainText("> " + line);
    if (headless_) {
        // What a script - or a ctest check - can see of which line a dialog
        // ran, as logMessage echoes what it reported.
        std::fprintf(stderr, "> %s\n", line.toUtf8().constData());
    }
    const QString trimmed = line.trimmed();
    const qsizetype space = trimmed.indexOf(' ');
    const QString verb = trimmed.left(space < 0 ? trimmed.size() : space).toUpper();
    const QString rest = space < 0 ? QString() : trimmed.mid(space);
    if (auto reply = runIfcLine(verb, rest, from)) {
        return std::move(*reply);
    }
    const auto refused =
        makeError(ErrorCode::InvalidArgument, "not an IFC line: " + line.toStdString());
    logMessage(qs(refused.describe()), true);
    return refused;
}

// ---- the dialogs -------------------------------------------------------------------

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
        // The writer's own test, so the dialog cannot promise what the file
        // will not do (a compound EPSG:7856+5711 is not georeferenced).
        state.georeferenced = katana::ifc::isEpsgCode(code);
        return state;
    };
    context.runLine = [this](const QString& line) {
        return runIfcCommand(line, IfcLineFrom::Dialog);
    };
    context.headless = [this] { return headless_; };
    return context;
}

IfcImportContext MainWindow::ifcImportContext()
{
    IfcImportContext context;
    context.runLine = [this](const QString& line) {
        return runIfcCommand(line, IfcLineFrom::Dialog);
    };
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
    // Already open, show() delivers no showEvent: the counts are read here.
    ifcExport_->refresh();
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
