#pragma once

// The symbol library's grid: one item per symbol cad::symbolLibrary lists,
// shown as a 64-pixel picture captioned with its name, with a badge for the
// entities in the drawing that draw it, and filtered by the three things a
// person narrows a library by - a group in the tree, a chip (All, In
// drawing, Used by codes, Missing, At vertices) and a search over name,
// group and survey code.
//
// A list model rather than a QSortFilterProxyModel over one: the filter is
// the whole of what it does, and it is over at most a few thousand entries,
// so a reset per change is simpler than a proxy and costs nothing a person
// could notice. Read-only (docs/cad.md: a list shows, the form beside it
// acts), and without Q_OBJECT, which a model does not need.
//
// Pictures come from the dialog (`picture`), which knows whether the
// Document is still there and which DefinitionThumbnails to ask; the model
// adds the badge and the missing mark, and keeps each composed picture until
// the entries change.

#include <functional>
#include <map>
#include <string>
#include <vector>

#include <QAbstractListModel>
#include <QImage>
#include <QPixmap>
#include <QString>

#include "katana/cad/symbol_assign.hpp"

namespace katana::qt {

// The chips, in the order the filter bar shows them.
enum class SymbolChip { All, InDrawing, UsedByCodes, Missing, VertexMode };

// What the group tree narrows to.
struct SymbolGroupFilter {
    enum class Kind {
        All,       // everything listed
        BuiltIn,   // Katana's own shapes
        Ungrouped, // library definitions with no group
        Missing,   // names nothing defines
        Group,     // a library group and every group below it
    };
    Kind kind = Kind::All;
    std::string path{}; // Group only: "Test" or "Test/Marks"

    friend bool operator==(const SymbolGroupFilter&, const SymbolGroupFilter&) = default;
};

struct SymbolFilter {
    QString text{}; // folded when compared: names are case-sensitive, a search is not (D3)
    SymbolChip chip = SymbolChip::All;
    SymbolGroupFilter group{};
};

// Whether one entry passes. Text matches the name, the group, or the key of
// a survey code that draws it, anywhere and with case folded.
[[nodiscard]] bool symbolMatches(const katana::cad::SymbolLibraryEntry& entry,
                                 const SymbolFilter& filter);

// The entities in the drawing that draw the symbol - points and lines
// wearing a style that names it - which is what the badge counts.
[[nodiscard]] std::size_t symbolUses(const katana::cad::SymbolLibraryEntry& entry);

class SymbolGridModel : public QAbstractListModel {
  public:
    static constexpr int kNameRole = Qt::UserRole + 1;    // QByteArray: the exact name
    static constexpr int kUsesRole = Qt::UserRole + 2;    // int: symbolUses
    static constexpr int kMissingRole = Qt::UserRole + 3; // bool: nothing defines it
    static constexpr int kPictureSize = 64;               // device-independent pixels

    explicit SymbolGridModel(QObject* parent = nullptr);

    // The picture of a name, kPictureSize across; a null image draws none.
    std::function<QImage(const katana::cad::SymbolLibraryEntry& entry)> picture{};

    // Replaces everything listed; the filter is kept and applied.
    void setEntries(std::vector<katana::cad::SymbolLibraryEntry> entries);
    void setFilter(SymbolFilter filter);
    [[nodiscard]] const SymbolFilter& filter() const { return filter_; }

    [[nodiscard]] const std::vector<katana::cad::SymbolLibraryEntry>& entries() const
    {
        return entries_;
    }
    // The entry shown at `row`; nullptr out of range.
    [[nodiscard]] const katana::cad::SymbolLibraryEntry* entryAt(int row) const;
    // The row showing exactly `name`, or -1.
    [[nodiscard]] int rowOf(std::string_view name) const;
    // How many entries pass `filter` with its chip replaced by `chip`: the
    // count a chip's label shows.
    [[nodiscard]] int countFor(SymbolChip chip) const;

    [[nodiscard]] int rowCount(const QModelIndex& parent = QModelIndex()) const override;
    [[nodiscard]] QVariant data(const QModelIndex& index, int role) const override;
    [[nodiscard]] Qt::ItemFlags flags(const QModelIndex& index) const override;

  private:
    void applyFilter();
    [[nodiscard]] QPixmap composed(std::size_t entry) const;
    [[nodiscard]] QString toolTip(const katana::cad::SymbolLibraryEntry& entry) const;

    std::vector<katana::cad::SymbolLibraryEntry> entries_{};
    std::vector<std::size_t> shown_{}; // indices into entries_, in order
    SymbolFilter filter_{};
    mutable std::map<std::size_t, QPixmap> pictures_{};
};

} // namespace katana::qt
