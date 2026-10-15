#pragma once

// BIM > Road Template Manager... (docs/road_design.md, "In the window"): the
// drawing's road templates - their components out from the centreline, each
// component's pavement courses - and a drawing of the template across a level
// road.
//
// NOTHING IS CHANGED HERE. Every button builds the ROADTEMPLATE line a person
// would type and hands it to the window's one executor (command_runner.hpp),
// so it is echoed, kept in the history and one undo step; an agent types the
// same line. The two grids are buffers, as the Alignment Manager's are: their
// cells are edited in place and applied by Apply as ONE line for the grid (a
// group of key=value for each edited row), so an Apply is one undo step and a
// value the verb refuses leaves the template as it was, with the edits still in
// the grid; never by a table's own signal. What the dialog shows is read from
// Document::roadTemplates, and the preview is cad's own section of it
// (road::templateSection), so the drawing and ROADTEMPLATE SECTION agree.
// Edits survive a reload that changed nothing the grid shows, and nothing else:
// not the choice of another template, not an undo or a typed line that changed
// the component.
//
// NON-MODAL, kept by the window between uses; it hears the Document through a
// DocumentWatcher, so a line typed on the command line, an undo or an opened
// project reloads it, keeping the chosen template and component.
//
// Object names, which the widget tests and the headless steps drive it by:
//   roadTemplateManagerDialog   the dialog (the action bimRoadTemplates' data)
//   roadTemplateList            the templates: name, left, right, courses, description
//   roadTemplateName            the name New, Copy and Rename use
//   roadTemplateNew             ROADTEMPLATE NEW "name"
//   roadTemplateCopy            ROADTEMPLATE NEW "name" FROM <chosen>
//   roadTemplateRename          ROADTEMPLATE RENAME <chosen> "name"
//   roadTemplateDelete          ROADTEMPLATE DELETE <chosen>
//   roadTemplateDefaults        ROADTEMPLATE DEFAULTS
//   roadTemplateFile            the file Export writes and Import reads (a text box: a
//                               headless run has no file dialog)
//   roadTemplateBrowse          a file dialog for it (none in a headless run)
//   roadTemplateOverwrite       replace what is there: a file on Export, the templates of the
//                               same name on Import (the line gets OVERWRITE)
//   roadTemplateExport          ROADTEMPLATE EXPORT <chosen> "file" [OVERWRITE]
//   roadTemplateExportAll       ROADTEMPLATE EXPORT ALL "file" [OVERWRITE]
//   roadTemplateImport          ROADTEMPLATE IMPORT "file" [OVERWRITE]
//   roadTemplateComponents      side, name, kind, width, crossfall, layer, faces (a kerb's:
//                               inward or outward), and what else it is; width, crossfall,
//                               layer and a kerb's faces edited in place
//   roadTemplateApplyComponents ROADTEMPLATE SET <t> side:name width= crossfall= layer=
//                               [faces=] [side:name ...], one line for every row edited
//   roadTemplateRevertComponents  the grid as the drawing holds it again
//   roadTemplateAddSide         left, right or both
//   roadTemplateAddKind         lane, shoulder, kerb, footpath, verge, median, drain, batter
//   roadTemplateAdd             ROADTEMPLATE ADD <t> <side> <kind>
//   roadTemplateRemove          ROADTEMPLATE REMOVE <t> side:name (the chosen row)
//   roadTemplateComponentKeys   key=value ... for the chosen component (shape=, cut=, ...)
//   roadTemplateSetKeys         ROADTEMPLATE SET <t> side:name <the keys>
//   roadTemplateCourses         the chosen component's courses, top down: name,
//                               thickness, material, IFC type - the last three edited
//   roadTemplateApplyCourses    ROADTEMPLATE COURSE <t> side:name SET <course> ... [<course>
//                               ...], one line for every row edited
//   roadTemplateCourseName, roadTemplateCourseThickness, roadTemplateCourseMaterial
//   roadTemplateCourseAdd       ROADTEMPLATE COURSE <t> side:name ADD <course> thickness=
//                               material=
//   roadTemplateCourseRemove    ROADTEMPLATE COURSE <t> side:name REMOVE <course>
//   roadTemplatePreview         the template across a level road, its courses coloured
//   roadTemplateStatus          what the last line replied, or why it was refused
//   roadTemplateClose

#include <QDialog>

#include <functional>
#include <memory>

#include "command_runner.hpp"

namespace katana::cad {
class Document;
}

namespace katana::qt {

struct RoadTemplateManagerContext {
    // The drawing. It may be destroyed before the dialog, which then does
    // nothing more (DocumentWatcher).
    katana::cad::Document* document = nullptr;
    // The window's one executor. May be empty: the dialog then says it
    // cannot run the line.
    CommandRunner run{};
    // True in a headless session, which opens no file dialog. May be empty.
    std::function<bool()> headless{};
};

// No Q_OBJECT: it connects to lambdas, which keeps moc out of it.
class RoadTemplateManagerDialog final : public QDialog {
  public:
    RoadTemplateManagerDialog(RoadTemplateManagerContext context, QWidget* parent = nullptr);
    ~RoadTemplateManagerDialog() override;

    // Chooses the template `name` in the list; false when there is none.
    bool selectTemplate(const QString& name);
    // The chosen template's name; empty when there is none.
    [[nodiscard]] QString currentTemplate() const;

  private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace katana::qt
