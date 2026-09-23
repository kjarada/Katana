#include "style_manager.hpp"

#include <algorithm>
#include <cmath>
#include <functional>
#include <optional>
#include <set>
#include <utility>

#include <QAbstractItemView>
#include <QCheckBox>
#include <QColorDialog>
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QFrame>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QItemSelectionModel>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QMenu>
#include <QPainter>
#include <QPixmap>
#include <QPushButton>
#include <QSignalBlocker>
#include <QSortFilterProxyModel>
#include <QSplitter>
#include <QStackedWidget>
#include <QTabWidget>
#include <QTableView>
#include <QTableWidget>
#include <QToolButton>
#include <QVBoxLayout>

#include "customisation/definition_thumbnails.hpp"
#include "customisation/document_watcher.hpp"
#include "customisation/filter_bar.hpp"
#include "customisation/name_picker.hpp"
#include "customisation/row_table_model.hpp"
#include "customisation/style_preview.hpp"
#include "kept_name_combo.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/purge.hpp"
#include "katana/cad/style_catalogue.hpp"
#include "katana/commands/command_stack.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/display.hpp"
#include "katana/entity/style_library.hpp"

namespace cmd = katana::commands;

namespace katana::qt {

namespace {

using katana::cad::LinetypeOrigin;
using katana::cad::LinetypeRow;
using katana::cad::LinetypeRowKind;
using katana::cad::StyleDiagnostic;
using katana::cad::StyleFields;
using katana::cad::StyleRow;
using katana::entity::Color;
using katana::entity::Linetype;
using katana::entity::Style;

// The error colour of a refusal in the dialog's own status line and of an
// invalid pattern: the theme has no token for it, and the log panel's red
// is the main window's own.
const QColor kErrorColour{0xE0, 0x5A, 0x4E};

QString fromName(std::string_view name)
{
    return QString::fromUtf8(name.data(), static_cast<qsizetype>(name.size()));
}
std::string toName(const QString& text) { return text.toStdString(); }

QColor toQColor(const Color& color) { return QColor(color.r, color.g, color.b, color.a); }
Color fromQColor(const QColor& color)
{
    return Color{static_cast<std::uint8_t>(color.red()), static_cast<std::uint8_t>(color.green()),
                 static_cast<std::uint8_t>(color.blue()), static_cast<std::uint8_t>(color.alpha())};
}

// A length as a person reads it: no trailing zeros, at most six figures.
QString number(double value) { return QString::number(value, 'g', 6); }

QString periodText(const LinetypeRow& row)
{
    switch (row.kind) {
    case LinetypeRowKind::Dash:
        return row.period > 0.0 ? number(row.period) + " m" : QStringLiteral("continuous");
    case LinetypeRowKind::Paper:
        return row.period > 0.0 ? number(row.period) + " mm" : QStringLiteral("-");
    case LinetypeRowKind::World:
        return row.period > 0.0 ? number(row.period) + " m" : QStringLiteral("-");
    case LinetypeRowKind::TwoPoint:
        break;
    }
    return QStringLiteral("stretched");
}

QString kindText(LinetypeRowKind kind)
{
    switch (kind) {
    case LinetypeRowKind::Dash:
        return QStringLiteral("dash pattern");
    case LinetypeRowKind::Paper:
        return QStringLiteral("paper");
    case LinetypeRowKind::World:
        return QStringLiteral("world");
    case LinetypeRowKind::TwoPoint:
        return QStringLiteral("two-point");
    }
    return {};
}

QString kindExplained(LinetypeRowKind kind)
{
    switch (kind) {
    case LinetypeRowKind::Dash:
        return QStringLiteral("a dash pattern in model units, saved with the drawing");
    case LinetypeRowKind::Paper:
        return QStringLiteral("12d paperstyle: millimetres on the plot, the same at every scale");
    case LinetypeRowKind::World:
        return QStringLiteral("12d worldstyle: model units, the same on the ground at every "
                              "scale");
    case LinetypeRowKind::TwoPoint:
        return QStringLiteral("12d twoptstyle: stretched between the two points it is drawn "
                              "across");
    }
    return {};
}

// A spin box that can show <varies> for a selection that does not agree: its
// minimum drops one below the real one and the special value text says so.
// Set without signals: showing a value is not an edit.
void showNumber(QDoubleSpinBox* box, std::optional<double> value, double minimum)
{
    const QSignalBlocker quiet(box);
    if (value) {
        box->setSpecialValueText({});
        box->setMinimum(minimum);
        box->setValue(*value);
    } else {
        box->setMinimum(minimum - 1.0);
        box->setSpecialValueText(kept::kVaries);
        box->setValue(minimum - 1.0);
    }
}
// A person's first edit of a box showing <varies>: back to its real range.
void leaveVaries(QDoubleSpinBox* box, double minimum)
{
    if (!box->specialValueText().isEmpty()) {
        const QSignalBlocker quiet(box);
        box->setSpecialValueText({});
        box->setMinimum(minimum);
    }
}

QPushButton* button(const QString& text, const char* objectName, const QString& tip,
                    QWidget* parent)
{
    auto* made = new QPushButton(text, parent);
    made->setObjectName(QString::fromLatin1(objectName));
    made->setToolTip(tip);
    // Enter in a field must never press a button behind the person's back:
    // a QDialog makes every push button autoDefault otherwise.
    made->setAutoDefault(false);
    return made;
}

QComboBox* scaleCombo(const char* objectName, QWidget* parent)
{
    auto* box = new QComboBox(parent);
    box->setObjectName(QString::fromLatin1(objectName));
    for (const int denominator : StylePreview::kPlotScales) {
        box->addItem(QStringLiteral("1:%1").arg(denominator), denominator);
    }
    box->setCurrentIndex(box->findData(500));
    box->setToolTip("The plot scale the preview draws at: a paper linestyle keeps its printed "
                    "size, a world one shrinks as the scale grows");
    return box;
}

// ---- the proxies -------------------------------------------------------------------

class StyleProxy final : public QSortFilterProxyModel {
  public:
    StyleProxy(const RowTableModel<StyleRow>& rows, QObject* parent)
        : QSortFilterProxyModel(parent), rows_(rows)
    {
        setSortRole(kSortRole);
        setSortCaseSensitivity(Qt::CaseInsensitive);
    }
    void setFilter(katana::cad::StyleFilter filter, std::string search)
    {
        beginFilterChange();
        filter_ = filter;
        search_ = std::move(search);
        endFilterChange(QSortFilterProxyModel::Direction::Rows);
    }
    [[nodiscard]] bool filtering() const
    {
        return filter_ != katana::cad::StyleFilter::All || !search_.empty();
    }

  protected:
    [[nodiscard]] bool filterAcceptsRow(int sourceRow, const QModelIndex&) const override
    {
        const StyleRow* row = rows_.rowAt(sourceRow);
        return row != nullptr && katana::cad::matchesFilter(*row, filter_) &&
               katana::cad::matchesSearch(*row, search_);
    }

  private:
    const RowTableModel<StyleRow>& rows_;
    katana::cad::StyleFilter filter_ = katana::cad::StyleFilter::All;
    std::string search_{};
};

// The Linetypes tab's chips, in order.
enum class LinetypeChip { All, Drawing, Library, Used, Unused };

class LinetypeProxy final : public QSortFilterProxyModel {
  public:
    LinetypeProxy(const RowTableModel<LinetypeRow>& rows, QObject* parent)
        : QSortFilterProxyModel(parent), rows_(rows)
    {
        setSortRole(kSortRole);
        setSortCaseSensitivity(Qt::CaseInsensitive);
    }
    void setFilter(LinetypeChip chip, std::string search, std::string group)
    {
        beginFilterChange();
        chip_ = chip;
        search_ = katana::core::lowered(search);
        group_ = std::move(group);
        endFilterChange(QSortFilterProxyModel::Direction::Rows);
    }
    [[nodiscard]] bool filtering() const
    {
        return chip_ != LinetypeChip::All || !search_.empty() || !group_.empty();
    }

  protected:
    [[nodiscard]] bool filterAcceptsRow(int sourceRow, const QModelIndex&) const override
    {
        const LinetypeRow* row = rows_.rowAt(sourceRow);
        if (row == nullptr) {
            return false;
        }
        switch (chip_) {
        case LinetypeChip::All:
            break;
        case LinetypeChip::Drawing:
            if (row->origin != LinetypeOrigin::Drawing) {
                return false;
            }
            break;
        case LinetypeChip::Library:
            if (row->origin != LinetypeOrigin::Library) {
                return false;
            }
            break;
        case LinetypeChip::Used:
            if (!row->users.used()) {
                return false;
            }
            break;
        case LinetypeChip::Unused:
            if (row->users.used()) {
                return false;
            }
            break;
        }
        // A group and everything under it: "Services" holds "Services/WATR".
        if (!group_.empty() && row->group != group_ &&
            !row->group.starts_with(group_ + "/")) {
            return false;
        }
        if (search_.empty()) {
            return true;
        }
        const auto has = [this](std::string_view text) {
            return katana::core::lowered(text).find(search_) != std::string::npos;
        };
        return has(row->name) || has(row->group) || has(row->sourceFile) ||
               has(row->description);
    }

  private:
    const RowTableModel<LinetypeRow>& rows_;
    LinetypeChip chip_ = LinetypeChip::All;
    std::string search_{};
    std::string group_{};
};

// One repeat of a pattern being edited, three times across: what Save would
// store, before it is stored. The StylePreview beside it draws the stored
// linetype as the drawing does; this draws the grid.
class PatternStrip final : public QWidget {
  public:
    explicit PatternStrip(QWidget* parent) : QWidget(parent)
    {
        setObjectName(QStringLiteral("patternStrip"));
        setMinimumHeight(28);
        setSizePolicy(QSizePolicy::Expanding, QSizePolicy::Fixed);
    }
    void setPattern(std::vector<double> lengths)
    {
        lengths_ = std::move(lengths);
        update();
    }

  protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        painter.setRenderHint(QPainter::Antialiasing, true);
        painter.fillRect(rect(), palette().base());
        QPen pen(palette().color(QPalette::Text), 2.0);
        pen.setCapStyle(Qt::FlatCap);
        painter.setPen(pen);
        const double y = height() / 2.0;
        const double left = 8.0;
        const double right = width() - 8.0;
        double period = 0.0;
        for (const double length : lengths_) {
            period += std::abs(length);
        }
        if (lengths_.empty() || !(period > 0.0) || right <= left) {
            painter.drawLine(QPointF(left, y), QPointF(right, y));
            return;
        }
        const double scale = (right - left) / (3.0 * period);
        double x = left;
        // A bound on the loop, not a limit anyone sees: three repeats of even
        // a long pattern are far fewer elements.
        for (int placed = 0; x < right && placed < 4096; ++placed) {
            const double length = lengths_[static_cast<std::size_t>(placed) % lengths_.size()];
            if (length > 0.0) {
                painter.drawLine(QPointF(x, y), QPointF(std::min(x + length * scale, right), y));
            } else if (length == 0.0) {
                painter.setBrush(pen.color());
                painter.drawEllipse(QPointF(x, y), 1.5, 1.5);
                painter.setBrush(Qt::NoBrush);
            }
            x += std::abs(length) * scale;
        }
    }

  private:
    std::vector<double> lengths_{};
};

// The standard colours a style colour menu offers, 12d's names for them.
struct NamedColour {
    const char* name;
    Color colour;
};
constexpr NamedColour kStandardColours[] = {
    {"red", {255, 0, 0, 255}},        {"yellow", {255, 255, 0, 255}},
    {"green", {0, 255, 0, 255}},      {"cyan", {0, 255, 255, 255}},
    {"blue", {0, 0, 255, 255}},       {"magenta", {255, 0, 255, 255}},
    {"white", {255, 255, 255, 255}},  {"black", {0, 0, 0, 255}},
    {"grey", {128, 128, 128, 255}},   {"orange", {255, 128, 0, 255}},
    {"brown", {150, 75, 0, 255}},     {"light grey", {192, 192, 192, 255}},
};

QIcon swatch(const QColor& colour)
{
    QPixmap pixmap(14, 14);
    pixmap.fill(colour);
    QPainter painter(&pixmap);
    painter.setPen(QColor(0x80, 0x80, 0x80));
    painter.drawRect(0, 0, 13, 13);
    return QIcon(pixmap);
}

} // namespace

// =====================================================================================

struct StyleManagerDialog::Impl {
    StyleManagerDialog* dialog = nullptr;
    CustomisationContext context{};
    // Used when the maker supplied no cache, so the tables still have pictures.
    std::unique_ptr<DefinitionThumbnails> ownThumbnails{};

    QTabWidget* tabs = nullptr;
    QLabel* statusLine = nullptr;

    // ---- styles
    RowTableModel<StyleRow>* styleModel = nullptr;
    StyleProxy* styleProxy = nullptr;
    QTableView* styleTable = nullptr;
    FilterBar* styleFilter = nullptr;
    QLabel* styleFormTitle = nullptr;
    NamePicker* linetypePicker = nullptr;
    NamePicker* symbolPicker = nullptr;
    QDoubleSpinBox* weightBox = nullptr;
    QToolButton* colourButton = nullptr;
    QCheckBox* byLayerColour = nullptr;
    QComboBox* hatchBox = nullptr;
    QDoubleSpinBox* symbolSizeBox = nullptr;
    QLineEdit* descriptionBox = nullptr;
    QPushButton* saveStyle = nullptr;
    QPushButton* revertStyle = nullptr;
    StylePreview* stylePreview = nullptr;
    QLabel* stylePreviewNote = nullptr;
    QPushButton* newStyle = nullptr;
    QPushButton* duplicateStyle = nullptr;
    QPushButton* renameStyle = nullptr;
    QPushButton* mergeStyles = nullptr;
    QPushButton* deleteStyles = nullptr;
    QPushButton* purgeStyles = nullptr;
    QPushButton* applyToSelection = nullptr;
    QPushButton* selectStyleUsers = nullptr;
    QPushButton* makeCurrent = nullptr;
    QString linetypePlaceholder{};
    QString symbolPlaceholder{};
    QColorDialog* colourDialog = nullptr;
    // The styles the form was loaded for, and what a person changed since.
    std::vector<std::string> formStyles{};
    StyleFields edited{};
    // The colour the form shows when it is not ByLayer.
    std::optional<Color> pickedColour{};

