#include "symbol_library.hpp"

#include <algorithm>
#include <fstream>
#include <map>
#include <utility>

#include <QComboBox>
#include <QDoubleSpinBox>
#include <QFileDialog>
#include <QFormLayout>
#include <QGridLayout>
#include <QGroupBox>
#include <QGuiApplication>
#include <QHBoxLayout>
#include <QItemSelectionModel>
#include <QLabel>
#include <QListView>
#include <QPushButton>
#include <QScrollArea>
#include <QSplitter>
#include <QTreeWidget>
#include <QVBoxLayout>

#include "definition_thumbnails.hpp"
#include "document_watcher.hpp"
#include "filter_bar.hpp"
#include "format.hpp"
#include "icons.hpp"
#include "katana/archive12d/customisation.hpp"
#include "katana/archive12d/style_library.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/symbol_assign.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/tables.hpp"
#include "name_picker.hpp"
#include "style_painter.hpp"
#include "style_preview.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace {

using katana::cad::DefinitionSource;
using katana::cad::SymbolLibraryEntry;
using katana::entity::LineStyle;

constexpr int kKindRole = Qt::UserRole;     // tree item: SymbolGroupFilter::Kind as int
constexpr int kPathRole = Qt::UserRole + 1; // tree item: the group path, UTF-8

// The chips, in SymbolChip's order.
const QStringList& chipLabels()
{
    static const QStringList labels{QStringLiteral("All"), QStringLiteral("In drawing"),
                                    QStringLiteral("Used by codes"), QStringLiteral("Missing"),
                                    QStringLiteral("At vertices")};
    return labels;
}

// What each chip's objectName is made from (FilterBar::chipObjectName): the
// label it was first shown with. The last was "Vertex mode", and tests and
// headless scripts find it as "filterVertexmode", so the name outlives the
// label.
const QStringList& chipNames()
{
    static const QStringList names{QStringLiteral("All"), QStringLiteral("In drawing"),
                                   QStringLiteral("Used by codes"), QStringLiteral("Missing"),
                                   QStringLiteral("Vertex mode")};
    return names;
}

QString text(const std::string& value)
{
    return QString::fromUtf8(value.data(), static_cast<qsizetype>(value.size()));
}

QString inQuotes(const std::string& value)
{
    return QLatin1Char('"') + text(value) + QLatin1Char('"');
}

QString pathText(const std::filesystem::path& path)
{
    return QString::fromStdU16String(path.filename().u16string());
}

QString describe(const katana::core::Error& error)
{
    return QString::fromStdString(error.describe());
}

// A length as a person reads it: at most three decimals, no trailing zeros,
// so 2 mm is "2" and a quarter metre "0.25".
QString number(double value)
{
    QString written = QString::number(value, 'f', 3);
    if (written.contains(QLatin1Char('.'))) {
        while (written.endsWith(QLatin1Char('0'))) {
            written.chop(1);
        }
        if (written.endsWith(QLatin1Char('.'))) {
            written.chop(1);
        }
    }
    return written == QStringLiteral("-0") ? QStringLiteral("0") : written;
}

QString times() { return QStringLiteral(" × "); }

// "a, b, c and 12 more": a load of 800 definitions is one readable line.
QString listOf(const std::vector<std::string>& names, std::size_t shown = 40)
{
    QStringList parts;
    for (std::size_t index = 0; index < names.size() && index < shown; ++index) {
        parts << text(names[index]);
    }
    QString joined = parts.join(QStringLiteral(", "));
    if (names.size() > shown) {
        joined += QStringLiteral(" and %1 more").arg(grouped(names.size() - shown));
    }
    return joined;
}

QString plural(std::size_t count, const char* one, const char* many)
{
    return QStringLiteral("%1 %2").arg(grouped(count),
                                       QString::fromLatin1(count == 1 ? one : many));
}

QString unitsText(katana::entity::StyleUnits units)
{
    switch (units) {
    case katana::entity::StyleUnits::World:
        return QStringLiteral("World: metres on the ground");
    case katana::entity::StyleUnits::Paper:
        return QStringLiteral("Paper: millimetres on the plot");
    case katana::entity::StyleUnits::TwoPoint:
        return QStringLiteral("Two-point: stretched between two points");
    }
    return {};
}

QString unitSuffix(katana::entity::StyleUnits units)
{
    return units == katana::entity::StyleUnits::Paper ? QStringLiteral(" mm")
                                                      : QStringLiteral(" m");
}

// Why D3 lists a definition that is not `mode vertex` as a symbol.
QString modeText(const katana::cad::CatalogueEntry& entry)
{
    if (entry.atVertices) {
        return QStringLiteral("at vertices: drawn at each vertex of a string");
    }
    QStringList reasons;
    if (entry.kind.namedBySurveyRule) {
        reasons << QStringLiteral("a survey code draws it as one");
    }
    if (entry.kind.namedByStyle) {
        reasons << QStringLiteral("a style names it as one");
    }
    if (entry.kind.listedAsSymbol) {
        reasons << QStringLiteral("its file is a symbol file");
    }
    return QStringLiteral("along a line; a symbol because %1")
        .arg(reasons.join(QStringLiteral(", ")));
}

QString contentText(const LineStyle& definition)
{
    std::size_t strokes = 0;
    std::size_t texts = 0;
    std::size_t pens = 0;
    for (const katana::entity::Stroke& stroke : definition.strokes) {
        switch (stroke.op) {
        case katana::entity::StrokeOp::Text:
            ++texts;
            break;
        case katana::entity::StrokeOp::Pen:
            ++pens;
            break;
        default:
            ++strokes;
            break;
        }
    }
    return QStringLiteral("%1, %2, %3")
        .arg(plural(strokes, "stroke", "strokes"), plural(texts, "text", "texts"),
             plural(pens, "pen change", "pen changes"));
}

