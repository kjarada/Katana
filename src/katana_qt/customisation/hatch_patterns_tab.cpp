#include "customisation/hatch_patterns_tab.hpp"

#include <QAbstractItemView>
#include <QCheckBox>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPainter>
#include <QPushButton>
#include <QSplitter>
#include <QTableWidget>
#include <QVBoxLayout>

#include <algorithm>
#include <cmath>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "command_word.hpp"
#include "customisation/document_watcher.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/hatch_pattern_rows.hpp"
#include "katana/cad/hatching.hpp"
#include "katana/cad/style_manager_rows.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/tables.hpp"
#include "katana/math/numerics.hpp"
#include "theme.hpp"

namespace katana::qt {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::HatchLineFamily;
using katana::entity::HatchPattern;

namespace {

QString qs(const std::string& text) { return QString::fromStdString(text); }

QPushButton* makeButton(const QString& text, const QString& objectName, const QString& tip,
                        QWidget* parent)
{
    auto* button = new QPushButton(text, parent);
    button->setObjectName(objectName);
    button->setToolTip(tip);
    button->setAutoDefault(false);
    return button;
}

QTableWidgetItem* cell(const QString& text, bool editable)
{
    auto* item = new QTableWidgetItem(text);
    const Qt::ItemFlags flags = item->flags();
    item->setFlags(editable ? (flags | Qt::ItemIsEditable) : (flags & ~Qt::ItemIsEditable));
    return item;
}

// The pattern drawn over a square, as the plan view draws it: the families
// through cad::hatchSegments, which the view calls; a solid pattern filled.
// The square is eight of the widest spacing across, so a handful of lines
// show whatever the pattern's size; the world-origin anchoring the hatcher
// keeps is why the square is centred on the origin.
class HatchSwatch final : public QWidget {
  public:
    explicit HatchSwatch(QWidget* parent) : QWidget(parent)
    {
        setObjectName("hatchPatternPreview");
        setMinimumSize(140, 140);
    }

    void display(std::optional<HatchPattern> pattern, QString note)
    {
        pattern_ = std::move(pattern);
        note_ = std::move(note);
        update();
    }

  protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        painter.fillRect(rect(), theme::panel());
        const double side = std::min(width(), height()) - 16.0;
        const QRectF box((width() - side) / 2.0, (height() - side) / 2.0, side, side);
        painter.setPen(QPen(theme::border(), 1.0));
        painter.drawRect(box);
        if (!pattern_ || pattern_->drawsNothing()) {
            painter.setPen(theme::textMuted());
            painter.drawText(box, Qt::AlignCenter | Qt::TextWordWrap,
                             note_.isEmpty() ? QString("Draws nothing") : note_);
            return;
        }
        if (pattern_->solid) {
            painter.fillRect(box.adjusted(1, 1, -1, -1), theme::accent());
            return;
        }
        double widest = 0.0;
        for (const HatchLineFamily& family : pattern_->families) {
            widest = std::max(widest, family.spacing);
        }
        const double half = 4.0 * widest;
        katana::geometry::Polyline2 square;
        square.vertices = {{-half, -half}, {half, -half}, {half, half}, {-half, half}};
        square.closed = true;
        const double scale = side / (2.0 * half);
        const auto at = [&](const katana::geometry::Point2& point) {
            return QPointF(box.center().x() + point.x * scale, box.center().y() - point.y * scale);
        };
        painter.setRenderHint(QPainter::Antialiasing);
        painter.setPen(QPen(theme::accent(), 1.0));
        for (const katana::geometry::Segment2& segment :
             katana::cad::hatchSegments(square, *pattern_)) {
            painter.drawLine(at(segment.start), at(segment.end));
        }
    }

  private:
    std::optional<HatchPattern> pattern_;
    QString note_;
};

} // namespace

// =====================================================================================

struct HatchPatternsTab::Impl {
    HatchPatternsTab* tab = nullptr;
    CustomisationContext context;
    // Every connection is made in this context, deleted first on the way
    // out so none can reach a half-destroyed Impl.
    std::unique_ptr<QObject> guard = std::make_unique<QObject>();
    std::unique_ptr<DocumentWatcher> watcher;