    // ---- linetypes
    RowTableModel<LinetypeRow>* linetypeModel = nullptr;
    LinetypeProxy* linetypeProxy = nullptr;
    QTableView* linetypeTable = nullptr;
    FilterBar* linetypeFilter = nullptr;
    QComboBox* groupBox = nullptr;
    QStackedWidget* linetypeDetail = nullptr;
    QLabel* noLinetype = nullptr;
    QWidget* patternPage = nullptr;
    QTableWidget* patternGrid = nullptr;
    QPushButton* addDash = nullptr;
    QPushButton* addGap = nullptr;
    QPushButton* addDot = nullptr;
    QPushButton* removeElement = nullptr;
    QPushButton* moveUp = nullptr;
    QPushButton* moveDown = nullptr;
    QLineEdit* linetypeDescription = nullptr;
    QLabel* patternStatus = nullptr;
    PatternStrip* patternStrip = nullptr;
    QPushButton* saveLinetype = nullptr;
    QPushButton* revertLinetype = nullptr;
    QWidget* libraryPage = nullptr;
    QLabel* libraryDetails = nullptr;
    QPushButton* newStyleUsing = nullptr;
    StylePreview* linetypePreview = nullptr;
    QPushButton* newLinetype = nullptr;
    QPushButton* duplicateLinetype = nullptr;
    QPushButton* renameLinetype = nullptr;
    QPushButton* mergeLinetypes = nullptr;
    QPushButton* deleteLinetypes = nullptr;
    QPushButton* purgeLinetypes = nullptr;
    QPushButton* selectLinetypeUsers = nullptr;
    // The drawing linetype the grid was loaded for, its elements as edited
    // (exact: a length nobody touched is the stored double, not what a spin
    // box rounded it to), and which parts a person changed.
    std::string patternFor{};
    std::vector<double> workingPattern{};
    bool patternEdited = false;
    bool descriptionEdited = false;
    bool patternValid = true;

    // ---- diagnostics
    RowTableModel<StyleDiagnostic>* diagnosticModel = nullptr;
    QTableView* diagnosticTable = nullptr;
    QLabel* diagnosticSummary = nullptr;
    QPushButton* selectDiagnosticUsers = nullptr;

    // ---- the prompt row and the purge panel, shared by the tabs
    QFrame* prompt = nullptr;
    QLabel* promptLabel = nullptr;
    QLineEdit* promptName = nullptr;
    QComboBox* promptChoice = nullptr;
    QPushButton* promptOk = nullptr;
    std::function<void(const std::string&)> promptAccept{};
    // The prompt asks for a choice from promptChoice rather than a typed name.
    bool promptIsChoice = false;
    QFrame* purgePanel = nullptr;
    QLabel* purgeLabel = nullptr;
    QListWidget* purgeList = nullptr;
    katana::cad::PurgeOptions purgeOptions{};

    QPushButton* undoButton = nullptr;
    QPushButton* redoButton = nullptr;

    // Set while the dialog changes its own tables and form, when Qt's signals
    // are not a person's doing.
    bool loading = false;
    // What the next reload selects, after a command this dialog ran.
    std::optional<std::vector<std::string>> selectStylesAfterReload{};
    std::optional<std::pair<std::string, LinetypeOrigin>> selectLinetypeAfterReload{};

    // The receiver of every connection to a child widget: destroyed with the
    // Impl, so no widget signal can reach a destroyed Impl while the dialog's
    // children are being deleted after it.
    std::unique_ptr<QObject> guard = std::make_unique<QObject>();
    // Last, so it goes first: no delivery reaches a half-destroyed Impl.
    std::unique_ptr<DocumentWatcher> watcher{};

    // ---------------------------------------------------------------------------------

    [[nodiscard]] bool alive() const { return watcher == nullptr || watcher->documentAlive(); }
    [[nodiscard]] katana::cad::Document& document() const { return *context.document; }
    [[nodiscard]] const katana::entity::Model& model() const { return document().model(); }

    void log(const QString& message, bool isError)
    {
        statusLine->setText(message);
        QPalette palette = statusLine->palette();
        palette.setColor(QPalette::WindowText, isError ? kErrorColour
                                                       : dialog->palette().color(
                                                             QPalette::WindowText));
        statusLine->setPalette(palette);
        if (context.log) {
            context.log(message, isError);
        }
    }

    // Runs a command and says what it did, or why not. The reload that shows
    // its effect comes from the watcher, once, from the event loop.
    bool run(cmd::CommandPtr command, const QString& done)
    {
        if (!alive()) {
            return false;
        }
        if (command == nullptr) {
            log("Nothing to do.", false);
            return false;
        }
        const auto status = document().execute(std::move(command));
        if (!status) {
            log(QString::fromStdString(status.error().describe()), true);
            return false;
        }
        log(done, false);
        return true;
    }

    // Several commands as one undo step; the one command itself when there is
    // only one.
    static cmd::CommandPtr together(std::string name, std::vector<cmd::CommandPtr> parts)
    {
        if (parts.size() == 1) {
            return std::move(parts.front());
        }
        auto transaction = std::make_unique<cmd::Transaction>(std::move(name));
        for (auto& part : parts) {
            transaction->add(std::move(part));
        }
        return transaction;
    }

    void connectGuarded(auto* sender, auto signal, auto&& slot)
    {
        QObject::connect(sender, signal, guard.get(), std::forward<decltype(slot)>(slot));
    }

    // ---- selection ----------------------------------------------------------------------

    [[nodiscard]] std::vector<int> selectedSourceRows(const QTableView* table,
                                                      const QSortFilterProxyModel* proxy) const
    {
        std::vector<int> rows;
        if (table->selectionModel() == nullptr) {
            return rows;
        }
        for (const QModelIndex& index : table->selectionModel()->selectedRows(0)) {
            rows.push_back(proxy != nullptr ? proxy->mapToSource(index).row() : index.row());
        }
        std::ranges::sort(rows);
        return rows;
    }

    [[nodiscard]] std::vector<std::string> selectedStyleNames() const
    {
        std::vector<std::string> names;
        for (const int row : selectedSourceRows(styleTable, styleProxy)) {
            if (const StyleRow* style = styleModel->rowAt(row)) {
                names.push_back(style->style.name);
            }
        }
        return names;
    }

    [[nodiscard]] std::vector<const LinetypeRow*> selectedLinetypeRows() const
    {
        std::vector<const LinetypeRow*> rows;
        for (const int row : selectedSourceRows(linetypeTable, linetypeProxy)) {
            if (const LinetypeRow* linetype = linetypeModel->rowAt(row)) {
                rows.push_back(linetype);
            }
        }
        return rows;
    }

    // The selected drawing linetypes' names; empty when a library row is
    // among the selection, since what acts on these acts on the model.
    [[nodiscard]] std::vector<std::string> selectedDrawingLinetypes() const
    {
        std::vector<std::string> names;
        for (const LinetypeRow* row : selectedLinetypeRows()) {
            if (row->origin != LinetypeOrigin::Drawing) {
                return {};
            }
            names.push_back(row->name);
        }
        return names;
    }

    // Selects the source rows `wanted` accepts, replacing the selection.
    template <typename Row>
    void selectWhere(QTableView* table, QSortFilterProxyModel* proxy,
                     const RowTableModel<Row>& rows,
                     const std::function<bool(const Row&)>& wanted)
    {
        QItemSelection selection;
        QModelIndex first;
        for (std::size_t i = 0; i < rows.rows().size(); ++i) {
            if (!wanted(rows.rows()[i])) {
                continue;
            }
            const QModelIndex index =
                proxy->mapFromSource(rows.index(static_cast<int>(i), 0));
            if (!index.isValid()) {
                continue;
            }
            selection.select(index, index);
            if (!first.isValid() || index.row() < first.row()) {
                first = index;
            }
        }
        QItemSelectionModel* model = table->selectionModel();
        model->select(selection, QItemSelectionModel::ClearAndSelect |
                                     QItemSelectionModel::Rows);
        if (first.isValid()) {
            model->setCurrentIndex(first, QItemSelectionModel::NoUpdate);
            table->scrollTo(first);
        }
    }

    void selectStyleNames(const std::vector<std::string>& names)
    {
        const std::set<std::string, std::less<>> wanted(names.begin(), names.end());
        selectWhere<StyleRow>(styleTable, styleProxy, *styleModel, [&](const StyleRow& row) {
            return wanted.contains(row.style.name);
        });
    }

    void selectLinetypeRow(const std::string& name, LinetypeOrigin origin)
    {
        selectWhere<LinetypeRow>(linetypeTable, linetypeProxy, *linetypeModel,
                                 [&](const LinetypeRow& row) {
                                     return row.name == name && row.origin == origin;
                                 });
    }

    // ---- reload ---------------------------------------------------------------------------

    void onDocumentChanged(const DocumentChanges& changes)
    {
        if (!alive()) {
            return;
        }
        if (changes.model || changes.library || changes.surveyMap || changes.current) {
            reload(changes.library || changes.surveyMap || changes.model);
        }
        updateButtons();
    }

    // Rebuilds the three tables from the Document, keeping what was selected.
    void reload(bool refreshPickers)
    {
        const std::vector<std::string> styles =
            selectStylesAfterReload.value_or(selectedStyleNames());
        std::optional<std::pair<std::string, LinetypeOrigin>> linetype =
            selectLinetypeAfterReload;
        if (!linetype) {
            const auto rows = selectedLinetypeRows();
            if (rows.size() == 1) {
                linetype = std::make_pair(rows.front()->name, rows.front()->origin);
            }
        }
        std::vector<std::pair<katana::cad::StyleDiagnosticKind, std::string>> diagnostics;
        for (const int row : selectedSourceRows(diagnosticTable, nullptr)) {
            if (const StyleDiagnostic* diagnostic = diagnosticModel->rowAt(row)) {
                diagnostics.emplace_back(diagnostic->kind, diagnostic->name);
            }
        }
        selectStylesAfterReload.reset();
        selectLinetypeAfterReload.reset();

        loading = true;
        styleModel->setRows(katana::cad::styleRows(document()));
        linetypeModel->setRows(katana::cad::linetypeRows(document()));
        diagnosticModel->setRows(katana::cad::styleDiagnostics(document()));
        fillGroups();
        fillHatches();
        if (refreshPickers) {
            linetypePicker->refresh();
            symbolPicker->refresh();
        }
        if (!styles.empty() && styleProxy->filtering()) {
            // A style asked for but filtered out is shown by clearing the
            // filter: selecting what cannot be seen would edit it unseen.
            const bool hidden = std::ranges::any_of(styles, [this](const std::string& name) {
                const int row = styleModel->indexOf(
                    [&](const StyleRow& candidate) { return candidate.style.name == name; });
                return row >= 0 &&
                       !styleProxy->mapFromSource(styleModel->index(row, 0)).isValid();
            });
            if (hidden) {
                styleFilter->setText({});
                styleFilter->setChip(0);
            }
        }
        selectStyleNames(styles);
        if (linetype) {
            selectLinetypeRow(linetype->first, linetype->second);
        }
        if (!diagnostics.empty()) {
            QItemSelection selection;
            for (std::size_t i = 0; i < diagnosticModel->rows().size(); ++i) {
                const StyleDiagnostic& row = diagnosticModel->rows()[i];
                if (std::ranges::find(diagnostics, std::make_pair(row.kind, row.name)) !=
                    diagnostics.end()) {
                    const QModelIndex index = diagnosticModel->index(static_cast<int>(i), 0);
                    selection.select(index, index);
                }
            }
            diagnosticTable->selectionModel()->select(
                selection, QItemSelectionModel::ClearAndSelect | QItemSelectionModel::Rows);
        }
        loading = false;

        // The form follows the selection - unless a person is part-way
        // through editing the very styles still selected, whose edits a
        // reload caused elsewhere must not throw away.
        if (selectedStyleNames() != formStyles || edited.empty()) {
            loadStyleForm();
        } else {
            refreshStylePreview();
        }
        const auto rows = selectedLinetypeRows();
        const bool sameLinetype = rows.size() == 1 &&
                                  rows.front()->origin == LinetypeOrigin::Drawing &&
                                  rows.front()->name == patternFor;
        if (!sameLinetype || !(patternEdited || descriptionEdited)) {
            loadLinetypeForm();
        } else {
            refreshLinetypePreview();
        }
        updateCounts();
        updateButtons();
    }

