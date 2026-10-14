#include "road_template_manager.hpp"

#include <QComboBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPainter>
#include <QPainterPath>
#include <QPushButton>
#include <QSplitter>
#include <QTableWidget>
#include <QVBoxLayout>

#include <algorithm>
#include <cmath>
#include <optional>
#include <set>
#include <string>
#include <utility>
#include <vector>

#include "customisation/document_watcher.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/road/road_section.hpp"
#include "katana/cad/road/road_template.hpp"
#include "road_dialog_support.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace road = katana::cad::road;
using road_ui::qs;

namespace {

QString fixed3(double value)
{
    return QString::number(value, 'f', 3);
}

QColor colourOf(const katana::entity::Color& colour)
{
    return QColor(colour.r, colour.g, colour.b, colour.a);
}

// What else a component is, in a word or two, for the grid's last column.
QString detailOf(const road::Component& component)
{
    switch (component.kind) {
    case road::ComponentKind::Kerb:
        return QString("%1, %2 m high, %3 m wide")
            .arg(qs(std::string(road::toString(component.shape))))
            .arg(fixed3(component.height))
            .arg(fixed3(road::kerbWidth(component)));
    case road::ComponentKind::Drain:
        return QString("invert %1 m down").arg(fixed3(component.depth));
    case road::ComponentKind::Batter: {
        QString text = QString("cut %1:1, fill %2:1, to %3 m")
                           .arg(component.cutSlope)
                           .arg(component.fillSlope)
                           .arg(component.maximumWidth);
        if (component.benchHeight > 0.0) {
            text += QString(", benches %1 m every %2 m")
                        .arg(component.benchWidth)
                        .arg(component.benchHeight);
        }
        return text;
    }
    default:
        return QString("%1 courses%2")
            .arg(component.courses.size())
            .arg(component.superelevated ? ", superelevated" : "");
    }
}

// The template across a level road (road::templateSection), drawn as a
// person reads a typical section: the courses and kerbs as coloured
// solids, the finished surface over them, a batter as a dashed stub falling
// at its fill slope. The vertical is exaggerated, as a section's is, so a
// 50 mm course can be seen at all; the factor is printed.
class TemplatePreview final : public QWidget {
  public:
    explicit TemplatePreview(QWidget* parent) : QWidget(parent)
    {
        setObjectName("roadTemplatePreview");
        setMinimumHeight(150);
    }

    void setTemplate(std::optional<road::RoadTemplate> roadTemplate)
    {
        template_ = std::move(roadTemplate);
        section_.reset();
        if (template_) {
            if (auto section = road::templateSection(*template_)) {
                section_ = std::move(*section);
            }
        }
        update();
    }

  protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        painter.fillRect(rect(), theme::panel());
        painter.setRenderHint(QPainter::Antialiasing);
        if (!template_ || !section_ || section_->top.size() < 2) {
            painter.setPen(theme::textMuted());
            painter.drawText(rect(), Qt::AlignCenter,
                             template_ ? "No components yet: Add one below" : "No template chosen");
            return;
        }
        double left = 0.0;
        double right = 0.0;
        double low = 0.0;
        double high = 0.0;
        const auto take = [&](double offset, double level) {
            left = std::max(left, offset);
            right = std::min(right, offset);
            low = std::min(low, level);
            high = std::max(high, level);
        };
        for (const road::SectionPoint& point : section_->top) {
            take(point.offset, point.level);
        }
        for (const road::SectionPoint& point : section_->formation) {
            take(point.offset, point.level);
        }
        // Room for a batter's stub each side.
        left += 2.0;
        right -= 2.0;
        low -= 0.5;
        high += 0.1;
        const QRectF box = QRectF(rect()).adjusted(12, 12, -12, -24);
        const double scale = box.width() / std::max(left - right, 1e-6);
        const double exaggeration =
            std::clamp(box.height() / std::max((high - low) * scale, 1e-9), 1.0, 10.0);
        const double centreY = box.center().y();
        const double middle = 0.5 * (low + high);
        // Offsets grow to the left: the left side is drawn on the left.
        const auto at = [&](double offset, double level) {
            return QPointF(box.left() + (left - offset) * scale,
                           centreY - (level - middle) * scale * exaggeration);
        };
        for (const road::SectionSolid& solid : section_->solids) {
            const road::Component& component = template_->components[solid.component];
            const QColor fill = solid.course ? colourOf(component.courses[*solid.course].colour)
                                             : colourOf(component.colour);
            QPolygonF polygon;
            for (const road::SolidColumn& column : solid.columns) {
                polygon << at(column.offset, column.top);
            }
            for (auto it = solid.columns.rbegin(); it != solid.columns.rend(); ++it) {
                polygon << at(it->offset, it->bottom);
            }
            painter.setPen(QPen(theme::border(), 0.8));
            painter.setBrush(fill);
            painter.drawPolygon(polygon);
        }
        painter.setBrush(Qt::NoBrush);
        QPainterPath top;
        top.moveTo(at(section_->top.front().offset, section_->top.front().level));
        for (const road::SectionPoint& point : section_->top) {
            top.lineTo(at(point.offset, point.level));
        }
        painter.setPen(QPen(theme::accent(), 2.0));
        painter.drawPath(top);
        // A batter's stub: 2 m out at the side's fill slope.
        for (const road::Side side : {road::Side::Left, road::Side::Right}) {
            const auto components = template_->side(side);
            if (components.empty() || components.back()->kind != road::ComponentKind::Batter) {
                continue;
            }
            const road::SectionPoint& hinge =
                side == road::Side::Left ? section_->top.front() : section_->top.back();
            const double sign = side == road::Side::Left ? 1.0 : -1.0;
            painter.setPen(QPen(theme::textMuted(), 1.0, Qt::DashLine));
            painter.drawLine(
                at(hinge.offset, hinge.level),
                at(hinge.offset + sign * 2.0, hinge.level - 2.0 / components.back()->fillSlope));
        }
        painter.setPen(theme::textMuted());
        painter.drawText(QRectF(rect()).adjusted(8, 0, -8, -4), Qt::AlignBottom | Qt::AlignRight,
                         QString("vertical x%1").arg(exaggeration, 0, 'f', 1));
    }

  private:
    std::optional<road::RoadTemplate> template_;
    std::optional<road::RoadSection> section_;
};

enum ComponentColumn { CSide, CName, CKind, CWidth, CCrossfall, CLayer, CDetail };
enum CourseColumn { KName, KThickness, KMaterial, KIfc };

} // namespace

struct RoadTemplateManagerDialog::Impl {
    RoadTemplateManagerDialog* dialog = nullptr;
    RoadTemplateManagerContext context;
    // The context every connection is made in: deleted first on the way
    // out, which disconnects them all before anything they read is gone.
    std::unique_ptr<QObject> guard = std::make_unique<QObject>();
    std::unique_ptr<DocumentWatcher> watcher;

    QTableWidget* list = nullptr;
    QLineEdit* name = nullptr;
    QTableWidget* components = nullptr;
    QComboBox* addSide = nullptr;
    QComboBox* addKind = nullptr;
    QLineEdit* keys = nullptr;
    QTableWidget* courses = nullptr;
    QLineEdit* courseName = nullptr;
    QLineEdit* courseThickness = nullptr;
    QLineEdit* courseMaterial = nullptr;
    TemplatePreview* preview = nullptr;
    QLabel* status = nullptr;

    // True while a grid is refilled, when its itemChanged is the dialog's
    // own doing and not an edit.
    bool loading = false;
    // The rows edited since each grid was filled, and what it was filled
    // from: a reload that changed nothing it shows keeps the edits.
    std::set<int> editedComponents;
    std::set<int> editedCourses;
    std::optional<road::RoadTemplate> filledFrom;
    QString filledComponent;

