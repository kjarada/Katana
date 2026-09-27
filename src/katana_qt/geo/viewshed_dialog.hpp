#pragma once

// Terrain > Analysis > Viewshed and Line of Sight (docs/terrain.md,
// "Viewshed and line of sight"): the window onto the RASTER VIEWSHED and LOS
// verbs (src/katana_app/geo/viewshed_verbs.cpp).
//
// The dialog computes nothing itself. Each tab writes the line its fields
// describe - shown, as it will run, in its command field - and Run hands it
// to the window's one executor as if it had been typed
// (terrain_dialog_support). A picked point is only written into the line
// (OBSERVER x,y, TARGET x,y). The lines are made by pure functions of the
// fields, tested without a window.
//
// viewshedDialog, tabs viewshedTabs:
//   Viewshed
//     viewshedSource     the surface or reference raster
//     viewshedObserver   an observer to add, typed x,y
//     viewshedAdd        adds it; viewshedPick picks one in a plan view
//                        (GeoServices::pickPoint; off with no picker)
//     viewshedObservers  the observers, one x,y each; viewshedRemove
//     viewshedUseScope   the observers are the points a scope takes
//                        instead (OBSERVERS), in viewshedScope
//                        (ScopeFilterWidget: viewshedScopeDrawing ...)
//     viewshedHeight     the eye above the ground (1.7)
//     viewshedTarget     the height above the ground a cell is seen at (0)
//     viewshedMax        how far to look; blank: the whole raster
//     viewshedCurvature  curvature and refraction: blank GDAL's 0.85714,
//                        none, or a coefficient
//     viewshedAreas      a layer to draw the visible area on; blank: none
//     viewshedName       the reference raster's name; blank: the source's
//     viewshedCommand, viewshedPreview, viewshedRun, viewshedReply
//   Line of Sight
//     losSource, losObserver, losPickObserver, losTarget, losPickTarget,
//     losHeight, losTargetHeight, losCurvature, losCommand, losRun, losReply

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
class QTabWidget;

namespace katana::qt {

struct ViewshedForm {
    // SURFACE <name> or RASTER <id>, as the source picker's data says it.
    QString source;
    QStringList observers; // "x,y" each
    bool useScope = false;
    QString scope;
    QString scopeError;
    QString height = "1.7";
    QString target = "0";
    QString max;
    QString curvature;
    QString areas;
    QString name;
};

//   RASTER VIEWSHED <source> (OBSERVER x,y)... | OBSERVERS <scope>
//                   height=<h> target=<t> [max=<m>] [curvature=<k>|none]
//                   [areas=<layer>] [NAME <n>]
// InvalidArgument naming the field: no source, no observer, a point, a
// height or a distance that does not read, a scope the controls cannot
// say, a layer or name with a double quote.
[[nodiscard]] katana::core::Result<QString> viewshedLine(const ViewshedForm& form);

struct LosForm {
    QString source;
    QString observer; // x,y
    QString target;   // x,y
    QString height = "1.7";
    QString targetHeight = "0";
    QString curvature;
};

//   LOS <source> OBSERVER x,y TARGET x,y height=<h> target=<t> [curvature=<k>|none]
[[nodiscard]] katana::core::Result<QString> losLine(const LosForm& form);

class ViewshedDialog final : public QDialog {
  public:
    explicit ViewshedDialog(TerrainDialogContext context, QWidget* parent = nullptr);
    ~ViewshedDialog() override;

    void reload();
    [[nodiscard]] ViewshedForm viewshedForm() const;
    [[nodiscard]] LosForm losForm() const;
    [[nodiscard]] ScopeFilterWidget& scopeControls() const { return *scope_; }
    void runViewshed(bool preview = false);
    void runLos();
    // Adds "x,y" to the observers; false, and nothing added, when it is not
    // two numbers.
    bool addObserver(const QString& text);

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void refresh();
    // Picks a point in a plan view into `field` (or the observer list when
    // null), saying so in `reply`.
    void pickInto(QLineEdit* field, QPlainTextEdit* reply);

    TerrainDialogContext context_;
    QTabWidget* tabs_ = nullptr;
    QComboBox* source_ = nullptr;
    QLineEdit* observer_ = nullptr;
    QPushButton* add_ = nullptr;
    QPushButton* pick_ = nullptr;
    QListWidget* observers_ = nullptr;
    QPushButton* remove_ = nullptr;
    QCheckBox* useScope_ = nullptr;
    ScopeFilterWidget* scope_ = nullptr;
    QLineEdit* height_ = nullptr;
    QLineEdit* target_ = nullptr;
    QLineEdit* max_ = nullptr;
    QLineEdit* curvature_ = nullptr;
    QLineEdit* areas_ = nullptr;
    QLineEdit* name_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* preview_ = nullptr;
    QPushButton* run_ = nullptr;
    QPlainTextEdit* reply_ = nullptr;
    QComboBox* losSource_ = nullptr;
    QLineEdit* losObserver_ = nullptr;
    QPushButton* losPickObserver_ = nullptr;
    QLineEdit* losTarget_ = nullptr;
    QPushButton* losPickTarget_ = nullptr;
    QLineEdit* losHeight_ = nullptr;
    QLineEdit* losTargetHeight_ = nullptr;
    QLineEdit* losCurvature_ = nullptr;
    QLineEdit* losCommand_ = nullptr;
    QPushButton* losRun_ = nullptr;
    QPlainTextEdit* losReply_ = nullptr;
    std::unique_ptr<TerrainRun> viewshedRunner_;
    std::unique_ptr<TerrainRun> losRunner_;
};

} // namespace katana::qt