QString codesText(const std::vector<katana::cad::SymbolCode>& codes)
{
    if (codes.empty()) {
        return QStringLiteral("No survey code draws it");
    }
    QStringList parts;
    for (const katana::cad::SymbolCode& code : codes) {
        const QString size = code.size > 0.0 ? QStringLiteral("size %1").arg(number(code.size))
                                             : QStringLiteral("its own size");
        const QString colour =
            code.colour.empty() ? QStringLiteral("the string's colour") : text(code.colour);
        parts << QStringLiteral("%1 (%2, %3)").arg(text(code.key), size, colour);
    }
    return parts.join(QStringLiteral("; "));
}

QString stylesText(const katana::entity::Users& users)
{
    if (users.styles.empty()) {
        return QStringLiteral("No style names it");
    }
    return QStringLiteral("%1 - %2 in the drawing")
        .arg(listOf(users.styles), plural(users.entities, "entity", "entities"));
}

void warn(QLabel* label, bool warning)
{
    label->setStyleSheet(warning ? QStringLiteral("color: %1;").arg(kUndefinedNameColour.name())
                                 : QString());
}

} // namespace

SymbolLibraryDialog::SymbolLibraryDialog(const CustomisationContext& context, QWidget* parent)
    : QDialog(parent), context_(context), document_(context.document)
{
    setObjectName(QStringLiteral("symbolLibraryDialog"));
    setWindowTitle(QStringLiteral("Symbol Library"));
    setModal(false);
    // Offscreen is how the tests and the headless checks run; nobody there
    // could answer a file dialog, and a modal one would wait for ever.
    const QString platform = QGuiApplication::platformName();
    headless_ = platform == QStringLiteral("offscreen") || platform == QStringLiteral("minimal");

    if (context_.thumbnails == nullptr) {
        ownThumbnails_ = std::make_unique<DefinitionThumbnails>();
        context_.thumbnails = ownThumbnails_.get();
    }
    thumbnails_ = context_.thumbnails;

    buildUi();
    watcher_ = std::make_unique<DocumentWatcher>(
        *document_, [this](const DocumentChanges& changes) { onDocumentChanged(changes); });
    reload();
}

SymbolLibraryDialog::~SymbolLibraryDialog() = default;

bool SymbolLibraryDialog::alive() const
{
    return document_ != nullptr && watcher_ != nullptr && watcher_->documentAlive();
}

void SymbolLibraryDialog::log(const QString& message, bool isError) const
{
    if (context_.log) {
        context_.log(message, isError);
    }
}

// ---- layout ------------------------------------------------------------------------------------

