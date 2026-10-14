// BIM > Road Objects' dialog (bim/road_object_dialog.hpp).

#include "bim/road_object_dialog.hpp"

#include <QButtonGroup>
#include <QComboBox>
#include <QFormLayout>
#include <QGridLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPainter>
#include <QPlainTextEdit>
#include <QPolygonF>
#include <QPushButton>
#include <QRadioButton>
#include <QShowEvent>
#include <QSignalBlocker>
#include <QStringList>
#include <QTabWidget>
#include <QVBoxLayout>
#include <QWidget>

#include <algorithm>
#include <cmath>
#include <functional>
#include <limits>

#include "customisation/document_watcher.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/terrain/surface_store.hpp"

namespace katana::qt {

using katana::cad::road::KindShape;
using katana::cad::road::ParameterSpec;
using katana::cad::road::ParameterType;
using katana::cad::road::RoadObjectKind;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

QString qs(std::string_view text)
{
    return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
}

// A value as a line takes it: as it is when it is one plain word, else in
// double quotes - which a line has no way to type inside one.
Result<QString> lineWord(const QString& value, std::string_view field)
{
    if (value.contains('"')) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string(field) + " cannot hold a double quote: a line has no way "
                                              "to type one",
                         value.toStdString());
    }
    if (value.isEmpty() || value.contains(' ') || value.contains('\t')) {
        return "\"" + value + "\"";
    }
    return value;
}

Result<QString> numberWord(const QString& text, std::string_view field)
{
    const QString trimmed = text.trimmed();
    if (!katana::core::parseFiniteDouble(trimmed.toStdString())) {
        return makeError(ErrorCode::InvalidArgument, std::string(field) + " must be a number",
                         text.toStdString());
    }
    return trimmed;
}

// "x,y" or "x,y,z".
bool readsAsPoint(const QString& word)
{
    const QStringList parts = word.split(',');
    if (parts.size() != 2 && parts.size() != 3) {
        return false;
    }
    return std::ranges::all_of(parts, [](const QString& part) {
        return katana::core::parseFiniteDouble(part.toStdString()).has_value();
    });
}

QColor qColour(const katana::entity::Color& colour)
{
    return QColor(colour.r, colour.g, colour.b, colour.a);
}

// The object the parameters make, drawn as it is seen: a barrier, a wall, a
// fence, a marking along its line - its cross-section; a post, a sign, a
// light pole from the front; a culvert across its barrel. The ground is the
// line at level 0; a one metre bar gives the scale.
class PreviewWidget final : public QWidget {
  public:
    explicit PreviewWidget(QWidget* parent) : QWidget(parent)
    {
        setObjectName("roadObjectPreview");
        setMinimumSize(220, 160);
    }

    void showModel(katana::cad::bim::BimModel model, bool acrossX)
    {
        model_ = std::move(model);
        acrossX_ = acrossX;
        update();
    }

  protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        painter.setRenderHint(QPainter::Antialiasing, true);
        painter.fillRect(rect(), palette().base());
        if (model_.objects.empty()) {
            painter.setPen(palette().text().color());
            painter.drawText(rect().adjusted(8, 8, -8, -8), Qt::AlignCenter | Qt::TextWordWrap,
                             model_.problems.empty()
                                 ? QString("Nothing to show")
                                 : qs(model_.problems.front().message));
            return;
        }
        // The view's horizontal axis: y along an alignment's x, x across a
        // culvert's.
        const auto across = [this](const katana::geometry::Point3& p) {
            return acrossX_ ? p.x : -p.y;
        };
        double h0 = std::numeric_limits<double>::infinity();
        double h1 = -h0;
        double z0 = 0.0;
        double z1 = 0.0;
        for (const auto& object : model_.objects) {
            for (const auto& vertex : object.mesh.vertices) {
                h0 = std::min(h0, across(vertex));
                h1 = std::max(h1, across(vertex));
                z0 = std::min(z0, vertex.z);
                z1 = std::max(z1, vertex.z);
            }
        }
        const double margin = 16.0;
        const double spanH = std::max(h1 - h0, 1.0);
        const double spanZ = std::max(z1 - z0, 1.0);
        const double scale = std::min((this->width() - 2 * margin) / spanH,
                                      (this->height() - 2 * margin - 14) / spanZ);
        if (!(scale > 0.0)) {
            return;
        }
        const double centreH = (h0 + h1) / 2.0;
        const auto map = [&](double h, double z) {
            return QPointF(this->width() / 2.0 + (h - centreH) * scale,
                           this->height() - margin - 14 - (z - z0) * scale);
        };
        for (const auto& object : model_.objects) {
            painter.setPen(Qt::NoPen);
            painter.setBrush(qColour(object.colour));
            for (const auto& face : object.mesh.faces) {
                QPolygonF triangle;
                for (const std::uint32_t index : face) {
                    const auto& vertex = object.mesh.vertices[index];
                    triangle << map(across(vertex), vertex.z);
                }
                painter.drawPolygon(triangle);
            }
        }
        // The ground, and a metre for the scale.
        painter.setPen(QPen(palette().text().color(), 1.0));
        painter.drawLine(QPointF(margin / 2, map(0, 0).y()),
                         QPointF(this->width() - margin / 2, map(0, 0).y()));
        const QPointF bar(margin, this->height() - 8.0);
        painter.drawLine(bar, bar + QPointF(scale, 0));
        painter.drawText(bar + QPointF(scale + 4, 4), "1 m");
    }

  private:
    katana::cad::bim::BimModel model_;
    bool acrossX_ = false;
};

// One page's parameter field: a text field, or a choice whose first item is
// the default (no data) and the rest the kind's words.
struct ParameterField {
    std::string key;
    QLineEdit* text = nullptr;
    QComboBox* choice = nullptr;

    [[nodiscard]] QString value() const
    {
        if (choice != nullptr) {
            return choice->currentData().toString();
        }
        return text != nullptr ? text->text().trimmed() : QString();
    }
};

} // namespace

Result<QString> roadObjectAddLine(const RoadObjectForm& form)
{
    QStringList words{"ROADOBJECT", "ADD", qs(kindWord(form.kind))};
    const auto text = [&words](const char* key, const QString& value) -> Status {
        if (value.trimmed().isEmpty()) {
            return {};
        }
        auto said = lineWord(value.trimmed(), key);
        if (!said) {
            return said.error();
        }
        words << QString(key) + "=" + *said;
        return {};
    };
    const auto number = [&words](const char* key, const QString& value) -> Status {
        if (value.trimmed().isEmpty()) {
            return {};
        }
        auto said = numberWord(value, key);
        if (!said) {
            return said.error();
        }
        words << QString(key) + "=" + *said;
        return {};
    };
    // The object's own keys first: after a scope's WHERE every key=value is
    // a condition.
    for (const auto& [key, value] : {std::pair{"name", form.name}, std::pair{"layer", form.layer},
                                     std::pair{"colour", form.colour},
                                     std::pair{"material", form.material},
                                     std::pair{"facility", form.facility},
                                     std::pair{"heights", form.heights},
                                     std::pair{"surface", form.surface}}) {
        if (auto status = text(key, value); !status) {
            return status.error();
        }
    }
    for (const auto& [key, value] :
         {std::pair{"crossfall", form.crossfall}, std::pair{"dz", form.dz}}) {
        if (auto status = number(key, value); !status) {
            return status.error();
        }
    }
    for (const auto& [key, value] : form.parameters) {
        const ParameterSpec* spec =
            katana::cad::road::findParameter(form.kind, key.toStdString());
        const std::string field = key.toStdString();
        const Status status = spec != nullptr && spec->type == ParameterType::Number
                                  ? number(field.c_str(), value)
                                  : text(field.c_str(), value);
        if (!status) {
            return status.error();
        }
    }
    switch (form.place) {
    case RoadObjectForm::Place::Alignment: {
        if (form.alignment.trimmed().isEmpty()) {
            return makeError(ErrorCode::InvalidArgument, "choose the alignment");
        }
        if (form.station.trimmed().isEmpty()) {
            return makeError(ErrorCode::InvalidArgument, "give the station");
        }
        auto name = lineWord(form.alignment.trimmed(), "the alignment");
        if (!name) {
            return name.error();
        }
        words << "ALIGNMENT" << *name;
        for (const auto& [key, value] :
             {std::pair{"station", form.station}, std::pair{"to", form.to},
              std::pair{"offset", form.offset}, std::pair{"spacing", form.spacing}}) {
            if (auto status = number(key, value); !status) {
                return status.error();
            }
        }
        break;
    }
    case RoadObjectForm::Place::Lines:
    case RoadObjectForm::Place::DrawnPoints:
        if (form.scope.trimmed().isEmpty()) {
            return makeError(ErrorCode::InvalidArgument, "say which part of the drawing");
        }
        if (auto status = number("spacing", form.spacing); !status) {
            return status.error();
        }
        // The scope ends the line, its WHERE with it.
        words << (form.place == RoadObjectForm::Place::Lines ? "ALONG" : "AT") << "FROM"
              << form.scope.trimmed();
        break;
    case RoadObjectForm::Place::Points: {
        const QStringList points = form.points.split(' ', Qt::SkipEmptyParts);
        if (points.isEmpty()) {
            return makeError(ErrorCode::InvalidArgument, "give the points: x,y[,z] x,y[,z] ...");
        }
        for (const QString& point : points) {
            if (!readsAsPoint(point)) {
                return makeError(ErrorCode::InvalidArgument, "a point is x,y or x,y,z",
                                 point.toStdString());
            }
        }
        words << "AT" << points;
        break;
    }
    }
    if (form.preview) {
        words << "PREVIEW";
    }
    return words.join(' ');
}

