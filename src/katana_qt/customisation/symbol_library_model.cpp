#include "symbol_library_model.hpp"

#include <algorithm>

#include <QFont>
#include <QFontMetrics>
#include <QPainter>

#include "name_picker.hpp"
#include "format.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace {

using katana::cad::DefinitionSource;
using katana::cad::SymbolLibraryEntry;

QString fromUtf8(const std::string& text)
{
    return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
}

bool containsFolded(const std::string& haystack, const QString& foldedNeedle)
{
    return fromUtf8(haystack).toCaseFolded().contains(foldedNeedle);
}

bool inGroup(const SymbolLibraryEntry& entry, const SymbolGroupFilter& group)
{
    const katana::cad::CatalogueEntry& item = entry.entry;
    switch (group.kind) {
    case SymbolGroupFilter::Kind::All:
        return true;
    case SymbolGroupFilter::Kind::BuiltIn:
        return item.source == DefinitionSource::BuiltIn;
    case SymbolGroupFilter::Kind::Ungrouped:
        return item.source == DefinitionSource::Library && item.group.empty();
    case SymbolGroupFilter::Kind::Missing:
        return item.missing;
    case SymbolGroupFilter::Kind::Group:
        // The group itself and everything below it: choosing "Test" in the
        // tree shows Test/Marks and Test/Vegetation too, as a folder does.
        return item.source == DefinitionSource::Library &&
               (item.group == group.path ||
                (item.group.size() > group.path.size() && item.group.starts_with(group.path) &&
                 item.group[group.path.size()] == '/'));
    }
    return true;
}

bool onChip(const SymbolLibraryEntry& entry, SymbolChip chip)
{
    switch (chip) {
    case SymbolChip::All:
        return true;
    case SymbolChip::InDrawing:
        return entry.entry.users.used();
    case SymbolChip::UsedByCodes:
        return !entry.codes.empty();
    case SymbolChip::Missing:
        return entry.entry.missing;
    case SymbolChip::VertexMode:
        return entry.entry.atVertices;
    }
    return true;
}

} // namespace

bool symbolMatches(const SymbolLibraryEntry& entry, const SymbolFilter& filter)
{
    if (!inGroup(entry, filter.group) || !onChip(entry, filter.chip)) {
        return false;
    }
    const QString needle = filter.text.trimmed().toCaseFolded();
    if (needle.isEmpty()) {
        return true;
    }
    return containsFolded(entry.entry.name, needle) || containsFolded(entry.entry.group, needle) ||
           std::ranges::any_of(entry.codes, [&](const katana::cad::SymbolCode& code) {
               return containsFolded(code.key, needle);
           });
}

std::size_t symbolUses(const SymbolLibraryEntry& entry) { return entry.entry.users.entities; }

SymbolGridModel::SymbolGridModel(QObject* parent) : QAbstractListModel(parent) {}

void SymbolGridModel::setEntries(std::vector<SymbolLibraryEntry> entries)
{
    beginResetModel();
    entries_ = std::move(entries);
    pictures_.clear();
    applyFilter();
    endResetModel();
}

void SymbolGridModel::setFilter(SymbolFilter filter)
{
    beginResetModel();
    filter_ = std::move(filter);
    applyFilter();
    endResetModel();
}

void SymbolGridModel::applyFilter()
{
    shown_.clear();
    for (std::size_t index = 0; index < entries_.size(); ++index) {
        if (symbolMatches(entries_[index], filter_)) {
            shown_.push_back(index);
        }
    }
}

const SymbolLibraryEntry* SymbolGridModel::entryAt(int row) const
{
    if (row < 0 || static_cast<std::size_t>(row) >= shown_.size()) {
        return nullptr;
    }
    return &entries_[shown_[static_cast<std::size_t>(row)]];
}

int SymbolGridModel::rowOf(std::string_view name) const
{
    for (std::size_t row = 0; row < shown_.size(); ++row) {
        if (entries_[shown_[row]].entry.name == name) {
            return static_cast<int>(row);
        }
    }
    return -1;
}

int SymbolGridModel::countFor(SymbolChip chip) const
{
    SymbolFilter filter = filter_;
    filter.chip = chip;
    return static_cast<int>(std::ranges::count_if(
        entries_, [&](const SymbolLibraryEntry& entry) { return symbolMatches(entry, filter); }));
}

int SymbolGridModel::rowCount(const QModelIndex& parent) const
{
    return parent.isValid() ? 0 : static_cast<int>(shown_.size());
}

Qt::ItemFlags SymbolGridModel::flags(const QModelIndex& index) const
{
    return index.isValid() ? Qt::ItemIsEnabled | Qt::ItemIsSelectable : Qt::NoItemFlags;
}

