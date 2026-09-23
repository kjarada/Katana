#include "name_picker.hpp"

#include <algorithm>
#include <cmath>

#include <QAbstractItemView>
#include <QCompleter>
#include <QLineEdit>
#include <QPainter>
#include <QPalette>
#include <QPixmap>
#include <QStandardItem>
#include <QStandardItemModel>
#include <QStringListModel>

#include "customisation_context.hpp"
#include "definition_thumbnails.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/style_resolver.hpp"
#include "katana/entity/tables.hpp"
#include "style_painter.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace {

using katana::cad::CatalogueEntry;
using katana::cad::DefinitionSource;
using katana::cad::NameRole;

// Picture sizes, in logical pixels: a linestyle needs length to show its
// pattern, a symbol is square.
constexpr QSize kLinestyleIconSize{48, 14};
constexpr QSize kSymbolIconSize{18, 18};
// A model linetype's picture: this many periods, as the thumbnails show a
// library linestyle (definition_thumbnails.cpp, kSamplePeriods).
constexpr double kSamplePeriods = 4.0;
constexpr double kMarginFraction = 0.12;

[[nodiscard]] QString fromName(std::string_view name)
{
    return QString::fromUtf8(name.data(), static_cast<qsizetype>(name.size()));
}

[[nodiscard]] QByteArray nameBytes(std::string_view name)
{
    return QByteArray(name.data(), static_cast<qsizetype>(name.size()));
}

[[nodiscard]] QString undefinedLabel(std::string_view name)
{
    return QObject::tr("%1 (not defined)").arg(fromName(name));
}

// A model Linetype along the style sample path, painted as the thumbnails
// paint a library linestyle. The thumbnail cache cannot hold it: it keys on
// the LIBRARY's generation, and a model linetype changes with the model.
[[nodiscard]] QImage modelLinetypeImage(const katana::cad::Document& document,
                                        const std::string& name, QSize pixels, QColor ground)
{
    QImage image(pixels, QImage::Format_ARGB32_Premultiplied);
    image.fill(ground);
    const katana::entity::Linetype* linetype = document.model().linetypes.find(name);
    const double period = linetype != nullptr ? linetype->patternLength() : 0.0;
    const double width = std::max(kSamplePeriods * period, 1.0);
    katana::entity::Style style;
    style.name = "sample";
    style.linetype = name;
    const katana::cad::StyleSamplePath sample = katana::cad::styleSamplePath(width);
    katana::cad::DashOptions dashes;
    dashes.viewScale = pixels.width() / width;
    const katana::cad::StyleDrawing drawing = katana::cad::styleSampleDrawing(
        document.model(), document.styleLibrary(), style, sample, 1.0, dashes);
    if (drawing.empty()) {
        return image;
    }
    QPainter painter(&image);
    painter.setRenderHint(QPainter::Antialiasing, true);
    StylePaintTarget target;
    target.view.resize(pixels.width(), pixels.height());
    target.view.fit(paintedExtent(drawing, painter.font()), kMarginFraction);
    target.entityPen = QPen(theme::text(), 1.5);
    target.entityPen.setCapStyle(Qt::FlatCap);
    paintStyleDrawing(painter, drawing, target);
    return image;
}

} // namespace