    QTableWidget* table = nullptr;
    QGroupBox* editor = nullptr;
    QCheckBox* solid = nullptr;
    QTableWidget* families = nullptr;
    QPushButton* addFamily = nullptr;
    QPushButton* removeFamily = nullptr;
    HatchSwatch* swatch = nullptr;
    QPushButton* save = nullptr;
    QPushButton* revert = nullptr;
    QLineEdit* name = nullptr;
    QPushButton* remove = nullptr;
    QPushButton* duplicate = nullptr;
    QPushButton* selectUsers = nullptr;
    QPushButton* newStyle = nullptr;
    QLabel* status = nullptr;

    bool loading = false;
    // The editor holds edits not yet saved, made to `editedFrom` as the
    // drawing held it; a reload that left that pattern alone keeps them.
    bool edited = false;
    std::optional<HatchPattern> editedFrom;

    [[nodiscard]] bool alive() const { return watcher != nullptr && watcher->documentAlive(); }
    [[nodiscard]] katana::cad::Document& document() const { return *context.document; }

    template <typename Sender, typename Signal, typename Slot>
    void connect(Sender* sender, Signal signal, Slot slot)
    {
        QObject::connect(sender, signal, guard.get(), std::move(slot));
    }

    [[nodiscard]] const HatchPattern* current() const
    {
        if (!alive()) {
            return nullptr;
        }
        const QString chosen = tab->currentPattern();
        return chosen.isEmpty() ? nullptr
                                : document().model().hatchPatterns.find(chosen.toStdString());
    }

    void setStatus(const QString& text, bool isError)
    {
        status->setStyleSheet(
            QString("color: %1").arg((isError ? theme::error() : theme::textMuted()).name()));
        status->setText(text);
    }

    // One line through the window's executor, what it said shown; true when
    // it was carried out. The tab reloads at once so the result is on screen
    // when this returns.
    bool run(const QString& line)
    {
        if (!alive()) {
            setStatus("The drawing this tab edited is gone.", true);
            return false;
        }
        if (!context.run) {
            setStatus("Nothing here can run " + line + ": the dialog has no command line.", true);
            return false;
        }
        const VerbOutcome outcome = context.run(line);
        if (outcome.ok) {
            const QStringList lines = outcome.reply.split('\n', Qt::SkipEmptyParts);
            setStatus(lines.isEmpty() ? line : lines.front(), false);
        } else {
            setStatus(outcome.error.isEmpty() ? line + " was refused." : outcome.error, true);
        }
        reload();
        return outcome.ok;
    }

    // The name field as a command word, or nullopt, said, when it is empty
    // or cannot be one.
    std::optional<QString> typedName()
    {
        const QString typed = name->text().trimmed();
        if (typed.isEmpty()) {
            setStatus("Type a name first.", true);
            return std::nullopt;
        }
        auto quoted = commandWord(typed, "the name");
        if (!quoted) {
            setStatus(qs(quoted.error().message), true);
            return std::nullopt;
        }
        return *quoted;
    }

    std::optional<QString> chosenName()
    {
        const QString chosen = tab->currentPattern();
        if (chosen.isEmpty()) {
            setStatus("Choose a hatch pattern first.", true);
            return std::nullopt;
        }
        auto quoted = commandWord(chosen, "the pattern's name");
        if (!quoted) {
            setStatus(qs(quoted.error().message), true);
            return std::nullopt;
        }
        return *quoted;
    }

    void build();
    void reload();
    void loadTable(const QString& keep);
    void loadEditor();
    void updateButtons();
    // The families as the grid holds them, "a s a s ...", or why they cannot
    // be: a row with one number, a number that is not one.
    [[nodiscard]] Result<QString> gridFamilies() const;
    // The pattern the editor would save, for the swatch.
    [[nodiscard]] Result<HatchPattern> editorPattern() const;
    void showSwatch();
    void markEdited();
};