    void updateCounts()
    {
        std::size_t used = 0;
        std::size_t missing = 0;
        for (const StyleRow& row : styleModel->rows()) {
            used += row.entities > 0 ? 1 : 0;
            missing += row.missing() ? 1 : 0;
        }
        const std::size_t total = styleModel->rows().size();
        styleFilter->setChipLabel(0, QStringLiteral("All (%1)").arg(total));
        styleFilter->setChipLabel(1, QStringLiteral("Used (%1)").arg(used));
        styleFilter->setChipLabel(2, QStringLiteral("Unused (%1)").arg(total - used));
        styleFilter->setChipLabel(3, QStringLiteral("Missing (%1)").arg(missing));

        std::size_t drawing = 0;
        std::size_t named = 0;
        for (const LinetypeRow& row : linetypeModel->rows()) {
            drawing += row.origin == LinetypeOrigin::Drawing ? 1 : 0;
            named += row.users.used() ? 1 : 0;
        }
        const std::size_t linetypes = linetypeModel->rows().size();
        linetypeFilter->setChipLabel(0, QStringLiteral("All (%1)").arg(linetypes));
        linetypeFilter->setChipLabel(1, QStringLiteral("Drawing (%1)").arg(drawing));
        linetypeFilter->setChipLabel(2, QStringLiteral("Library (%1)").arg(linetypes - drawing));
        linetypeFilter->setChipLabel(3, QStringLiteral("Used (%1)").arg(named));
        linetypeFilter->setChipLabel(4, QStringLiteral("Unused (%1)").arg(linetypes - named));

        std::size_t missingNames = 0;
        std::size_t collisions = 0;
        for (const StyleDiagnostic& row : diagnosticModel->rows()) {
            (row.kind == katana::cad::StyleDiagnosticKind::Collision ? collisions
                                                                      : missingNames) += 1;
        }
        const int page = tabs->indexOf(diagnosticTable->parentWidget());
        tabs->setTabText(page, missingNames + collisions == 0
                                   ? QStringLiteral("Diagnostics")
                                   : QStringLiteral("Diagnostics (%1)")
                                         .arg(missingNames + collisions));
        if (missingNames + collisions == 0) {
            diagnosticSummary->setText(
                "No problems: every linetype and symbol a style or layer names is defined, and "
                "no drawing linetype shares its name with a library linestyle.");
        } else {
            diagnosticSummary->setText(
                QStringLiteral("%1 name(s) that nothing defines - drawn by a fallback until a "
                               "library defining them is loaded or the style is changed - and "
                               "%2 drawing linetype(s) a library linestyle of the same name "
                               "shadows (the library's is what is drawn).")
                    .arg(missingNames)
                    .arg(collisions));
        }
    }

    void fillGroups()
    {
        const QString was = groupBox->currentData().toString();
        const QSignalBlocker quiet(groupBox);
        groupBox->clear();
        groupBox->addItem(QStringLiteral("All groups"), QString());
        for (const std::string& group : katana::entity::styleGroups(document().styleLibrary())) {
            groupBox->addItem(fromName(group), fromName(group));
        }
        const int row = groupBox->findData(was);
        groupBox->setCurrentIndex(row >= 0 ? row : 0);
    }

    void fillHatches()
    {
        std::vector<kept::Choice> choices{{QStringLiteral("(ByLayer)"), std::string()}};
        for (const std::string& name : model().hatchPatterns.names()) {
            choices.push_back({fromName(name), name});
        }
        const std::optional<std::string> was = kept::current(hatchBox);
        const bool varies = hatchBox->count() > 0 && !was.has_value();
        kept::fill(hatchBox, choices);
        if (varies) {
            kept::showVaries(hatchBox);
        } else {
            kept::show(hatchBox, was.value_or(std::string()));
        }
    }

    // ---- the style form ---------------------------------------------------------------------

    [[nodiscard]] std::vector<Style> stylesNamed(const std::vector<std::string>& names) const
    {
        std::vector<Style> styles;
        for (const std::string& name : names) {
            if (const Style* style = model().styles.find(name)) {
                styles.push_back(*style);
            }
        }
        return styles;
    }

    void showPicker(NamePicker* picker, const std::optional<std::string>& value,
                    const QString& placeholder)
    {
        picker->lineEdit()->setPlaceholderText(value ? placeholder : kept::kVaries);
        picker->setCurrentName(value.value_or(std::string()));
    }

    void showColour()
    {
        if (!byLayerColour->isTristate() || byLayerColour->checkState() != Qt::PartiallyChecked) {
            const bool byLayer = byLayerColour->isChecked();
            const QColor shown = pickedColour ? toQColor(*pickedColour) : QColor(Qt::white);
            colourButton->setIcon(swatch(byLayer ? QColor(Qt::transparent) : shown));
            colourButton->setText(byLayer ? QStringLiteral("ByLayer")
                                          : QString::fromStdString(fromQColor(shown).toHex()));
            colourButton->setEnabled(!byLayer);
        } else {
            colourButton->setIcon(QIcon());
            colourButton->setText(kept::kVaries);
            colourButton->setEnabled(true);
        }
    }

    void loadStyleForm()
    {
        formStyles = selectedStyleNames();
        edited = {};
        const std::vector<Style> styles = stylesNamed(formStyles);
        const StyleFields common = katana::cad::commonFields(styles);
        const bool any = !styles.empty();

        loading = true;
        for (QWidget* field :
             std::initializer_list<QWidget*>{linetypePicker, symbolPicker, weightBox, colourButton,
                                             byLayerColour, hatchBox, symbolSizeBox,
                                             descriptionBox}) {
            field->setEnabled(any);
        }
        if (!any) {
            styleFormTitle->setText("Select a style to edit it, or several to edit them "
                                    "together.");
        } else if (styles.size() == 1) {
            styleFormTitle->setText(QStringLiteral("<b>%1</b>")
                                        .arg(fromName(styles.front().name).toHtmlEscaped()));
        } else {
            styleFormTitle->setText(
                QStringLiteral("<b>%1 styles</b> - a field showing &lt;varies&gt; differs "
                               "between them and is left as each has it unless you change "
                               "it.")
                    .arg(styles.size()));
        }
        showPicker(linetypePicker, any ? common.linetype : std::optional<std::string>(""),
                   linetypePlaceholder);
        showPicker(symbolPicker, any ? common.symbol : std::optional<std::string>(""),
                   symbolPlaceholder);
        showNumber(weightBox, any ? common.lineWeight : std::optional<double>(0.25), 0.0);
        showNumber(symbolSizeBox, any ? common.symbolSize : std::optional<double>(0.0), 0.0);
        {
            const QSignalBlocker quiet(byLayerColour);
            if (!any || common.color) {
                const std::optional<Color> colour = any ? *common.color : std::nullopt;
                byLayerColour->setTristate(false);
                byLayerColour->setChecked(!colour.has_value());
                pickedColour = colour;
            } else {
                byLayerColour->setTristate(true);
                byLayerColour->setCheckState(Qt::PartiallyChecked);
                pickedColour.reset();
            }
        }
        showColour();
        if (any && !common.hatchPattern) {
            kept::showVaries(hatchBox);
        } else {
            kept::show(hatchBox, any ? *common.hatchPattern : std::string());
        }
        {
            const QSignalBlocker quiet(descriptionBox);
            descriptionBox->setText(any && common.description ? fromName(*common.description)
                                                              : QString());
            descriptionBox->setPlaceholderText(any && !common.description ? kept::kVaries
                                                                          : QString());
        }
        loading = false;
        refreshStylePreview();
        updateButtons();
    }

    // A person changed a field of the form.
    void afterEdit()
    {
        refreshStylePreview();
        updateButtons();
    }

    void refreshStylePreview()
    {
        const std::vector<Style> styles = stylesNamed(formStyles);
        if (styles.empty()) {
            stylePreview->clear();
            stylePreviewNote->setText({});
            return;
        }
        // What Save would store, before it is stored: the first style with
        // the edits written over it.
        const Style shown = katana::cad::applyEdit(styles.front(), edited);
        stylePreview->setStyle(shown);
        QString note = styles.size() > 1
                           ? QStringLiteral("Showing %1, the first of %2.")
                                 .arg(fromName(shown.name))
                                 .arg(styles.size())
                           : QString();
        if (katana::entity::isByLayer(shown.linetype)) {
            note += (note.isEmpty() ? "" : " ") +
                    QStringLiteral("ByLayer: drawn in each layer's linetype; shown plain.");
        }
        stylePreviewNote->setText(note);
    }

    void saveStyles()
    {
        if (!alive() || formStyles.empty()) {
            return;
        }
        auto command = katana::cad::editStylesCommand(model(), formStyles, edited);
        if (!command) {
            log(QString::fromStdString(command.error().describe()), true);
            return;
        }
        if (*command == nullptr) {
            // Not an edit: nothing is pushed on the undo stack (QT-02).
            log("Nothing changed: the style is as it was.", false);
            edited = {};
            updateButtons();
            return;
        }
        const QString what = formStyles.size() == 1
                                 ? QStringLiteral("Style %1 updated.").arg(fromName(formStyles[0]))
                                 : QStringLiteral("%1 styles updated as one step.")
                                       .arg(formStyles.size());
        if (run(std::move(*command), what)) {
            edited = {};
            selectStylesAfterReload = formStyles;
        }
    }

    // ---- the linetype form -------------------------------------------------------------------

    void loadLinetypeForm()
    {
        patternEdited = false;
        descriptionEdited = false;
        patternFor.clear();
        const auto rows = selectedLinetypeRows();
        if (rows.size() != 1) {
            noLinetype->setText(rows.empty() ? QStringLiteral("Select a linetype.")
                                             : QStringLiteral("%1 selected.").arg(rows.size()));
            linetypeDetail->setCurrentWidget(noLinetype);
            refreshLinetypePreview();
            return;
        }
        const LinetypeRow& row = *rows.front();
        if (row.origin == LinetypeOrigin::Library) {
            showLibraryDetails(row);
            linetypeDetail->setCurrentWidget(libraryPage);
            refreshLinetypePreview();
            return;
        }
        const Linetype* linetype = model().linetypes.find(row.name);
        if (linetype == nullptr) {
            linetypeDetail->setCurrentWidget(noLinetype);
            return;
        }
        patternFor = linetype->name;
        workingPattern.clear();
        for (const auto& element : linetype->pattern) {
            workingPattern.push_back(element.length);
        }
        {
            const QSignalBlocker quiet(linetypeDescription);
            linetypeDescription->setText(fromName(linetype->description));
        }
        // "continuous" is what an unset chain resolves to: the table refuses
        // it a pattern, and the grid says so rather than failing on Save.
        const bool protectedItem = linetype->name == katana::entity::kContinuousLinetype;
        for (QWidget* part : std::initializer_list<QWidget*>{patternGrid, addDash, addGap,
                                                            addDot, removeElement, moveUp,
                                                            moveDown}) {
            part->setEnabled(!protectedItem);
        }
        rebuildGrid();
        linetypeDetail->setCurrentWidget(patternPage);
        if (protectedItem) {
            patternStatus->setText("continuous is the plain line everything falls back to: its "
                                   "pattern cannot change.");
        }
        refreshLinetypePreview();
    }

    void showLibraryDetails(const LinetypeRow& row)
    {
        const katana::entity::LineStyle* definition =
            document().styleLibrary().find(row.name);
        QString text = QStringLiteral("<b>%1</b><br>").arg(fromName(row.name).toHtmlEscaped());
        text += QStringLiteral("Group: %1<br>")
                    .arg(row.group.empty() ? QStringLiteral("(none)")
                                           : fromName(row.group).toHtmlEscaped());
        text += QStringLiteral("File: %1<br>")
                    .arg(row.sourceFile.empty() ? QStringLiteral("(made in this session)")
                                                : fromName(row.sourceFile).toHtmlEscaped());
        text += QStringLiteral("Kind: %1<br>").arg(kindExplained(row.kind).toHtmlEscaped());
        text += QStringLiteral("One repeat: %1<br>").arg(periodText(row));
        if (definition != nullptr) {
            text += QStringLiteral("%1 stroke(s), %2 text(s)<br>")
                        .arg(definition->strokes.size())
                        .arg(definition->texts.size());
        }
        text += QStringLiteral("Used by: %1<br>")
                    .arg(row.users.used() ? fromName(row.users.describe()).toHtmlEscaped()
                                          : QStringLiteral("nothing"));
        if (row.collision) {
            text += QStringLiteral("<br>The drawing also has a linetype of this name. This "
                                   "linestyle is what is drawn; rename or merge the drawing's "
                                   "to end the clash.<br>");
        }
        text += QStringLiteral("<br><i>Library linestyles belong to the session's 12d "
                               "customisation and are read-only here.</i>");
        libraryDetails->setText(text);
    }

    void refreshLinetypePreview()
    {
        const auto rows = selectedLinetypeRows();
        if (rows.size() != 1) {
            linetypePreview->clear();
            return;
        }
        if (rows.front()->origin == LinetypeOrigin::Library) {
            linetypePreview->setLinestyle(rows.front()->name);
            return;
        }
        // The stored linetype as the drawing draws it - through the same
        // resolver, so a shadowed name (D2) shows the library's strokes.
        Style sample;
        sample.name = rows.front()->name;
        sample.linetype = rows.front()->name;
        linetypePreview->setStyle(sample);
    }

    [[nodiscard]] Linetype linetypeFromForm() const
    {
        Linetype linetype;
        if (const Linetype* stored = model().linetypes.find(patternFor)) {
            linetype = *stored;
        }
        if (patternEdited) {
            linetype.pattern.clear();
            for (const double length : workingPattern) {
                linetype.pattern.push_back(katana::entity::LinetypeElement{length});
            }
        }
        if (descriptionEdited) {
            linetype.description = toName(linetypeDescription->text());
        }
        return linetype;
    }

    enum class Element { Dash, Gap, Dot };

    static Element elementOf(double length)
    {
        return length > 0.0 ? Element::Dash : length < 0.0 ? Element::Gap : Element::Dot;
    }

    void rebuildGrid()
    {
        loading = true;
        const int current = patternGrid->currentRow();
        patternGrid->setRowCount(static_cast<int>(workingPattern.size()));
        for (int row = 0; row < patternGrid->rowCount(); ++row) {
            const double length = workingPattern[static_cast<std::size_t>(row)];
            auto* kind = new QComboBox(patternGrid);
            kind->setObjectName(QStringLiteral("patternKind%1").arg(row));
            kind->addItems({"Dash", "Gap", "Dot"});
            kind->setCurrentIndex(static_cast<int>(elementOf(length)));
            auto* size = new QDoubleSpinBox(patternGrid);
            size->setObjectName(QStringLiteral("patternLength%1").arg(row));
            size->setDecimals(4);
            size->setRange(0.0, 1.0e6);
            size->setSingleStep(0.1);
            size->setSuffix(" m");
            size->setValue(std::abs(length));
            size->setEnabled(length != 0.0);
            connectGuarded(kind, &QComboBox::activated, [this, row](int choice) {
                if (loading) {
                    return;
                }
                changeElement(row, static_cast<Element>(choice));
            });
            connectGuarded(size, &QDoubleSpinBox::valueChanged, [this, row](double value) {
                if (loading) {
                    return;
                }
                double& element = workingPattern[static_cast<std::size_t>(row)];
                element = element < 0.0 ? -value : value;
                patternChanged();
            });
            patternGrid->setCellWidget(row, 0, kind);
            patternGrid->setCellWidget(row, 1, size);
        }
        if (patternGrid->rowCount() > 0) {
            patternGrid->setCurrentCell(std::clamp(current, 0, patternGrid->rowCount() - 1), 0);
        }
        loading = false;
        validatePattern();
    }