NamePicker::NamePicker(const CustomisationContext& context, NameRole role, bool offerByLayer,
                       QWidget* parent)
    : QComboBox(parent), document_(context.document), thumbnails_(context.thumbnails),
      role_(role), offerByLayer_(role == NameRole::Linetype && offerByLayer)
{
    setEditable(true);
    // Typing never adds an item, and Enter never looks for a case-folded
    // duplicate to jump to (QComboBox does that when duplicates are off):
    // "kerb" typed is "kerb", not the "Kerb" further down.
    setInsertPolicy(QComboBox::NoInsert);
    setDuplicatesEnabled(true);
    setIconSize(role_ == NameRole::Symbol ? kSymbolIconSize : kLinestyleIconSize);
    setSizeAdjustPolicy(QComboBox::AdjustToMinimumContentsLengthWithIcon);
    setMinimumContentsLength(18);
    lineEdit()->setPlaceholderText(role_ == NameRole::Symbol ? tr("(no symbol)")
                                                             : tr("(no linetype)"));
    fieldPalette_ = lineEdit()->palette();

    // Our own completer over the names alone - no headers, no decorated
    // label - in place of the combo's, which searches its whole model by
    // prefix. Set on the line edit, not through QComboBox::setCompleter,
    // which would map a completion back to a row by a case-folded search and
    // could land on "Kerb" for "KERB".
    completions_ = new QStringListModel(this);
    completer_ = new QCompleter(completions_, this);
    completer_->setCaseSensitivity(Qt::CaseInsensitive);
    completer_->setFilterMode(Qt::MatchContains);
    completer_->setCompletionMode(QCompleter::PopupCompletion);
    completer_->setMaxVisibleItems(16);
    QComboBox::setCompleter(nullptr);
    lineEdit()->setCompleter(completer_);

    // Lambdas, not slots: no moc here.
    QObject::connect(this, &QComboBox::activated, this, [this](int) { chosen(); });
    QObject::connect(lineEdit(), &QLineEdit::editingFinished, this, [this] {
        // Enter on a completion reaches the field first (QCompleter hands
        // the key to its widget before acting on it), which would report the
        // typed fragment as a name; the completion's own activation follows.
        if (completer_->popup() != nullptr && completer_->popup()->isVisible()) {
            return;
        }
        chosen();
    });
    QObject::connect(completer_, qOverload<const QString&>(&QCompleter::activated), this,
                     [this](const QString& text) {
                         if (updating_) {
                             return;
                         }
                         // The line edit already shows the completion; make
                         // its item current so currentName reads its bytes.
                         if (const int row = rowForText(text); row >= 0) {
                             updating_ = true;
                             setCurrentIndex(row);
                             updating_ = false;
                         }
                         chosen();
                     });
    QObject::connect(this, &QComboBox::currentIndexChanged, this, [this](int row) {
        // The field says "not defined" in the mark's colour, and its tooltip
        // what is drawn instead, while the kept name is the one shown.
        const bool undefined =
            row >= 0 && itemData(row, kSourceRole).toInt() ==
                            static_cast<int>(DefinitionSource::Undefined);
        QPalette palette = fieldPalette_;
        if (undefined) {
            palette.setColor(QPalette::Text, kUndefinedNameColour);
        }
        lineEdit()->setPalette(palette);
        setToolTip(row >= 0 ? itemData(row, Qt::ToolTipRole).toString() : QString());
    });

    rebuild({});
}

void NamePicker::setCurrentName(std::string_view name)
{
    // The whole list is rebuilt only by refresh(): a form sets its pickers on
    // every row a person clicks, and the list itself has not changed. The
    // kept item is the one row that follows the name.
    updating_ = true;
    auto* items = qobject_cast<QStandardItemModel*>(model());
    if (count() > 0 &&
        itemData(0, kSourceRole).toInt() == static_cast<int>(DefinitionSource::Undefined)) {
        items->removeRow(0);
    }
    bool listed = false;
    for (int row = 0; row < count() && !listed; ++row) {
        const QVariant stored = itemData(row, kNameRole);
        listed = stored.isValid() && stored.toByteArray() == nameBytes(name);
    }
    QStringList completions = completions_->stringList();
    if (!completions.isEmpty() && keptCompletion_) {
        completions.removeFirst();
    }
    keptCompletion_ = false;
    if (!name.empty() && !listed) {
        CatalogueEntry missing;
        missing.name = std::string(name);
        missing.source = DefinitionSource::Undefined;
        missing.missing = true;
        QStandardItem* item = addEntry(missing);
        items->insertRow(0, items->takeRow(item->row()));
        completions.prepend(fromName(name));
        keptCompletion_ = true;
    }
    completions_->setStringList(completions);
    selectName(name);
    lastName_ = std::string(name);
    updating_ = false;
}