void SymbolLibraryDialog::buildUi()
{
    auto* outer = new QVBoxLayout(this);

    auto* toolbar = new QHBoxLayout;
    load_ = new QPushButton(icon(Icon::Import), QStringLiteral("Load .4d..."), this);
    load_->setObjectName(QStringLiteral("loadLibrary"));
    load_->setToolTip(QStringLiteral(
        "Merge a style or symbol library (.4d) into this session's: its definitions are "
        "added, or replace those of the same name; nothing else is removed"));
    export_ =
        new QPushButton(icon(Icon::Export), QStringLiteral("Export Selected to .4d..."), this);
    export_->setObjectName(QStringLiteral("exportSelected"));
    export_->setToolTip(
        QStringLiteral("Write the selected library symbols to a symbol library (.4d)"));
    toolbar->addWidget(load_);
    toolbar->addWidget(export_);
    toolbar->addStretch(1);
    outer->addLayout(toolbar);

    auto* splitter = new QSplitter(Qt::Horizontal, this);
    splitter->setObjectName(QStringLiteral("symbolSplitter"));
    splitter->setChildrenCollapsible(false);

    // Left: the groups.
    tree_ = new QTreeWidget(splitter);
    tree_->setObjectName(QStringLiteral("symbolGroups"));
    tree_->setHeaderHidden(true);
    tree_->setMinimumWidth(150);

    // Centre: search, chips, the grid.
    auto* centre = new QWidget(splitter);
    auto* centreLayout = new QVBoxLayout(centre);
    centreLayout->setContentsMargins(0, 0, 0, 0);
    filterBar_ = new FilterBar(chipNames(), centre);
    for (int index = 0; index < chipLabels().size(); ++index) {
        filterBar_->setChipLabel(index, chipLabels()[index]);
    }
    filterBar_->setObjectName(QStringLiteral("symbolFilter"));
    filterBar_->setPlaceholderText(QStringLiteral("Search name, group or survey code"));
    model_ = new SymbolGridModel(this);
    model_->picture = [this](const SymbolLibraryEntry& entry) { return pictureOf(entry); };
    grid_ = new QListView(centre);
    grid_->setObjectName(QStringLiteral("symbolGrid"));
    grid_->setViewMode(QListView::IconMode);
    grid_->setMovement(QListView::Static);
    grid_->setResizeMode(QListView::Adjust);
    grid_->setWrapping(true);
    // Not uniform item sizes: that measures the first caption ("arrow") for
    // every item, and a longer name is then elided rather than wrapped.
    grid_->setWordWrap(true);
    grid_->setTextElideMode(Qt::ElideMiddle);
    grid_->setIconSize(QSize(SymbolGridModel::kPictureSize, SymbolGridModel::kPictureSize));
    // Room for a two-line caption under the picture.
    grid_->setGridSize(
        QSize(SymbolGridModel::kPictureSize + 44, SymbolGridModel::kPictureSize + 40));
    grid_->setSelectionMode(QAbstractItemView::ExtendedSelection);
    grid_->setModel(model_);
    countLabel_ = new QLabel(centre);
    countLabel_->setObjectName(QStringLiteral("symbolCount"));
    centreLayout->addWidget(filterBar_);
    centreLayout->addWidget(grid_, 1);
    centreLayout->addWidget(countLabel_);

    // Right: preview, details, actions.
    auto* right = new QWidget(splitter);
    auto* rightLayout = new QVBoxLayout(right);
    rightLayout->setContentsMargins(0, 0, 0, 0);
    preview_ = new StylePreview(*document_, right);
    preview_->setObjectName(QStringLiteral("symbolPreview"));
    preview_->setMinimumSize(240, 160);
    auto* scaleRow = new QHBoxLayout;
    auto* scaleLabel = new QLabel(QStringLiteral("Plot scale"), right);
    plotScale_ = new QComboBox(right);
    plotScale_->setObjectName(QStringLiteral("plotScale"));
    for (const int denominator : StylePreview::kPlotScales) {
        plotScale_->addItem(QStringLiteral("1:%1").arg(denominator), denominator);
    }
    plotScale_->setCurrentIndex(plotScale_->findData(preview_->scaleDenominator()));
    scaleLabel->setBuddy(plotScale_);
    ground_ = new QComboBox(right);
    ground_->setObjectName(QStringLiteral("previewGround"));
    ground_->addItem(QStringLiteral("On paper"), static_cast<int>(PreviewGround::Paper));
    ground_->addItem(QStringLiteral("On screen"), static_cast<int>(PreviewGround::Screen));
    scaleRow->addWidget(scaleLabel);
    scaleRow->addWidget(plotScale_);
    scaleRow->addWidget(ground_);
    scaleRow->addStretch(1);

    auto* details = new QWidget;
    details->setObjectName(QStringLiteral("symbolDetails"));
    auto* form = new QFormLayout(details);
    form_ = form;
    form->setFieldGrowthPolicy(QFormLayout::AllNonFixedFieldsGrow);
    const auto field = [&](QLabel*& label, const char* objectName, const QString& title) {
        label = new QLabel(details);
        label->setObjectName(QString::fromLatin1(objectName));
        label->setWordWrap(true);
        label->setTextInteractionFlags(Qt::TextSelectableByMouse);
        if (title.isEmpty()) {
            form->addRow(label);
        } else {
            form->addRow(title, label);
        }
    };
    field(name_, "detailName", {});
    QFont bold = name_->font();
    bold.setBold(true);
    if (bold.pointSizeF() > 0.0) {
        bold.setPointSizeF(bold.pointSizeF() * 1.15);
    }
    name_->setFont(bold);
    field(missing_, "detailMissing", {});
    warn(missing_, true);
    field(print_, "detailPrint", QStringLiteral("Prints"));
    field(source_, "detailSource", QStringLiteral("Source"));
    field(group_, "detailGroup", QStringLiteral("Group"));
    field(units_, "detailUnits", QStringLiteral("Units"));
    field(mode_, "detailMode", QStringLiteral("Mode"));
    field(factor_, "detailFactor", QStringLiteral("Factor"));
    field(origin_, "detailOrigin", QStringLiteral("Origin"));
    field(extent_, "detailExtent", QStringLiteral("Size"));
    field(content_, "detailContent", QStringLiteral("Content"));
    field(codes_, "detailCodes", QStringLiteral("Survey codes"));
    field(styles_, "detailStyles", QStringLiteral("Styles"));
    auto* scroll = new QScrollArea(right);
    scroll->setObjectName(QStringLiteral("symbolDetailsScroll"));
    scroll->setWidgetResizable(true);
    scroll->setFrameShape(QFrame::NoFrame);
    // Wrap, never scroll sideways: a long list of codes reads down the pane.
    scroll->setHorizontalScrollBarPolicy(Qt::ScrollBarAlwaysOff);
    scroll->setWidget(details);

    auto* actions = new QGroupBox(QStringLiteral("Use"), right);
    actions->setObjectName(QStringLiteral("symbolActions"));
    auto* grid = new QGridLayout(actions);
    size_ = new QDoubleSpinBox(actions);
    size_->setObjectName(QStringLiteral("assignSize"));
    size_->setRange(0.0, 10000.0);
    size_->setDecimals(3);
    size_->setSingleStep(0.5);
    size_->setSuffix(QStringLiteral(" m"));
    // 0 is not a size: it is "the definition's own", which a point's style
    // says with symbolSize 0.
    size_->setSpecialValueText(QStringLiteral("Own size"));
    size_->setToolTip(QStringLiteral("The symbol's width on the ground; Own size draws it as "
                                     "its definition says"));
    assign_ =
        new QPushButton(icon(Icon::Point), QStringLiteral("Assign to Selected Points"), actions);
    assign_->setObjectName(QStringLiteral("assignToPoints"));
    targetStyle_ = new QComboBox(actions);
    targetStyle_->setObjectName(QStringLiteral("targetStyle"));
    setOnStyle_ = new QPushButton(QStringLiteral("Set on Style"), actions);
    setOnStyle_->setObjectName(QStringLiteral("setOnStyle"));
    replaceWith_ = new NamePicker(context_, katana::cad::NameRole::Symbol, false, actions);
    replaceWith_->setObjectName(QStringLiteral("replaceWith"));
    replace_ = new QPushButton(QStringLiteral("Replace in Styles"), actions);
    replace_->setObjectName(QStringLiteral("replaceInStyles"));
    replace_->setToolTip(QStringLiteral(
        "Every style drawing this symbol draws the one chosen beside instead (one undo step)"));
    selectUsing_ =
        new QPushButton(icon(Icon::ZoomTo), QStringLiteral("Select Points Using"), actions);
    selectUsing_->setObjectName(QStringLiteral("selectPointsUsing"));
    grid->addWidget(new QLabel(QStringLiteral("Size"), actions), 0, 0);
    grid->addWidget(size_, 0, 1);
    grid->addWidget(assign_, 0, 2);
    grid->addWidget(new QLabel(QStringLiteral("Style"), actions), 1, 0);
    grid->addWidget(targetStyle_, 1, 1);
    grid->addWidget(setOnStyle_, 1, 2);
    grid->addWidget(new QLabel(QStringLiteral("With"), actions), 2, 0);
    grid->addWidget(replaceWith_, 2, 1);
    grid->addWidget(replace_, 2, 2);
    grid->addWidget(selectUsing_, 3, 2);
    grid->setColumnStretch(1, 1);

    rightLayout->addWidget(preview_, 2);
    rightLayout->addLayout(scaleRow);
    rightLayout->addWidget(scroll, 3);
    rightLayout->addWidget(actions);

    splitter->addWidget(tree_);
    splitter->addWidget(centre);
    splitter->addWidget(right);
    splitter->setStretchFactor(0, 0);
    splitter->setStretchFactor(1, 1);
    splitter->setStretchFactor(2, 0);
    splitter->setSizes({190, 560, 380});
    outer->addWidget(splitter, 1);

    auto* bottom = new QHBoxLayout;
    bottom->addStretch(1);
    auto* close = new QPushButton(QStringLiteral("Close"), this);
    close->setObjectName(QStringLiteral("closeButton"));
    bottom->addWidget(close);
    outer->addLayout(bottom);
    resize(1180, 720);

    // ---- wiring: lambdas, no moc. Nothing here runs a command except the
    // action buttons' clicked.
    QObject::connect(close, &QPushButton::clicked, this, [this] { this->close(); });
    QObject::connect(tree_, &QTreeWidget::currentItemChanged, this, [this] {
        if (!rebuilding_) {
            applyFilter();
        }
    });
    filterBar_->onTextChanged = [this](const QString&) {
        if (!rebuilding_) {
            applyFilter();
        }
    };
    filterBar_->onChipChanged = [this](int) {
        if (!rebuilding_) {
            applyFilter();
        }
    };
    QObject::connect(grid_->selectionModel(), &QItemSelectionModel::currentChanged, this,
                     [this] { currentChanged(); });
    QObject::connect(grid_->selectionModel(), &QItemSelectionModel::selectionChanged, this,
                     [this] { updateActions(); });
    QObject::connect(plotScale_, &QComboBox::currentIndexChanged, this, [this] {
        preview_->setScaleDenominator(plotScale_->currentData().toInt());
        updatePrintSize();
    });
    QObject::connect(ground_, &QComboBox::currentIndexChanged, this, [this] {
        preview_->setGround(static_cast<PreviewGround>(ground_->currentData().toInt()));
    });
    QObject::connect(size_, &QDoubleSpinBox::valueChanged, this, [this] {
        if (!current_.empty()) {
            preview_->setSymbol(current_, size_->value());
        }
        updatePrintSize();
    });
    QObject::connect(assign_, &QPushButton::clicked, this, [this] { assignToSelectedPoints(); });
    QObject::connect(setOnStyle_, &QPushButton::clicked, this, [this] {
        setOnStyle(targetStyle_->currentData().toByteArray().toStdString());
    });
    QObject::connect(replace_, &QPushButton::clicked, this,
                     [this] { replaceInStyles(replaceWith_->currentName()); });
    // Choosing or typing a replacement only enables the button beside it.
    QObject::connect(replaceWith_, &QComboBox::currentIndexChanged, this,
                     [this] { updateActions(); });
    QObject::connect(replaceWith_, &QComboBox::currentTextChanged, this,
                     [this] { updateActions(); });
    QObject::connect(selectUsing_, &QPushButton::clicked, this, [this] { selectPointsUsing(); });
    QObject::connect(load_, &QPushButton::clicked, this, [this] { loadClicked(); });
    QObject::connect(export_, &QPushButton::clicked, this, [this] { exportClicked(); });
}

