#pragma once

// Terrain > Analysis > Statistics by Area (docs/terrain.md, "Statistics by
// area"): the window onto the RASTER ZONAL verb
// (src/katana_app/geo/zonal_verbs.cpp).
//
// The dialog computes nothing itself. It writes the line its fields describe
// - shown, as it will run, in zonalCommand - and Run hands it to the
// window's one executor as if it had been typed (terrain_dialog_support):
// the statistics are the verb's background job, the properties one undo
// step, and the reply - a zone record per shape - comes back into
// zonalReply. The line is made by a pure function of the fields, tested
// without a window.
//
// zonalStatsDialog:
//   zonalSource     the surface or reference raster
//   zonalScope      which closed shapes are the zones (ScopeFilterWidget:
//                   zonalScopeDrawing, zonalFilterLayer ...)
//   zonalStats      the statistics, ticked (mean, min, max, count, sum ...)
//   zonalPrefix     the properties' first part: <prefix>_mean ...
//   zonalPixels     fractional, centre or all-touched
//   zonalCsv        a CSV file of the zones as well; blank: none
//   zonalOverwrite  replace that file when it is there
//   zonalCommand    the exact line Run will run (read-only)
//   zonalPreview    run it with PREVIEW: what would be read, nothing written
//   zonalRun        run it
//   zonalReply      what it said

#include <QDialog>
#include <QString>
#include <QStringList>

#include <memory>

#include "geo/terrain_dialog_support.hpp"
#include "katana/core/error.hpp"

class QCheckBox;
class QComboBox;
class QLineEdit;
class QListWidget;
class QPlainTextEdit;
class QPushButton;
class QShowEvent;

namespace katana::qt {

struct ZonalForm {
    // SURFACE <name> or RASTER <id>, as the source picker's data says it.
    QString source;
    QStringList stats;
    QString prefix = "zone";
    QString pixels = "fractional";
    QString csv;
    bool overwrite = false;
    // The scope and filter words (ScopeFilterWidget::verbWords), or - in
    // scopeError - why the controls say none.
    QString scope;
    QString scopeError;
};

//   RASTER ZONAL <source> <scope> stats=<a,b,...> prefix=<p> pixels=<how>
//                [csv=<file>] [OVERWRITE]
// InvalidArgument naming the field: no source, no statistic ticked, a
// prefix that is no property key's first part, a file name with a double
// quote, a scope the controls cannot say.
[[nodiscard]] katana::core::Result<QString> zonalLine(const ZonalForm& form);

class ZonalStatsDialog final : public QDialog {
  public:
    explicit ZonalStatsDialog(TerrainDialogContext context, QWidget* parent = nullptr);
    ~ZonalStatsDialog() override;

    // Refills the sources and the scope's layers and views, keeping what is
    // chosen: done when shown.
    void reload();
    [[nodiscard]] ZonalForm form() const;
    [[nodiscard]] katana::core::Result<QString> command() const;
    [[nodiscard]] ScopeFilterWidget& scopeControls() const { return *scope_; }
    void run(bool preview = false);

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void refresh();

    TerrainDialogContext context_;
    QComboBox* source_ = nullptr;
    ScopeFilterWidget* scope_ = nullptr;
    QListWidget* stats_ = nullptr;
    QLineEdit* prefix_ = nullptr;
    QComboBox* pixels_ = nullptr;
    QLineEdit* csv_ = nullptr;
    QCheckBox* overwrite_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* preview_ = nullptr;
    QPushButton* run_ = nullptr;
    QPlainTextEdit* reply_ = nullptr;
    std::unique_ptr<TerrainRun> runner_;
};

} // namespace katana::qt