std::string NamePicker::currentName() const
{
    const QString text = lineEdit()->text();
    if (const int row = rowForText(text); row >= 0) {
        const QByteArray stored = itemData(row, kNameRole).toByteArray();
        return std::string(stored.constData(), static_cast<std::size_t>(stored.size()));
    }
    return text.toStdString();
}

bool NamePicker::currentIsDefined() const
{
    const int row = rowForText(lineEdit()->text());
    return row >= 0 &&
           itemData(row, kSourceRole).toInt() != static_cast<int>(DefinitionSource::Undefined);
}

void NamePicker::refresh() { rebuild(currentName()); }

std::vector<std::string> NamePicker::names() const
{
    std::vector<std::string> result;
    for (int row = 0; row < count(); ++row) {
        const QVariant stored = itemData(row, kNameRole);
        if (stored.isValid()) {
            const QByteArray bytes = stored.toByteArray();
            result.emplace_back(bytes.constData(), static_cast<std::size_t>(bytes.size()));
        }
    }
    return result;
}

void NamePicker::rebuild(std::string_view keep)
{
    const std::string kept(keep); // `keep` may point into the list being cleared
    updating_ = true;
    clear();
    std::vector<CatalogueEntry> choices = role_ == NameRole::Symbol
                                              ? katana::cad::symbolChoices(*document_)
                                              : katana::cad::linetypeChoices(*document_,
                                                                             offerByLayer_);
    QStringList completions;
    const bool listed = std::ranges::any_of(
        choices, [&kept](const CatalogueEntry& entry) { return entry.name == kept; });
    keptCompletion_ = false;
    if (!kept.empty() && !listed) {
        CatalogueEntry missing;
        missing.name = kept;
        missing.source = DefinitionSource::Undefined;
        missing.missing = true;
        addEntry(missing);
        completions << fromName(kept);
        keptCompletion_ = true;
    }
    struct Section {
        DefinitionSource source;
        QString title; // empty: no header row
    };
    const std::vector<Section> sections =
        role_ == NameRole::Symbol
            ? std::vector<Section>{{DefinitionSource::BuiltIn, tr("Built-in symbols")},
                                   {DefinitionSource::Library, tr("Library symbols")}}
            : std::vector<Section>{{DefinitionSource::BuiltIn, {}}, // ByLayer, alone
                                   {DefinitionSource::ModelLinetype, tr("Drawing linetypes")},
                                   {DefinitionSource::Library, tr("Library linestyles")}};
    for (const Section& section : sections) {
        const bool any = std::ranges::any_of(choices, [&section](const CatalogueEntry& entry) {
            return entry.source == section.source;
        });
        if (!any) {
            continue;
        }
        if (!section.title.isEmpty()) {
            addHeader(section.title);
        }
        for (const CatalogueEntry& entry : choices) {
            if (entry.source == section.source) {
                addEntry(entry);
                completions << fromName(entry.name);
            }
        }
    }
    completions_->setStringList(completions);
    selectName(kept);
    lastName_ = kept;
    updating_ = false;
}

void NamePicker::addHeader(const QString& title)
{
    auto* item = new QStandardItem(title);
    // Neither enabled nor selectable: the popup shows it and skips it, and
    // the arrow keys pass over it.
    item->setFlags(Qt::NoItemFlags);
    item->setData(-1, kSourceRole);
    QFont font = item->font();
    font.setBold(true);
    item->setFont(font);
    qobject_cast<QStandardItemModel*>(model())->appendRow(item);
}