    void changeElement(int row, Element element)
    {
        double& length = workingPattern[static_cast<std::size_t>(row)];
        // Keep the size a person gave when only the kind changes; a dot
        // becoming a dash or a gap needs one, and half a metre is visible.
        const double size = std::abs(length) > 0.0 ? std::abs(length) : 0.5;
        length = element == Element::Dash ? size : element == Element::Gap ? -size : 0.0;
        patternChanged();
        rebuildGrid();
    }

    void insertElement(double length)
    {
        const int at = patternGrid->currentRow() < 0 ? static_cast<int>(workingPattern.size())
                                                     : patternGrid->currentRow() + 1;
        workingPattern.insert(workingPattern.begin() + at, length);
        patternChanged();
        rebuildGrid();
        patternGrid->setCurrentCell(at, 0);
    }

    void patternChanged()
    {
        patternEdited = true;
        validatePattern();
    }

    void validatePattern()
    {
        Linetype linetype;
        linetype.name = patternFor;
        for (const double length : workingPattern) {
            linetype.pattern.push_back(katana::entity::LinetypeElement{length});
        }
        const auto valid = katana::entity::validate(linetype);
        patternValid = valid.ok();
        QPalette palette = patternStatus->palette();
        if (valid) {
            patternStatus->setText(
                linetype.pattern.empty()
                    ? QStringLiteral("No elements: a continuous line.")
                    : QStringLiteral("Valid. One repeat is %1 m.")
                          .arg(number(linetype.patternLength())));
            palette.setColor(QPalette::WindowText, dialog->palette().color(QPalette::WindowText));
        } else {
            patternStatus->setText(QString::fromStdString(valid.error().message));
            palette.setColor(QPalette::WindowText, kErrorColour);
        }
        patternStatus->setPalette(palette);
        patternStrip->setPattern(workingPattern);
        updateButtons();
    }

    void saveLinetypeForm()
    {
        if (!alive() || patternFor.empty() || !patternValid) {
            return;
        }
        cmd::CommandPtr command = cmd::updateLinetypeIfChanged(model(), linetypeFromForm());
        if (command == nullptr) {
            log("Nothing changed: the linetype is as it was.", false);
            patternEdited = descriptionEdited = false;
            updateButtons();
            return;
        }
        const std::string name = patternFor;
        if (run(std::move(command), QStringLiteral("Linetype %1 updated.").arg(fromName(name)))) {
            patternEdited = descriptionEdited = false;
            selectLinetypeAfterReload = std::make_pair(name, LinetypeOrigin::Drawing);
        }
    }

    // ---- the prompt row and the purge panel -----------------------------------------------

    void askName(const QString& question, const QString& suggestion,
                 std::function<void(const std::string&)> accept)
    {
        purgePanel->hide();
        promptLabel->setText(question);
        promptChoice->hide();
        promptName->show();
        promptName->setText(suggestion);
        promptName->selectAll();
        promptIsChoice = false;
        promptAccept = std::move(accept);
        prompt->show();
        promptName->setFocus();
    }

    void askChoice(const QString& question, const std::vector<std::string>& choices,
                   std::function<void(const std::string&)> accept)
    {
        purgePanel->hide();
        promptLabel->setText(question);
        promptName->hide();
        promptChoice->clear();
        for (const std::string& choice : choices) {
            promptChoice->addItem(fromName(choice), kept::bytes(choice));
        }
        promptChoice->show();
        promptIsChoice = true;
        promptAccept = std::move(accept);
        prompt->show();
        promptChoice->setFocus();
    }

    void acceptPrompt()
    {
        std::string answer;
        if (promptIsChoice) {
            const QByteArray chosen = promptChoice->currentData().toByteArray();
            answer.assign(chosen.constData(), static_cast<std::size_t>(chosen.size()));
        } else {
            // Names are kept byte for byte, but blanks either end of a typed
            // name are never meant (and a style name may not start with one).
            answer = std::string(katana::core::trimmed(toName(promptName->text())));
        }
        if (answer.empty()) {
            log("Give a name first.", true);
            return;
        }
        prompt->hide();
        auto accept = std::move(promptAccept);
        promptAccept = nullptr;
        if (accept) {
            accept(answer);
        }
    }

    void showPurge(katana::cad::PurgeOptions options, const QString& title)
    {
        if (!alive()) {
            return;
        }
        prompt->hide();
        if (!document().currentStyle().empty()) {
            options.keepStyles.push_back(document().currentStyle());
        }
        const katana::commands::TableItems plan = katana::cad::planPurge(model(), options);
        if (plan.empty()) {
            log("Nothing is unused: there is nothing to purge.", false);
            purgePanel->hide();
            return;
        }
        purgeOptions = std::move(options);
        purgeLabel->setText(title);
        purgeList->clear();
        const auto add = [this](const QString& kind, const std::string& name, int table) {
            auto* item = new QListWidgetItem(QStringLiteral("%1  %2").arg(kind, fromName(name)),
                                             purgeList);
            item->setFlags(Qt::ItemIsEnabled | Qt::ItemIsUserCheckable);
            item->setCheckState(Qt::Checked);
            item->setData(Qt::UserRole, kept::bytes(name));
            item->setData(Qt::UserRole + 1, table);
        };
        for (const std::string& name : plan.styles) {
            add(QStringLiteral("Style"), name, 0);
        }
        for (const std::string& name : plan.linetypes) {
            add(QStringLiteral("Linetype"), name, 1);
        }
        for (const std::string& name : plan.hatchPatterns) {
            add(QStringLiteral("Hatch pattern"), name, 2);
        }
        purgePanel->show();
    }

    void acceptPurge()
    {
        if (!alive()) {
            return;
        }
        katana::commands::TableItems checked;
        katana::cad::PurgeOptions options = purgeOptions;
        for (int row = 0; row < purgeList->count(); ++row) {
            const QListWidgetItem* item = purgeList->item(row);
            const QByteArray bytes = item->data(Qt::UserRole).toByteArray();
            std::string name(bytes.constData(), static_cast<std::size_t>(bytes.size()));
            const int table = item->data(Qt::UserRole + 1).toInt();
            if (item->checkState() != Qt::Checked) {
                // A style kept keeps what it names: planned again below.
                if (table == 0) {
                    options.keepStyles.push_back(std::move(name));
                }
                continue;
            }
            (table == 0   ? checked.styles
             : table == 1 ? checked.linetypes
                          : checked.hatchPatterns)
                .push_back(std::move(name));
        }
        // Planned again with the unchecked styles kept, so a linetype only a
        // kept style names is not in the set (the command would refuse it).
        const katana::commands::TableItems plan = katana::cad::planPurge(model(), options);
        const auto both = [](const std::vector<std::string>& a,
                             const std::vector<std::string>& b) {
            std::vector<std::string> out;
            for (const std::string& name : a) {
                if (std::ranges::find(b, name) != b.end()) {
                    out.push_back(name);
                }
            }
            return out;
        };
        katana::commands::TableItems items{both(checked.styles, plan.styles),
                                           both(checked.linetypes, plan.linetypes),
                                           both(checked.hatchPatterns, plan.hatchPatterns)};
        purgePanel->hide();
        if (items.empty()) {
            log("Nothing was checked: nothing purged.", false);
            return;
        }
        const std::size_t count = items.size();
        run(katana::commands::purgeTableItems(std::move(items)),
            QStringLiteral("Purged %1 unused item(s) as one step.").arg(count));
    }

    // ---- buttons ------------------------------------------------------------------------------

    void updateButtons()
    {
        if (!alive()) {
            for (QPushButton* each : dialog->findChildren<QPushButton*>()) {
                each->setEnabled(false);
            }
            return;
        }
        const std::size_t styles = selectedStyleNames().size();
        duplicateStyle->setEnabled(styles == 1);
        renameStyle->setEnabled(styles == 1);
        mergeStyles->setEnabled(styles >= 1 && model().styles.size() > styles);
        deleteStyles->setEnabled(styles >= 1);
        selectStyleUsers->setEnabled(styles >= 1);
        makeCurrent->setEnabled(styles == 1);
        const bool isCurrent = styles == 1 && !document().currentStyle().empty() &&
                               selectedStyleNames().front() == document().currentStyle();
        makeCurrent->setText(isCurrent ? QStringLiteral("Draw ByLayer")
                                       : QStringLiteral("Make Current"));
        makeCurrent->setToolTip(isCurrent ? "New work stops using this style and is drawn "
                                            "ByLayer"
                                          : "Draw new work in this style");
        const std::size_t picked = document().selection().size();
        applyToSelection->setText(picked > 0
                                      ? QStringLiteral("Apply to Selection (%1)").arg(picked)
                                      : QStringLiteral("Apply to Selection"));
        applyToSelection->setEnabled(styles == 1 && picked > 0);
        // Save is offered whenever there is a style to save: saving one
        // nobody edited is allowed, and is no command at all (QT-02).
        const bool dirty = !edited.empty();
        saveStyle->setEnabled(styles >= 1);
        revertStyle->setEnabled(styles >= 1 && dirty);

        const auto rows = selectedLinetypeRows();
        const std::vector<std::string> drawing = selectedDrawingLinetypes();
        const bool anyContinuous =
            std::ranges::find(drawing, std::string(katana::entity::kContinuousLinetype)) !=
            drawing.end();
        duplicateLinetype->setEnabled(drawing.size() == 1);
        renameLinetype->setEnabled(drawing.size() == 1 && !anyContinuous);
        std::size_t drawingTotal = 0;
        for (const LinetypeRow& row : linetypeModel->rows()) {
            drawingTotal += row.origin == LinetypeOrigin::Drawing ? 1 : 0;
        }
        mergeLinetypes->setEnabled(!drawing.empty() && !anyContinuous &&
                                   drawingTotal > drawing.size());
        deleteLinetypes->setEnabled(!drawing.empty() && !anyContinuous);
        selectLinetypeUsers->setEnabled(!rows.empty());
        const bool linetypeDirty = patternEdited || descriptionEdited;
        saveLinetype->setEnabled(!patternFor.empty() && linetypeDirty && patternValid);
        revertLinetype->setEnabled(!patternFor.empty() && linetypeDirty);
        const int current = patternGrid->currentRow();
        removeElement->setEnabled(current >= 0 && patternGrid->isEnabled());
        moveUp->setEnabled(current > 0 && patternGrid->isEnabled());
        moveDown->setEnabled(current >= 0 && current + 1 < patternGrid->rowCount() &&
                             patternGrid->isEnabled());

        selectDiagnosticUsers->setEnabled(
            !selectedSourceRows(diagnosticTable, nullptr).empty());

        undoButton->setEnabled(document().history().canUndo());
        redoButton->setEnabled(document().history().canRedo());
    }

    void selectUsers(katana::cad::UsageTable table, const std::vector<std::string>& names)
    {
        if (!alive() || names.empty()) {
            return;
        }
        const std::vector<katana::entity::EntityId> ids =
            katana::cad::entitiesUsing(model(), table, names);
        if (ids.empty()) {
            log("No entity is drawn with that.", false);
            return;
        }
        if (context.selectAndShow) {
            context.selectAndShow(ids);
        } else {
            document().selection().set(ids);
            document().notifySelectionChanged();
        }
        log(QStringLiteral("%1 entit%2 selected.").arg(ids.size()).arg(ids.size() == 1 ? "y"
                                                                                       : "ies"),
            false);
    }

    // ---- building --------------------------------------------------------------------------

    QWidget* buildStylesPage();
    QWidget* buildLinetypesPage();
    QWidget* buildDiagnosticsPage();
    void buildPrompt(QWidget* parent);
    void buildPurge(QWidget* parent);
    void connectStyleForm();
    void connectStyleButtons();
    void connectLinetypePage();
    [[nodiscard]] QVariant styleThumbnail(const Style& style) const;
    [[nodiscard]] QVariant linestyleThumbnail(const std::string& name) const;

    // Clears every hook a child widget holds into this Impl: the children
    // outlive it by a moment (they are deleted by ~QWidget, after the Impl).
    void detach()
    {
        linetypePicker->onNameChosen = nullptr;
        symbolPicker->onNameChosen = nullptr;
        styleFilter->onTextChanged = nullptr;
        styleFilter->onChipChanged = nullptr;
        linetypeFilter->onTextChanged = nullptr;
        linetypeFilter->onChipChanged = nullptr;
    }
};

// ---- thumbnails -----------------------------------------------------------------------------

QVariant StyleManagerDialog::Impl::styleThumbnail(const Style& style) const
{
    if (!alive()) {
        return {};
    }
    const QColor ground = styleTable->palette().color(QPalette::Base);
    if (!style.symbol.empty()) {
        return context.thumbnails
            ->thumbnail(document(), ThumbnailKind::Symbol, style.symbol, QSize(18, 18), ground)
            .image;
    }
    const katana::entity::LineStyle* definition =
        document().styleLibrary().find(style.linetype);
    if (definition != nullptr && !definition->atVertices) {
        return context.thumbnails
            ->thumbnail(document(), ThumbnailKind::Linestyle, style.linetype, QSize(48, 16),
                        ground)
            .image;
    }
    return {};
}

QVariant StyleManagerDialog::Impl::linestyleThumbnail(const std::string& name) const
{
    if (!alive()) {
        return {};
    }
    return context.thumbnails
        ->thumbnail(document(), ThumbnailKind::Linestyle, name, QSize(48, 16),
                    linetypeTable->palette().color(QPalette::Base))
        .image;
}