katana::cad::bim::BimModel
roadObjectPreview(RoadObjectKind kind,
                  const std::vector<std::pair<std::string, std::string>>& parameters)
{
    // A straight road 30 m long on level ground.
    katana::entity::Alignment road;
    road.name = "preview";
    road.horizontal.pis = {katana::geometry::AlignmentPI{katana::geometry::Point2(0, 0)},
                           katana::geometry::AlignmentPI{katana::geometry::Point2(30, 0)}};
    road.vertical = katana::geometry::VerticalAlignment{
        {katana::geometry::ProfilePVI{0.0, 0.0}, katana::geometry::ProfilePVI{30.0, 0.0}}};
    katana::cad::road::RoadObject object;
    object.id = "preview";
    object.kind = kind;
    object.placement.alignment = road.name;
    switch (katana::cad::road::shapeOf(kind)) {
    case KindShape::Linear:
        object.placement.station = 10.0;
        object.placement.to = 20.0;
        break;
    case KindShape::Point:
        object.placement.station = 15.0;
        object.placement.offset = 2.0;
        break;
    case KindShape::Crossing:
        object.placement.station = 15.0;
        break;
    }
    for (const auto& [key, value] : parameters) {
        // What does not read is left at its default: the line will say why.
        (void)katana::cad::road::applyKey(object, key, value);
    }
    katana::cad::road::PlacementContext context;
    context.alignment = [&road](std::string_view name) -> const katana::entity::Alignment* {
        return name == road.name ? &road : nullptr;
    };
    return katana::cad::road::buildRoadObject(context, object);
}

struct RoadObjectDialog::Impl {
    RoadObjectDialogContext context;
    QTabWidget* kinds = nullptr;
    std::vector<std::vector<ParameterField>> pages;
    QLabel* source = nullptr;
    PreviewWidget* preview = nullptr;
    QRadioButton* placeAlignment = nullptr;
    QRadioButton* placeLines = nullptr;
    QRadioButton* placePoints = nullptr;
    QRadioButton* placeDrawnPoints = nullptr;
    QComboBox* alignment = nullptr;
    QLineEdit* station = nullptr;
    QLineEdit* to = nullptr;
    QLineEdit* offset = nullptr;
    QLineEdit* spacing = nullptr;
    QLineEdit* points = nullptr;
    ScopeFilterWidget* scope = nullptr;
    QComboBox* heights = nullptr;
    QComboBox* surface = nullptr;
    QLineEdit* crossfall = nullptr;
    QLineEdit* dz = nullptr;
    QLineEdit* name = nullptr;
    QLineEdit* layer = nullptr;
    QLineEdit* colour = nullptr;
    QLineEdit* material = nullptr;
    QLineEdit* facility = nullptr;
    QLineEdit* line = nullptr;
    QPushButton* previewRun = nullptr;
    QPushButton* add = nullptr;
    QPlainTextEdit* reply = nullptr;
    QLabel* status = nullptr;
    // Makes the line again from the fields, and the preview: every edit's.
    std::function<void()> refresh;
    // Last, so it goes first: it calls back into the fields above.
    std::unique_ptr<DocumentWatcher> watcher;

