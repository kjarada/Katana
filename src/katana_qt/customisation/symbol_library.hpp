#pragma once

// The symbol library: AutoCAD's Blocks palette and MicroStation's cell
// selector in one non-modal window over the drawing.
//
//   left    a tree of groups - All, Built-in, the library's `/` groups,
//           "(ungrouped)", and "Not defined" when a name resolves to nothing
//   centre  a grid of 64-pixel pictures (DefinitionThumbnails), captioned
//           with the name and badged with how many entities draw it, under
//           a FilterBar: search over name, group and survey code, and the
//           chips All / In drawing / Used by codes / Missing / At vertices
//   right   a StylePreview at a plot scale (insertion crosshair, scale bar),
//           the details of the current symbol - including how big it
//           prints: "2 x 2 mm at 1:500, 1 x 1 m on the ground" - and the
//           actions
//
// WHAT IS LISTED is cad::symbolLibrary: decision D3's symbols (a definition
// is a symbol when it is `mode vertex`, a survey code draws it as one, a
// style names it as one, or its file is a symbol file), the built-in shapes,
// and every symbol name a style or a code gives that nothing defines, marked
// in the pickers' amber with the shape it is drawn as instead.
//
// THE ACTIONS. Model changes go through Document::execute, one undo step
// each, and only ever from a button - never from the grid's own selection
// signals (docs/cad.md):
//   Assign to Selected Points   cad::assignSymbolToPoints: the points among
//                               the selection move into a style that draws
//                               the symbol at the size given, found or made
//   Set on Style                the chosen style's symbol, through
//                               commands::updateStyleIfChanged
//   Select Points Using         the points wearing a style that names it,
//                               selected and shown (CustomisationContext)
//   Replace in Styles           cad::replaceSymbolInStyles: every style
//                               naming the current symbol names another
//   Load .4d...                 archive12d::readCustomisation, MERGED into
//                               the session's library (decision D1: never a
//                               Replace from here), with what each file
//                               added and replaced written to the log
//   Export Selected to .4d...   archive12d::writeStyleLibrary of the
//                               selected library definitions
//
// HEADLESS. A headless session never opens a file dialog: loading and
// exporting are loadLibraryFile / exportSelectedTo, taking a path, which the
// buttons call once a file dialog has given one. In a headless session (the
// offscreen platform, or setHeadless) the buttons ask chooseLoadFile /
// chooseExportFile when set and otherwise say in the log what to call.
//
// LIFETIME. The dialog may outlive its Document: it watches it through a
// DocumentWatcher (its last member) and does nothing once it has gone.
// Reloads are the watcher's - deferred and coalesced to the event loop -
// never a rebuild inside a Document notification.

#include <filesystem>
#include <functional>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

#include <QDialog>

#include "customisation_context.hpp"
#include "symbol_library_model.hpp"

class QComboBox;
class QDoubleSpinBox;
class QFormLayout;
class QLabel;
class QListView;
class QPushButton;
class QTreeWidget;
class QTreeWidgetItem;

namespace katana::qt {

class DefinitionThumbnails;
class DocumentWatcher;
class FilterBar;
class NamePicker;
class StylePreview;
struct DocumentChanges;

class SymbolLibraryDialog : public QDialog {
  public:
    explicit SymbolLibraryDialog(const CustomisationContext& context, QWidget* parent = nullptr);
    ~SymbolLibraryDialog() override;

    SymbolLibraryDialog(const SymbolLibraryDialog&) = delete;
    SymbolLibraryDialog& operator=(const SymbolLibraryDialog&) = delete;

    // ---- what is shown ---------------------------------------------------------------

    // The symbol the details, the preview and the actions are about; empty
    // when none is.
    [[nodiscard]] std::string currentSymbol() const { return current_; }
    // Makes `name` current and selects it alone, clearing the tree, the
    // chip and the search first when they hide it. False when nothing lists
    // that exact name.
    bool selectSymbol(std::string_view name);
    // The names in the grid, in order.
    [[nodiscard]] std::vector<std::string> shownNames() const;
    // The names selected in the grid, in grid order.
    [[nodiscard]] std::vector<std::string> selectedNames() const;
    // Rebuilds from the Document now. The watcher calls it on its own
    // (deferred); a caller needing the result at once may too.
    void reload();

    [[nodiscard]] SymbolGridModel& gridModel() const { return *model_; }
    [[nodiscard]] StylePreview* preview() const { return preview_; }