// ---- the styles page ---------------------------------------------------------------------------

QWidget* StyleManagerDialog::Impl::buildStylesPage()
{
    auto* page = new QWidget(tabs);
    page->setObjectName(QStringLiteral("stylesPage"));
    auto* layout = new QVBoxLayout(page);

    styleFilter = new FilterBar({"All", "Used", "Unused", "Missing"}, page);
    styleFilter->setPlaceholderText("Search names, linetypes, symbols and descriptions");
    layout->addWidget(styleFilter);

    using Model = RowTableModel<StyleRow>;
    const auto amberWhen = [](bool missing, int role, const QString& why) -> QVariant {
        if (!missing) {
            return {};
        }
        if (role == Qt::ForegroundRole) {
            return QBrush(kUndefinedNameColour);
        }
        if (role == Qt::ToolTipRole) {
            return why;
        }
        return {};
    };
    std::vector<Model::Column> columns{
        {"Name",
         [](const StyleRow& row, int role) -> QVariant {
             switch (role) {
             case Qt::DisplayRole:
                 return row.current ? fromName(row.style.name) + QStringLiteral("  (current)")
                                    : fromName(row.style.name);
             case kSortRole:
                 return fromName(row.style.name);
             case Qt::FontRole: {
                 if (!row.current) {
                     return {};
                 }
                 QFont font;
                 font.setBold(true);
                 return font;
             }
             case Qt::ToolTipRole:
                 return row.current ? QStringLiteral("The current style: new work is drawn in it")
                                    : QVariant();
             default:
                 return {};
             }
         }},
        {"Preview",
         [this](const StyleRow& row, int role) -> QVariant {
             return role == Qt::DecorationRole ? styleThumbnail(row.style) : QVariant();
         }},
        {"Linetype",
         [amberWhen](const StyleRow& row, int role) -> QVariant {
             if (role == Qt::DisplayRole) {
                 return fromName(row.style.linetype);
             }
             return amberWhen(row.missingLinetype, role,
                              "Nothing defines this linetype: drawn as a solid line until a "
                              "library defining it is loaded");
         }},
        {"Weight",
         [](const StyleRow& row, int role) -> QVariant {
             switch (role) {
             case Qt::DisplayRole:
                 return QString::number(row.style.lineWeight, 'f', 2);
             case kSortRole:
                 return row.style.lineWeight;
             case Qt::TextAlignmentRole:
                 return static_cast<int>(Qt::AlignRight | Qt::AlignVCenter);
             default:
                 return {};
             }
         }},
        {"Colour",
         [](const StyleRow& row, int role) -> QVariant {
             switch (role) {
             case Qt::DisplayRole:
                 return row.style.color ? QString::fromStdString(row.style.color->toHex())
                                        : QStringLiteral("ByLayer");
             case Qt::DecorationRole:
                 return row.style.color ? QVariant(toQColor(*row.style.color)) : QVariant();
             default:
                 return {};
             }
         }},
        {"Hatch",
         [](const StyleRow& row, int role) -> QVariant {
             return role == Qt::DisplayRole ? (row.style.hatchPattern.empty()
                                                   ? QStringLiteral("ByLayer")
                                                   : fromName(row.style.hatchPattern))
                                            : QVariant();
         }},
        {"Symbol",
         [amberWhen](const StyleRow& row, int role) -> QVariant {
             if (role == Qt::DisplayRole) {
                 return fromName(row.style.symbol);
             }
             if (role == Qt::ToolTipRole && !row.missingSymbol && !row.style.symbol.empty()) {
                 return row.style.symbolSize > 0.0
                            ? QStringLiteral("%1 m wide").arg(number(row.style.symbolSize))
                            : QStringLiteral("at its own size");
             }
             return amberWhen(row.missingSymbol, role,
                              "Nothing defines this symbol: a built-in shape is drawn in its "
                              "place");
         }},
        {"Used",
         [](const StyleRow& row, int role) -> QVariant {
             switch (role) {
             case Qt::DisplayRole:
                 return QString::number(row.entities);
             case kSortRole:
                 return static_cast<qulonglong>(row.entities);
             case Qt::TextAlignmentRole:
                 return static_cast<int>(Qt::AlignRight | Qt::AlignVCenter);
             case Qt::ToolTipRole:
                 return QStringLiteral("Entities wearing this style");
             default:
                 return {};
             }
         }},
        Model::textColumn("Description",
                          [](const StyleRow& row) { return fromName(row.style.description); }),
    };
    styleModel = new Model(std::move(columns), page);
    styleProxy = new StyleProxy(*styleModel, page);
    styleProxy->setSourceModel(styleModel);

    auto* splitter = new QSplitter(Qt::Horizontal, page);
    styleTable = new QTableView(splitter);
    styleTable->setObjectName(QStringLiteral("styleTable"));
    styleTable->setModel(styleProxy);
    styleTable->setSelectionBehavior(QAbstractItemView::SelectRows);
    styleTable->setSelectionMode(QAbstractItemView::ExtendedSelection);
    styleTable->setEditTriggers(QAbstractItemView::NoEditTriggers);
    styleTable->setSortingEnabled(true);
    styleTable->sortByColumn(0, Qt::AscendingOrder);
    styleTable->setAlternatingRowColors(true);
    styleTable->setWordWrap(false);
    styleTable->verticalHeader()->setVisible(false);
    styleTable->verticalHeader()->setDefaultSectionSize(22);
    styleTable->horizontalHeader()->setStretchLastSection(true);
    styleTable->setIconSize(QSize(48, 16));
    styleTable->setColumnWidth(0, 200);
    styleTable->setColumnWidth(1, 60);
    styleTable->setColumnWidth(2, 170);

    auto* side = new QWidget(splitter);
    auto* sideLayout = new QVBoxLayout(side);
    sideLayout->setContentsMargins(0, 0, 0, 0);
    auto* formBox = new QGroupBox("Definition", side);
    auto* form = new QFormLayout(formBox);
    styleFormTitle = new QLabel(formBox);
    styleFormTitle->setObjectName(QStringLiteral("styleFormTitle"));
    styleFormTitle->setWordWrap(true);
    form->addRow(styleFormTitle);

    linetypePicker = new NamePicker(context, katana::cad::NameRole::Linetype, true, formBox);
    linetypePicker->setObjectName(QStringLiteral("styleLinetype"));
    linetypePlaceholder = linetypePicker->lineEdit()->placeholderText();
    form->addRow("Linetype", linetypePicker);
    weightBox = new QDoubleSpinBox(formBox);
    weightBox->setObjectName(QStringLiteral("styleWeight"));
    weightBox->setRange(0.0, 10.0);
    weightBox->setDecimals(3);
    weightBox->setSingleStep(0.05);
    weightBox->setSuffix(" mm on paper");
    form->addRow("Line weight", weightBox);
    auto* colourRow = new QWidget(formBox);
    auto* colourLayout = new QHBoxLayout(colourRow);
    colourLayout->setContentsMargins(0, 0, 0, 0);
    colourButton = new QToolButton(colourRow);
    colourButton->setObjectName(QStringLiteral("styleColour"));
    colourButton->setToolButtonStyle(Qt::ToolButtonTextBesideIcon);
    colourButton->setPopupMode(QToolButton::InstantPopup);
    colourButton->setMinimumWidth(120);
    byLayerColour = new QCheckBox("ByLayer", colourRow);
    byLayerColour->setObjectName(QStringLiteral("styleColourByLayer"));
    byLayerColour->setToolTip("Drawn in the colour of the layer it is on");
    colourLayout->addWidget(colourButton);
    colourLayout->addWidget(byLayerColour);
    colourLayout->addStretch(1);
    form->addRow("Colour", colourRow);
    hatchBox = new QComboBox(formBox);
    hatchBox->setObjectName(QStringLiteral("styleHatch"));
    form->addRow("Hatch", hatchBox);
    symbolPicker = new NamePicker(context, katana::cad::NameRole::Symbol, false, formBox);
    symbolPicker->setObjectName(QStringLiteral("styleSymbol"));
    symbolPlaceholder = symbolPicker->lineEdit()->placeholderText();
    form->addRow("Symbol", symbolPicker);
    symbolSizeBox = new QDoubleSpinBox(formBox);
    symbolSizeBox->setObjectName(QStringLiteral("styleSymbolSize"));
    symbolSizeBox->setRange(0.0, 10000.0);
    symbolSizeBox->setDecimals(4);
    symbolSizeBox->setSuffix(" m wide (0 = its own size)");
    form->addRow("Symbol size", symbolSizeBox);
    descriptionBox = new QLineEdit(formBox);
    descriptionBox->setObjectName(QStringLiteral("styleDescription"));
    form->addRow("Description", descriptionBox);
    auto* saveRow = new QWidget(formBox);
    auto* saveLayout = new QHBoxLayout(saveRow);
    saveLayout->setContentsMargins(0, 0, 0, 0);
    saveStyle = button("Save", "styleSave",
                       "Store the fields you changed; the others stay as each style has them",
                       saveRow);
    revertStyle = button("Revert", "styleRevert", "Forget the changes made in the form", saveRow);
    saveLayout->addStretch(1);
    saveLayout->addWidget(revertStyle);
    saveLayout->addWidget(saveStyle);
    form->addRow(saveRow);
    sideLayout->addWidget(formBox);

    auto* previewBox = new QGroupBox("Preview", side);
    auto* previewLayout = new QVBoxLayout(previewBox);
    stylePreview = new StylePreview(document(), previewBox);
    stylePreview->setObjectName(QStringLiteral("stylePreview"));
    stylePreview->setMinimumHeight(150);
    previewLayout->addWidget(stylePreview, 1);
    auto* previewControls = new QWidget(previewBox);
    auto* controlsLayout = new QHBoxLayout(previewControls);
    controlsLayout->setContentsMargins(0, 0, 0, 0);
    auto* scale = scaleCombo("stylePreviewScale", previewControls);
    auto* screen = new QCheckBox("Screen", previewControls);
    screen->setObjectName(QStringLiteral("stylePreviewScreen"));
    screen->setToolTip("Show it on the viewport's dark ground rather than on paper");
    stylePreviewNote = new QLabel(previewControls);
    stylePreviewNote->setObjectName(QStringLiteral("stylePreviewNote"));
    stylePreviewNote->setWordWrap(true);
    controlsLayout->addWidget(new QLabel("Plot scale", previewControls));
    controlsLayout->addWidget(scale);
    controlsLayout->addWidget(screen);
    controlsLayout->addWidget(stylePreviewNote, 1);
    previewLayout->addWidget(previewControls);
    sideLayout->addWidget(previewBox, 1);
    stylePreview->setScaleDenominator(scale->currentData().toInt());
    connectGuarded(scale, &QComboBox::currentIndexChanged, [this, scale](int) {
        stylePreview->setScaleDenominator(scale->currentData().toInt());
    });
    connectGuarded(screen, &QCheckBox::toggled, [this](bool onScreen) {
        stylePreview->setGround(onScreen ? PreviewGround::Screen : PreviewGround::Paper);
    });

    splitter->addWidget(styleTable);
    splitter->addWidget(side);
    splitter->setStretchFactor(0, 3);
    splitter->setStretchFactor(1, 2);
    layout->addWidget(splitter, 1);

    auto* buttons = new QWidget(page);
    auto* buttonLayout = new QHBoxLayout(buttons);
    buttonLayout->setContentsMargins(0, 0, 0, 0);
    newStyle = button("New...", "styleNew", "A new style, drawn plainly until you define it",
                      buttons);
    duplicateStyle = button("Duplicate...", "styleDuplicate", "A copy of the selected style",
                            buttons);
    renameStyle = button("Rename...", "styleRename",
                         "Rename it; every entity wearing it comes too, as one undo step",
                         buttons);
    mergeStyles = button("Merge Into...", "styleMerge",
                         "Move every entity wearing the selected styles onto another style, "
                         "then delete them: one undo step",
                         buttons);
    deleteStyles = button("Delete", "styleDelete",
                          "Refused while any entity wears it, saying how many and which first",
                          buttons);
    purgeStyles = button("Purge...", "stylePurge",
                         "Delete the styles, linetypes and hatch patterns nothing uses: one "
                         "undo step",
                         buttons);
    applyToSelection = button("Apply to Selection", "styleApplyToSelection",
                              "Give the entities selected in the drawing now this style",
                              buttons);
    selectStyleUsers = button("Select Users", "styleSelectUsers",
                              "Select the entities wearing the selected styles", buttons);
    makeCurrent = button("Make Current", "styleMakeCurrent", "Draw new work in this style",
                         buttons);
    for (QPushButton* each : {newStyle, duplicateStyle, renameStyle, mergeStyles, deleteStyles,
                              purgeStyles}) {
        buttonLayout->addWidget(each);
    }
    buttonLayout->addStretch(1);
    for (QPushButton* each : {applyToSelection, selectStyleUsers, makeCurrent}) {
        buttonLayout->addWidget(each);
    }
    layout->addWidget(buttons);
    return page;
}