    [[nodiscard]] bool alive() const { return watcher != nullptr && watcher->documentAlive(); }

    template <typename Sender, typename Signal, typename Slot>
    void connect(Sender* sender, Signal signal, Slot slot)
    {
        QObject::connect(sender, signal, guard.get(), std::move(slot));
    }

    [[nodiscard]] const road::RoadTemplate* current() const
    {
        if (!alive()) {
            return nullptr;
        }
        const QString chosen = dialog->currentTemplate();
        return chosen.isEmpty() ? nullptr
                                : context.document->roadTemplates().find(chosen.toStdString());
    }

    // The chosen component's reference, side:name; empty for none.
    [[nodiscard]] QString chosenComponent() const
    {
        const int row = components->currentRow();
        if (row < 0 || components->item(row, CSide) == nullptr) {
            return {};
        }
        return components->item(row, CSide)->text() + ":" + components->item(row, CName)->text();
    }

    [[nodiscard]] const road::Component* component(const road::RoadTemplate& roadTemplate,
                                                   const QString& ref) const
    {
        const auto colon = ref.indexOf(':');
        if (colon < 0) {
            return nullptr;
        }
        const auto side = road::sideFrom(ref.left(colon).toStdString());
        if (!side) {
            return nullptr;
        }
        const auto index = roadTemplate.find(*side, ref.mid(colon + 1).toStdString());
        return index ? &roadTemplate.components[*index] : nullptr;
    }