// ---- reloading ---------------------------------------------------------------------------------

void SymbolLibraryDialog::onDocumentChanged(const DocumentChanges& changes)
{
    if (!alive()) {
        updateActions();
        return;
    }
    if (changes.model || changes.library || changes.surveyMap) {
        reload();
    } else if (changes.selection) {
        updateActions();
    }
}

void SymbolLibraryDialog::reload()
{
    if (!alive()) {
        return;
    }
    const std::vector<std::string> selected = selectedNames();
    rebuilding_ = true;
    model_->setEntries(katana::cad::symbolLibrary(*document_));
    rebuildTree();
    rebuildStyles();
    replaceWith_->refresh();
    rebuilding_ = false;
    applyFilter(selected);
}

void SymbolLibraryDialog::rebuildTree()
{
    // What was chosen, found again by what it is rather than by an item that
    // is about to be deleted.
    SymbolGroupFilter keep;
    if (const QTreeWidgetItem* item = tree_->currentItem(); item != nullptr) {
        keep.kind = static_cast<SymbolGroupFilter::Kind>(item->data(0, kKindRole).toInt());
        keep.path = item->data(0, kPathRole).toByteArray().toStdString();
    }

    std::size_t builtIn = 0;
    std::size_t ungrouped = 0;
    std::size_t missing = 0;
    // Every group and every group above it, with the entries at or below it.
    // A std::map walks a path before any path it prefixes, so each parent
    // exists before its children are made.
    std::map<std::string, std::size_t> groups;
    for (const SymbolLibraryEntry& entry : model_->entries()) {
        const katana::cad::CatalogueEntry& item = entry.entry;
        if (item.missing) {
            ++missing;
        } else if (item.source == DefinitionSource::BuiltIn) {
            ++builtIn;
        } else if (item.group.empty()) {
            ++ungrouped;
        } else {
            for (std::size_t slash = item.group.find('/'); slash != std::string::npos;
                 slash = item.group.find('/', slash + 1)) {
                ++groups[item.group.substr(0, slash)];
            }
            ++groups[item.group];
        }
    }

    tree_->clear();
    QTreeWidgetItem* chosen = nullptr;
    const auto add = [&](QTreeWidgetItem* parent, const QString& label, std::size_t count,
                         SymbolGroupFilter::Kind kind, const std::string& path) {
        auto* item = parent == nullptr ? new QTreeWidgetItem(tree_) : new QTreeWidgetItem(parent);
        item->setText(0, QStringLiteral("%1 (%2)").arg(label, grouped(count)));
        item->setData(0, kKindRole, static_cast<int>(kind));
        item->setData(0, kPathRole, QByteArray::fromStdString(path));
        if (kind == keep.kind && path == keep.path) {
            chosen = item;
        }
        return item;
    };
    QTreeWidgetItem* all = add(nullptr, QStringLiteral("All symbols"), model_->entries().size(),
                               SymbolGroupFilter::Kind::All, {});
    if (builtIn > 0) {
        add(nullptr, QStringLiteral("Built-in"), builtIn, SymbolGroupFilter::Kind::BuiltIn, {});
    }
    std::map<std::string, QTreeWidgetItem*> made;
    for (const auto& [path, count] : groups) {
        const std::size_t slash = path.rfind('/');
        QTreeWidgetItem* parent = nullptr;
        if (slash != std::string::npos) {
            parent = made.at(path.substr(0, slash));
        }
        const std::string leaf = slash == std::string::npos ? path : path.substr(slash + 1);
        made[path] = add(parent, text(leaf), count, SymbolGroupFilter::Kind::Group, path);
    }
    if (ungrouped > 0) {
        add(nullptr, QStringLiteral("(ungrouped)"), ungrouped, SymbolGroupFilter::Kind::Ungrouped,
            {});
    }
    if (missing > 0) {
        QTreeWidgetItem* item = add(nullptr, QStringLiteral("Not defined"), missing,
                                    SymbolGroupFilter::Kind::Missing, {});
        item->setForeground(0, kUndefinedNameColour);
    }
    tree_->expandAll();
    tree_->setCurrentItem(chosen != nullptr ? chosen : all);
}

