#include "corridor_dialog.hpp"

#include <QComboBox>
#include <QFormLayout>
#include <QGridLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QTableWidget>
#include <QVBoxLayout>

#include <optional>
#include <string>
#include <utility>

#include "customisation/document_watcher.hpp"
#include "katana/cad/bim/bim_object.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/road/corridor_model.hpp"
#include "katana/cad/road/road_template.hpp"
#include "katana/core/text.hpp"
#include "road_dialog_support.hpp"

namespace katana::qt {

namespace road = katana::cad::road;
using road_ui::qs;

namespace {

constexpr const char* kNoSurface = "(none)";

QString stationText(const std::optional<double>& station)
{
    return station ? exactNumber(*station) : QString();
}

// Makes `combo` show `stored`: its item ignoring case - the verb finds a
// template, an alignment or a surface that way (RoadTemplateSet::find,
// SurfaceStore::find), so a corridor naming "URBAN STREET" is valid against a
// template called "urban street" - or, when it has none (a surface since
// removed, an alignment that lost its design profile), an extra item holding
// the name. QComboBox::setCurrentText does neither: on a combo that is not
// editable it keeps the item showing before, and Apply then wrote THAT item
// over what the corridor names (docs/road_design.md, "In the window").
void showStored(QComboBox* combo, const QString& stored)
{
    int at = combo->findText(stored);
    if (at < 0) {
        at = combo->findText(stored, Qt::MatchFixedString); // ignoring case
    }
    if (at < 0) {
        combo->addItem(stored);
        at = combo->count() - 1;
        combo->setItemData(at, "The corridor names this, but the drawing has none of that name",
                           Qt::ToolTipRole);
    }
    combo->setCurrentIndex(at);
}

// What a refilled combo keeps of what it showed.
enum class Keep {
    Nothing,   // the first item: the caller is about to say what to show
    IfPresent, // what it showed, when that is still one of the items
    Always,    // what it showed; a name no longer among the items stays, as an extra
               // item, so that the combo never flips to some other item behind
               // an unapplied edit or a name the corridor holds
};

// Refills a combo with `items`.
void refill(QComboBox* combo, const QStringList& items, Keep keep)
{
    const QString shown = combo->currentText();
    combo->blockSignals(true);
    combo->clear();
    combo->addItems(items);
    if (!shown.isEmpty()) {
        if (keep == Keep::Always) {
            showStored(combo, shown);
        } else if (const int at = combo->findText(shown); keep == Keep::IfPresent && at >= 0) {
            combo->setCurrentIndex(at);
        }
    }
    combo->blockSignals(false);
}

// What Apply writes for a name a combo shows: the spelling the corridor
// already holds when it is the same name in another case, so that Apply
// round-trips the stored text and re-points nothing.
QString spelledAs(const QString& shown, const std::string* stored)
{
    if (stored != nullptr && katana::core::equalsIgnoringCase(shown.toStdString(), *stored)) {
        return road_ui::qs(*stored);
    }
    return shown;
}

} // namespace

struct CorridorDialog::Impl {
    CorridorDialog* dialog = nullptr;
    CorridorDialogContext context;
    std::unique_ptr<QObject> guard = std::make_unique<QObject>();
    std::unique_ptr<DocumentWatcher> watcher;

    QTableWidget* list = nullptr;
    QLineEdit* name = nullptr;
    QComboBox* alignment = nullptr;
    QComboBox* roadTemplate = nullptr;
    QComboBox* surface = nullptr;
    QLineEdit* interval = nullptr;
    QLineEdit* from = nullptr;
    QLineEdit* to = nullptr;
    QTableWidget* regions = nullptr;
    QLineEdit* regionFrom = nullptr;
    QLineEdit* regionTo = nullptr;
    QComboBox* regionTemplate = nullptr;
    QTableWidget* superelevation = nullptr;
    QLineEdit* superStation = nullptr;
    QLineEdit* superLeft = nullptr;
    QLineEdit* superRight = nullptr;
    QLabel* problems = nullptr;
    QLabel* status = nullptr;
    bool loading = false;
    // The stored corridor the definition fields were last filled from. The
    // fields are a buffer, applied by Apply: a reload that found the chosen
    // corridor as it was keeps what was typed, as the Road Template
    // Manager's grids do; one that found it changed (an undo, a typed line,
    // a new drawing) shows what the drawing holds.
    std::optional<road::Corridor> filledFrom;