    // ---- actions: each logs what it did or why it did nothing, and returns
    // whether the drawing or a file changed ------------------------------------------------

    // The current symbol onto the points among the Document's selection, at
    // the size in the size field (0: the definition's own).
    bool assignToSelectedPoints();
    // `style`'s symbol set to the current one.
    bool setOnStyle(const std::string& style);
    // Selects (and through the context shows) the points wearing a style
    // that names the current symbol.
    bool selectPointsUsing();
    // Every style naming the current symbol names `replacement` instead.
    // Refused (and logged) for an empty `replacement`: taking a symbol off
    // its styles is not a replacement.
    bool replaceInStyles(const std::string& replacement);
    // Reads a customisation file (a .4d library, or a survey code file - the
    // extension does not say which) and MERGES it into the session's
    // customisation.
    bool loadLibraryFile(const std::filesystem::path& path);
    // Writes the selected library definitions to a .4d file. Built-in and
    // undefined names cannot be written, and are named in the log.
    bool exportSelectedTo(const std::filesystem::path& path);

    // ---- file dialogs ----------------------------------------------------------------

    void setHeadless(bool headless) { headless_ = headless; }
    [[nodiscard]] bool headless() const { return headless_; }
    // Asked instead of a file dialog when set; an empty path cancels.
    std::function<std::filesystem::path()> chooseLoadFile{};
    std::function<std::filesystem::path()> chooseExportFile{};

  private:
    void buildUi();
    void onDocumentChanged(const DocumentChanges& changes);
    void rebuildTree();
    void rebuildStyles();
    // Refilters, keeping the grid's selection (by name: rows mean nothing
    // across a reset) and the current symbol where they are still shown.
    void applyFilter();
    void applyFilter(const std::vector<std::string>& selected);
    void updateChipCounts();
    void restoreCurrent(const std::vector<std::string>& selected);
    void currentChanged();
    void showDetails();
    void updatePrintSize();
    void showFilledRows();
    void updateActions();
    void loadClicked();
    void exportClicked();
    [[nodiscard]] QImage pictureOf(const katana::cad::SymbolLibraryEntry& entry) const;
    [[nodiscard]] const katana::cad::SymbolLibraryEntry* currentEntry() const;
    [[nodiscard]] bool alive() const;
    void log(const QString& message, bool isError) const;

    CustomisationContext context_{};
    katana::cad::Document* document_ = nullptr;
    // The context's picture cache, or this dialog's own when it gave none:
    // a grid of names without pictures is not a symbol library.
    std::unique_ptr<DefinitionThumbnails> ownThumbnails_;
    DefinitionThumbnails* thumbnails_ = nullptr;
    bool headless_ = false;
    std::string current_{};
    bool rebuilding_ = false;

    SymbolGridModel* model_ = nullptr;
    QTreeWidget* tree_ = nullptr;
    FilterBar* filterBar_ = nullptr;
    QListView* grid_ = nullptr;
    QLabel* countLabel_ = nullptr;
    StylePreview* preview_ = nullptr;
    QComboBox* plotScale_ = nullptr;
    QComboBox* ground_ = nullptr;

    QFormLayout* form_ = nullptr;
    QLabel* name_ = nullptr;
    QLabel* source_ = nullptr;
    QLabel* group_ = nullptr;
    QLabel* units_ = nullptr;
    QLabel* mode_ = nullptr;
    QLabel* factor_ = nullptr;
    QLabel* origin_ = nullptr;
    QLabel* extent_ = nullptr;
    QLabel* content_ = nullptr;
    QLabel* codes_ = nullptr;
    QLabel* styles_ = nullptr;
    QLabel* print_ = nullptr;
    QLabel* missing_ = nullptr;

    QDoubleSpinBox* size_ = nullptr;
    QPushButton* assign_ = nullptr;
    QComboBox* targetStyle_ = nullptr;
    QPushButton* setOnStyle_ = nullptr;
    NamePicker* replaceWith_ = nullptr;
    QPushButton* replace_ = nullptr;
    QPushButton* selectUsing_ = nullptr;
    QPushButton* load_ = nullptr;
    QPushButton* export_ = nullptr;

    // Last, so it is destroyed first: no delivery reaches a half-destroyed
    // dialog, and it is how every action knows the Document is still there.
    std::unique_ptr<DocumentWatcher> watcher_;
};

} // namespace katana::qt