void SymbolLibraryDialog::rebuildStyles()
{
    const QByteArray keep = targetStyle_->currentData().toByteArray();
    targetStyle_->clear();
    for (const std::string& name : document_->model().styles.names()) {
        targetStyle_->addItem(text(name), QByteArray::fromStdString(name));
    }
    if (const int index = targetStyle_->findData(keep); index >= 0) {
        targetStyle_->setCurrentIndex(index);
    }
}

void SymbolLibraryDialog::applyFilter() { applyFilter(selectedNames()); }

void SymbolLibraryDialog::applyFilter(const std::vector<std::string>& selected)
{
    SymbolFilter filter;
    filter.text = filterBar_->text();
    filter.chip = static_cast<SymbolChip>(std::max(0, filterBar_->chip()));
    if (const QTreeWidgetItem* item = tree_->currentItem(); item != nullptr) {
        filter.group.kind = static_cast<SymbolGroupFilter::Kind>(item->data(0, kKindRole).toInt());
        filter.group.path = item->data(0, kPathRole).toByteArray().toStdString();
    }
    const bool wasRebuilding = rebuilding_;
    rebuilding_ = true; // a reset drops the current index; it is restored by name below
    model_->setFilter(std::move(filter));
    rebuilding_ = wasRebuilding;
    updateChipCounts();
    countLabel_->setText(QStringLiteral("%1 of %2 symbols")
                             .arg(grouped(static_cast<std::uint64_t>(model_->rowCount())),
                                  grouped(model_->entries().size())));
    restoreCurrent(selected);
}

void SymbolLibraryDialog::updateChipCounts()
{
    const QStringList& labels = chipLabels();
    for (int index = 0; index < labels.size(); ++index) {
        const auto chip = static_cast<SymbolChip>(index);
        const auto count = static_cast<std::size_t>(model_->countFor(chip));
        filterBar_->setChipLabel(index,
                                 QStringLiteral("%1 (%2)").arg(labels[index], grouped(count)));
    }
}

void SymbolLibraryDialog::restoreCurrent(const std::vector<std::string>& selected)
{
    const int row = model_->rowOf(current_);
    // Exactly the names asked for, where they are still shown, whether or not
    // the current symbol is among them: a Ctrl+click that deselects an item
    // leaves it current, and putting it back in place of what the person
    // kept made the next search or reload export the wrong symbols.
    // selectSymbol asks for its symbol alone by passing just that name.
    QItemSelection selection;
    for (const std::string& name : selected) {
        if (const int at = model_->rowOf(name); at >= 0) {
            selection.select(model_->index(at), model_->index(at));
        }
    }
    const bool wasRebuilding = rebuilding_;
    rebuilding_ = true;
    grid_->selectionModel()->select(selection, QItemSelectionModel::ClearAndSelect);
    if (row >= 0) {
        grid_->selectionModel()->setCurrentIndex(model_->index(row), QItemSelectionModel::NoUpdate);
    }
    rebuilding_ = wasRebuilding;
    if (row < 0) {
        // Filtered away: nothing is current, so no action can reach a
        // symbol the person can no longer see.
        current_.clear();
    } else {
        grid_->scrollTo(model_->index(row));
    }
    showDetails();
}

void SymbolLibraryDialog::currentChanged()
{
    if (rebuilding_) {
        return;
    }
    const SymbolLibraryEntry* entry = model_->entryAt(grid_->currentIndex().row());
    current_ = entry == nullptr ? std::string() : entry->entry.name;
    showDetails();
}

const SymbolLibraryEntry* SymbolLibraryDialog::currentEntry() const
{
    if (current_.empty()) {
        return nullptr;
    }
    const auto& entries = model_->entries();
    const auto found = std::ranges::find_if(
        entries, [&](const SymbolLibraryEntry& entry) { return entry.entry.name == current_; });
    return found == entries.end() ? nullptr : &*found;
}

bool SymbolLibraryDialog::selectSymbol(std::string_view name)
{
    if (std::ranges::none_of(model_->entries(), [&](const SymbolLibraryEntry& entry) {
            return entry.entry.name == name;
        })) {
        return false;
    }
    if (model_->rowOf(name) < 0) {
        rebuilding_ = true;
        filterBar_->setText({});
        filterBar_->setChip(0);
        tree_->setCurrentItem(tree_->topLevelItem(0));
        rebuilding_ = false;
    }
    current_ = std::string(name);
    applyFilter({current_});
    return !current_.empty();
}

std::vector<std::string> SymbolLibraryDialog::shownNames() const
{
    std::vector<std::string> names;
    for (int row = 0; row < model_->rowCount(); ++row) {
        names.push_back(model_->entryAt(row)->entry.name);
    }
    return names;
}