    [[nodiscard]] bool alive() const { return watcher != nullptr && watcher->documentAlive(); }

    template <typename Sender, typename Signal, typename Slot>
    void connect(Sender* sender, Signal signal, Slot slot)
    {
        QObject::connect(sender, signal, guard.get(), std::move(slot));
    }

    [[nodiscard]] const road::Corridor* current() const
    {
        if (!alive()) {
            return nullptr;
        }
        const QString chosen = dialog->currentCorridor();
        return chosen.isEmpty() ? nullptr
                                : context.document->corridors().find(chosen.toStdString());
    }

    std::optional<QString> corridorWord()
    {
        const QString chosen = dialog->currentCorridor();
        if (chosen.isEmpty()) {
            road_ui::setStatus(status, "Choose a corridor first, or make one with New.", true);
            return std::nullopt;
        }
        return road_ui::word(status, chosen, "the corridor's name");
    }

    bool run(const QString& line)
    {
        if (!alive()) {
            road_ui::setStatus(status, "The drawing this dialog edited is gone.", true);
            return false;
        }
        const bool ok = road_ui::run(context.run, status, line);
        reload();
        return ok;
    }

    // A field that must hold one number (or may be empty), as the line
    // takes it; nullopt, said on the status line, for anything else.
    std::optional<QString> numberField(QLineEdit* field, const QString& what, bool mayBeEmpty)
    {
        const QString text = field->text().trimmed();
        if (text.isEmpty() && mayBeEmpty) {
            return QString();
        }
        if (!road_ui::number(text)) {
            road_ui::setStatus(status,
                               "The " + what + " '" + text +
                                   "' is not a number; type one number, with a decimal point",
                               true);
            return std::nullopt;
        }
        return text;
    }

    // alignment= template= surface= interval= from= to=, from the fields.
    // `stored` is the corridor being set, or null for a new one: a name the
    // combos show in another case than the corridor holds it is written as
    // the corridor holds it.
    std::optional<QString> definition(bool named, const road::Corridor* stored)
    {
        const QString alignmentName =
            spelledAs(alignment->currentText(), stored != nullptr ? &stored->alignment : nullptr);
        const QString templateName = spelledAs(
            roadTemplate->currentText(), stored != nullptr ? &stored->templateName : nullptr);
        const auto alignmentWord = road_ui::word(status, alignmentName, "the alignment's name");
        const auto templateWord = road_ui::word(status, templateName, "the template's name");
        if (!alignmentWord || !templateWord) {
            return std::nullopt;
        }
        if (alignment->currentText().isEmpty() || roadTemplate->currentText().isEmpty()) {
            road_ui::setStatus(status,
                               "A corridor needs an alignment with a design profile and a road "
                               "template: Terrain > Alignment Manager, and Road Template "
                               "Manager",
                               true);
            return std::nullopt;
        }
        const auto step = numberField(interval, "interval", false);
        const auto start = numberField(from, "start station", true);
        const auto end = numberField(to, "end station", true);
        if (!step || !start || !end) {
            return std::nullopt;
        }
        const QString surfaceName =
            surface->currentText() == kNoSurface
                ? QString()
                : spelledAs(surface->currentText(), stored != nullptr ? &stored->surface : nullptr);
        const auto surfaceOption = road_ui::option(status, "surface", surfaceName);
        if (!surfaceOption) {
            return std::nullopt;
        }
        QString text = named ? "ALIGNMENT " + *alignmentWord + " TEMPLATE " + *templateWord
                             : "alignment=" + *alignmentWord + " template=" + *templateWord;
        if (named) {
            if (!surfaceName.isEmpty()) {
                text += " " + *surfaceOption;
            }
            text += " interval=" + *step;
            if (!start->isEmpty()) {
                text += " from=" + *start;
            }
            if (!end->isEmpty()) {
                text += " to=" + *end;
            }
        } else {
            // SET writes every key, an empty one clearing it.
            text += " " + *surfaceOption + " interval=" + *step + " from=" + *start + " to=" + *end;
        }
        return text;
    }

    // The drawing's alignments with a design profile, road templates and
    // surfaces, as reload last read them: what the combos are filled with.
    QStringList alignmentItems;
    QStringList templateItems;
    QStringList surfaceItems;
    void refillNamed(Keep keep)
    {
        refill(alignment, alignmentItems, keep);
        refill(roadTemplate, templateItems, keep);
        refill(surface, surfaceItems, keep);
        // The region's template is a choice, not a name a corridor holds.
        refill(regionTemplate, templateItems, Keep::IfPresent);
    }