QStandardItem* NamePicker::addEntry(const CatalogueEntry& entry)
{
    const bool undefined = entry.source == DefinitionSource::Undefined;
    auto* item = new QStandardItem(undefined ? undefinedLabel(entry.name) : fromName(entry.name));
    item->setData(nameBytes(entry.name), kNameRole);
    item->setData(static_cast<int>(entry.source), kSourceRole);
    item->setIcon(iconFor(entry));
    QString tip;
    switch (entry.source) {
    case DefinitionSource::Undefined:
        item->setForeground(kUndefinedNameColour);
        tip = fallbackDescription(entry.name);
        break;
    case DefinitionSource::ModelLinetype:
        tip = tr("A linetype of this drawing");
        break;
    case DefinitionSource::BuiltIn:
        tip = role_ == NameRole::Symbol ? tr("A built-in symbol")
                                        : tr("The linetype of the entity's layer");
        break;
    case DefinitionSource::Library:
        tip = tr("12d %1 from %2")
                  .arg(role_ == NameRole::Symbol ? tr("symbol") : tr("linestyle"),
                       entry.sourceFile.empty() ? tr("the loaded library")
                                                : fromName(entry.sourceFile));
        if (!entry.group.empty()) {
            tip += tr(", group %1").arg(fromName(entry.group));
        }
        if (entry.collision) {
            tip += tr(". Also a linetype of this drawing: the library's definition is the "
                      "one drawn.");
        }
        break;
    }
    item->setData(tip, Qt::ToolTipRole);
    qobject_cast<QStandardItemModel*>(model())->appendRow(item);
    return item;
}

void NamePicker::selectName(std::string_view name)
{
    if (name.empty()) {
        setCurrentIndex(-1);
        lineEdit()->clear();
        return;
    }
    const QByteArray wanted = nameBytes(name);
    for (int row = 0; row < count(); ++row) {
        const QVariant stored = itemData(row, kNameRole);
        if (stored.isValid() && stored.toByteArray() == wanted) {
            setCurrentIndex(row);
            return;
        }
    }
    // Not reached through setCurrentName or rebuild, which list the name
    // first; kept anyway so the field never shows something else.
    setCurrentIndex(-1);
    lineEdit()->setText(fromName(name));
}

int NamePicker::rowForText(const QString& text) const
{
    const int current = currentIndex();
    if (current >= 0 && itemText(current) == text && itemData(current, kNameRole).isValid()) {
        return current;
    }
    for (int row = 0; row < count(); ++row) {
        if (itemText(row) == text && itemData(row, kNameRole).isValid()) {
            return row;
        }
    }
    return -1;
}

void NamePicker::chosen()
{
    if (updating_) {
        return;
    }
    std::string name = currentName();
    if (name == lastName_) {
        return; // finished an edit that changed nothing
    }
    lastName_ = name;
    // Through a copy: the callback may rebuild the form this picker is in.
    if (const auto callback = onNameChosen) {
        callback(name);
    }
}

QIcon NamePicker::iconFor(const CatalogueEntry& entry) const
{
    if (thumbnails_ == nullptr) {
        return {};
    }
    const QSize logical = iconSize();
    const qreal ratio = devicePixelRatioF();
    // The thumbnails take DEVICE pixels (definition_thumbnails.hpp).
    const QSize pixels(static_cast<int>(std::lround(logical.width() * ratio)),
                       static_cast<int>(std::lround(logical.height() * ratio)));
    const QColor ground = theme::raised();
    QImage image;
    if (role_ == NameRole::Symbol) {
        image = thumbnails_->thumbnail(*document_, ThumbnailKind::Symbol, entry.name, pixels,
                                       ground)
                    .image;
    } else if (entry.source == DefinitionSource::ModelLinetype) {
        image = modelLinetypeImage(*document_, entry.name, pixels, ground);
    } else if (entry.source == DefinitionSource::BuiltIn) {
        return {}; // ByLayer has no pattern of its own to show
    } else {
        image = thumbnails_->thumbnail(*document_, ThumbnailKind::Linestyle, entry.name,
                                       pixels, ground)
                    .image;
    }
    if (image.isNull()) {
        return {};
    }
    image.setDevicePixelRatio(ratio);
    return QIcon(QPixmap::fromImage(image));
}

QString NamePicker::fallbackDescription(std::string_view name) const
{
    if (role_ == NameRole::Symbol) {
        return tr("Not defined in the drawing or any loaded library: drawn as the built-in "
                  "symbol \"%1\" until a library defines it.")
            .arg(fromName(katana::entity::builtInSymbolFor(name)));
    }
    return tr("Not defined in the drawing or any loaded library: drawn as a solid line "
              "until a library or a linetype defines it.");
}

} // namespace katana::qt