void StyleManagerDialog::Impl::connectStyleForm()
{
    // The chips are in StyleFilter's order: All, Used, Unused, Missing.
    const auto refilter = [this] {
        styleProxy->setFilter(static_cast<katana::cad::StyleFilter>(styleFilter->chip()),
                              toName(styleFilter->text()));
    };
    styleFilter->onTextChanged = [refilter](const QString&) { refilter(); };
    styleFilter->onChipChanged = [refilter](int) { refilter(); };
    connectGuarded(styleTable->selectionModel(), &QItemSelectionModel::selectionChanged,
                   [this](const QItemSelection&, const QItemSelection&) {
                       if (loading) {
                           return;
                       }
                       loadStyleForm();
                   });

    linetypePicker->onNameChosen = [this](const std::string& name) {
        if (loading) {
            return;
        }
        edited.linetype = name;
        afterEdit();
    };
    symbolPicker->onNameChosen = [this](const std::string& name) {
        if (loading) {
            return;
        }
        edited.symbol = name;
        afterEdit();
    };
    connectGuarded(weightBox, &QDoubleSpinBox::valueChanged, [this](double) {
        if (loading) {
            return;
        }
        leaveVaries(weightBox, 0.0);
        edited.lineWeight = weightBox->value();
        afterEdit();
    });
    connectGuarded(symbolSizeBox, &QDoubleSpinBox::valueChanged, [this](double) {
        if (loading) {
            return;
        }
        leaveVaries(symbolSizeBox, 0.0);
        edited.symbolSize = symbolSizeBox->value();
        afterEdit();
    });
    connectGuarded(descriptionBox, &QLineEdit::textEdited, [this](const QString& text) {
        if (loading) {
            return;
        }
        descriptionBox->setPlaceholderText({});
        edited.description = toName(text);
        afterEdit();
    });
    connectGuarded(hatchBox, &QComboBox::activated, [this](int) {
        if (loading) {
            return;
        }
        // Choosing <varies> again means "leave each as it is".
        if (const auto name = kept::current(hatchBox)) {
            edited.hatchPattern = *name;
        } else {
            edited.hatchPattern.reset();
        }
        afterEdit();
    });
    connectGuarded(byLayerColour, &QCheckBox::clicked, [this](bool checked) {
        if (loading) {
            return;
        }
        // Out of <varies> for good once a person has said which.
        {
            const QSignalBlocker quiet(byLayerColour);
            byLayerColour->setTristate(false);
            byLayerColour->setChecked(checked);
        }
        if (checked) {
            edited.color = std::optional<Color>();
        } else {
            if (!pickedColour) {
                pickedColour = Color{255, 255, 255, 255};
            }
            edited.color = pickedColour;
        }
        showColour();
        afterEdit();
    });

    const auto choose = [this](const Color& colour) {
        pickedColour = colour;
        {
            const QSignalBlocker quiet(byLayerColour);
            byLayerColour->setTristate(false);
            byLayerColour->setChecked(false);
        }
        edited.color = colour;
        showColour();
        afterEdit();
    };
    auto* menu = new QMenu(colourButton);
    for (const NamedColour& named : kStandardColours) {
        QAction* action = menu->addAction(swatch(toQColor(named.colour)),
                                          QString::fromLatin1(named.name));
        const Color colour = named.colour;
        connectGuarded(action, &QAction::triggered, [choose, colour](bool) { choose(colour); });
    }
    menu->addSeparator();
    QAction* other = menu->addAction("Other...");
    connectGuarded(other, &QAction::triggered, [this, choose](bool) {
        // Window-modal but not blocking (open(), not exec()): the answer
        // arrives as a signal, and nothing waits on it.
        if (colourDialog == nullptr) {
            colourDialog = new QColorDialog(dialog);
            colourDialog->setObjectName(QStringLiteral("styleColourDialog"));
            connectGuarded(colourDialog, &QColorDialog::colorSelected,
                           [choose](const QColor& colour) {
                               if (colour.isValid()) {
                                   choose(fromQColor(colour));
                               }
                           });
        }
        colourDialog->setCurrentColor(pickedColour ? toQColor(*pickedColour) : QColor(Qt::white));
        colourDialog->open();
    });
    colourButton->setMenu(menu);

    connectGuarded(saveStyle, &QPushButton::clicked, [this] { saveStyles(); });
    connectGuarded(revertStyle, &QPushButton::clicked, [this] { loadStyleForm(); });
}

void StyleManagerDialog::Impl::connectStyleButtons()
{
    connectGuarded(newStyle, &QPushButton::clicked, [this] {
        askName("Name of the new style:",
                fromName(katana::cad::freeStyleName(model(), "New Style")),
                [this](const std::string& name) {
                    Style style;
                    style.name = name;
                    if (run(cmd::createStyle(style),
                            QStringLiteral("Style %1 created.").arg(fromName(name)))) {
                        selectStylesAfterReload = std::vector<std::string>{name};
                    }
                });
    });
    connectGuarded(duplicateStyle, &QPushButton::clicked, [this] {
        const auto names = selectedStyleNames();
        if (names.size() != 1) {
            return;
        }
        const std::string from = names.front();
        askName(QStringLiteral("Name of the copy of %1:").arg(fromName(from)),
                fromName(katana::cad::freeStyleName(model(), from)),
                [this, from](const std::string& to) {
                    if (run(cmd::duplicateStyle(from, to),
                            QStringLiteral("Style %1 copied as %2.")
                                .arg(fromName(from), fromName(to)))) {
                        selectStylesAfterReload = std::vector<std::string>{to};
                    }
                });
    });
    connectGuarded(renameStyle, &QPushButton::clicked, [this] {
        const auto names = selectedStyleNames();
        if (names.size() != 1) {
            return;
        }
        const std::string from = names.front();
        askName(QStringLiteral("New name for %1:").arg(fromName(from)), fromName(from),
                [this, from](const std::string& to) {
                    if (to == from) {
                        return;
                    }
                    if (run(cmd::renameStyle(from, to),
                            QStringLiteral("Style %1 renamed to %2; every entity wearing it "
                                           "came too.")
                                .arg(fromName(from), fromName(to)))) {
                        selectStylesAfterReload = std::vector<std::string>{to};
                    }
                });
    });
    connectGuarded(mergeStyles, &QPushButton::clicked, [this] {
        const auto names = selectedStyleNames();
        if (names.empty()) {
            return;
        }
        std::vector<std::string> targets;
        for (const std::string& name : model().styles.names()) {
            if (std::ranges::find(names, name) == names.end()) {
                targets.push_back(name);
            }
        }
        askChoice(names.size() == 1
                      ? QStringLiteral("Merge %1 into:").arg(fromName(names.front()))
                      : QStringLiteral("Merge the %1 selected styles into:").arg(names.size()),
                  targets, [this, names](const std::string& into) {
                      std::vector<cmd::CommandPtr> parts;
                      for (const std::string& from : names) {
                          parts.push_back(cmd::mergeStyle(from, into));
                      }
                      if (run(together("MergeStyles", std::move(parts)),
                              QStringLiteral("Merged %1 style(s) into %2; their entities wear "
                                             "it now.")
                                  .arg(names.size())
                                  .arg(fromName(into)))) {
                          selectStylesAfterReload = std::vector<std::string>{into};
                      }
                  });
    });
    connectGuarded(deleteStyles, &QPushButton::clicked, [this] {
        const auto names = selectedStyleNames();
        if (names.empty()) {
            return;
        }
        std::vector<cmd::CommandPtr> parts;
        for (const std::string& name : names) {
            parts.push_back(cmd::deleteStyle(name));
        }
        run(together("DeleteStyles", std::move(parts)),
            names.size() == 1 ? QStringLiteral("Style %1 deleted.").arg(fromName(names.front()))
                              : QStringLiteral("%1 styles deleted.").arg(names.size()));
    });
    connectGuarded(purgeStyles, &QPushButton::clicked, [this] {
        showPurge(katana::cad::PurgeOptions{},
                  "Unused styles, linetypes and hatch patterns - uncheck any to keep:");
    });
    connectGuarded(applyToSelection, &QPushButton::clicked, [this] {
        const auto names = selectedStyleNames();
        if (!alive() || names.size() != 1) {
            return;
        }
        // The selection as it is NOW, not as it was when the dialog opened:
        // the dialog may sit beside the drawing for as long as it likes.
        const auto ids = document().selection().ids();
        if (ids.empty()) {
            log("Select the entities to style in the drawing first.", true);
            return;
        }
        run(cmd::setEntityStyle(ids, names.front()),
            QStringLiteral("%1 entit%2 now wear%3 style %4.")
                .arg(ids.size())
                .arg(ids.size() == 1 ? "y" : "ies")
                .arg(ids.size() == 1 ? "s" : "")
                .arg(fromName(names.front())));
    });
    connectGuarded(selectStyleUsers, &QPushButton::clicked,
                   [this] { selectUsers(katana::cad::UsageTable::Style, selectedStyleNames()); });
    connectGuarded(makeCurrent, &QPushButton::clicked, [this] {
        const auto names = selectedStyleNames();
        if (!alive() || names.size() != 1) {
            return;
        }
        // Not a command: the current style is session state, as the current
        // layer is (decision D9).
        const bool clearing = document().currentStyle() == names.front();
        const std::string next = clearing ? std::string() : names.front();
        if (const auto status = document().setCurrentStyle(next); !status) {
            log(QString::fromStdString(status.error().describe()), true);
            return;
        }
        log(clearing ? QStringLiteral("New work is drawn ByLayer.")
                     : QStringLiteral("New work is drawn in style %1.").arg(fromName(next)),
            false);
    });
}

// ---- the linetypes page ---------------------------------------------------------------------