    [[nodiscard]] bool documentAlive() const
    {
        return context.document != nullptr && watcher != nullptr && watcher->documentAlive();
    }
};

RoadObjectDialog::RoadObjectDialog(RoadObjectDialogContext context, QWidget* parent)
    : QDialog(parent), impl_(std::make_unique<Impl>())
{
    Impl& d = *impl_;
    d.context = std::move(context);
    setObjectName("roadObjectDialog");
    setWindowTitle("Road Objects");
    setModal(false);

    const auto named = [](auto* widget, const char* name) {
        widget->setObjectName(name);
        return widget;
    };
    const auto field = [&named](const char* name, const QString& placeholder) {
        auto* edit = named(new QLineEdit, name);
        edit->setPlaceholderText(placeholder);
        return edit;
    };

    // ---- a page per kind ----
    d.kinds = named(new QTabWidget, "roadObjectKinds");
    for (const RoadObjectKind kind : katana::cad::road::roadObjectKinds()) {
        auto* page = new QWidget;
        page->setObjectName(QString("roadObjectPage_%1").arg(qs(kindWord(kind))));
        auto* form = new QFormLayout(page);
        std::vector<ParameterField> fields;
        for (const ParameterSpec& spec : katana::cad::road::parametersOf(kind)) {
            ParameterField made;
            made.key = std::string(spec.key);
            const QString objectName =
                QString("roadObject_%1_%2").arg(qs(kindWord(kind)), qs(spec.key));
            QString label = qs(spec.key);
            if (!spec.unit.empty()) {
                label += QString(" (%1)").arg(qs(spec.unit));
            }
            QWidget* widget = nullptr;
            if (spec.type == ParameterType::Choice) {
                made.choice = new QComboBox;
                made.choice->setObjectName(objectName);
                made.choice->addItem(QString("default (%1)").arg(qs(spec.defaultText)), QString());
                for (const std::string_view choice : spec.choices) {
                    made.choice->addItem(qs(choice), qs(choice));
                }
                widget = made.choice;
            } else {
                made.text = new QLineEdit;
                made.text->setObjectName(objectName);
                made.text->setPlaceholderText(
                    spec.defaultText.empty() ? QString("none")
                                             : QString("default: %1").arg(qs(spec.defaultText)));
                widget = made.text;
            }
            widget->setToolTip(qs(spec.description));
            form->addRow(label, widget);
            fields.push_back(made);
        }
        d.pages.push_back(std::move(fields));
        d.kinds->addTab(page, qs(kindTitle(kind)));
    }
    d.source = named(new QLabel, "roadObjectKindSource");
    d.source->setWordWrap(true);
    d.preview = new PreviewWidget(this);

    // ---- where it goes ----
    auto* placement = named(new QGroupBox("Placement"), "roadObjectPlacement");
    d.placeAlignment = named(new QRadioButton("Along an &alignment"), "roadObjectPlaceAlignment");
    d.placeLines = named(new QRadioButton("Along &lines in the drawing"), "roadObjectPlaceLines");
    d.placePoints = named(new QRadioButton("At points &typed"), "roadObjectPlacePoints");
    d.placeDrawnPoints =
        named(new QRadioButton("At points in the &drawing"), "roadObjectPlaceDrawnPoints");
    auto* places = new QButtonGroup(this);
    for (QRadioButton* button :
         {d.placeAlignment, d.placeLines, d.placePoints, d.placeDrawnPoints}) {
        places->addButton(button);
    }
    d.placeAlignment->setChecked(true);
    d.alignment = named(new QComboBox, "roadObjectAlignment");
    d.station = field("roadObjectStation", "chainage");
    d.to = field("roadObjectTo", "end chainage");
    d.offset = field("roadObjectOffset", "0 (left of the centreline +)");
    d.spacing = field("roadObjectSpacing", "one every ... m");
    d.points = field("roadObjectPoints", "x,y[,z] x,y[,z] ...");
    d.scope = new ScopeFilterWidget("roadObjectSource", this);
    d.scope->views = d.context.views;
    d.scope->setChoice(ScopeChoice::Selection);
    auto* placeGrid = new QGridLayout(placement);
    placeGrid->addWidget(d.placeAlignment, 0, 0);
    placeGrid->addWidget(d.alignment, 0, 1);
    placeGrid->addWidget(new QLabel("station"), 0, 2);
    placeGrid->addWidget(d.station, 0, 3);
    placeGrid->addWidget(new QLabel("to"), 0, 4);
    placeGrid->addWidget(d.to, 0, 5);
    placeGrid->addWidget(new QLabel("offset"), 1, 2);
    placeGrid->addWidget(d.offset, 1, 3);
    placeGrid->addWidget(new QLabel("spacing"), 1, 4);
    placeGrid->addWidget(d.spacing, 1, 5);
    placeGrid->addWidget(d.placePoints, 2, 0);
    placeGrid->addWidget(d.points, 2, 1, 1, 5);
    placeGrid->addWidget(d.placeLines, 3, 0);
    placeGrid->addWidget(d.placeDrawnPoints, 3, 1);
    placeGrid->addWidget(d.scope, 4, 0, 1, 6);

    // ---- what it stands on, and what it is called ----
    auto* heights = named(new QGroupBox("Heights"), "roadObjectHeightsGroup");
    d.heights = named(new QComboBox, "roadObjectHeights");
    d.heights->addItem("automatic", QString());
    for (const char* word : {"profile", "surface", "line"}) {
        d.heights->addItem(word, QString(word));
    }
    d.surface = named(new QComboBox, "roadObjectSurface");
    d.surface->setEditable(true);
    d.crossfall = field("roadObjectCrossfall", "0 %");
    d.dz = field("roadObjectDz", "0 m");
    auto* heightForm = new QFormLayout(heights);
    heightForm->addRow("Heights from", d.heights);
    heightForm->addRow("Surface", d.surface);
    heightForm->addRow("Crossfall (%)", d.crossfall);
    heightForm->addRow("Raised by (m)", d.dz);
    auto* common = named(new QGroupBox("Object"), "roadObjectCommon");
    d.name = field("roadObjectName", "the kind and its id");
    d.layer = field("roadObjectLayer", "road/objects/<kind>");
    d.colour = field("roadObjectColour", "#RRGGBB or a colour name");
    d.material = field("roadObjectMaterial", "the kind's");
    d.facility = field("roadObjectFacility", "the road it is part of");
    auto* commonForm = new QFormLayout(common);
    commonForm->addRow("Name", d.name);
    commonForm->addRow("Layer", d.layer);
    commonForm->addRow("Colour", d.colour);
    commonForm->addRow("Material", d.material);
    commonForm->addRow("Road", d.facility);

    // ---- the line, and running it ----
    d.line = named(new QLineEdit, "roadObjectLine");
    d.line->setReadOnly(true);
    d.previewRun = named(new QPushButton("&Preview"), "roadObjectPreviewRun");
    d.add = named(new QPushButton("A&dd"), "roadObjectAdd");
    auto* close = named(new QPushButton("&Close"), "roadObjectClose");
    d.reply = named(new QPlainTextEdit, "roadObjectReply");
    d.reply->setReadOnly(true);
    d.reply->setMaximumHeight(90);
    d.status = named(new QLabel, "roadObjectStatus");
    d.status->setWordWrap(true);

    auto* top = new QHBoxLayout;
    auto* left = new QVBoxLayout;
    left->addWidget(d.kinds);
    left->addWidget(d.source);
    top->addLayout(left, 3);
    top->addWidget(d.preview, 2);
    auto* middle = new QHBoxLayout;
    middle->addWidget(heights);
    middle->addWidget(common);
    auto* buttons = new QHBoxLayout;
    buttons->addWidget(d.line, 1);
    buttons->addWidget(d.previewRun);
    buttons->addWidget(d.add);
    buttons->addWidget(close);
    auto* layout = new QVBoxLayout(this);
    layout->addLayout(top);
    layout->addWidget(placement);
    layout->addLayout(middle);
    layout->addLayout(buttons);
    layout->addWidget(d.reply);
    layout->addWidget(d.status);

    // ---- wiring: every edit makes the line again, never runs it ----
    d.refresh = [this] {
        Impl& self = *impl_;
        const RoadObjectKind kind = currentKind();
        const KindShape shape = katana::cad::road::shapeOf(kind);
        const RoadObjectForm::Place place = form().place;
        const bool onAlignment = place == RoadObjectForm::Place::Alignment;
        self.alignment->setEnabled(onAlignment);
        self.station->setEnabled(onAlignment);
        self.offset->setEnabled(onAlignment);
        self.to->setEnabled(onAlignment && shape != KindShape::Crossing);
        self.spacing->setEnabled(shape == KindShape::Point &&
                                 place != RoadObjectForm::Place::Points &&
                                 place != RoadObjectForm::Place::DrawnPoints);
        self.points->setEnabled(place == RoadObjectForm::Place::Points);
        self.scope->setEnabled(place == RoadObjectForm::Place::Lines ||
                               place == RoadObjectForm::Place::DrawnPoints);
        self.source->setText(qs(katana::cad::road::kindSource(kind)));
        const auto made = roadObjectAddLine(form());
        if (made) {
            self.line->setText(*made);
            self.status->clear();
        } else {
            self.line->clear();
            self.status->setText(qs(made.error().message));
        }
        self.add->setEnabled(made.ok());
        self.previewRun->setEnabled(made.ok());
        std::vector<std::pair<std::string, std::string>> given;
        for (const auto& [key, value] : form().parameters) {
            given.emplace_back(key.toStdString(), value.toStdString());
        }
        self.preview->showModel(roadObjectPreview(kind, given), shape == KindShape::Crossing);
    };
    const auto refresh = [this] { impl_->refresh(); };
    connect(d.kinds, &QTabWidget::currentChanged, this, [refresh] { refresh(); });
    for (const auto& page : d.pages) {
        for (const ParameterField& made : page) {
            if (made.choice != nullptr) {
                connect(made.choice, &QComboBox::currentIndexChanged, this,
                        [refresh] { refresh(); });
            } else {
                connect(made.text, &QLineEdit::textChanged, this, [refresh] { refresh(); });
            }
        }
    }
    for (QRadioButton* button :
         {d.placeAlignment, d.placeLines, d.placePoints, d.placeDrawnPoints}) {
        connect(button, &QRadioButton::toggled, this, [refresh] { refresh(); });
    }
    for (QLineEdit* edit : {d.station, d.to, d.offset, d.spacing, d.points, d.crossfall, d.dz,
                            d.name, d.layer, d.colour, d.material, d.facility}) {
        connect(edit, &QLineEdit::textChanged, this, [refresh] { refresh(); });
    }
    for (QComboBox* combo : {d.alignment, d.heights}) {
        connect(combo, &QComboBox::currentIndexChanged, this, [refresh] { refresh(); });
    }
    connect(d.surface, &QComboBox::currentTextChanged, this, [refresh] { refresh(); });
    d.scope->onChanged = refresh;

    const auto run = [this](bool asPreview) {
        Impl& self = *impl_;
        RoadObjectForm asked = form();
        asked.preview = asPreview;
        const auto made = roadObjectAddLine(asked);
        if (!made) {
            self.status->setText(qs(made.error().message));
            return;
        }
        if (!self.context.run) {
            self.status->setText("There is no command line to run it on here.");
            return;
        }
        const VerbOutcome outcome = self.context.run(*made);
        self.reply->setPlainText(outcome.ok ? outcome.reply : outcome.error);
        self.status->setText(outcome.ok ? (asPreview ? "Previewed: nothing was changed."
                                                     : "Added - one undo step.")
                                        : "Refused: " + outcome.error);
    };
    connect(d.previewRun, &QPushButton::clicked, this, [run] { run(true); });
    connect(d.add, &QPushButton::clicked, this, [run] { run(false); });
    connect(close, &QPushButton::clicked, this, [this] { hide(); });

    if (d.context.document != nullptr) {
        // The alignments, surfaces and layers follow the drawing.
        d.watcher = std::make_unique<DocumentWatcher>(
            *d.context.document, [this](const DocumentChanges& changes) {
                if (changes.model) {
                    reload();
                }
            });
    }
    reload();
    refresh();
}