void HatchPatternsTab::Impl::build()
{
    auto* layout = new QVBoxLayout(tab);
    auto* splitter = new QSplitter(Qt::Horizontal, tab);

    table = new QTableWidget(0, 4, splitter);
    table->setObjectName("hatchPatternTable");
    table->setHorizontalHeaderLabels({"Name", "Kind", "Used by", "Description"});
    table->verticalHeader()->setVisible(false);
    table->setSelectionBehavior(QAbstractItemView::SelectRows);
    table->setSelectionMode(QAbstractItemView::SingleSelection);
    table->setEditTriggers(QAbstractItemView::NoEditTriggers);
    table->horizontalHeader()->setStretchLastSection(true);

    editor = new QGroupBox("Pattern", splitter);
    editor->setObjectName("hatchPatternEditor");
    auto* editorLayout = new QVBoxLayout(editor);
    solid = new QCheckBox("Solid fill", editor);
    solid->setObjectName("hatchPatternSolid");
    solid->setToolTip("A filled area rather than families of lines");
    editorLayout->addWidget(solid);
    families = new QTableWidget(0, 2, editor);
    families->setObjectName("hatchPatternFamilies");
    families->setHorizontalHeaderLabels({"Angle (degrees)", "Spacing"});
    families->verticalHeader()->setVisible(false);
    families->setSelectionBehavior(QAbstractItemView::SelectRows);
    families->setSelectionMode(QAbstractItemView::SingleSelection);
    families->horizontalHeader()->setStretchLastSection(true);
    families->setToolTip("One family of parallel lines a row: the angle counter-clockwise from "
                         "east in degrees, the spacing in model units - a crosshatch is two");
    editorLayout->addWidget(families, 1);
    auto* familyButtons = new QHBoxLayout();
    addFamily = makeButton("Add Family", "hatchFamilyAdd", "Another family of lines", editor);
    removeFamily =
        makeButton("Remove Family", "hatchFamilyRemove", "Drop the chosen family", editor);
    familyButtons->addWidget(addFamily);
    familyButtons->addWidget(removeFamily);
    familyButtons->addStretch(1);
    editorLayout->addLayout(familyButtons);
    swatch = new HatchSwatch(editor);
    editorLayout->addWidget(swatch, 1);
    auto* saveRow = new QHBoxLayout();
    revert = makeButton("Revert", "hatchPatternRevert", "Forget the edits made above", editor);
    save = makeButton("Save", "hatchPatternSave",
                      "HATCH SET name angle spacing ... (or SOLID): one undo step", editor);
    saveRow->addStretch(1);
    saveRow->addWidget(revert);
    saveRow->addWidget(save);
    editorLayout->addLayout(saveRow);

    splitter->addWidget(table);
    splitter->addWidget(editor);
    splitter->setStretchFactor(0, 3);
    splitter->setStretchFactor(1, 2);
    layout->addWidget(splitter, 1);

    auto* nameRow = new QHBoxLayout();
    name = new QLineEdit(tab);
    name->setObjectName("hatchPatternName");
    name->setPlaceholderText("name for New, New Solid, Duplicate or New Style");
    auto* create = makeButton("New", "hatchPatternNew",
                              "HATCH NEW name angle spacing ...: the families in the grid", tab);
    auto* createSolid =
        makeButton("New Solid", "hatchPatternNewSolid", "HATCH SOLID name: a solid fill", tab);
    duplicate = makeButton("Duplicate", "hatchPatternDuplicate",
                           "The chosen pattern again, under the name", tab);
    newStyle = makeButton("New Style Using This", "hatchPatternNewStyle",
                          "STYLE NEW name HATCH pattern: a style that hatches with the chosen "
                          "pattern",
                          tab);
    nameRow->addWidget(new QLabel("Name:", tab));
    nameRow->addWidget(name, 1);
    for (QPushButton* button : {create, createSolid, duplicate, newStyle}) {
        nameRow->addWidget(button);
    }
    layout->addLayout(nameRow);

    auto* buttons = new QHBoxLayout();
    remove = makeButton("Delete", "hatchPatternDelete",
                        "HATCH DELETE name: refused while a layer or style names it", tab);
    auto* purge = makeButton("Purge", "hatchPatternPurge",
                             "PURGE HATCHES: delete every pattern nothing uses, one undo step",
                             tab);
    selectUsers = makeButton("Select Users", "hatchPatternSelectUsers",
                             "Select the entities hatched with the chosen pattern", tab);
    status = new QLabel(tab);
    status->setObjectName("hatchPatternStatus");
    status->setWordWrap(true);
    buttons->addWidget(remove);
    buttons->addWidget(purge);
    buttons->addWidget(status, 1);
    buttons->addWidget(selectUsers);
    layout->addLayout(buttons);

    connect(table, &QTableWidget::itemSelectionChanged, [this] {
        if (loading) {
            return;
        }
        edited = false;
        loadEditor();
        updateButtons();
    });
    connect(solid, &QCheckBox::toggled, [this](bool) {
        if (!loading) {
            markEdited();
        }
    });
    // An edit marks the grid and redraws the swatch; nothing runs from the
    // table's own signal (docs/desktop.md, "The rules a dialog or panel
    // follows").
    connect(families, &QTableWidget::cellChanged, [this](int, int) {
        if (!loading) {
            markEdited();
        }
    });
    connect(addFamily, &QPushButton::clicked, [this] {
        loading = true;
        const int row = families->rowCount();
        families->insertRow(row);
        families->setItem(row, 0, cell({}, true));
        families->setItem(row, 1, cell({}, true));
        loading = false;
        families->setCurrentCell(row, 0);
        markEdited();
    });
    connect(removeFamily, &QPushButton::clicked, [this] {
        const int row = families->currentRow();
        if (row < 0) {
            setStatus("Choose the family to remove first.", true);
            return;
        }
        families->removeRow(row);
        markEdited();
    });
    connect(revert, &QPushButton::clicked, [this] {
        edited = false;
        loadEditor();
        updateButtons();
    });
    connect(save, &QPushButton::clicked, [this] {
        const auto chosen = chosenName();
        if (!chosen) {
            return;
        }
        if (solid->isChecked()) {
            run("HATCH SET " + *chosen + " SOLID");
            return;
        }
        const auto grid = gridFamilies();
        if (!grid) {
            setStatus(qs(grid.error().message), true);
            return;
        }
        run("HATCH SET " + *chosen + " " + *grid);
    });
    connect(create, &QPushButton::clicked, [this] {
        const auto typed = typedName();
        if (!typed) {
            return;
        }
        const auto grid = gridFamilies();
        if (!grid) {
            setStatus(qs(grid.error().message), true);
            return;
        }
        if (run("HATCH NEW " + *typed + " " + *grid)) {
            tab->selectPattern(name->text().trimmed());
        }
    });
    connect(createSolid, &QPushButton::clicked, [this] {
        const auto typed = typedName();
        if (typed && run("HATCH SOLID " + *typed)) {
            tab->selectPattern(name->text().trimmed());
        }
    });
    connect(duplicate, &QPushButton::clicked, [this] {
        const HatchPattern* pattern = current();
        const auto typed = typedName();
        if (pattern == nullptr || !typed) {
            return;
        }
        const QString line = pattern->solid
                                 ? "HATCH SOLID " + *typed
                                 : "HATCH NEW " + *typed + " " +
                                       qs(katana::cad::hatchFamiliesText(*pattern));
        if (run(line)) {
            tab->selectPattern(name->text().trimmed());
        }
    });
    connect(newStyle, &QPushButton::clicked, [this] {
        const auto chosen = chosenName();
        if (!chosen) {
            return;
        }
        if (const auto typed = typedName()) {
            run("STYLE NEW " + *typed + " HATCH " + *chosen);
        }
    });
    connect(remove, &QPushButton::clicked, [this] {
        if (const auto chosen = chosenName()) {
            run("HATCH DELETE " + *chosen);
        }
    });
    connect(purge, &QPushButton::clicked, [this] { run("PURGE HATCHES"); });
    connect(selectUsers, &QPushButton::clicked, [this] {
        const QString chosen = tab->currentPattern();
        if (!alive() || chosen.isEmpty()) {
            return;
        }
        const std::vector<katana::entity::EntityId> ids = katana::cad::entitiesUsing(
            document().model(), katana::cad::UsageTable::HatchPattern, {chosen.toStdString()});
        if (ids.empty()) {
            setStatus("No entity is hatched with " + chosen + ".", false);
            return;
        }
        if (context.selectAndShow) {
            context.selectAndShow(ids);
        } else {
            document().selection().set(ids);
            document().notifySelectionChanged();
        }
        setStatus(QString("%1 entit%2 selected.").arg(ids.size()).arg(ids.size() == 1 ? "y" : "ies"),
                  false);
    });
}