QWidget* StyleManagerDialog::Impl::buildLinetypesPage()
{
    auto* page = new QWidget(tabs);
    page->setObjectName(QStringLiteral("linetypesPage"));
    auto* layout = new QVBoxLayout(page);

    auto* filterRow = new QWidget(page);
    auto* filterLayout = new QHBoxLayout(filterRow);
    filterLayout->setContentsMargins(0, 0, 0, 0);
    linetypeFilter = new FilterBar({"All", "Drawing", "Library", "Used", "Unused"}, filterRow);
    linetypeFilter->setPlaceholderText("Search names, groups, files and descriptions");
    groupBox = new QComboBox(filterRow);
    groupBox->setObjectName(QStringLiteral("linetypeGroup"));
    groupBox->setToolTip("Library linestyles of one group and the groups under it");
    groupBox->setSizeAdjustPolicy(QComboBox::AdjustToContents);
    filterLayout->addWidget(linetypeFilter, 1);
    filterLayout->addWidget(groupBox);
    layout->addWidget(filterRow);

    using Model = RowTableModel<LinetypeRow>;
    std::vector<Model::Column> columns{
        {"Name",
         [this](const LinetypeRow& row, int role) -> QVariant {
             switch (role) {
             case Qt::DisplayRole:
             case kSortRole:
                 return fromName(row.name);
             case Qt::DecorationRole:
                 return row.origin == LinetypeOrigin::Library ? linestyleThumbnail(row.name)
                                                              : QVariant();
             case Qt::ForegroundRole:
                 return row.collision && row.origin == LinetypeOrigin::Drawing
                            ? QVariant(QBrush(kUndefinedNameColour))
                            : QVariant();
             case Qt::ToolTipRole:
                 return row.collision
                            ? QStringLiteral("Both a drawing linetype and a library linestyle: "
                                             "the library's is drawn")
                            : QVariant();
             default:
                 return {};
             }
         }},
        {"Source",
         [](const LinetypeRow& row, int role) -> QVariant {
             if (role != Qt::DisplayRole) {
                 return {};
             }
             if (row.origin == LinetypeOrigin::Drawing) {
                 return row.collision ? QStringLiteral("Drawing (shadowed)")
                                      : QStringLiteral("Drawing");
             }
             return QStringLiteral("Library");
         }},
        Model::textColumn("Group", [](const LinetypeRow& row) { return fromName(row.group); }),
        Model::textColumn("Kind", [](const LinetypeRow& row) { return kindText(row.kind); }),
        {"Period",
         [](const LinetypeRow& row, int role) -> QVariant {
             switch (role) {
             case Qt::DisplayRole:
                 return periodText(row);
             case kSortRole:
                 return row.period;
             case Qt::TextAlignmentRole:
                 return static_cast<int>(Qt::AlignRight | Qt::AlignVCenter);
             default:
                 return {};
             }
         }},
        {"Used",
         [](const LinetypeRow& row, int role) -> QVariant {
             switch (role) {
             case Qt::DisplayRole:
                 return QString::number(row.users.entities);
             case kSortRole:
                 return static_cast<qulonglong>(row.users.entities);
             case Qt::TextAlignmentRole:
                 return static_cast<int>(Qt::AlignRight | Qt::AlignVCenter);
             case Qt::ToolTipRole:
                 return row.users.used() ? fromName(row.users.describe())
                                         : QStringLiteral("Nothing uses it");
             default:
                 return {};
             }
         }},
        Model::textColumn("Description",
                          [](const LinetypeRow& row) {
                              return fromName(row.origin == LinetypeOrigin::Drawing
                                                  ? row.description
                                                  : row.sourceFile);
                          }),
    };
    linetypeModel = new Model(std::move(columns), page);
    linetypeProxy = new LinetypeProxy(*linetypeModel, page);
    linetypeProxy->setSourceModel(linetypeModel);

    auto* splitter = new QSplitter(Qt::Horizontal, page);
    linetypeTable = new QTableView(splitter);
    linetypeTable->setObjectName(QStringLiteral("linetypeTable"));
    linetypeTable->setModel(linetypeProxy);
    linetypeTable->setSelectionBehavior(QAbstractItemView::SelectRows);
    linetypeTable->setSelectionMode(QAbstractItemView::ExtendedSelection);
    linetypeTable->setEditTriggers(QAbstractItemView::NoEditTriggers);
    linetypeTable->setSortingEnabled(true);
    linetypeTable->sortByColumn(0, Qt::AscendingOrder);
    linetypeTable->setAlternatingRowColors(true);
    linetypeTable->setWordWrap(false);
    linetypeTable->verticalHeader()->setVisible(false);
    linetypeTable->verticalHeader()->setDefaultSectionSize(22);
    linetypeTable->horizontalHeader()->setStretchLastSection(true);
    linetypeTable->setIconSize(QSize(48, 16));
    linetypeTable->setColumnWidth(0, 220);

    auto* side = new QWidget(splitter);
    auto* sideLayout = new QVBoxLayout(side);
    sideLayout->setContentsMargins(0, 0, 0, 0);
    linetypeDetail = new QStackedWidget(side);
    linetypeDetail->setObjectName(QStringLiteral("linetypeDetail"));

    noLinetype = new QLabel("Select a linetype.", linetypeDetail);
    noLinetype->setObjectName(QStringLiteral("noLinetype"));
    noLinetype->setAlignment(Qt::AlignCenter);
    linetypeDetail->addWidget(noLinetype);

    patternPage = new QGroupBox("Pattern", linetypeDetail);
    patternPage->setObjectName(QStringLiteral("patternEditor"));
    auto* patternLayout = new QVBoxLayout(patternPage);
    patternGrid = new QTableWidget(0, 2, patternPage);
    patternGrid->setObjectName(QStringLiteral("patternGrid"));
    patternGrid->setHorizontalHeaderLabels({"Element", "Length"});
    patternGrid->horizontalHeader()->setStretchLastSection(true);
    patternGrid->verticalHeader()->setDefaultSectionSize(26);
    patternGrid->setSelectionBehavior(QAbstractItemView::SelectRows);
    patternGrid->setSelectionMode(QAbstractItemView::SingleSelection);
    patternGrid->setEditTriggers(QAbstractItemView::NoEditTriggers);
    patternGrid->setToolTip("A dash is drawn, a gap is not, a dot is a point: lengths in model "
                            "units, so a 1 m dash is 1 m on the ground at every scale");
    patternLayout->addWidget(patternGrid, 1);
    auto* gridButtons = new QWidget(patternPage);
    auto* gridLayout = new QHBoxLayout(gridButtons);
    gridLayout->setContentsMargins(0, 0, 0, 0);
    addDash = button("+ Dash", "patternAddDash", "Add a dash after the selected element",
                     gridButtons);
    addGap = button("+ Gap", "patternAddGap", "Add a gap after the selected element", gridButtons);
    addDot = button("+ Dot", "patternAddDot", "Add a dot after the selected element", gridButtons);
    removeElement = button("Remove", "patternRemove", "Remove the selected element", gridButtons);
    moveUp = button("Up", "patternUp", "Move the selected element earlier", gridButtons);
    moveDown = button("Down", "patternDown", "Move the selected element later", gridButtons);
    for (QPushButton* each : {addDash, addGap, addDot, removeElement, moveUp, moveDown}) {
        gridLayout->addWidget(each);
    }
    gridLayout->addStretch(1);
    patternLayout->addWidget(gridButtons);
    patternStrip = new PatternStrip(patternPage);
    patternLayout->addWidget(patternStrip);
    patternStatus = new QLabel(patternPage);
    patternStatus->setObjectName(QStringLiteral("patternStatus"));
    patternStatus->setWordWrap(true);
    patternLayout->addWidget(patternStatus);
    auto* descriptionRow = new QWidget(patternPage);
    auto* descriptionLayout = new QFormLayout(descriptionRow);
    descriptionLayout->setContentsMargins(0, 0, 0, 0);
    linetypeDescription = new QLineEdit(descriptionRow);
    linetypeDescription->setObjectName(QStringLiteral("linetypeDescription"));
    descriptionLayout->addRow("Description", linetypeDescription);
    patternLayout->addWidget(descriptionRow);
    auto* saveRow = new QWidget(patternPage);
    auto* saveLayout = new QHBoxLayout(saveRow);
    saveLayout->setContentsMargins(0, 0, 0, 0);
    saveLinetype = button("Save", "linetypeSave",
                          "Store the pattern and description; disabled while the pattern is "
                          "invalid",
                          saveRow);
    revertLinetype = button("Revert", "linetypeRevert", "Forget the changes made above", saveRow);
    saveLayout->addStretch(1);
    saveLayout->addWidget(revertLinetype);
    saveLayout->addWidget(saveLinetype);
    patternLayout->addWidget(saveRow);
    linetypeDetail->addWidget(patternPage);

    libraryPage = new QGroupBox("Library linestyle", linetypeDetail);
    libraryPage->setObjectName(QStringLiteral("libraryPage"));
    auto* libraryLayout = new QVBoxLayout(libraryPage);
    libraryDetails = new QLabel(libraryPage);
    libraryDetails->setObjectName(QStringLiteral("libraryDetails"));
    libraryDetails->setWordWrap(true);
    libraryDetails->setTextFormat(Qt::RichText);
    libraryDetails->setAlignment(Qt::AlignTop | Qt::AlignLeft);
    libraryLayout->addWidget(libraryDetails, 1);
    newStyleUsing = button("New Style Using This", "newStyleUsing",
                           "A drawing style whose linetype is this linestyle", libraryPage);
    libraryLayout->addWidget(newStyleUsing, 0, Qt::AlignRight);
    linetypeDetail->addWidget(libraryPage);
    sideLayout->addWidget(linetypeDetail, 1);

    auto* previewBox = new QGroupBox("As drawn", side);
    auto* previewLayout = new QVBoxLayout(previewBox);
    linetypePreview = new StylePreview(document(), previewBox);
    linetypePreview->setObjectName(QStringLiteral("linetypePreview"));
    linetypePreview->setMinimumHeight(120);
    previewLayout->addWidget(linetypePreview, 1);
    auto* controls = new QWidget(previewBox);
    auto* controlsLayout = new QHBoxLayout(controls);
    controlsLayout->setContentsMargins(0, 0, 0, 0);
    auto* scale = scaleCombo("linetypePreviewScale", controls);
    auto* screen = new QCheckBox("Screen", controls);
    screen->setObjectName(QStringLiteral("linetypePreviewScreen"));
    controlsLayout->addWidget(new QLabel("Plot scale", controls));
    controlsLayout->addWidget(scale);
    controlsLayout->addWidget(screen);
    controlsLayout->addStretch(1);
    previewLayout->addWidget(controls);
    sideLayout->addWidget(previewBox);
    linetypePreview->setScaleDenominator(scale->currentData().toInt());
    connectGuarded(scale, &QComboBox::currentIndexChanged, [this, scale](int) {
        linetypePreview->setScaleDenominator(scale->currentData().toInt());
    });
    connectGuarded(screen, &QCheckBox::toggled, [this](bool onScreen) {
        linetypePreview->setGround(onScreen ? PreviewGround::Screen : PreviewGround::Paper);
    });

    splitter->addWidget(linetypeTable);
    splitter->addWidget(side);
    splitter->setStretchFactor(0, 3);
    splitter->setStretchFactor(1, 2);
    layout->addWidget(splitter, 1);

    auto* buttons = new QWidget(page);
    auto* buttonLayout = new QHBoxLayout(buttons);
    buttonLayout->setContentsMargins(0, 0, 0, 0);
    newLinetype = button("New...", "linetypeNew", "A new drawing linetype: a dash and a gap to "
                                                  "start from",
                         buttons);
    duplicateLinetype = button("Duplicate...", "linetypeDuplicate",
                               "A copy of the selected drawing linetype", buttons);
    renameLinetype = button("Rename...", "linetypeRename",
                            "Rename it; every layer and style naming it comes too, as one undo "
                            "step",
                            buttons);
    mergeLinetypes = button("Merge Into...", "linetypeMerge",
                            "Point every layer and style naming the selected linetypes at "
                            "another, then delete them: one undo step",
                            buttons);
    deleteLinetypes = button("Delete", "linetypeDelete",
                             "Refused while a layer or style names it, saying how many and "
                             "which first",
                             buttons);
    purgeLinetypes = button("Purge...", "linetypePurge",
                            "Delete the drawing linetypes nothing uses: one undo step", buttons);
    selectLinetypeUsers = button("Select Users", "linetypeSelectUsers",
                                 "Select the entities drawn with the selected linetypes", buttons);
    for (QPushButton* each : {newLinetype, duplicateLinetype, renameLinetype, mergeLinetypes,
                              deleteLinetypes, purgeLinetypes}) {
        buttonLayout->addWidget(each);
    }
    buttonLayout->addStretch(1);
    buttonLayout->addWidget(selectLinetypeUsers);
    layout->addWidget(buttons);
    return page;
}

void StyleManagerDialog::Impl::connectLinetypePage()
{
    const auto refilter = [this] {
        linetypeProxy->setFilter(static_cast<LinetypeChip>(linetypeFilter->chip()),
                                 toName(linetypeFilter->text()),
                                 toName(groupBox->currentData().toString()));
    };
    linetypeFilter->onTextChanged = [refilter](const QString&) { refilter(); };
    linetypeFilter->onChipChanged = [refilter](int) { refilter(); };
    connectGuarded(groupBox, &QComboBox::currentIndexChanged, [refilter](int) { refilter(); });
    connectGuarded(linetypeTable->selectionModel(), &QItemSelectionModel::selectionChanged,
                   [this](const QItemSelection&, const QItemSelection&) {
                       if (loading) {
                           return;
                       }
                       loadLinetypeForm();
                       updateButtons();
                   });
    connectGuarded(patternGrid, &QTableWidget::currentCellChanged,
                   [this](int, int, int, int) { updateButtons(); });
    connectGuarded(linetypeDescription, &QLineEdit::textEdited, [this](const QString&) {
        if (loading) {
            return;
        }
        descriptionEdited = true;
        updateButtons();
    });
    connectGuarded(addDash, &QPushButton::clicked, [this] { insertElement(1.0); });
    connectGuarded(addGap, &QPushButton::clicked, [this] { insertElement(-0.5); });
    connectGuarded(addDot, &QPushButton::clicked, [this] { insertElement(0.0); });
    connectGuarded(removeElement, &QPushButton::clicked, [this] {
        const int row = patternGrid->currentRow();
        if (row < 0 || row >= static_cast<int>(workingPattern.size())) {
            return;
        }
        workingPattern.erase(workingPattern.begin() + row);
        patternChanged();
        rebuildGrid();
    });
    const auto move = [this](int by) {
        const int row = patternGrid->currentRow();
        const int to = row + by;
        if (row < 0 || to < 0 || to >= static_cast<int>(workingPattern.size())) {
            return;
        }
        std::swap(workingPattern[static_cast<std::size_t>(row)],
                  workingPattern[static_cast<std::size_t>(to)]);
        patternChanged();
        rebuildGrid();
        patternGrid->setCurrentCell(to, 0);
    };
    connectGuarded(moveUp, &QPushButton::clicked, [move] { move(-1); });
    connectGuarded(moveDown, &QPushButton::clicked, [move] { move(1); });
    connectGuarded(saveLinetype, &QPushButton::clicked, [this] { saveLinetypeForm(); });
    connectGuarded(revertLinetype, &QPushButton::clicked, [this] {
        loadLinetypeForm();
        updateButtons();
    });

    connectGuarded(newStyleUsing, &QPushButton::clicked, [this] {
        const auto rows = selectedLinetypeRows();
        if (rows.size() != 1 || rows.front()->origin != LinetypeOrigin::Library) {
            return;
        }
        const std::string linestyle = rows.front()->name;
        askName(QStringLiteral("Name of the new style drawn with %1:").arg(fromName(linestyle)),
                fromName(katana::cad::freeStyleName(model(), linestyle)),
                [this, linestyle](const std::string& name) {
                    Style style;
                    style.name = name;
                    style.linetype = linestyle;
                    if (run(cmd::createStyle(style),
                            QStringLiteral("Style %1 created, drawn with %2.")
                                .arg(fromName(name), fromName(linestyle)))) {
                        selectStylesAfterReload = std::vector<std::string>{name};
                        tabs->setCurrentIndex(0);
                    }
                });
    });

    connectGuarded(newLinetype, &QPushButton::clicked, [this] {
        askName("Name of the new linetype:",
                fromName(katana::cad::freeLinetypeName(document(), "New Linetype")),
                [this](const std::string& name) {
                    Linetype linetype;
                    linetype.name = name;
                    // A pattern to start from rather than a continuous line,
                    // which would need changing before it meant anything.
                    linetype.pattern = {{1.0}, {-0.5}};
                    if (run(cmd::createLinetype(linetype),
                            QStringLiteral("Linetype %1 created.").arg(fromName(name)))) {
                        selectLinetypeAfterReload = std::make_pair(name, LinetypeOrigin::Drawing);
                    }
                });
    });
    connectGuarded(duplicateLinetype, &QPushButton::clicked, [this] {
        const auto names = selectedDrawingLinetypes();
        if (names.size() != 1) {
            return;
        }
        const std::string from = names.front();
        askName(QStringLiteral("Name of the copy of %1:").arg(fromName(from)),
                fromName(katana::cad::freeLinetypeName(document(), from)),
                [this, from](const std::string& to) {
                    if (run(cmd::duplicateLinetype(from, to),
                            QStringLiteral("Linetype %1 copied as %2.")
                                .arg(fromName(from), fromName(to)))) {
                        selectLinetypeAfterReload = std::make_pair(to, LinetypeOrigin::Drawing);
                    }
                });
    });
    connectGuarded(renameLinetype, &QPushButton::clicked, [this] {
        const auto names = selectedDrawingLinetypes();
        if (names.size() != 1) {
            return;
        }
        const std::string from = names.front();
        askName(QStringLiteral("New name for %1:").arg(fromName(from)), fromName(from),
                [this, from](const std::string& to) {
                    if (to == from) {
                        return;
                    }
                    if (run(cmd::renameLinetype(from, to),
                            QStringLiteral("Linetype %1 renamed to %2; every layer and style "
                                           "naming it came too.")
                                .arg(fromName(from), fromName(to)))) {
                        selectLinetypeAfterReload = std::make_pair(to, LinetypeOrigin::Drawing);
                    }
                });
    });
    connectGuarded(mergeLinetypes, &QPushButton::clicked, [this] {
        const auto names = selectedDrawingLinetypes();
        if (names.empty()) {
            return;
        }
        // Drawing linetypes only: the commands cannot see the library, so a
        // library linestyle is no merge target (a rename onto its name is).
        std::vector<std::string> targets;
        for (const std::string& name : model().linetypes.names()) {
            if (std::ranges::find(names, name) == names.end()) {
                targets.push_back(name);
            }
        }
        askChoice(names.size() == 1
                      ? QStringLiteral("Merge %1 into:").arg(fromName(names.front()))
                      : QStringLiteral("Merge the %1 selected linetypes into:").arg(names.size()),
                  targets, [this, names](const std::string& into) {
                      std::vector<cmd::CommandPtr> parts;
                      for (const std::string& from : names) {
                          parts.push_back(cmd::mergeLinetype(from, into));
                      }
                      if (run(together("MergeLinetypes", std::move(parts)),
                              QStringLiteral("Merged %1 linetype(s) into %2.")
                                  .arg(names.size())
                                  .arg(fromName(into)))) {
                          selectLinetypeAfterReload =
                              std::make_pair(into, LinetypeOrigin::Drawing);
                      }
                  });
    });
    connectGuarded(deleteLinetypes, &QPushButton::clicked, [this] {
        const auto names = selectedDrawingLinetypes();
        if (names.empty()) {
            return;
        }
        std::vector<cmd::CommandPtr> parts;
        for (const std::string& name : names) {
            parts.push_back(cmd::deleteLinetype(name));
        }
        run(together("DeleteLinetypes", std::move(parts)),
            names.size() == 1
                ? QStringLiteral("Linetype %1 deleted.").arg(fromName(names.front()))
                : QStringLiteral("%1 linetypes deleted.").arg(names.size()));
    });
    connectGuarded(purgeLinetypes, &QPushButton::clicked, [this] {
        showPurge(katana::cad::PurgeOptions{.styles = false, .linetypes = true, .hatches = false},
                  "Drawing linetypes nothing uses - uncheck any to keep:");
    });
    connectGuarded(selectLinetypeUsers, &QPushButton::clicked, [this] {
        std::vector<std::string> names;
        for (const LinetypeRow* row : selectedLinetypeRows()) {
            names.push_back(row->name);
        }
        selectUsers(katana::cad::UsageTable::Linetype, names);
    });
}

