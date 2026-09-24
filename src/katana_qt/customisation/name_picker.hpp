#pragma once

// An editable combo for a linetype name or a symbol name - the one picker the
// style form, the layer form, the symbol browser and the survey-code manager
// all use, so the QT-02 rule is kept in one place:
//
//   A PICKER NEVER DROPS THE NAME IT WAS GIVEN (decision D3). A name in no
//   list - a library name whose library is not loaded, a typo in an imported
//   survey code file - is kept as a marked item, "<name> (not defined)" in amber,
//   whose tooltip says what it draws as meanwhile, and currentName() gives
//   the name back exactly as it was set. The old combos rebuilt their list
//   and wrote whatever it showed back into the style, rewriting names they
//   could not show.
//
//   A linetype name that needs no definition is kept the same way but NOT
//   marked: the plain continuous line ("1", "0", "continuous" in any case;
//   D4) and ByLayer in any spelling - exactly the names cad::missingNames
//   never reports, so the picker's mark and a manager's Missing count agree.
//
// The list, in sections (header rows that cannot be chosen):
//   linetype: [ByLayer - a Style's linetype only]; Drawing linetypes (the
//             model's Linetype table); Library linestyles (non-vertex library
//             definitions, decision D2)
//   symbol:   Built-in symbols; Library symbols (decision D3's four reasons)
// each entry with a picture from the shared DefinitionThumbnails, and the
// kept undefined name, when there is one, first of all. The choices come
// from cad::linetypeChoices / cad::symbolChoices, so the picker and the
// catalogue verbs (STYLE SYMBOLS) cannot disagree.
//
// Names are case-SENSITIVE and kept byte for byte (stored as bytes, never
// round-tripped through a QString); the completer's search folds case and
// matches anywhere in a name, as a person looking for "kerb" expects. Only
// the search folds: a name typed and finished is that name, in that case -
// "Kerb" typed is never swapped for a listed "KERB", nor "KERB" for "Kerb".
// (QComboBox's own lookup when an edit finishes folds case whenever its line
// edit's completer does, so the picker's completer folds through its model
// and reports itself case-sensitive; see FoldedNameCompleter.)
//
// onNameChosen fires on the user's choice only - picking from the list,
// picking a highlighted completion, or finishing typing (Enter, with the
// completion popup open or not, or leaving the field) with a name different
// from the last one - and never on setCurrentName or
// refresh: a form applies an edit when a person makes one, and a command is
// never run from a list's own change signal (docs/cad.md).
//
// Holds the Document by pointer and reads it only in the constructor,
// setCurrentName and refresh. A dialog that may outlive its Document calls
// neither once DocumentWatcher::documentAlive() is false.

#include <functional>
#include <string>
#include <string_view>
#include <vector>

#include <QColor>
#include <QComboBox>
#include <QPalette>

#include "katana/cad/style_catalogue.hpp"

class QCompleter;
class QStandardItem;
class QStandardItemModel;

namespace katana::qt {

class DefinitionThumbnails;
struct CustomisationContext;

// The mark for a name nothing defines - in this picker, and wherever a
// manager's table shows one, so "not defined" reads the same everywhere. A
// status colour, like the viewport's drawing colours, not chrome: amber,
// between the theme's text and its error red, since a missing name is drawn
// (by a fallback) and is not an error.
inline const QColor kUndefinedNameColour{0xE8, 0xA8, 0x3A};

class NamePicker : public QComboBox {
  public:
    // Item data roles, for a dialog or a test reading the list directly.
    // kNameRole holds the exact name as a QByteArray; header rows hold none.
    static constexpr int kNameRole = Qt::UserRole + 1;
    // A cad::DefinitionSource as an int; header rows hold -1.
    static constexpr int kSourceRole = Qt::UserRole + 2;

    // `role` picks the list. `offerByLayer` adds ByLayer to a linetype list -
    // a Style's linetype may inherit its layer's, a Layer's may not - and is
    // ignored for symbols. `context.document` must be set;
    // `context.thumbnails` may be null (no pictures).
    NamePicker(const CustomisationContext& context, katana::cad::NameRole role,
               bool offerByLayer, QWidget* parent = nullptr);

    // Shows `name`, adding it first when no section holds it exactly: as the
    // marked undefined item, or unmarked for a plain-line linetype name.
    // "" shows nothing chosen (no symbol; for a linetype, nothing to keep).
    // Does not fire onNameChosen.
    void setCurrentName(std::string_view name);
    // The name being shown, exactly: the stored bytes of a listed item (never
    // its decorated label), or what was typed, as UTF-8, when it matches no
    // item's text exactly (case-sensitively).
    [[nodiscard]] std::string currentName() const;
    // Whether currentName() is defined by something - a listed name, or a
    // plain-line linetype name, which needs no definition - rather than kept
    // (the marked item) or typed. False for "" (nothing chosen). Agrees with
    // cad::missingNames on the plain-line names, which it never reports.
    [[nodiscard]] bool currentIsDefined() const;

    // Rebuilds the list from the Document - after a library or survey map is
    // loaded, or a model linetype is added - keeping the current name (marked
    // if it is now undefined). Does not fire onNameChosen.
    void refresh();

    // The names offered, in list order, without the headers: the kept name
    // first when there is one.
    [[nodiscard]] std::vector<std::string> names() const;
    [[nodiscard]] katana::cad::NameRole role() const { return role_; }
    [[nodiscard]] QCompleter* nameCompleter() const { return completer_; }

    // The user chose a name (see the header comment). Receives currentName().
    std::function<void(const std::string& name)> onNameChosen{};

  private:
    void rebuild(std::string_view keep);
    void addHeader(const QString& title);
    QStandardItem* addEntry(const katana::cad::CatalogueEntry& entry);
    void selectName(std::string_view name);
    // The row whose item's text is exactly `text`, preferring the current
    // row; -1 when none.
    [[nodiscard]] int rowForText(const QString& text) const;
    // Called by every way a person can choose: fires when the name moved.
    void chosen();
    [[nodiscard]] QIcon iconFor(const katana::cad::CatalogueEntry& entry) const;
    [[nodiscard]] QString fallbackDescription(std::string_view name) const;
    // A linetype name that needs no definition, by cad::missingNames' rule.
    [[nodiscard]] bool isPlainName(std::string_view name) const;
    // The entry for a name no section lists: Undefined, or plain.
    [[nodiscard]] katana::cad::CatalogueEntry keptEntry(std::string_view name) const;
    [[nodiscard]] QString plainDescription(std::string_view name) const;

    katana::cad::Document* document_ = nullptr;
    DefinitionThumbnails* thumbnails_ = nullptr;
    katana::cad::NameRole role_ = katana::cad::NameRole::Linetype;
    bool offerByLayer_ = false;
    QCompleter* completer_ = nullptr;
    QStandardItemModel* completions_ = nullptr;
    // Row 0 of the list, and of the completions, is the kept name - marked
    // when undefined, plain when a plain-line name.
    bool kept_ = false;
    // The name last set or last reported, so a person finishing an edit that
    // changed nothing does not fire onNameChosen.
    std::string lastName_{};
    // Set while the picker changes its own list and text, when Qt's signals
    // are not the user's doing.
    bool updating_ = false;
    // The field's own palette, restored when the kept name stops being shown.
    QPalette fieldPalette_{};
};

} // namespace katana::qt
