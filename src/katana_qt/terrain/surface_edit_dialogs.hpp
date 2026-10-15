#pragma once

// Terrain > Edit Surface's dialogs and Terrain > Combine Surfaces
// (docs/surface_editing.md, "In the window").
//
// A dialog edits nothing itself. It writes the SURFACE line its fields
// describe - shown, as it will run, in its <p>Command field - and Run hands
// that line to the window's one executor (MainWindow::runVerbLine) as if it
// had been typed; Preview runs it with PREVIEW, which works the edit out and
// changes nothing. The lines are made by pure functions of the fields,
// tested without a window. No moc, as the rest of the window.
//
// SurfaceLevelsDialog is seven dialogs by its kind, each with the object
// names of its prefix p (surfaceHeights, surfaceSmooth, surfaceThin,
// surfaceDeleteLong, surfaceDeletePoints, surfaceDeleteTriangles,
// surfaceAddPoints):
//   <p>Dialog      the dialog
//   <p>Surface     the surface edited
//   <p>Operation   Raise / Lower / Set Level / Scale (heights)
//   <p>Value       dz, the level, the factor (heights); the tolerance (thin);
//                  the length or the angle (delete long)
//   <p>Base        Scale's base; blank: the lowest level changed (heights)
//   <p>Passes, <p>Weight          (smooth)
//   <p>Measure     Long sides / Thin angles (delete long)
//   <p>Inside      only inside a polygon of the drawing (heights, smooth)
//   <p>Side        Inside the polygons / Outside them (delete triangles)
//   <p>Scope       which polygon (the polygons of delete points and delete
//                  triangles, the points of add points: always asked): the
//                  shared ScopeFilterWidget, its controls
//                  <p>ScopeSelection, <p>ScopeDrawing, <p>FilterLayer ...
//   <p>Command     the exact line Run will run (read-only)
//   <p>Preview     run it with PREVIEW
//   <p>Run         run it
//   <p>Reply       what it said
//
// SurfaceCombineDialog (surfaceCombineDialog):
//   surfaceCombineMode        Merge / Paste / Clip / Offset
//   surfaceCombineMergeList   the surfaces to merge, ticked, in rank order
//   surfaceCombineUp, surfaceCombineDown   move the chosen one in the order
//   surfaceCombineTop, surfaceCombineBase  (paste)
//   surfaceCombineSource      the surface clipped or offset
//   surfaceCombineDz          the offset
//   surfaceCombineScope       the clip's polygon (ScopeFilterWidget)
//   surfaceCombineName        the new surface; blank pastes in place
//   surfaceCombineCommand, surfaceCombinePreview, surfaceCombineRun,
//   surfaceCombineReply

#include <QDialog>
#include <QString>
#include <QStringList>

#include <functional>
#include <memory>
#include <vector>

#include "command_runner.hpp"
#include "customisation/scope_filter_widget.hpp"
#include "katana/core/error.hpp"

class QCheckBox;
class QComboBox;
class QLineEdit;
class QListWidget;
class QPlainTextEdit;
class QPushButton;
class QShowEvent;
class QSpinBox;
class QStackedWidget;

namespace katana::cad {
class Document;
}