std::vector<std::string> SymbolLibraryDialog::selectedNames() const
{
    QModelIndexList rows = grid_->selectionModel()->selectedIndexes();
    std::ranges::sort(rows,
                      [](const QModelIndex& a, const QModelIndex& b) { return a.row() < b.row(); });
    std::vector<std::string> names;
    for (const QModelIndex& index : rows) {
        if (const SymbolLibraryEntry* entry = model_->entryAt(index.row()); entry != nullptr) {
            names.push_back(entry->entry.name);
        }
    }
    return names;
}

QImage SymbolLibraryDialog::pictureOf(const SymbolLibraryEntry& entry) const
{
    if (!alive() || thumbnails_ == nullptr) {
        return {};
    }
    // Device pixels, as DefinitionThumbnails asks, so a HiDPI grid is sharp.
    const qreal ratio = grid_ != nullptr ? grid_->devicePixelRatioF() : 1.0;
    const int pixels = qRound(SymbolGridModel::kPictureSize * ratio);
    QImage image = thumbnails_
                       ->thumbnail(*document_, ThumbnailKind::Symbol, entry.entry.name,
                                   QSize(pixels, pixels), theme::viewport())
                       .image;
    image.setDevicePixelRatio(ratio);
    return image;
}

// ---- details -----------------------------------------------------------------------------------

void SymbolLibraryDialog::showDetails()
{
    const SymbolLibraryEntry* entry = alive() ? currentEntry() : nullptr;
    for (QLabel* label : {source_, group_, units_, mode_, factor_, origin_, extent_, content_,
                          codes_, styles_, print_, missing_}) {
        label->clear();
        if (label != missing_) {
            warn(label, false);
        }
    }
    if (entry == nullptr) {
        name_->setText(QStringLiteral("Choose a symbol"));
        preview_->clear();
        showFilledRows();
        updateActions();
        return;
    }

    const katana::cad::CatalogueEntry& item = entry->entry;
    name_->setText(text(item.name));
    preview_->setSymbol(item.name, size_->value());
    codes_->setText(codesText(entry->codes));
    styles_->setText(stylesText(item.users));

    switch (item.source) {
    case DefinitionSource::Library:
        if (const LineStyle* definition = document_->definitionFor(item.name);
            definition != nullptr) {
            source_->setText(definition->source.empty()
                                 ? QStringLiteral("Made in this session")
                                 : text(definition->source));
            group_->setText(definition->group.empty() ? QStringLiteral("(ungrouped)")
                                                      : text(definition->group));
            units_->setText(unitsText(definition->units));
            mode_->setText(modeText(item));
            factor_->setText(number(definition->factor));
            const katana::geometry::Box2 bounds = definition->bounds();
            extent_->setText(bounds.empty()
                                 ? QStringLiteral("Draws nothing to measure")
                                 : number(bounds.width()) + times() + number(bounds.height()) +
                                       unitSuffix(definition->units));
            content_->setText(contentText(*definition));
        }
        break;
    case DefinitionSource::BuiltIn:
        source_->setText(QStringLiteral("Built in: Katana draws it without a library"));
        group_->setText(QStringLiteral("Built-in"));
        units_->setText(QStringLiteral("model units: the style's size, else the plain mark's"));
        mode_->setText(QStringLiteral("a point mark"));
        break;
    case DefinitionSource::Undefined:
    case DefinitionSource::ModelLinetype:
        source_->setText(QStringLiteral("Not defined in any loaded library"));
        missing_->setText(QStringLiteral("Not defined - drawn as the built-in \"%1\" until a "
                                         "library defines it")
                              .arg(text(entry->fallback)));
        break;
    }
    updatePrintSize();
    updateActions();
}

void SymbolLibraryDialog::showFilledRows()
{
    // A row says something or is not there: a built-in shape has no source
    // file, units or strokes, and empty rows read as values missing.
    for (QLabel* label : {missing_, print_, source_, group_, units_, mode_, factor_, origin_,
                          extent_, content_, codes_, styles_}) {
        form_->setRowVisible(label, !label->text().isEmpty());
    }
}

void SymbolLibraryDialog::updatePrintSize()
{
    const SymbolLibraryEntry* entry = alive() ? currentEntry() : nullptr;
    if (entry == nullptr) {
        print_->clear();
        showFilledRows();
        return;
    }
    const int denominator = plotScale_->currentData().toInt();
    const double size = size_->value();
    const QString scale = QStringLiteral("1:%1").arg(denominator);

    if (entry->entry.source == DefinitionSource::Library) {
        const LineStyle* definition = document_->definitionFor(entry->entry.name);
        // A text measured in the face the preview beside this paints it in,
        // so "how big it prints" and the picture agree about a symbol's
        // letters; cad alone can only estimate them.
        const QFont font = preview_->font();
        const katana::cad::TextExtent measured = [&font](const katana::cad::StyleTextMark& mark) {
            return styleTextExtent(mark, font);
        };
        const auto printed =
            definition != nullptr
                ? katana::cad::symbolPrintSize(*definition, size, denominator, measured)
                : std::nullopt;
        if (!printed) {
            print_->setText(QStringLiteral("Draws nothing to measure"));
            showFilledRows();
            return;
        }
        const QString paper =
            number(printed->paperWidth) + times() + number(printed->paperHeight);
        const QString ground =
            number(printed->groundWidth) + times() + number(printed->groundHeight);
        print_->setText(
            QStringLiteral("%1 mm at %2, %3 m on the ground").arg(paper, scale, ground));
        const katana::geometry::Point2 origin = definition->origin;
        QString originText = QStringLiteral("(%1, %2)").arg(number(origin.x), number(origin.y));
        if (!printed->insertionInside) {
            originText += QStringLiteral(" - insertion point outside the drawing: the mark "
                                         "sits away from the point it is put on");
        }
        origin_->setText(originText);
        warn(origin_, !printed->insertionInside);
        showFilledRows();
        return;
    }
    // A built-in shape (or the stand-in for a missing name) has no size of
    // its own: without one it is the viewport's plain mark, a screen size.
    const QString prefix = entry->entry.missing ? QStringLiteral("As its stand-in: ") : QString();
    if (size > 0.0) {
        const QString paper = number(size * 1000.0 / denominator);
        print_->setText(prefix + QStringLiteral("%1 mm across at %2, %3 m on the ground")
                                     .arg(paper, scale, number(size)));
    } else {
        print_->setText(prefix + QStringLiteral("the plain point mark's screen size; give it a "
                                                "size to fix what it prints"));
    }
    showFilledRows();
}

