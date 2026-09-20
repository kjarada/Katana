#pragma once

// Desktop shell of the 2D CAD application (PLAN.MD Phase 08).
//
// The window owns the Document and nothing else of substance: every panel is a
// view that is rebuilt from the document when it reports a change, and every
// user action becomes a Command or a line for the CommandInterpreter.

#include <QMainWindow>

#include <filesystem>
#include <memory>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/log.hpp"
#include "katana/terrain/tin_builder.hpp"
#include "katana/interop/reference_data.hpp"
#include "viewport_container.hpp"

class QAction;
class QActionGroup;
class QCloseEvent;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QTableWidget;
class QTreeWidget;
class QTreeWidgetItem;
class QMenu;

namespace katana::qt {

class MainWindow final : public QMainWindow {
  public:
    explicit MainWindow(QWidget* parent = nullptr);

    // Opens a project given on the command line.
    void openProject(const QString& directory);
    // Imports a data file given on the command line, routed by its extension
    // exactly as File > Import does.
    void importPath(const QString& path);

  protected:
    void closeEvent(QCloseEvent* event) override;
    // Up / Down browse the command history; Esc cancels the active tool.
    bool eventFilter(QObject* watched, QEvent* event) override;

  private:
    void buildActions();
    void buildViewMenu(QMenu* viewMenu);
    void refreshViewMenu();

    // ---- terrain, 3D and sections (PLAN.MD Phases 14, 15, 21) -------------
    void buildSurfaceFromPointCloud();
    void buildSurfaceFromRaster();
    void buildSurfaceFromDrawing();
    void addSurface(std::string name, katana::terrain::TinSurface surface);
    void cutSectionAlongSelection();
    void setVerticalExaggeration();
    void buildDocks();
    void buildReferenceDock();
    void buildStatusBar();

    void refreshAll();
    void refreshTitle();
    void refreshLayers();
    void refreshProperties();
    void refreshReferences();

    // ---- import / export (PLAN.MD Phase 20) ---------------------------------
    void importFile();
    void importVectorFile(const std::filesystem::path& path);
    void importRasterFile(const std::filesystem::path& path);
    void importPointCloudFile(const std::filesystem::path& path);
    void exportVectorFile();
    void removeSelectedReference();
    void zoomToSelectedReference();
    void onReferenceCellChanged(int row, int column);

    // Returns false when the user cancels (unsaved changes).
    [[nodiscard]] bool confirmDiscard();
    void newDocument();
    void openDocument();
    bool saveDocument();
    bool saveDocumentAs();

    void runCommandLine();
    void logMessage(const QString& text, bool isError = false);
    void addLayer();
    void addChildLayer();
    void renameSelectedLayer();
    void deleteCurrentLayer();
    void onLayerItemChanged(QTreeWidgetItem* item, int column);
    void onLayerItemDoubleClicked(QTreeWidgetItem* item, int column);
    // Full path of the selected layer, empty when nothing is selected. The
    // path is read from the item rather than rebuilt by walking parents: it is
    // the layer's identity everywhere else and must have one definition.
    [[nodiscard]] std::string selectedLayerPath() const;

    katana::core::Logger logger_;
    katana::cad::Document document_{&logger_};
    katana::cad::CommandInterpreter interpreter_{document_};

    ViewportContainer* views_ = nullptr;

    // Surfaces shown in the 3D and section views. Built on demand from
    // imported point clouds, rasters and drawing geometry, and owned here for
    // the same reason reference data is: katana_cad must stay free of GDAL and
    // PDAL so it still builds with -DKATANA_BUILD_IO=OFF.
    // unique_ptr, NOT a vector of values: sceneSurfaces_ holds raw pointers
    // into this store, and a vector of values would move every surface - and
    // dangle every one of those pointers - the moment it reallocated.
    std::vector<std::unique_ptr<katana::terrain::TinSurface>> surfaceStore_;
    std::vector<katana::cad::SceneSurface> sceneSurfaces_;
    QTreeWidget* layerTree_ = nullptr;
    QTableWidget* referenceTable_ = nullptr;
    QTableWidget* propertyTable_ = nullptr;
    QPlainTextEdit* commandLog_ = nullptr;
    QLineEdit* commandInput_ = nullptr;
    QLabel* coordinateLabel_ = nullptr;
    QLabel* snapLabel_ = nullptr;
    QLabel* layerLabel_ = nullptr;
    QAction* undoAction_ = nullptr;
    QAction* redoAction_ = nullptr;
    QAction* gridAction_ = nullptr;
    QAction* snapAction_ = nullptr;
    QActionGroup* toolGroup_ = nullptr;
    std::vector<QAction*> layoutActions_;
    std::vector<QAction*> kindActions_;

    // Imported imagery and point clouds. Owned here rather than by the
    // Document so that katana_cad stays free of GDAL and PDAL, which is what
    // lets it build with -DKATANA_BUILD_IO=OFF for the sanitizer job.
    katana::interop::ReferenceData reference_;

    bool refreshingLayers_ = false;     // suppresses cellChanged while rebuilding
    bool refreshingReferences_ = false; // ditto, for the reference table
    int historyCursor_ = 0;         // position while browsing command history
};

} // namespace katana::qt
