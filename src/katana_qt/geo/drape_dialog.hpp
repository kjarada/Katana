#pragma once

// Terrain > Analysis > Drape and Sample Heights (docs/terrain.md, "Sampling
// and drape"): the window onto the DRAPE and RASTER SAMPLE verbs
// (src/katana_app/geo/drape_verbs.cpp).
//
// The dialog computes nothing itself. Each tab writes the line its fields
// describe - shown, as it will run, in its command field - and Run hands it
// to the window's one executor as if it had been typed
// (terrain_dialog_support). A picked point is only written into the line
// (AT x,y), so what runs is still a line an agent could type. The lines are
// made by pure functions of the fields, tested without a window.
//
// drapeDialog, tabs drapeTabs:
//   Drape
//     drapeSource    the surface or reference raster
//     drapeMethod    how a raster is read between cell centres (bilinear
//                    ...); off for a surface, read on its triangles
//     drapeScope     what is draped (ScopeFilterWidget: drapeScopeDrawing,
//                    drapeFilterLayer ...)
//     drapeCommand, drapePreview, drapeRun, drapeReply
//   Sample
//     sampleSource, sampleMethod
//     samplePoint    a point to add, typed x,y
//     sampleAdd      adds it to the list
//     samplePick     picks one in a plan view (GeoServices::pickPoint); off
//                    when the window lends no picker
//     samplePoints   the points, one x,y each
//     sampleRemove   takes the chosen point off the list
//     sampleCommand, sampleRun, sampleReply

#include <QDialog>
#include <QString>
#include <QStringList>

#include <memory>

#include "geo/terrain_dialog_support.hpp"
#include "katana/core/error.hpp"

class QComboBox;
class QLineEdit;
class QListWidget;
class QPlainTextEdit;
class QPushButton;
class QShowEvent;
class QTabWidget;

namespace katana::qt {

struct DrapeForm {
    // SURFACE <name> or RASTER <id>, as the source picker's data says it.
    QString source;
    QString method = "bilinear";
    QString scope;
    QString scopeError;
};

//   DRAPE <source> <scope> [method=<m>]
// method= only for a raster. InvalidArgument naming the field: no source, a
// scope the controls cannot say.
[[nodiscard]] katana::core::Result<QString> drapeLine(const DrapeForm& form);

struct SampleForm {
    QString source;
    QString method = "bilinear";
    QStringList points; // "x,y" each
};

//   RASTER SAMPLE <source> AT x,y [AT x,y]... [method=<m>]
// InvalidArgument naming the field: no source, no point, a point that is
// not two numbers.
[[nodiscard]] katana::core::Result<QString> sampleLine(const SampleForm& form);

class DrapeDialog final : public QDialog {
  public:
    explicit DrapeDialog(TerrainDialogContext context, QWidget* parent = nullptr);
    ~DrapeDialog() override;

    void reload();
    [[nodiscard]] DrapeForm drapeForm() const;
    [[nodiscard]] SampleForm sampleForm() const;
    [[nodiscard]] ScopeFilterWidget& scopeControls() const { return *scope_; }
    void runDrape(bool preview = false);
    void runSample();
    // Adds "x,y" to the sample points; false, and nothing added, when it is
    // not two numbers.
    bool addSamplePoint(const QString& text);

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void refresh();

    TerrainDialogContext context_;
    QTabWidget* tabs_ = nullptr;
    QComboBox* drapeSource_ = nullptr;
    QComboBox* drapeMethod_ = nullptr;
    ScopeFilterWidget* scope_ = nullptr;
    QLineEdit* drapeCommand_ = nullptr;
    QPushButton* drapePreview_ = nullptr;
    QPushButton* drapeRun_ = nullptr;
    QPlainTextEdit* drapeReply_ = nullptr;
    QComboBox* sampleSource_ = nullptr;
    QComboBox* sampleMethod_ = nullptr;
    QLineEdit* samplePoint_ = nullptr;
    QPushButton* sampleAdd_ = nullptr;
    QPushButton* samplePick_ = nullptr;
    QListWidget* samplePoints_ = nullptr;
    QPushButton* sampleRemove_ = nullptr;
    QLineEdit* sampleCommand_ = nullptr;
    QPushButton* sampleRun_ = nullptr;
    QPlainTextEdit* sampleReply_ = nullptr;
    std::unique_ptr<TerrainRun> drapeRunner_;
    std::unique_ptr<TerrainRun> sampleRunner_;
};

// A point as a dialog writes it into a line: "12.345,-6.5", to the
// millimetre, locale-independently.
[[nodiscard]] QString pointWords(double x, double y);

} // namespace katana::qt