void SymbolLibraryDialog::updateActions()
{
    const bool live = alive();
    const SymbolLibraryEntry* entry = live ? currentEntry() : nullptr;
    const std::size_t selected = live ? document_->selection().size() : 0;
    assign_->setEnabled(entry != nullptr && selected > 0);
    assign_->setToolTip(QStringLiteral("Put the points among the %1 selected into a style that "
                                       "draws this symbol at the size given (one undo step)")
                            .arg(grouped(selected)));
    setOnStyle_->setEnabled(entry != nullptr && targetStyle_->count() > 0);
    // Only once a replacement is chosen: the picker starts empty, and an
    // empty replacement took the symbol off every style naming it - its
    // points and lines lost their mark - and was logged as a success.
    const std::string replacement = replaceWith_->currentName();
    replace_->setEnabled(entry != nullptr && !entry->entry.users.styles.empty() &&
                         !replacement.empty() && replacement != entry->entry.name);
    selectUsing_->setEnabled(entry != nullptr && entry->entry.users.entities > 0);
    size_->setEnabled(entry != nullptr);
    load_->setEnabled(live);
    bool exportable = false;
    if (live) {
        for (const std::string& name : selectedNames()) {
            exportable = exportable || document_->definitionFor(name) != nullptr;
        }
    }
    export_->setEnabled(exportable);
}

// ---- actions -----------------------------------------------------------------------------------

bool SymbolLibraryDialog::assignToSelectedPoints()
{
    if (!alive()) {
        return false;
    }
    if (current_.empty()) {
        log(QStringLiteral("Assign to points: choose a symbol first"), true);
        return false;
    }
    auto assignment = katana::cad::assignSymbolToPoints(*document_, document_->selection().ids(),
                                                        current_, size_->value());
    if (!assignment) {
        log(QStringLiteral("Assign %1 to points: %2")
                .arg(inQuotes(current_), describe(assignment.error())),
            true);
        return false;
    }
    katana::cad::SymbolAssignment done = std::move(assignment).value();
    QString left;
    if (done.notPoints > 0) {
        left += QStringLiteral(" %1 not a point and left as %2.")
                    .arg(done.notPoints == 1 ? QStringLiteral("1 selected entity is")
                                             : QStringLiteral("%1 selected entities are")
                                                   .arg(grouped(done.notPoints)),
                         done.notPoints == 1 ? QStringLiteral("it was")
                                             : QStringLiteral("they were"));
    }
    if (!done.command) {
        log(QStringLiteral("%1 already in style %2, which draws %3: nothing to change.%4")
                .arg(plural(done.alreadyInStyle, "point is", "points are"), inQuotes(done.style),
                     inQuotes(current_), left),
            false);
        return false;
    }
    if (const auto status = document_->execute(std::move(done.command)); !status) {
        log(QStringLiteral("Assign %1 to points: %2")
                .arg(inQuotes(current_), describe(status.error())),
            true);
        return false;
    }
    log(QStringLiteral("%1 now in style %2%3, drawing %4.%5")
            .arg(plural(done.points, "point", "points"), inQuotes(done.style),
                 done.createsStyle ? QStringLiteral(" (new)") : QString(), inQuotes(current_),
                 left),
        false);
    return true;
}

bool SymbolLibraryDialog::setOnStyle(const std::string& style)
{
    if (!alive()) {
        return false;
    }
    if (current_.empty()) {
        log(QStringLiteral("Set on style: choose a symbol first"), true);
        return false;
    }
    const katana::entity::Model& model = document_->model();
    const katana::entity::Style* found = model.styles.find(style);
    if (found == nullptr) {
        log(QStringLiteral("Set on style: there is no style %1").arg(inQuotes(style)), true);
        return false;
    }
    katana::entity::Style changed = *found;
    changed.symbol = current_;
    auto command = katana::commands::updateStyleIfChanged(model, std::move(changed));
    if (!command) {
        log(QStringLiteral("Style %1 already draws %2").arg(inQuotes(style), inQuotes(current_)),
            false);
        return false;
    }
    if (const auto status = document_->execute(std::move(command)); !status) {
        log(QStringLiteral("Set %1 on style %2: %3")
                .arg(inQuotes(current_), inQuotes(style), describe(status.error())),
            true);
        return false;
    }
    log(QStringLiteral("Style %1 now draws %2").arg(inQuotes(style), inQuotes(current_)), false);
    return true;
}

bool SymbolLibraryDialog::selectPointsUsing()
{
    if (!alive() || current_.empty()) {
        return false;
    }
    const katana::cad::SymbolUsers users = katana::cad::symbolUsers(*document_, current_);
    if (users.points.empty()) {
        log(QStringLiteral("No point draws %1").arg(inQuotes(current_)), false);
        return false;
    }
    if (context_.selectAndShow) {
        context_.selectAndShow(users.points);
    } else {
        document_->selection().set(users.points);
        document_->notifySelectionChanged();
    }
    QString others;
    if (users.otherEntities > 0) {
        others = QStringLiteral(" (%1 in the same styles also draw it at their vertices)")
                     .arg(plural(users.otherEntities, "line", "lines"));
    }
    log(QStringLiteral("Selected %1 drawing %2%3")
            .arg(plural(users.points.size(), "point", "points"), inQuotes(current_), others),
        false);
    return true;
}