QVariant SymbolGridModel::data(const QModelIndex& index, int role) const
{
    const SymbolLibraryEntry* entry = entryAt(index.row());
    if (entry == nullptr || index.column() != 0) {
        return {};
    }
    switch (role) {
    case Qt::DisplayRole:
        return fromUtf8(entry->entry.name);
    case Qt::DecorationRole:
        return composed(shown_[static_cast<std::size_t>(index.row())]);
    case Qt::ToolTipRole:
        return toolTip(*entry);
    case Qt::ForegroundRole:
        if (entry->entry.missing) {
            return kUndefinedNameColour;
        }
        return {};
    case kNameRole:
        return QByteArray::fromStdString(entry->entry.name);
    case kUsesRole:
        return static_cast<int>(symbolUses(*entry));
    case kMissingRole:
        return entry->entry.missing;
    default:
        return {};
    }
}

QPixmap SymbolGridModel::composed(std::size_t index) const
{
    if (const auto found = pictures_.find(index); found != pictures_.end()) {
        return found->second;
    }
    const SymbolLibraryEntry& entry = entries_[index];
    QImage image = picture ? picture(entry) : QImage();
    if (image.isNull()) {
        image = QImage(kPictureSize, kPictureSize, QImage::Format_ARGB32_Premultiplied);
        image.fill(Qt::transparent);
    }
    image = image.convertToFormat(QImage::Format_ARGB32_Premultiplied);
    {
        QPainter painter(&image);
        painter.setRenderHint(QPainter::Antialiasing, true);
        // Logical pixels: a picture painted for a HiDPI screen carries its
        // device pixel ratio, and the painter works in logical units on it.
        const QSizeF logical = image.deviceIndependentSize();
        const QRectF frame = QRectF(QPointF(0.0, 0.0), logical).adjusted(1.0, 1.0, -1.0, -1.0);
        if (entry.entry.missing) {
            // The picker's amber (NamePicker): a name nothing defines looks
            // the same in every manager.
            painter.setPen(QPen(kUndefinedNameColour, 2.0));
            painter.setBrush(Qt::NoBrush);
            painter.drawRect(frame);
        }
        if (const std::size_t uses = symbolUses(entry); uses > 0) {
            // The count in the top-right corner, as a palette shows how many
            // references a block has: readable at a glance, never covering
            // the middle of the mark.
            QFont font = painter.font();
            font.setPixelSize(10);
            font.setBold(true);
            painter.setFont(font);
            const QString text = uses > 999 ? QStringLiteral("999+") : QString::number(uses);
            const QFontMetrics metrics(font);
            const int width = std::max(16, metrics.horizontalAdvance(text) + 8);
            const QRectF badge(logical.width() - width - 2.0, 2.0, width, 14.0);
            painter.setPen(Qt::NoPen);
            painter.setBrush(theme::accent());
            painter.drawRoundedRect(badge, 7.0, 7.0);
            painter.setPen(theme::accentText());
            painter.drawText(badge, Qt::AlignCenter, text);
        }
    }
    QPixmap pixmap = QPixmap::fromImage(image);
    pictures_.emplace(index, pixmap);
    return pixmap;
}

QString SymbolGridModel::toolTip(const SymbolLibraryEntry& entry) const
{
    QStringList lines;
    lines << fromUtf8(entry.entry.name);
    switch (entry.entry.source) {
    case DefinitionSource::Library:
        lines << (entry.entry.group.empty() ? QStringLiteral("(ungrouped)")
                                            : fromUtf8(entry.entry.group));
        if (!entry.entry.sourceFile.empty()) {
            lines << QStringLiteral("From %1").arg(fromUtf8(entry.entry.sourceFile));
        }
        break;
    case DefinitionSource::BuiltIn:
        lines << QStringLiteral("Built in: Katana draws it without a library");
        break;
    case DefinitionSource::Undefined:
    case DefinitionSource::ModelLinetype:
        lines << QStringLiteral("Not defined - drawn as the built-in \"%1\"")
                     .arg(fromUtf8(entry.fallback));
        break;
    }
    const std::size_t uses = symbolUses(entry);
    if (uses > 0) {
        lines << QStringLiteral("Drawn by %1 %2 in the drawing")
                     .arg(grouped(uses), uses == 1 ? QStringLiteral("entity")
                                                   : QStringLiteral("entities"));
    }
    if (!entry.codes.empty()) {
        QStringList keys;
        for (const katana::cad::SymbolCode& code : entry.codes) {
            keys << fromUtf8(code.key);
        }
        lines << QStringLiteral("Survey codes: %1").arg(keys.join(QStringLiteral(", ")));
    }
    return lines.join(QLatin1Char('\n'));
}

} // namespace katana::qt