// ---- the diagnostics page -------------------------------------------------------------------

QWidget* StyleManagerDialog::Impl::buildDiagnosticsPage()
{
    auto* page = new QWidget(tabs);
    page->setObjectName(QStringLiteral("diagnosticsPage"));
    auto* layout = new QVBoxLayout(page);
    diagnosticSummary = new QLabel(page);
    diagnosticSummary->setObjectName(QStringLiteral("diagnosticSummary"));
    diagnosticSummary->setWordWrap(true);
    layout->addWidget(diagnosticSummary);

    using Model = RowTableModel<StyleDiagnostic>;
    std::vector<Model::Column> columns{
        {"Problem",
         [](const StyleDiagnostic& row, int role) -> QVariant {
             if (role == Qt::DisplayRole) {
                 switch (row.kind) {
                 case katana::cad::StyleDiagnosticKind::MissingLinetype:
                     return QStringLiteral("Linetype not defined");
                 case katana::cad::StyleDiagnosticKind::MissingSymbol:
                     return QStringLiteral("Symbol not defined");
                 case katana::cad::StyleDiagnosticKind::Collision:
                     return QStringLiteral("Name in both");
                 }
             }
             if (role == Qt::ForegroundRole &&
                 row.kind != katana::cad::StyleDiagnosticKind::Collision) {
                 return QBrush(kUndefinedNameColour);
             }
             return {};
         }},
        Model::textColumn("Name", [](const StyleDiagnostic& row) { return fromName(row.name); }),
        Model::textColumn("Used by",
                          [](const StyleDiagnostic& row) {
                              return row.users.used() ? fromName(row.users.describe())
                                                      : QStringLiteral("nothing");
                          }),
        Model::textColumn("Drawn as",
                          [](const StyleDiagnostic& row) { return fromName(row.drawnAs); }),
    };
    diagnosticModel = new Model(std::move(columns), page);
    diagnosticTable = new QTableView(page);
    diagnosticTable->setObjectName(QStringLiteral("diagnosticTable"));
    diagnosticTable->setModel(diagnosticModel);
    diagnosticTable->setSelectionBehavior(QAbstractItemView::SelectRows);
    diagnosticTable->setSelectionMode(QAbstractItemView::ExtendedSelection);
    diagnosticTable->setEditTriggers(QAbstractItemView::NoEditTriggers);
    diagnosticTable->setWordWrap(false);
    diagnosticTable->verticalHeader()->setVisible(false);
    diagnosticTable->horizontalHeader()->setStretchLastSection(true);
    diagnosticTable->setColumnWidth(0, 150);
    diagnosticTable->setColumnWidth(1, 220);
    diagnosticTable->setColumnWidth(2, 320);
    layout->addWidget(diagnosticTable, 1);

    auto* buttons = new QWidget(page);
    auto* buttonLayout = new QHBoxLayout(buttons);
    buttonLayout->setContentsMargins(0, 0, 0, 0);
    selectDiagnosticUsers = button("Select Users", "diagnosticSelectUsers",
                                   "Select the entities drawn with the selected names", buttons);
    buttonLayout->addStretch(1);
    buttonLayout->addWidget(selectDiagnosticUsers);
    layout->addWidget(buttons);

    connectGuarded(diagnosticTable->selectionModel(), &QItemSelectionModel::selectionChanged,
                   [this](const QItemSelection&, const QItemSelection&) { updateButtons(); });
    connectGuarded(selectDiagnosticUsers, &QPushButton::clicked, [this] {
        std::vector<std::string> linetypes;
        std::vector<std::string> symbols;
        for (const int row : selectedSourceRows(diagnosticTable, nullptr)) {
            if (const StyleDiagnostic* diagnostic = diagnosticModel->rowAt(row)) {
                (diagnostic->kind == katana::cad::StyleDiagnosticKind::MissingSymbol
                     ? symbols
                     : linetypes)
                    .push_back(diagnostic->name);
            }
        }
        if (!alive()) {
            return;
        }
        std::vector<katana::entity::EntityId> ids = katana::cad::entitiesUsing(
            model(), katana::cad::UsageTable::Linetype, linetypes);
        const auto more =
            katana::cad::entitiesUsing(model(), katana::cad::UsageTable::Symbol, symbols);
        ids.insert(ids.end(), more.begin(), more.end());
        std::ranges::sort(ids);
        ids.erase(std::unique(ids.begin(), ids.end()), ids.end());
        if (ids.empty()) {
            log("No entity is drawn with that; a layer or an unused style names it.", false);
            return;
        }
        if (context.selectAndShow) {
            context.selectAndShow(ids);
        } else {
            document().selection().set(ids);
            document().notifySelectionChanged();
        }
        log(QStringLiteral("%1 entit%2 selected.").arg(ids.size()).arg(ids.size() == 1 ? "y"
                                                                                       : "ies"),
            false);
    });
    return page;
}

// ---- the prompt row and the purge panel ----------------------------------------------------

void StyleManagerDialog::Impl::buildPrompt(QWidget* parent)
{
    prompt = new QFrame(parent);
    prompt->setObjectName(QStringLiteral("promptPanel"));
    prompt->setFrameShape(QFrame::StyledPanel);
    auto* layout = new QHBoxLayout(prompt);
    promptLabel = new QLabel(prompt);
    promptLabel->setObjectName(QStringLiteral("promptLabel"));
    promptName = new QLineEdit(prompt);
    promptName->setObjectName(QStringLiteral("promptName"));
    promptChoice = new QComboBox(prompt);
    promptChoice->setObjectName(QStringLiteral("promptChoice"));
    promptChoice->setSizeAdjustPolicy(QComboBox::AdjustToContents);
    promptOk = button("OK", "promptOk", {}, prompt);
    auto* cancel = button("Cancel", "promptCancel", {}, prompt);
    layout->addWidget(promptLabel);
    layout->addWidget(promptName, 1);
    layout->addWidget(promptChoice, 1);
    layout->addWidget(promptOk);
    layout->addWidget(cancel);
    prompt->hide();
    connectGuarded(promptOk, &QPushButton::clicked, [this] { acceptPrompt(); });
    connectGuarded(promptName, &QLineEdit::returnPressed, [this] { acceptPrompt(); });
    connectGuarded(cancel, &QPushButton::clicked, [this] {
        prompt->hide();
        promptAccept = nullptr;
    });
}

void StyleManagerDialog::Impl::buildPurge(QWidget* parent)
{
    purgePanel = new QFrame(parent);
    purgePanel->setObjectName(QStringLiteral("purgePanel"));
    purgePanel->setFrameShape(QFrame::StyledPanel);
    auto* layout = new QVBoxLayout(purgePanel);
    purgeLabel = new QLabel(purgePanel);
    purgeList = new QListWidget(purgePanel);
    purgeList->setObjectName(QStringLiteral("purgeList"));
    purgeList->setMaximumHeight(140);
    layout->addWidget(purgeLabel);
    layout->addWidget(purgeList);
    auto* buttons = new QWidget(purgePanel);
    auto* buttonLayout = new QHBoxLayout(buttons);
    buttonLayout->setContentsMargins(0, 0, 0, 0);
    auto* ok = button("Purge Checked", "purgeOk", "Delete the checked items as one undo step",
                      buttons);
    auto* cancel = button("Cancel", "purgeCancel", {}, buttons);
    buttonLayout->addStretch(1);
    buttonLayout->addWidget(ok);
    buttonLayout->addWidget(cancel);
    layout->addWidget(buttons);
    purgePanel->hide();
    connectGuarded(ok, &QPushButton::clicked, [this] { acceptPurge(); });
    connectGuarded(cancel, &QPushButton::clicked, [this] { purgePanel->hide(); });
}

// =====================================================================================

StyleManagerDialog::StyleManagerDialog(const CustomisationContext& context, QWidget* parent)
    : QDialog(parent), impl_(std::make_unique<Impl>())
{
    Impl& impl = *impl_;
    impl.dialog = this;
    impl.context = context;
    if (impl.context.thumbnails == nullptr) {
        impl.ownThumbnails = std::make_unique<DefinitionThumbnails>();
        impl.context.thumbnails = impl.ownThumbnails.get();
    }

    setWindowTitle("Styles and Linetypes");
    setObjectName(QStringLiteral("styleManager"));
    resize(1180, 760);

    auto* layout = new QVBoxLayout(this);
    impl.tabs = new QTabWidget(this);
    impl.tabs->setObjectName(QStringLiteral("managerTabs"));
    // The status line first: log() writes to it from anywhere after this.
    impl.statusLine = new QLabel(this);
    impl.statusLine->setObjectName(QStringLiteral("statusLine"));
    impl.statusLine->setWordWrap(true);
    impl.tabs->addTab(impl.buildStylesPage(), "Styles");
    impl.tabs->addTab(impl.buildLinetypesPage(), "Linetypes");
    impl.tabs->addTab(impl.buildDiagnosticsPage(), "Diagnostics");
    layout->addWidget(impl.tabs, 1);
    impl.buildPrompt(this);
    impl.buildPurge(this);
    layout->addWidget(impl.prompt);
    layout->addWidget(impl.purgePanel);

    auto* bottom = new QWidget(this);
    auto* bottomLayout = new QHBoxLayout(bottom);
    bottomLayout->setContentsMargins(0, 0, 0, 0);
    impl.undoButton = button("Undo", "undoButton", "Undo the last change to the drawing", bottom);
    impl.redoButton = button("Redo", "redoButton", "Redo what was undone", bottom);
    auto* close = button("Close", "closeButton", {}, bottom);
    bottomLayout->addWidget(impl.undoButton);
    bottomLayout->addWidget(impl.redoButton);
    bottomLayout->addWidget(impl.statusLine, 1);
    bottomLayout->addWidget(close);
    layout->addWidget(bottom);

    impl.connectStyleForm();
    impl.connectStyleButtons();
    impl.connectLinetypePage();
    impl.connectGuarded(close, &QPushButton::clicked, [this] { reject(); });
    impl.connectGuarded(impl.undoButton, &QPushButton::clicked, [&impl] {
        if (!impl.alive()) {
            return;
        }
        if (const auto status = impl.document().undo(); !status) {
            impl.log(QString::fromStdString(status.error().describe()), true);
            return;
        }
        impl.log("Undone.", false);
    });
    impl.connectGuarded(impl.redoButton, &QPushButton::clicked, [&impl] {
        if (!impl.alive()) {
            return;
        }
        if (const auto status = impl.document().redo(); !status) {
            impl.log(QString::fromStdString(status.error().describe()), true);
            return;
        }
        impl.log("Redone.", false);
    });

    impl.reload(true);
    // Last: from here on the Document is heard, once per event-loop turn.
    impl.watcher = std::make_unique<DocumentWatcher>(
        *impl.context.document, [&impl](const DocumentChanges& changes) {
            impl.onDocumentChanged(changes);
        });
}

StyleManagerDialog::~StyleManagerDialog()
{
    // The watcher first, then every hook the children hold: the children
    // are deleted by ~QWidget, after impl_, and must not call into it.
    impl_->watcher.reset();
    impl_->detach();
    impl_->guard.reset();
}

void StyleManagerDialog::showFirstRows()
{
    for (QTableView* table : {impl_->styleTable, impl_->linetypeTable}) {
        if (table->model()->rowCount() > 0) {
            table->selectRow(0);
        }
    }
}

std::vector<std::string> StyleManagerDialog::selectedStyles() const
{
    return impl_->selectedStyleNames();
}

void StyleManagerDialog::selectStyles(const std::vector<std::string>& names)
{
    impl_->selectStyleNames(names);
}

std::vector<std::pair<std::string, katana::cad::LinetypeOrigin>>
StyleManagerDialog::selectedLinetypes() const
{
    std::vector<std::pair<std::string, katana::cad::LinetypeOrigin>> selected;
    for (const LinetypeRow* row : impl_->selectedLinetypeRows()) {
        selected.emplace_back(row->name, row->origin);
    }
    return selected;
}

void StyleManagerDialog::selectLinetype(const std::string& name,
                                        katana::cad::LinetypeOrigin origin)
{
    impl_->selectLinetypeRow(name, origin);
}

} // namespace katana::qt