bool SymbolLibraryDialog::replaceInStyles(const std::string& replacement)
{
    if (!alive() || current_.empty()) {
        return false;
    }
    // "" is not a symbol to put in its place: set on a style, it would take
    // the symbol off (see updateActions).
    if (replacement.empty()) {
        log(QStringLiteral("Replace %1: choose the symbol to put in its place first")
                .arg(inQuotes(current_)),
            true);
        return false;
    }
    const std::vector<std::string> styles = katana::cad::symbolUsers(*document_, current_).styles;
    auto command = katana::cad::replaceSymbolInStyles(document_->model(), current_, replacement);
    if (!command) {
        log(QStringLiteral("Replace %1: no style draws it, or it is the replacement")
                .arg(inQuotes(current_)),
            false);
        return false;
    }
    if (const auto status = document_->execute(std::move(command)); !status) {
        log(QStringLiteral("Replace %1 with %2: %3")
                .arg(inQuotes(current_), inQuotes(replacement), describe(status.error())),
            true);
        return false;
    }
    log(QStringLiteral("Replaced %1 with %2 in %3: %4")
            .arg(inQuotes(current_), inQuotes(replacement),
                 plural(styles.size(), "style", "styles"), listOf(styles)),
        false);
    return true;
}

bool SymbolLibraryDialog::loadLibraryFile(const std::filesystem::path& path)
{
    if (!alive()) {
        return false;
    }
    const QString file = pathText(path);
    auto loaded = katana::archive12d::readCustomisation({path});
    if (!loaded) {
        log(QStringLiteral("Could not load %1: %2").arg(file, describe(loaded.error())), true);
        return false;
    }
    for (const std::string& warning : loaded.value().warnings) {
        log(text(warning), false);
    }
    // Decision D1: a load MERGES. Replacing the session's library from a
    // symbol browser would throw away every definition the drawing uses
    // that this file does not happen to hold.
    katana::archive12d::CustomisationMerge merged = katana::archive12d::mergeCustomisation(
        document_->styleLibrary(), document_->surveyMap(), loaded.value(),
        katana::archive12d::LoadMode::Merge);
    for (const std::string& problem : merged.problems) {
        log(text(problem), true);
    }
    const bool changed = merged.libraryLoaded || merged.mapLoaded;
    for (const katana::archive12d::FileMerge& each : merged.files) {
        const bool map = each.kind == katana::archive12d::CustomisationFile::MapFile;
        QString line = QStringLiteral("Merged %1 into the %2: %3 added")
                           .arg(each.name.empty() ? file : text(each.name),
                                map ? QStringLiteral("survey map") : QStringLiteral("library"),
                                grouped(each.added.size()));
        if (!each.added.empty()) {
            line += QStringLiteral(" (%1)").arg(listOf(each.added));
        }
        line += QStringLiteral(", %1 replaced").arg(grouped(each.replaced.size()));
        if (!each.replaced.empty()) {
            line += QStringLiteral(" (%1)").arg(listOf(each.replaced));
        }
        log(line, false);
    }
    if (!changed) {
        log(QStringLiteral("%1 held nothing to load").arg(file), false);
        return false;
    }
    if (merged.libraryLoaded) {
        document_->setStyleLibrary(std::move(merged.library));
    }
    if (merged.mapLoaded) {
        document_->setSurveyMap(std::move(merged.map));
    }
    return true;
}

bool SymbolLibraryDialog::exportSelectedTo(const std::filesystem::path& path)
{
    if (!alive()) {
        return false;
    }
    std::vector<std::string> names;
    std::vector<std::string> skipped;
    for (const std::string& name : selectedNames()) {
        (document_->definitionFor(name) != nullptr ? names : skipped).push_back(name);
    }
    if (!skipped.empty()) {
        log(QStringLiteral("Not exported, as no library defines them: %1").arg(listOf(skipped)),
            false);
    }
    if (names.empty()) {
        log(QStringLiteral("Export: select one or more library symbols in the grid first"), true);
        return false;
    }
    katana::archive12d::StyleLibraryWriteOptions options;
    options.names = names;
    options.comments = {"Symbols exported from Katana's symbol library"};
    const auto written = katana::archive12d::writeStyleLibrary(document_->styleLibrary(), options);
    if (!written) {
        log(QStringLiteral("Export to %1: %2").arg(pathText(path), describe(written.error())),
            true);
        return false;
    }
    std::ofstream out(path, std::ios::binary | std::ios::trunc);
    out << written.value();
    out.close();
    if (!out) {
        log(QStringLiteral("Export: could not write %1").arg(pathText(path)), true);
        return false;
    }
    log(QStringLiteral("Exported %1 to %2: %3")
            .arg(plural(names.size(), "symbol", "symbols"), pathText(path), listOf(names)),
        false);
    return true;
}

void SymbolLibraryDialog::loadClicked()
{
    std::filesystem::path path;
    if (chooseLoadFile) {
        path = chooseLoadFile();
    } else if (headless_) {
        log(QStringLiteral("Load .4d: a headless session opens no file dialog; "
                           "call loadLibraryFile with a path"),
            true);
        return;
    } else {
        const QString chosen = QFileDialog::getOpenFileName(
            this, QStringLiteral("Load a Style or Symbol Library"), {},
            QStringLiteral("Style and symbol libraries (*.4d);;All files (*)"));
        path = std::filesystem::path(chosen.toStdU16String());
    }
    if (!path.empty()) {
        loadLibraryFile(path);
    }
}

void SymbolLibraryDialog::exportClicked()
{
    std::filesystem::path path;
    if (chooseExportFile) {
        path = chooseExportFile();
    } else if (headless_) {
        log(QStringLiteral("Export to .4d: a headless session opens no file dialog; "
                           "call exportSelectedTo with a path"),
            true);
        return;
    } else {
        const QString chosen = QFileDialog::getSaveFileName(
            this, QStringLiteral("Export Symbols to a Symbol Library"),
            QStringLiteral("symbols_export.4d"),
            QStringLiteral("Style and symbol libraries (*.4d);;All files (*)"));
        path = std::filesystem::path(chosen.toStdU16String());
    }
    if (!path.empty()) {
        exportSelectedTo(path);
    }
}

} // namespace katana::qt