    std::optional<QString> templateWord()
    {
        const QString chosen = dialog->currentTemplate();
        if (chosen.isEmpty()) {
            road_ui::setStatus(status, "Choose a template first, or make one with New.", true);
            return std::nullopt;
        }
        return road_ui::word(status, chosen, "the template's name");
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

    void build();
    void reload();
    void fillComponents(bool force);
    void fillCourses();
    void applyComponents();
    void applyCourses();
};

void RoadTemplateManagerDialog::Impl::build()
{
    dialog->setObjectName("roadTemplateManagerDialog");
    dialog->setWindowTitle("Road Template Manager");
    auto* layout = new QVBoxLayout(dialog);

    // ---- the templates ---------------------------------------------------------------
    list = road_ui::table("roadTemplateList",
                          {"Template", "Left", "Right", "Courses", "Description"}, dialog);
    list->setMaximumHeight(150);
    layout->addWidget(list);
    auto* templateRow = new QHBoxLayout;
    name = road_ui::field("roadTemplateName", "name",
                          "The name New, Copy and Rename give a template", dialog);
    auto* newButton =
        road_ui::button("New", "roadTemplateNew", "ROADTEMPLATE NEW: an empty template", dialog);
    auto* copyButton =
        road_ui::button("Copy", "roadTemplateCopy",
                        "ROADTEMPLATE NEW ... FROM: a copy of the chosen template", dialog);
    auto* renameButton = road_ui::button(
        "Rename", "roadTemplateRename",
        "ROADTEMPLATE RENAME: the chosen template, and the corridors that use it", dialog);
    auto* deleteButton =
        road_ui::button("Delete", "roadTemplateDelete",
                        "ROADTEMPLATE DELETE: refused while a corridor uses it", dialog);
    auto* defaultsButton = road_ui::button(
        "Add Standard Templates", "roadTemplateDefaults",
        "ROADTEMPLATE DEFAULTS: rural two-lane, urban street, divided road", dialog);
    templateRow->addWidget(name, 1);
    for (QPushButton* each : {newButton, copyButton, renameButton, deleteButton, defaultsButton}) {
        templateRow->addWidget(each);
    }
    layout->addLayout(templateRow);

    // ---- components and courses beside the preview ---------------------------------------
    auto* splitter = new QSplitter(Qt::Vertical, dialog);
    auto* upper = new QWidget(splitter);
    auto* upperLayout = new QVBoxLayout(upper);
    upperLayout->setContentsMargins(0, 0, 0, 0);
    upperLayout->addWidget(road_ui::muted(
        "Components out from the centreline, each side in order. Width, crossfall (percent, "
        "outward: -3 falls) and layer are edited in place and applied together.",
        upper));
    components = road_ui::table(
        "roadTemplateComponents",
        {"Side", "Name", "Kind", "Width (m)", "Crossfall (%)", "Layer", "What else"}, upper);
    components->setEditTriggers(QAbstractItemView::DoubleClicked |
                                QAbstractItemView::EditKeyPressed |
                                QAbstractItemView::AnyKeyPressed);
    upperLayout->addWidget(components, 1);
    auto* componentRow = new QHBoxLayout;
    auto* applyComponentsButton = road_ui::button(
        "Apply", "roadTemplateApplyComponents",
        "ROADTEMPLATE SET for each edited row: its width, crossfall and layer", upper);
    auto* revertComponentsButton = road_ui::button("Revert", "roadTemplateRevertComponents",
                                                   "The grid as the drawing holds it again", upper);
    addSide = new QComboBox(upper);
    addSide->setObjectName("roadTemplateAddSide");
    addSide->addItems({"both", "left", "right"});
    addKind = new QComboBox(upper);
    addKind->setObjectName("roadTemplateAddKind");
    for (const auto kind :
         {road::ComponentKind::Lane, road::ComponentKind::Shoulder, road::ComponentKind::Kerb,
          road::ComponentKind::Footpath, road::ComponentKind::Verge, road::ComponentKind::Median,
          road::ComponentKind::Drain, road::ComponentKind::Batter}) {
        addKind->addItem(qs(std::string(road::toString(kind))));
    }
    auto* addButton = road_ui::button(
        "Add", "roadTemplateAdd",
        "ROADTEMPLATE ADD: a component of this kind on this side, before the batter", upper);
    auto* removeButton = road_ui::button("Remove", "roadTemplateRemove",
                                         "ROADTEMPLATE REMOVE: the chosen component", upper);
    componentRow->addWidget(applyComponentsButton);
    componentRow->addWidget(revertComponentsButton);
    componentRow->addStretch(1);
    componentRow->addWidget(addSide);
    componentRow->addWidget(addKind);
    componentRow->addWidget(addButton);
    componentRow->addWidget(removeButton);
    upperLayout->addLayout(componentRow);
    auto* keysRow = new QHBoxLayout;
    keys = road_ui::field(
        "roadTemplateComponentKeys", "shape=kerbgutter height=0.15  |  cut=2 fill=4 benchheight=",
        "key=value for the chosen component: a kerb's shape height depth gutter gutterfall "
        "material, a drain's depth, a batter's cut fill maxwidth benchheight benchwidth, "
        "colour=, superelevated=on|off, name=",
        upper);
    auto* setKeysButton =
        road_ui::button("Set", "roadTemplateSetKeys", "ROADTEMPLATE SET with these keys", upper);
    keysRow->addWidget(keys, 1);
    keysRow->addWidget(setKeysButton);
    upperLayout->addLayout(keysRow);

    upperLayout->addWidget(road_ui::muted(
        "The chosen component's courses, top down. Thickness, material and IFC type are edited "
        "in place and applied together.",
        upper));
    courses = road_ui::table("roadTemplateCourses",
                             {"Course", "Thickness (m)", "Material", "IFC type"}, upper);
    courses->setEditTriggers(QAbstractItemView::DoubleClicked | QAbstractItemView::EditKeyPressed |
                             QAbstractItemView::AnyKeyPressed);
    upperLayout->addWidget(courses, 1);
    auto* courseRow = new QHBoxLayout;
    auto* applyCoursesButton =
        road_ui::button("Apply", "roadTemplateApplyCourses",
                        "ROADTEMPLATE COURSE ... SET for each edited row", upper);
    courseName =
        road_ui::field("roadTemplateCourseName", "course", "A new course's name (a word)", upper);
    courseThickness = road_ui::field("roadTemplateCourseThickness", "thickness m",
                                     "A new course's thickness, metres", upper);
    courseMaterial =
        road_ui::field("roadTemplateCourseMaterial", "material",
                       "A new course's material: asphalt, concrete, crushed rock ...", upper);
    auto* courseAddButton =
        road_ui::button("Add Course", "roadTemplateCourseAdd",
                        "ROADTEMPLATE COURSE ... ADD: under the chosen component's courses", upper);
    auto* courseRemoveButton =
        road_ui::button("Remove Course", "roadTemplateCourseRemove",
                        "ROADTEMPLATE COURSE ... REMOVE: the chosen course", upper);
    courseRow->addWidget(applyCoursesButton);
    courseRow->addStretch(1);
    courseRow->addWidget(courseName);
    courseRow->addWidget(courseThickness);
    courseRow->addWidget(courseMaterial);
    courseRow->addWidget(courseAddButton);
    courseRow->addWidget(courseRemoveButton);
    upperLayout->addLayout(courseRow);
    preview = new TemplatePreview(splitter);
    splitter->addWidget(upper);
    splitter->addWidget(preview);
    splitter->setStretchFactor(0, 3);
    splitter->setStretchFactor(1, 2);
    layout->addWidget(splitter, 1);

    status = new QLabel(dialog);
    status->setObjectName("roadTemplateStatus");
    status->setWordWrap(true);
    layout->addWidget(status);
    auto* closeRow = new QHBoxLayout;
    closeRow->addStretch(1);
    auto* closeButton = road_ui::button("Close", "roadTemplateClose", "Close the manager", dialog);
    closeRow->addWidget(closeButton);
    layout->addLayout(closeRow);

    // ---- what each does ---------------------------------------------------------------
    connect(list, &QTableWidget::itemSelectionChanged, [this] {
        if (!loading) {
            editedComponents.clear();
            fillComponents(true);
        }
    });
    connect(components, &QTableWidget::itemSelectionChanged, [this] {
        if (!loading) {
            fillCourses();
        }
    });
    connect(components, &QTableWidget::itemChanged, [this](QTableWidgetItem* item) {
        if (!loading && item != nullptr) {
            editedComponents.insert(item->row());
        }
    });
    connect(courses, &QTableWidget::itemChanged, [this](QTableWidgetItem* item) {
        if (!loading && item != nullptr) {
            editedCourses.insert(item->row());
        }
    });
    connect(newButton, &QPushButton::clicked, [this] {
        if (const auto word = road_ui::word(status, name->text().trimmed(), "the new name")) {
            if (run("ROADTEMPLATE NEW " + *word)) {
                dialog->selectTemplate(name->text().trimmed());
            }
        }
    });
    connect(copyButton, &QPushButton::clicked, [this] {
        const auto from = templateWord();
        const auto word = road_ui::word(status, name->text().trimmed(), "the new name");
        if (from && word && run("ROADTEMPLATE NEW " + *word + " FROM " + *from)) {
            dialog->selectTemplate(name->text().trimmed());
        }
    });
    connect(renameButton, &QPushButton::clicked, [this] {
        const auto from = templateWord();
        const auto word = road_ui::word(status, name->text().trimmed(), "the new name");
        if (from && word && run("ROADTEMPLATE RENAME " + *from + " " + *word)) {
            dialog->selectTemplate(name->text().trimmed());
        }
    });
    connect(deleteButton, &QPushButton::clicked, [this] {
        if (const auto word = templateWord()) {
            run("ROADTEMPLATE DELETE " + *word);
        }
    });
    connect(defaultsButton, &QPushButton::clicked, [this] { run("ROADTEMPLATE DEFAULTS"); });
    connect(applyComponentsButton, &QPushButton::clicked, [this] { applyComponents(); });
    connect(revertComponentsButton, &QPushButton::clicked, [this] {
        editedComponents.clear();
        fillComponents(true);
    });
    connect(addButton, &QPushButton::clicked, [this] {
        if (const auto word = templateWord()) {
            run("ROADTEMPLATE ADD " + *word + " " + addSide->currentText() + " " +
                addKind->currentText());
        }
    });
    connect(removeButton, &QPushButton::clicked, [this] {
        const auto word = templateWord();
        const QString ref = chosenComponent();
        if (!word) {
            return;
        }
        if (ref.isEmpty()) {
            road_ui::setStatus(status, "Choose a component to remove.", true);
            return;
        }
        run("ROADTEMPLATE REMOVE " + *word + " " + ref);
    });
    connect(setKeysButton, &QPushButton::clicked, [this] {
        const auto word = templateWord();
        const QString ref = chosenComponent();
        if (!word) {
            return;
        }
        if (ref.isEmpty() || keys->text().trimmed().isEmpty()) {
            road_ui::setStatus(status, "Choose a component and type key=value to set.", true);
            return;
        }
        if (run("ROADTEMPLATE SET " + *word + " " + ref + " " + keys->text().trimmed())) {
            keys->clear();
        }
    });
    connect(applyCoursesButton, &QPushButton::clicked, [this] { applyCourses(); });
    connect(courseAddButton, &QPushButton::clicked, [this] {
        const auto word = templateWord();
        const QString ref = chosenComponent();
        if (!word) {
            return;
        }
        if (ref.isEmpty()) {
            road_ui::setStatus(status, "Choose the component the course is under.", true);
            return;
        }
        const auto courseWord = road_ui::word(status, courseName->text().trimmed(), "the course");
        const auto material = road_ui::option(status, "material", courseMaterial->text().trimmed());
        if (!courseWord || !material) {
            return;
        }
        QString line = "ROADTEMPLATE COURSE " + *word + " " + ref + " ADD " + *courseWord +
                       " thickness=" + courseThickness->text().trimmed();
        if (!courseMaterial->text().trimmed().isEmpty()) {
            line += " " + *material;
        }
        if (run(line)) {
            courseName->clear();
            courseThickness->clear();
            courseMaterial->clear();
        }
    });
    connect(courseRemoveButton, &QPushButton::clicked, [this] {
        const auto word = templateWord();
        const QString ref = chosenComponent();
        const int row = courses->currentRow();
        if (!word) {
            return;
        }
        if (ref.isEmpty() || row < 0) {
            road_ui::setStatus(status, "Choose the course to remove.", true);
            return;
        }
        run("ROADTEMPLATE COURSE " + *word + " " + ref + " REMOVE " +
            courses->item(row, KName)->text());
    });
    connect(closeButton, &QPushButton::clicked, [this] { dialog->close(); });
}

void RoadTemplateManagerDialog::Impl::applyComponents()
{
    const auto word = templateWord();
    if (!word) {
        return;
    }
    if (editedComponents.empty()) {
        road_ui::setStatus(status, "Nothing is edited to apply.", false);
        return;
    }
    // Every row checked before any line runs, so a cell that is not one
    // number refuses the whole Apply rather than half of it.
    QStringList lines;
    for (const int row : editedComponents) {
        if (components->item(row, CSide) == nullptr) {
            continue;
        }
        const QString ref =
            components->item(row, CSide)->text() + ":" + components->item(row, CName)->text();
        QString line = "ROADTEMPLATE SET " + *word + " " + ref;
        for (const auto& [column, key] :
             {std::pair{CWidth, QString("width")}, std::pair{CCrossfall, QString("crossfall")}}) {
            const QTableWidgetItem* item = components->item(row, column);
            if (item == nullptr || !(item->flags() & Qt::ItemIsEditable)) {
                continue;
            }
            if (!road_ui::number(item->text())) {
                road_ui::setStatus(status,
                                   ref + ": the " + key + " '" + item->text() +
                                       "' is not a number; type one number, with a decimal point",
                                   true);
                return;
            }
            line += " " + key + "=" + item->text().trimmed();
        }
        const auto layer =
            road_ui::option(status, "layer", components->item(row, CLayer)->text().trimmed());
        if (!layer) {
            return;
        }
        line += " " + *layer;
        lines << line;
    }
    for (const QString& line : lines) {
        if (!road_ui::run(context.run, status, line)) {
            reload();
            return;
        }
    }
    editedComponents.clear();
    reload();
}

void RoadTemplateManagerDialog::Impl::applyCourses()
{
    const auto word = templateWord();
    const QString ref = chosenComponent();
    if (!word) {
        return;
    }
    if (ref.isEmpty() || editedCourses.empty()) {
        road_ui::setStatus(status, "Nothing is edited to apply.", false);
        return;
    }
    QStringList lines;
    for (const int row : editedCourses) {
        if (courses->item(row, KName) == nullptr) {
            continue;
        }
        const QString thickness = courses->item(row, KThickness)->text().trimmed();
        if (!road_ui::number(thickness)) {
            road_ui::setStatus(status,
                               courses->item(row, KName)->text() + ": the thickness '" + thickness +
                                   "' is not a number",
                               true);
            return;
        }
        const auto material =
            road_ui::option(status, "material", courses->item(row, KMaterial)->text().trimmed());
        const auto ifc = road_ui::option(status, "ifc", courses->item(row, KIfc)->text().trimmed());
        if (!material || !ifc) {
            return;
        }
        lines << "ROADTEMPLATE COURSE " + *word + " " + ref + " SET " +
                     courses->item(row, KName)->text() + " thickness=" + thickness + " " +
                     *material + " " + *ifc;
    }
    for (const QString& line : lines) {
        if (!road_ui::run(context.run, status, line)) {
            reload();
            return;
        }
    }
    editedCourses.clear();
    reload();
}

void RoadTemplateManagerDialog::Impl::reload()
{
    if (!alive()) {
        return;
    }
    const QString chosen = dialog->currentTemplate();
    const road::RoadTemplateSet& set = context.document->roadTemplates();
    loading = true;
    list->setRowCount(0);
    for (const road::RoadTemplate& each : set.templates) {
        const int row = list->rowCount();
        list->insertRow(row);
        std::size_t courseCount = 0;
        for (const road::Component& component : each.components) {
            courseCount += component.courses.size();
        }
        list->setItem(row, 0, road_ui::cell(qs(each.name)));
        list->setItem(row, 1, road_ui::cell(QString::number(each.side(road::Side::Left).size())));
        list->setItem(row, 2, road_ui::cell(QString::number(each.side(road::Side::Right).size())));
        list->setItem(row, 3, road_ui::cell(QString::number(courseCount)));
        list->setItem(row, 4, road_ui::cell(qs(each.description)));
    }
    // Chosen again quietly: a reload is not a choice, and keeps the edits.
    if (!chosen.isEmpty()) {
        dialog->selectTemplate(chosen);
    }
    loading = false;
    fillComponents(false);
    if (const auto readable = context.document->roadTemplatesStatus(); !readable) {
        road_ui::setStatus(status, qs(readable.error().describe()), true);
    }
}

void RoadTemplateManagerDialog::Impl::fillComponents(bool force)
{
    const road::RoadTemplate* roadTemplate = current();
    preview->setTemplate(roadTemplate != nullptr ? std::optional(*roadTemplate) : std::nullopt);
    // Unapplied edits survive a reload that did not change the template.
    if (!force && !editedComponents.empty() && roadTemplate != nullptr && filledFrom &&
        *filledFrom == *roadTemplate) {
        return;
    }
    const QString keep = chosenComponent();
    loading = true;
    components->setRowCount(0);
    editedComponents.clear();
    filledFrom.reset();
    if (roadTemplate != nullptr) {
        filledFrom = *roadTemplate;
        for (const road::Side side : {road::Side::Left, road::Side::Right}) {
            for (const road::Component* each : roadTemplate->side(side)) {
                const int row = components->rowCount();
                components->insertRow(row);
                const bool hasWidth = each->kind != road::ComponentKind::Batter;
                const bool hasCrossfall = each->kind != road::ComponentKind::Batter &&
                                          (each->kind != road::ComponentKind::Kerb ||
                                           each->shape == road::KerbShape::EdgeStrip);
                components->setItem(row, CSide,
                                    road_ui::cell(qs(std::string(road::toString(side)))));
                components->setItem(row, CName, road_ui::cell(qs(each->name)));
                components->setItem(row, CKind,
                                    road_ui::cell(qs(std::string(road::toString(each->kind)))));
                components->setItem(
                    row, CWidth,
                    road_ui::cell(hasWidth ? exactNumber(each->width) : QString(), hasWidth));
                components->setItem(
                    row, CCrossfall,
                    road_ui::cell(hasCrossfall ? exactNumber(each->crossfall) : QString(),
                                  hasCrossfall));
                components->setItem(row, CLayer, road_ui::cell(qs(each->layer), true));
                components->setItem(row, CDetail, road_ui::cell(detailOf(*each)));
                components->item(row, CSide)
                    ->setForeground(QBrush(colourOf(each->colour).lighter(150)));
            }
        }
    }
    loading = false;
    for (int row = 0; row < components->rowCount(); ++row) {
        if (components->item(row, CSide)->text() + ":" + components->item(row, CName)->text() ==
            keep) {
            components->selectRow(row);
            break;
        }
    }
    fillCourses();
}

void RoadTemplateManagerDialog::Impl::fillCourses()
{
    const road::RoadTemplate* roadTemplate = current();
    const QString ref = chosenComponent();
    const road::Component* chosen =
        roadTemplate != nullptr ? component(*roadTemplate, ref) : nullptr;
    if (!editedCourses.empty() && ref == filledComponent && chosen != nullptr) {
        return;
    }
    loading = true;
    courses->setRowCount(0);
    editedCourses.clear();
    filledComponent = ref;
    if (chosen != nullptr) {
        for (const road::PavementCourse& course : chosen->courses) {
            const int row = courses->rowCount();
            courses->insertRow(row);
            courses->setItem(row, KName, road_ui::cell(qs(course.name)));
            courses->setItem(row, KThickness, road_ui::cell(exactNumber(course.thickness), true));
            courses->setItem(row, KMaterial, road_ui::cell(qs(course.material), true));
            courses->setItem(row, KIfc, road_ui::cell(qs(course.ifcType), true));
            courses->item(row, KName)->setBackground(QBrush(colourOf(course.colour)));
        }
    }
    loading = false;
}

// =====================================================================================

RoadTemplateManagerDialog::RoadTemplateManagerDialog(RoadTemplateManagerContext context,
                                                     QWidget* parent)
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
    resize(900, 720);
}

RoadTemplateManagerDialog::~RoadTemplateManagerDialog()
{
    // The connections first: they read the widgets and the document.
    impl_->guard.reset();
}

bool RoadTemplateManagerDialog::selectTemplate(const QString& name)
{
    for (int row = 0; row < impl_->list->rowCount(); ++row) {
        if (impl_->list->item(row, 0)->text().compare(name, Qt::CaseInsensitive) == 0) {
            impl_->list->selectRow(row);
            return true;
        }
    }
    return false;
}

QString RoadTemplateManagerDialog::currentTemplate() const
{
    const int row = impl_->list->currentRow();
    if (row < 0 || impl_->list->item(row, 0) == nullptr) {
        return {};
    }
    return impl_->list->item(row, 0)->text();
}

} // namespace katana::qt
