#pragma once

// Terrain > Analysis > Slope and Aspect (docs/terrain.md, "Slope and
// aspect"): the window onto the RASTER SLOPE and RASTER ASPECT verbs
// (src/katana_app/geo/slope_verbs.cpp).
//
// The dialog computes nothing itself. It writes the line its fields describe
// - shown, as it will run, in slopeCommand - and Run hands it to the
// window's one executor as if it had been typed (terrain_dialog_support):
// the analysis is the verb's background job, the class areas one undo step,
// and its reply - each class's area - comes back into slopeReply. The line
// is made by a pure function of the fields, tested without a window.
//
// slopeAnalysisDialog:
//   slopeSource      the surface or reference raster
//   slopeKind        Slope or Aspect
//   slopeUnit        percent or degree (slope)
//   slopeClasses     the breaks between classes, "5,10,25"; blank: none
//   slopeAreasLayer  the layer the class areas go under, <layer>/<class>
//   slopeMinArea     regions smaller than this many square units are merged
//                    into their neighbours; blank: none
//   slopeName        the reference raster's name; blank: the source's
//   slopeClip        keep the analysis inside closed shapes of the drawing
//   slopeScope       which shapes (ScopeFilterWidget: slopeScopeDrawing,
//                    slopeFilterLayer ...), on with slopeClip
//   slopeCommand     the exact line Run will run (read-only)
//   slopePreview     run it with PREVIEW: what would be read, nothing made
//   slopeRun         run it
//   slopeReply       what it said

#include <QDialog>
#include <QString>

#include <memory>

#include "geo/terrain_dialog_support.hpp"
#include "katana/core/error.hpp"

class QCheckBox;
class QComboBox;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QShowEvent;

namespace katana::qt {

struct SlopeForm {
    // SURFACE <name> or RASTER <id>, as the source picker's data says it.
    QString source;
    bool aspect = false;
    QString unit = "percent";
    QString classes;
    QString areas;
    QString minArea;
    QString name;
    bool clip = false;
    // The scope and filter words (ScopeFilterWidget::verbWords), or - in
    // scopeError - why the controls say none.
    QString scope;
    QString scopeError;
};

//   RASTER SLOPE <source> unit=<u> [classes=<b1>,...] [areas=<layer>] [min_area=<m2>]
//                [NAME <n>] [<scope>]
//   RASTER ASPECT <source> [NAME <n>] [<scope>]
// areas= and min_area= are written only with classes; the scope only with
// clip. InvalidArgument naming the field: no source, classes that are not
// numbers, a least area that is not a positive number, a name or layer with
// a double quote, a scope the controls cannot say.
[[nodiscard]] katana::core::Result<QString> slopeLine(const SlopeForm& form);

class SlopeAnalysisDialog final : public QDialog {
  public:
    explicit SlopeAnalysisDialog(TerrainDialogContext context, QWidget* parent = nullptr);
    ~SlopeAnalysisDialog() override;

    // Refills the sources and the scope's layers and views, keeping what is
    // chosen: done when shown.
    void reload();
    [[nodiscard]] SlopeForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const;
    [[nodiscard]] ScopeFilterWidget& scopeControls() const { return *scope_; }
    void run(bool preview = false);

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void refresh();

    TerrainDialogContext context_;
    QComboBox* source_ = nullptr;
    QComboBox* kind_ = nullptr;
    QComboBox* unit_ = nullptr;
    QLineEdit* classes_ = nullptr;
    QLineEdit* areas_ = nullptr;
    QLineEdit* minArea_ = nullptr;
    QLineEdit* name_ = nullptr;
    QCheckBox* clip_ = nullptr;
    ScopeFilterWidget* scope_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* preview_ = nullptr;
    QPushButton* run_ = nullptr;
    QPlainTextEdit* reply_ = nullptr;
    std::unique_ptr<TerrainRun> runner_;
};

} // namespace katana::qt