void HatchPatternsTab::Impl::reload()
{
    if (!alive()) {
        return;
    }
    loadTable(tab->currentPattern());
    loadEditor();
    updateButtons();
}

void HatchPatternsTab::Impl::loadTable(const QString& keep)
{
    loading = true;
    const std::vector<katana::cad::HatchPatternRow> rows =
        katana::cad::hatchPatternRows(document().model());
    table->setRowCount(static_cast<int>(rows.size()));
    int keepRow = -1;
    for (std::size_t i = 0; i < rows.size(); ++i) {
        const katana::cad::HatchPatternRow& row = rows[i];
        const int at = static_cast<int>(i);
        table->setItem(at, 0, cell(qs(row.name), false));
        table->setItem(at, 1, cell(qs(katana::cad::hatchPatternKind(row)), false));
        table->setItem(at, 2,
                       cell(row.users.used() ? qs(row.users.describe()) : QString("nothing"),
                            false));
        table->setItem(at, 3, cell(qs(row.description), false));
        if (qs(row.name) == keep) {
            keepRow = at;
        }
    }
    table->clearSelection();
    if (keepRow >= 0) {
        table->selectRow(keepRow);
    }
    table->resizeColumnToContents(0);
    loading = false;
}

void HatchPatternsTab::Impl::loadEditor()
{
    const HatchPattern* pattern = current();
    // Unsaved edits survive a reload that left their pattern as it was.
    if (edited && pattern != nullptr && editedFrom && *pattern == *editedFrom) {
        showSwatch();
        return;
    }
    loading = true;
    families->setRowCount(0);
    solid->setChecked(pattern != nullptr && pattern->solid);
    if (pattern != nullptr) {
        for (const HatchLineFamily& family : pattern->families) {
            const int row = families->rowCount();
            families->insertRow(row);
            families->setItem(row, 0, cell(qs(katana::cad::hatchAngleDegrees(family.angle)), true));
            families->setItem(row, 1,
                              cell(qs(katana::core::formatExactReal(family.spacing)), true));
        }
    }
    loading = false;
    edited = false;
    editedFrom = pattern != nullptr ? std::optional<HatchPattern>(*pattern) : std::nullopt;
    showSwatch();
}