RoadObjectDialog::~RoadObjectDialog() = default;

void RoadObjectDialog::showKind(RoadObjectKind kind)
{
    impl_->kinds->setCurrentIndex(static_cast<int>(kind));
}

RoadObjectKind RoadObjectDialog::currentKind() const
{
    const auto kinds = katana::cad::road::roadObjectKinds();
    const int index = std::clamp(impl_->kinds->currentIndex(), 0,
                                 static_cast<int>(kinds.size()) - 1);
    return kinds[static_cast<std::size_t>(index)];
}

RoadObjectForm RoadObjectDialog::form() const
{
    const Impl& d = *impl_;
    RoadObjectForm made;
    made.kind = currentKind();
    if (d.placeLines->isChecked()) {
        made.place = RoadObjectForm::Place::Lines;
    } else if (d.placePoints->isChecked()) {
        made.place = RoadObjectForm::Place::Points;
    } else if (d.placeDrawnPoints->isChecked()) {
        made.place = RoadObjectForm::Place::DrawnPoints;
    }
    made.alignment = d.alignment->currentText();
    made.station = d.station->text();
    made.to = d.to->isEnabled() ? d.to->text() : QString();
    made.offset = d.offset->text();
    made.spacing = d.spacing->isEnabled() ? d.spacing->text() : QString();
    made.points = d.points->text();
    if (made.place == RoadObjectForm::Place::Lines ||
        made.place == RoadObjectForm::Place::DrawnPoints) {
        if (const auto words = d.scope->verbWords()) {
            made.scope = *words;
        }
    }
    made.heights = d.heights->currentData().toString();
    made.surface = d.surface->currentText().trimmed();
    made.crossfall = d.crossfall->text();
    made.dz = d.dz->text();
    made.name = d.name->text();
    made.layer = d.layer->text();
    made.colour = d.colour->text();
    made.material = d.material->text();
    made.facility = d.facility->text();
    const std::size_t page = static_cast<std::size_t>(made.kind);
    if (page < d.pages.size()) {
        for (const ParameterField& parameter : d.pages[page]) {
            const QString value = parameter.value();
            if (!value.isEmpty()) {
                made.parameters.emplace_back(qs(parameter.key), value);
            }
        }
    }
    return made;
}