    void build();
    void reload();
    // `force` fills the definition fields from the chosen corridor whatever
    // they hold: a choice of another corridor, as against a reload.
    void fillChosen(bool force);
};

void CorridorDialog::Impl::build()
{
    dialog->setObjectName("corridorDialog");
    dialog->setWindowTitle("Corridors");
    auto* layout = new QVBoxLayout(dialog);
    list = road_ui::table("corridorList",
                          {"Corridor", "Alignment", "Template", "Surface", "Interval", "From", "To",
                           "Regions", "Superelevation"},
                          dialog);
    list->setMaximumHeight(140);
    layout->addWidget(list);

    // ---- the definition ----------------------------------------------------------------
    auto* definitionBox = new QGroupBox("Corridor", dialog);
    auto* form = new QGridLayout(definitionBox);
    name = road_ui::field("corridorName", "name", "The name New and Rename give a corridor",
                          definitionBox);
    alignment = new QComboBox(definitionBox);
    alignment->setObjectName("corridorAlignment");
    alignment->setToolTip("An alignment with a design profile (Terrain > Alignment Manager)");
    roadTemplate = new QComboBox(definitionBox);
    roadTemplate->setObjectName("corridorTemplate");
    roadTemplate->setToolTip("The road template swept along it (BIM > Road Template Manager)");
    surface = new QComboBox(definitionBox);
    surface->setObjectName("corridorSurface");
    surface->setToolTip("The surface the batters meet and the earthworks are measured to");
    interval = road_ui::field("corridorInterval", "10", "Metres between sections", definitionBox);
    interval->setText("10");
    from = road_ui::field("corridorFrom", "start", "The first station; empty: where it begins",
                          definitionBox);
    to = road_ui::field("corridorTo", "end", "The last station; empty: where it ends",
                        definitionBox);
    form->addWidget(new QLabel("Name", definitionBox), 0, 0);
    form->addWidget(name, 0, 1);
    form->addWidget(new QLabel("Alignment", definitionBox), 0, 2);
    form->addWidget(alignment, 0, 3);
    form->addWidget(new QLabel("Template", definitionBox), 1, 0);
    form->addWidget(roadTemplate, 1, 1);
    form->addWidget(new QLabel("Target surface", definitionBox), 1, 2);
    form->addWidget(surface, 1, 3);
    form->addWidget(new QLabel("Interval (m)", definitionBox), 2, 0);
    form->addWidget(interval, 2, 1);
    auto* range = new QHBoxLayout;
    range->addWidget(from);
    range->addWidget(to);
    form->addWidget(new QLabel("Stations", definitionBox), 2, 2);
    form->addLayout(range, 2, 3);
    auto* definitionButtons = new QHBoxLayout;
    auto* newButton = road_ui::button(
        "New", "corridorNew",
        "CORRIDOR NEW: this alignment and template, with the layers its objects go on",
        definitionBox);
    auto* applyButton =
        road_ui::button("Apply", "corridorApply",
                        "CORRIDOR SET: the chosen corridor as the fields say", definitionBox);
    auto* renameButton = road_ui::button("Rename", "corridorRename",
                                         "CORRIDOR RENAME: the chosen corridor", definitionBox);
    auto* deleteButton = road_ui::button("Delete", "corridorDelete",
                                         "CORRIDOR DELETE: the chosen corridor", definitionBox);
    for (QPushButton* each : {newButton, applyButton, renameButton, deleteButton}) {
        definitionButtons->addWidget(each);
    }
    definitionButtons->addStretch(1);
    form->addLayout(definitionButtons, 3, 0, 1, 4);
    layout->addWidget(definitionBox);

    // ---- regions and superelevation, side by side ----------------------------------------
    auto* tables = new QHBoxLayout;
    auto* regionBox = new QGroupBox("Regions: another template between two stations", dialog);
    auto* regionLayout = new QVBoxLayout(regionBox);
    regions = road_ui::table("corridorRegions", {"From", "To", "Template"}, regionBox);
    regionLayout->addWidget(regions);
    auto* regionRow = new QHBoxLayout;
    regionFrom = road_ui::field("corridorRegionFrom", "from", "Where the region starts", regionBox);
    regionTo = road_ui::field("corridorRegionTo", "to", "Where it ends", regionBox);
    regionTemplate = new QComboBox(regionBox);
    regionTemplate->setObjectName("corridorRegionTemplate");
    auto* regionAdd =
        road_ui::button("Add", "corridorRegionAdd", "CORRIDOR REGION ... ADD", regionBox);
    auto* regionRemove =
        road_ui::button("Remove", "corridorRegionRemove",
                        "CORRIDOR REGION ... REMOVE: the chosen region", regionBox);
    for (QWidget* each : std::initializer_list<QWidget*>{regionFrom, regionTo, regionTemplate,
                                                         regionAdd, regionRemove}) {
        regionRow->addWidget(each);
    }
    regionLayout->addLayout(regionRow);
    tables->addWidget(regionBox);

    auto* superBox = new QGroupBox("Superelevation: crossfall each side, percent outward", dialog);
    auto* superLayout = new QVBoxLayout(superBox);
    superelevation =
        road_ui::table("corridorSuperelevation", {"Station", "Left (%)", "Right (%)"}, superBox);
    superLayout->addWidget(superelevation);
    auto* superRow = new QHBoxLayout;
    superStation = road_ui::field("corridorSuperStation", "station", "The station", superBox);
    superLeft = road_ui::field("corridorSuperLeft", "left %",
                               "The left side's crossfall there, outward: -3 falls", superBox);
    superRight = road_ui::field("corridorSuperRight", "right %",
                                "The right side's crossfall there, outward", superBox);
    auto* superAdd = road_ui::button("Add", "corridorSuperAdd",
                                     "CORRIDOR SUPERELEVATION ... ADD (replacing one at the "
                                     "station)",
                                     superBox);
    auto* superRemove =
        road_ui::button("Remove", "corridorSuperRemove",
                        "CORRIDOR SUPERELEVATION ... REMOVE: the chosen entry", superBox);
    for (QWidget* each : std::initializer_list<QWidget*>{superStation, superLeft, superRight,
                                                         superAdd, superRemove}) {
        superRow->addWidget(each);
    }
    superLayout->addLayout(superRow);
    tables->addWidget(superBox);
    layout->addLayout(tables, 1);

    // ---- what it builds ----------------------------------------------------------------
    auto* buildRow = new QHBoxLayout;
    auto* buildButton = road_ui::button(
        "Build", "corridorBuild",
        "CORRIDOR BUILD: its sections, objects, cut and fill, and problems", dialog);
    auto* stringsButton = road_ui::button(
        "Build Strings", "corridorStrings",
        "CORRIDOR STRINGS: its 3D strings as polylines, replacing the ones made before", dialog);
    auto* surfacesButton = road_ui::button(
        "Build Surfaces", "corridorSurfaces",
        "CORRIDOR SURFACE: its finished and subgrade surfaces into the drawing's surfaces", dialog);
    buildRow->addWidget(buildButton);
    buildRow->addWidget(stringsButton);
    buildRow->addWidget(surfacesButton);
    buildRow->addStretch(1);
    layout->addLayout(buildRow);
    problems = road_ui::muted(QString(), dialog);
    problems->setObjectName("corridorProblems");
    layout->addWidget(problems);
    status = new QLabel(dialog);
    status->setObjectName("corridorStatus");
    status->setWordWrap(true);
    layout->addWidget(status);
    auto* closeRow = new QHBoxLayout;
    closeRow->addStretch(1);
    auto* closeButton = road_ui::button("Close", "corridorClose", "Close the dialog", dialog);
    closeRow->addWidget(closeButton);
    layout->addLayout(closeRow);

    connect(list, &QTableWidget::itemSelectionChanged, [this] {
        if (!loading) {
            fillChosen(true);
        }
    });
    // New and Rename take the name from the field BEFORE the line runs: the
    // reload that follows it refills the field from the corridor still
    // chosen, so reading it afterwards would choose that corridor again.
    connect(newButton, &QPushButton::clicked, [this] {
        const QString newName = name->text().trimmed();
        const auto word = road_ui::word(status, newName, "the new name");
        const auto rest = definition(true, nullptr);
        if (word && rest && run("CORRIDOR NEW " + *word + " " + *rest)) {
            dialog->selectCorridor(newName);
        }
    });
    connect(applyButton, &QPushButton::clicked, [this] {
        const auto word = corridorWord();
        if (!word) {
            return;
        }
        if (const auto rest = definition(false, current())) {
            run("CORRIDOR SET " + *word + " " + *rest);
        }
    });
    connect(renameButton, &QPushButton::clicked, [this] {
        const QString newName = name->text().trimmed();
        const auto word = corridorWord();
        const auto newWord = road_ui::word(status, newName, "the new name");
        if (word && newWord && run("CORRIDOR RENAME " + *word + " " + *newWord)) {
            dialog->selectCorridor(newName);
        }
    });
    connect(deleteButton, &QPushButton::clicked, [this] {
        if (const auto word = corridorWord()) {
            run("CORRIDOR DELETE " + *word);
        }
    });
    connect(regionAdd, &QPushButton::clicked, [this] {
        const auto word = corridorWord();
        if (!word) {
            return;
        }
        const auto start = numberField(regionFrom, "region's start", false);
        const auto end = numberField(regionTo, "region's end", false);
        const auto templateWord =
            road_ui::word(status, regionTemplate->currentText(), "the region's template");
        if (start && end && templateWord) {
            run("CORRIDOR REGION " + *word + " ADD " + *start + " " + *end + " " + *templateWord);
        }
    });
    connect(regionRemove, &QPushButton::clicked, [this] {
        const auto word = corridorWord();
        const int row = regions->currentRow();
        if (!word) {
            return;
        }
        if (row < 0) {
            road_ui::setStatus(status, "Choose the region to remove.", true);
            return;
        }
        run("CORRIDOR REGION " + *word + " REMOVE " + regions->item(row, 0)->text());
    });
    connect(superAdd, &QPushButton::clicked, [this] {
        const auto word = corridorWord();
        if (!word) {
            return;
        }
        const auto station = numberField(superStation, "station", false);
        const auto left = numberField(superLeft, "left crossfall", false);
        const auto right = numberField(superRight, "right crossfall", false);
        if (station && left && right) {
            run("CORRIDOR SUPERELEVATION " + *word + " ADD " + *station + " " + *left + " " +
                *right);
        }
    });
    connect(superRemove, &QPushButton::clicked, [this] {
        const auto word = corridorWord();
        const int row = superelevation->currentRow();
        if (!word) {
            return;
        }
        if (row < 0) {
            road_ui::setStatus(status, "Choose the entry to remove.", true);
            return;
        }
        run("CORRIDOR SUPERELEVATION " + *word + " REMOVE " + superelevation->item(row, 0)->text());
    });
    connect(buildButton, &QPushButton::clicked, [this] {
        if (const auto word = corridorWord()) {
            run("CORRIDOR BUILD " + *word);
        }
    });
    connect(stringsButton, &QPushButton::clicked, [this] {
        if (const auto word = corridorWord()) {
            run("CORRIDOR STRINGS " + *word);
        }
    });
    connect(surfacesButton, &QPushButton::clicked, [this] {
        if (const auto word = corridorWord()) {
            run("CORRIDOR SURFACE " + *word);
        }
    });
    connect(closeButton, &QPushButton::clicked, [this] { dialog->close(); });
}

void CorridorDialog::Impl::reload()
{
    if (!alive()) {
        return;
    }
    const katana::cad::Document& document = *context.document;
    QStringList alignments;
    for (const katana::entity::Alignment& each : document.model().alignments.all()) {
        if (each.vertical.has_value()) {
            alignments << qs(each.name);
        }
    }
    QStringList templates;
    for (const road::RoadTemplate& each : document.roadTemplates().templates) {
        templates << qs(each.name);
    }
    QStringList surfaces{kNoSurface};
    for (const auto& each : document.surfaces().all()) {
        surfaces << qs(each.name);
    }
    alignmentItems = alignments;
    templateItems = templates;
    surfaceItems = surfaces;
    // The three the corridor itself names keep a name the drawing no longer
    // offers, as an extra item (showStored); the region's is only a choice.
    refillNamed(Keep::Always);

    const QString chosen = dialog->currentCorridor();
    loading = true;
    list->setRowCount(0);
    for (const road::Corridor& each : document.corridors().corridors) {
        const int row = list->rowCount();
        list->insertRow(row);
        list->setItem(row, 0, road_ui::cell(qs(each.name)));
        list->setItem(row, 1, road_ui::cell(qs(each.alignment)));
        list->setItem(row, 2, road_ui::cell(qs(each.templateName)));
        list->setItem(row, 3, road_ui::cell(qs(each.surface)));
        list->setItem(row, 4, road_ui::cell(exactNumber(each.interval)));
        list->setItem(row, 5, road_ui::cell(stationText(each.start)));
        list->setItem(row, 6, road_ui::cell(stationText(each.end)));
        list->setItem(row, 7, road_ui::cell(QString::number(each.regions.size())));
        list->setItem(row, 8, road_ui::cell(QString::number(each.superelevation.size())));
    }
    if (!chosen.isEmpty()) {
        dialog->selectCorridor(chosen);
    }
    loading = false;
    fillChosen(false);
    if (alignments.isEmpty()) {
        road_ui::setStatus(status,
                           "No alignment has a design profile. Define one in Terrain > "
                           "Alignment Manager (Vertical), or with ALIGN DESIGN name s,z ...",
                           true);
    }
}

void CorridorDialog::Impl::fillChosen(bool force)
{
    regions->setRowCount(0);
    superelevation->setRowCount(0);
    problems->clear();
    const road::Corridor* corridor = current();
    if (corridor == nullptr) {
        filledFrom.reset();
        return;
    }
    if (force || !filledFrom || *filledFrom != *corridor) {
        // From the drawing's own names, so an extra item for a name the last
        // corridor held and this one does not is not left in the lists.
        refillNamed(Keep::Nothing);
        name->setText(qs(corridor->name));
        showStored(alignment, qs(corridor->alignment));
        showStored(roadTemplate, qs(corridor->templateName));
        showStored(surface, corridor->surface.empty() ? QString(kNoSurface) : qs(corridor->surface));
        interval->setText(exactNumber(corridor->interval));
        from->setText(stationText(corridor->start));
        to->setText(stationText(corridor->end));
        filledFrom = *corridor;
    }
    for (const road::TemplateRegion& region : corridor->regions) {
        const int row = regions->rowCount();
        regions->insertRow(row);
        regions->setItem(row, 0, road_ui::cell(exactNumber(region.start)));
        regions->setItem(row, 1, road_ui::cell(exactNumber(region.end)));
        regions->setItem(row, 2, road_ui::cell(qs(region.templateName)));
    }
    for (const road::SuperelevationPoint& point : corridor->superelevation) {
        const int row = superelevation->rowCount();
        superelevation->insertRow(row);
        superelevation->setItem(row, 0, road_ui::cell(exactNumber(point.station)));
        superelevation->setItem(row, 1, road_ui::cell(exactNumber(point.left)));
        superelevation->setItem(row, 2, road_ui::cell(exactNumber(point.right)));
    }
    // What it could not build, from the road model the views draw.
    QStringList lines;
    const std::string source = "corridor " + corridor->name;
    for (const katana::cad::bim::BimProblem& problem :
         katana::cad::bim::bimModel(*context.document)->problems) {
        if (problem.source == source) {
            lines << qs(problem.message);
        }
    }
    problems->setText(lines.isEmpty() ? QString("No problems: everything it names is there.")
                                      : lines.join('\n'));
}

// =====================================================================================

CorridorDialog::CorridorDialog(CorridorDialogContext context, QWidget* parent)
    : QDialog(parent), impl_(std::make_unique<Impl>())
{
    impl_->dialog = this;
    impl_->context = std::move(context);
    impl_->build();
    if (impl_->context.document != nullptr) {
        impl_->watcher = std::make_unique<DocumentWatcher>(*impl_->context.document,
                                                           [this](const DocumentChanges& changes) {
                                                               if (changes.model) {
                                                                   impl_->reload();
                                                               }
                                                           });
    }
    impl_->reload();
    if (impl_->list->rowCount() > 0) {
        impl_->list->selectRow(0);
    }
    resize(920, 640);
}

CorridorDialog::~CorridorDialog()
{
    impl_->guard.reset();
}

bool CorridorDialog::selectCorridor(const QString& name)
{
    for (int row = 0; row < impl_->list->rowCount(); ++row) {
        if (impl_->list->item(row, 0)->text().compare(name, Qt::CaseInsensitive) == 0) {
            impl_->list->selectRow(row);
            return true;
        }
    }
    return false;
}

QString CorridorDialog::currentCorridor() const
{
    const int row = impl_->list->currentRow();
    if (row < 0 || impl_->list->item(row, 0) == nullptr) {
        return {};
    }
    return impl_->list->item(row, 0)->text();
}

} // namespace katana::qt