void HatchPatternsTab::Impl::updateButtons()
{
    const HatchPattern* pattern = current();
    // "none" draws nothing on purpose and is not the tab's to change.
    const bool editable =
        pattern != nullptr && !katana::entity::HatchPatternPolicy::isProtected(pattern->name);
    editor->setEnabled(editable);
    families->setEnabled(editable && !solid->isChecked());
    addFamily->setEnabled(editable && !solid->isChecked());
    removeFamily->setEnabled(editable && !solid->isChecked());
    save->setEnabled(editable && edited);
    revert->setEnabled(editable && edited);
    remove->setEnabled(editable);
    duplicate->setEnabled(editable);
    newStyle->setEnabled(pattern != nullptr);
    selectUsers->setEnabled(pattern != nullptr);
}

Result<QString> HatchPatternsTab::Impl::gridFamilies() const
{
    QStringList words;
    for (int row = 0; row < families->rowCount(); ++row) {
        const QTableWidgetItem* angle = families->item(row, 0);
        const QTableWidgetItem* spacing = families->item(row, 1);
        const QString a = angle != nullptr ? angle->text().trimmed() : QString();
        const QString s = spacing != nullptr ? spacing->text().trimmed() : QString();
        if (a.isEmpty() && s.isEmpty()) {
            continue; // a row added and never filled
        }
        if (a.isEmpty() || s.isEmpty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "family " + std::to_string(row + 1) + " needs an angle and a spacing");
        }
        words << a << s;
    }
    if (words.isEmpty()) {
        return makeError(ErrorCode::InvalidArgument,
                         "a pattern needs a family of lines, or to be a solid fill");
    }
    return words.join(' ');
}