void RoadObjectDialog::reload()
{
    Impl& d = *impl_;
    if (!d.documentAlive()) {
        d.scope->reloadViews();
        return;
    }
    const katana::cad::Document& document = *d.context.document;
    d.scope->reload(document.model());
    {
        const QSignalBlocker quiet(d.alignment);
        const QString kept = d.alignment->currentText();
        d.alignment->clear();
        for (const std::string& name : document.model().alignments.names()) {
            d.alignment->addItem(qs(name));
        }
        const int index = d.alignment->findText(kept);
        d.alignment->setCurrentIndex(index >= 0 ? index : (d.alignment->count() > 0 ? 0 : -1));
        // With no alignment to place along, along lines is the place to start.
        if (d.alignment->count() == 0 && d.placeAlignment->isChecked()) {
            d.placeLines->setChecked(true);
        }
    }
    {
        const QSignalBlocker quiet(d.surface);
        const QString kept = d.surface->currentText();
        d.surface->clear();
        d.surface->addItem(QString());
        for (const auto& surface : document.surfaces().all()) {
            d.surface->addItem(qs(surface.name));
        }
        d.surface->setCurrentText(kept);
    }
    if (d.refresh) {
        d.refresh();
    }
}

void RoadObjectDialog::showEvent(QShowEvent* event)
{
    // The views open now, not when the dialog was made.
    reload();
    QDialog::showEvent(event);
}

} // namespace katana::qt