namespace katana::qt {

class DocumentWatcher;

// What the dialogs ask of whoever shows them; the window supplies these, a
// test its own.
struct SurfaceEditDialogContext {
    // The window's one executor.
    CommandRunner run;
    // The drawing: its surfaces fill the pickers, its layers the scope. May
    // be null (the lists are then empty).
    katana::cad::Document* document = nullptr;
    // The workspace's open views, for a scope's View choice. May be unset.
    std::function<std::vector<ScopeFilterView>()> views;
};

enum class SurfaceLevelsKind {
    Heights,
    Smooth,
    Thin,
    DeleteLong,
    // The three that take the drawing's data by a scope, as the shared scope
    // widget gives it - the polygons of any number (a union), or the levelled
    // points - and that a click on one entity cannot say.
    DeletePoints,
    DeleteTriangles,
    AddPoints,
};

// The object name prefix of a kind: surfaceHeights, surfaceSmooth ...
[[nodiscard]] QString surfaceLevelsPrefix(SurfaceLevelsKind kind);

struct SurfaceLevelsForm {
    QString surface;
    // Heights: 0 Raise, 1 Lower, 2 Set Level, 3 Scale.
    int operation = 0;
    QString value;
    QString base;
    int passes = 3;
    QString weight = "0.5";
    // Delete long: sides longer than the value (true) or angles below it.
    bool longSides = true;
    // Delete triangles: those outside the polygons (the union is kept).
    bool outside = false;
    bool inside = false;
    // The scope words (ScopeFilterWidget::verbWords), or why there are none.
    QString scope;
    QString scopeError;
};

//   SURFACE EDIT <s> RAISE|LOWER|SETLEVEL|SCALE <v> [base=<b>] [INSIDE FROM <scope>]
//   SURFACE EDIT <s> SMOOTH passes=<n> weight=<w> [INSIDE FROM <scope>]
//   SURFACE EDIT <s> THIN <tolerance>
//   SURFACE EDIT <s> DELETE LONG <length> | DELETE THIN <degrees>
//   SURFACE EDIT <s> DELETE POINTS INSIDE FROM <scope>
//   SURFACE EDIT <s> DELETE TRIANGLES INSIDE|OUTSIDE FROM <scope>
//   SURFACE EDIT <s> ADD POINTS FROM <scope>
// InvalidArgument naming the field: no surface, a value that is not a number
// (or not positive where it must be), a base that is not a number, a weight
// outside (0, 1], a scope the controls cannot say.
[[nodiscard]] katana::core::Result<QString> surfaceLevelsLine(SurfaceLevelsKind kind,
                                                              const SurfaceLevelsForm& form);

enum class SurfaceCombineMode { Merge, Paste, Clip, Offset };

struct SurfaceCombineForm {
    SurfaceCombineMode mode = SurfaceCombineMode::Merge;
    QStringList merge; // in rank order: later ones win
    QString top;
    QString base;
    QString surface;
    QString dz;
    QString name;
    QString scope;
    QString scopeError;
};

//   SURFACE MERGE <a> <b> ... NAME <new>
//   SURFACE PASTE <top> INTO <base> [NAME <new>]
//   SURFACE CLIP <s> FROM <scope> NAME <new>
//   SURFACE OFFSET <s> <dz> NAME <new>
// InvalidArgument naming the field: fewer than two surfaces to merge, a top
// that is its base, no surface, a dz that is not a number, no name where one
// is needed, a scope the controls cannot say.
[[nodiscard]] katana::core::Result<QString> surfaceCombineLine(const SurfaceCombineForm& form);

class SurfaceLevelsDialog final : public QDialog {
  public:
    SurfaceLevelsDialog(SurfaceLevelsKind kind, SurfaceEditDialogContext context,
                        QWidget* parent = nullptr);
    ~SurfaceLevelsDialog() override;

    // Refills the surfaces and the scope's layers and views, keeping what is
    // chosen: done when shown.
    void reload();
    [[nodiscard]] SurfaceLevelsForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const;

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void refreshCommand();
    void runLine(bool preview);

    SurfaceLevelsKind kind_;
    SurfaceEditDialogContext context_;
    QComboBox* surface_ = nullptr;
    QComboBox* operation_ = nullptr;
    QLineEdit* value_ = nullptr;
    QLineEdit* base_ = nullptr;
    QSpinBox* passes_ = nullptr;
    QLineEdit* weight_ = nullptr;
    QComboBox* measure_ = nullptr;
    QComboBox* side_ = nullptr;
    QCheckBox* inside_ = nullptr;
    ScopeFilterWidget* scope_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* preview_ = nullptr;
    QPushButton* run_ = nullptr;
    QPlainTextEdit* reply_ = nullptr;
    // Last, so it is destroyed first: it reloads the lists when the drawing
    // changes under the dialog (an undo, a typed SURFACE line, New, Open).
    std::unique_ptr<DocumentWatcher> watcher_;
};

class SurfaceCombineDialog final : public QDialog {
  public:
    explicit SurfaceCombineDialog(SurfaceEditDialogContext context, QWidget* parent = nullptr);
    ~SurfaceCombineDialog() override;

    void reload();
    void setMode(SurfaceCombineMode mode);
    [[nodiscard]] SurfaceCombineForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const;

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void refreshCommand();
    void runLine(bool preview);
    void move(int by);

    SurfaceEditDialogContext context_;
    QComboBox* mode_ = nullptr;
    QStackedWidget* pages_ = nullptr;
    QListWidget* mergeList_ = nullptr;
    QComboBox* top_ = nullptr;
    QComboBox* base_ = nullptr;
    QComboBox* source_ = nullptr;
    QLineEdit* dz_ = nullptr;
    ScopeFilterWidget* scope_ = nullptr;
    QLineEdit* name_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* preview_ = nullptr;
    QPushButton* run_ = nullptr;
    QPlainTextEdit* reply_ = nullptr;
    std::unique_ptr<DocumentWatcher> watcher_; // last: see SurfaceLevelsDialog
};

} // namespace katana::qt