Result<HatchPattern> HatchPatternsTab::Impl::editorPattern() const
{
    HatchPattern pattern;
    pattern.name = editedFrom ? editedFrom->name : std::string("preview");
    if (solid->isChecked()) {
        pattern.solid = true;
        return pattern;
    }
    const auto grid = gridFamilies();
    if (!grid) {
        return grid.error();
    }
    const QStringList words = grid->split(' ');
    for (qsizetype i = 0; i + 1 < words.size(); i += 2) {
        const auto angle = katana::core::parseFiniteDouble(words[i].toStdString());
        const auto spacing = katana::core::parseFiniteDouble(words[i + 1].toStdString());
        if (!angle || !spacing) {
            return makeError(ErrorCode::InvalidArgument,
                             "family " + std::to_string(i / 2 + 1) + " is not two numbers");
        }
        HatchLineFamily family;
        family.angle = *angle * katana::math::kDegToRad;
        family.spacing = *spacing;
        // Drawn with the offset HATCH SET keeps (by position), so the swatch
        // shows what Save will draw.
        const auto index = static_cast<std::size_t>(i / 2);
        if (editedFrom && index < editedFrom->families.size()) {
            family.offset = editedFrom->families[index].offset;
        }
        pattern.families.push_back(family);
    }
    if (auto valid = katana::entity::validate(pattern); !valid) {
        return valid.error();
    }
    return pattern;
}

void HatchPatternsTab::Impl::showSwatch()
{
    if (current() == nullptr) {
        swatch->display(std::nullopt, "Choose a hatch pattern");
        return;
    }
    auto pattern = editorPattern();
    if (!pattern) {
        swatch->display(std::nullopt, qs(pattern.error().message));
        return;
    }
    swatch->display(std::move(*pattern), {});
}

void HatchPatternsTab::Impl::markEdited()
{
    edited = true;
    showSwatch();
    updateButtons();
}

// =====================================================================================

HatchPatternsTab::HatchPatternsTab(const CustomisationContext& context, QWidget* parent)
    : QWidget(parent), impl_(std::make_unique<Impl>())
{
    Impl& impl = *impl_;
    impl.tab = this;
    impl.context = context;
    setObjectName("hatchPatternsPage");
    impl.build();
    impl.watcher = std::make_unique<DocumentWatcher>(
        *impl.context.document, [&impl](const DocumentChanges& changes) {
            if (changes.model) {
                impl.reload();
            }
        });
    impl.reload();
}

HatchPatternsTab::~HatchPatternsTab()
{
    // The watcher, then every connection, before the children that could
    // signal into the Impl on the way out.
    impl_->watcher.reset();
    impl_->guard.reset();
}

bool HatchPatternsTab::selectPattern(const QString& name)
{
    for (int row = 0; row < impl_->table->rowCount(); ++row) {
        if (impl_->table->item(row, 0)->text() == name) {
            impl_->table->selectRow(row);
            return true;
        }
    }
    return false;
}

QString HatchPatternsTab::currentPattern() const
{
    const QList<QTableWidgetItem*> chosen = impl_->table->selectedItems();
    if (chosen.isEmpty()) {
        return {};
    }
    const QTableWidgetItem* first = impl_->table->item(chosen.front()->row(), 0);
    return first != nullptr ? first->text() : QString();
}

} // namespace katana::qt
