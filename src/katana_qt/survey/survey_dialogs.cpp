#include "survey/survey_dialogs.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QFontDatabase>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QVBoxLayout>

#include <optional>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_tools.hpp"
#include "katana/core/text.hpp"
#include "katana/survey/angles.hpp"
#include "theme.hpp"
#include "view_workspace.hpp"

namespace katana::qt {

namespace cad = katana::cad;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

std::string text(const QLineEdit* field) { return field->text().trimmed().toStdString(); }
std::string text(const QPlainTextEdit* field) { return field->toPlainText().toStdString(); }

// Names the field an error came from, so "a position is E,N ..." says WHICH
// position. The library's message and context are kept as they are.
template <typename T>
Result<T> inField(const char* field, Result<T> result)
{
    if (!result) {
        katana::core::Error error = result.error();
        error.message = std::string(field) + ": " + error.message;
        return error;
    }
    return result;
}

// A field that may be left empty: nullopt when it is, the parse otherwise -
// so an empty field is "not given", never zero.
Result<std::optional<double>> optionalNumber(const QLineEdit* field, const char* what)
{
    const std::string value = text(field);
    if (value.empty()) {
        return std::optional<double>{};
    }
    auto number = inField(what, cad::parseSurveyNumber(value, what));
    if (!number) {
        return number.error();
    }
    return std::optional<double>{*number};
}

// A position as the fields take it back: exact, so what Use Selection filled
// in is the entity's coordinate to the last bit and not a rounding of it.
QString positionText(const cad::SurveyPosition& position)
{
    std::string value = katana::core::formatExactReal(position.point.x) + "," +
                        katana::core::formatExactReal(position.point.y);
    if (position.elevation) {
        value += "," + katana::core::formatExactReal(*position.elevation);
    }
    return QString::fromStdString(value);
}

QString selectionSummary(const cad::Document& document)
{
    const std::size_t selected = document.selection().size();
    const std::size_t points = cad::selectedPointPositions(document).size();
    return QString("%1 selected, %2 of them point entities").arg(selected).arg(points);
}

// The library's one sentence on angle text (cad::surveyAngleFormats), as a
// sentence of the note: capitalised and stopped.
QString sentence(std::string_view text)
{
    QString result = QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
    if (!result.isEmpty()) {
        result[0] = result[0].toUpper();
    }
    return result + ".";
}

QLabel* mutedLabel(const QString& text, QWidget* parent)
{
    auto* label = new QLabel(text, parent);
    label->setWordWrap(true);
    label->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    return label;
}

} // namespace

// ---- the frame ------------------------------------------------------------------------------

SurveyToolDialog::SurveyToolDialog(SurveyDialogContext context, const QString& objectName,
                                   const QString& title, const QString& note, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName(objectName);
    setWindowTitle(title);
    // Non-modal: kept open beside the drawing while the selection changes.
    setModal(false);
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(mutedLabel(note, this));
    form_ = new QFormLayout();
    layout->addLayout(form_);

    message_ = new QLabel(this);
    message_->setObjectName("message");
    message_->setWordWrap(true);
    message_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    layout->addWidget(message_);

    result_ = new QPlainTextEdit(this);
    result_->setObjectName("result");
    result_->setReadOnly(true);
    result_->setFont(katana::qt::theme::monospaceFont());
    // The reports are tables: wrapping would break their columns.
    result_->setLineWrapMode(QPlainTextEdit::NoWrap);
    result_->setPlaceholderText("The report appears here and in the command log.");
    result_->setMinimumHeight(180);
    layout->addWidget(result_, 1);

    verbs_ = new QHBoxLayout();
    verbs_->addStretch(1);
    auto* close = new QPushButton("Close", this);
    close->setObjectName("close");
    close->setAutoDefault(false);
    connect(close, &QPushButton::clicked, this, [this] { hide(); });
    verbs_->addWidget(close);
    layout->addLayout(verbs_);
}

QLineEdit* SurveyToolDialog::makeField(const QString& objectName, const QString& placeholder,
                                       const QString& tip)
{
    auto* field = new QLineEdit(this);
    field->setObjectName(objectName);
    field->setPlaceholderText(placeholder);
    field->setToolTip(tip);
    return field;
}

QLineEdit* SurveyToolDialog::addField(const QString& label, const QString& objectName,
                                      const QString& placeholder, const QString& tip)
{
    QLineEdit* field = makeField(objectName, placeholder, tip);
    form_->addRow(label, field);
    return field;
}

void SurveyToolDialog::addRow(const QString& label, std::initializer_list<QWidget*> widgets)
{
    auto* row = new QHBoxLayout();
    for (QWidget* widget : widgets) {
        row->addWidget(widget, qobject_cast<QLabel*>(widget) != nullptr ? 0 : 1);
    }
    form_->addRow(label, row);
}

QPlainTextEdit* SurveyToolDialog::addTextField(const QString& label, const QString& objectName,
                                               const QString& placeholder, const QString& tip)
{
    auto* field = new QPlainTextEdit(this);
    field->setObjectName(objectName);
    field->setPlaceholderText(placeholder);
    field->setToolTip(tip);
    field->setFont(katana::qt::theme::monospaceFont());
    field->setLineWrapMode(QPlainTextEdit::NoWrap);
    field->setTabChangesFocus(true);
    // About eight lines: a longer book scrolls, and the report below keeps
    // the room it needs.
    field->setMaximumHeight(field->fontMetrics().lineSpacing() * 8 + 12);
    form_->addRow(label, field);
    return field;
}

QComboBox* SurveyToolDialog::addChoice(const QString& label, const QString& objectName,
                                       const QStringList& items, const QString& tip)
{
    auto* choice = new QComboBox(this);
    choice->setObjectName(objectName);
    choice->addItems(items);
    choice->setToolTip(tip);
    form_->addRow(label, choice);
    return choice;
}

QPushButton* SurveyToolDialog::addVerb(const QString& text, const QString& objectName, Verb verb)
{
    auto* button = new QPushButton(text, this);
    button->setObjectName(objectName);
    // The first verb is what Enter in a field does; no other button is.
    button->setDefault(!hasDefaultVerb_);
    button->setAutoDefault(!hasDefaultVerb_);
    hasDefaultVerb_ = true;
    connect(button, &QPushButton::clicked, this, [this, verb = std::move(verb)] {
        const auto report = verb();
        if (!report) {
            showError(report.error());
            return;
        }
        message_->clear();
        result_->setPlainText(QString::fromStdString(*report));
        context_.log(QString::fromStdString(*report), false);
    });
    // Before Close, which stays last.
    verbs_->insertWidget(verbs_->count() - 1, button);
    return button;
}

QPushButton* SurveyToolDialog::addFiller(const QString& text, const QString& objectName,
                                         std::function<Status()> fill)
{
    auto* button = new QPushButton(text, this);
    button->setObjectName(objectName);
    button->setAutoDefault(false);
    connect(button, &QPushButton::clicked, this, [this, fill = std::move(fill)] {
        message_->clear();
        if (const auto status = fill(); !status) {
            showError(status.error());
        }
    });
    // Left of the stretch: fillers read the drawing, verbs act on the form.
    verbs_->insertWidget(0, button);
    return button;
}

void SurveyToolDialog::frameViews() const
{
    if (context_.views != nullptr) {
        context_.views->zoomExtentsAll();
    }
}

void SurveyToolDialog::setNote(const QString& text)
{
    message_->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    message_->setText(text);
}

void SurveyToolDialog::showError(const katana::core::Error& error)
{
    QString text = QString::fromStdString(error.message);
    if (!error.context.empty()) {
        text += " - " + QString::fromStdString(error.context);
    }
    message_->setStyleSheet(QString("color: %1").arg(theme::error().name()));
    message_->setText(text);
    context_.log(windowTitle() + ": " + QString::fromStdString(error.describe()), true);
}

// ---- inverse --------------------------------------------------------------------------------

SurveyInverseDialog::SurveyInverseDialog(SurveyDialogContext context, QWidget* parent)
    : SurveyToolDialog(std::move(context), "surveyInverseDialog", "Inverse",
                       "Horizontal distance, azimuth and bearing between two positions - and the "
                       "height difference, slope distance and grade when both have an elevation "
                       "(a missing one is never taken as zero). Plane grid arithmetic: no scale "
                       "factor is applied. Use Selection takes two selected points, or the two "
                       "ends of one selected line or open polyline.",
                       parent)
{
    const QString tip = "E,N or E,N,Z - easting first - or the id of a point entity";
    from_ = addField("From:", "from", "E,N[,Z] or point id", tip);
    to_ = addField("To:", "to", "E,N[,Z] or point id", tip);
    addFiller("Use Selection", "useSelection", [this] { return useSelection(); });
    addVerb("Compute", "compute", [this]() -> Result<std::string> {
        const auto from =
            inField("From", cad::parseSurveyPosition(document(), text(from_)));
        if (!from) {
            return from.error();
        }
        const auto to = inField("To", cad::parseSurveyPosition(document(), text(to_)));
        if (!to) {
            return to.error();
        }
        const auto result = cad::computeInverse(*from, *to);
        if (!result) {
            return result.error();
        }
        return cad::formatInverseReport(*result);
    });
    resize(640, 420);
}

Status SurveyInverseDialog::useSelection()
{
    const cad::Document& doc = document();
    const std::vector<entity::EntityId> ids = doc.selection().ids();
    const std::vector<cad::SurveyPosition> points = cad::selectedPointPositions(doc);
    if (ids.size() == 2 && points.size() == 2) {
        from_->setText(QString::number(*points[0].entity));
        to_->setText(QString::number(*points[1].entity));
        return {};
    }
    if (ids.size() == 1) {
        const auto ends = cad::endsOfLine(doc, ids.front());
        if (!ends) {
            return ends.error();
        }
        // The fields take points by id but not a line's ends, so the ends go
        // in as their coordinates - with the heights the line carries.
        from_->setText(positionText(ends->first));
        to_->setText(positionText(ends->second));
        // The source is "line 5 start": the note names the line, not its end.
        setNote(QString("The ends of %1, as coordinates.")
                    .arg(QString::fromStdString(ends->first.source).chopped(6)));
        return {};
    }
    return makeError(ErrorCode::InvalidState,
                     "select two point entities, or one line or open polyline",
                     selectionSummary(doc).toStdString());
}

// ---- forward --------------------------------------------------------------------------------

SurveyForwardDialog::SurveyForwardDialog(SurveyDialogContext context, QWidget* parent)
    : SurveyToolDialog(
          std::move(context), "surveyForwardDialog", "Forward Point",
          "A new point from a position, a direction and a horizontal distance (a radiation). "
          "With a height difference the new point's height is the start's plus it - refused "
          "when the start has none. Add Point puts it on the current layer as one undoable "
          "step. " + sentence(cad::surveyAngleFormats()),
          parent)
{
    from_ = addField("From:", "from", "E,N[,Z] or point id",
                     "E,N or E,N,Z - easting first - or the id of a point entity");
    direction_ = addField("Direction:", "direction", "azimuth or bearing, e.g. N 36d52m11.63s E",
                          "An azimuth clockwise from grid north, or a quadrant bearing");
    distance_ = addField("Horizontal distance:", "distance", "drawing units", {});
    heightDifference_ = addField("Height difference:", "heightDifference", "optional",
                                 "Leave empty for a point with no height");
    name_ = addField("Point name:", "name", "optional", "Written as the new point's name");
    addFiller("Use Selection", "useSelection", [this] { return useSelection(); });
    addVerb("Compute", "compute", [this] { return compute(false); });
    addVerb("Add Point", "addPoint", [this] { return compute(true); });
    resize(640, 460);
}

Status SurveyForwardDialog::useSelection()
{
    const cad::Document& doc = document();
    const std::vector<cad::SurveyPosition> points = cad::selectedPointPositions(doc);
    if (points.size() != 1 || doc.selection().size() != 1) {
        return makeError(ErrorCode::InvalidState, "select the one point entity to start from",
                         selectionSummary(doc).toStdString());
    }
    from_->setText(QString::number(*points.front().entity));
    return {};
}

Result<std::string> SurveyForwardDialog::compute(bool add)
{
    cad::Document& doc = document();
    cad::ForwardInput input;
    const auto from = inField("From", cad::parseSurveyPosition(doc, text(from_)));
    if (!from) {
        return from.error();
    }
    input.from = *from;
    const auto direction = inField("Direction", cad::parseSurveyDirection(text(direction_)));
    if (!direction) {
        return direction.error();
    }
    input.azimuth = *direction;
    const auto distance = cad::parseSurveyNumber(text(distance_), "the horizontal distance");
    if (!distance) {
        return distance.error();
    }
    input.distance = *distance;
    const auto heightDifference = optionalNumber(heightDifference_, "the height difference");
    if (!heightDifference) {
        return heightDifference.error();
    }
    input.heightDifference = *heightDifference;
    input.name = text(name_);

    const auto result = cad::computeForward(input);
    if (!result) {
        return result.error();
    }
    std::string report = cad::formatForwardReport(*result);
    if (!add) {
        return report;
    }
    auto command = cad::forwardPointCommand(doc, *result);
    if (!command) {
        return command.error();
    }
    if (auto status = doc.execute(std::move(*command)); !status) {
        return status.error();
    }
    frameViews();
    return report + "\n  Created on layer " + doc.currentLayer();
}

// ---- angle calculator -----------------------------------------------------------------------

SurveyAngleDialog::SurveyAngleDialog(SurveyDialogContext context, QWidget* parent)
    : SurveyToolDialog(std::move(context), "surveyAngleCalculatorDialog",
                       "Angle and Bearing Calculator",
                       "Converts an angle between degrees, minutes and seconds, decimal degrees, "
                       "gons (400 to the circle) and radians, and reads it as a direction: its "
                       "azimuth, quadrant bearing and reverse. Degrees are typed with a mark after "
                       "each field - 36d52m11.63s, 36:52:11.63, 36-52-11.63 - or as decimals; "
                       "there is no DDD.MMSS form, which would make 36.5211 mean two angles.",
                       parent)
{
    angle_ = addField("Angle:", "angle", "e.g. 36d52m11.63s, 100, 0.5, S 45 W", {});
    // In the order of cad::AngleInputUnit.
    unit_ = addChoice("Entered in:", "unit",
                      {"Degrees", "Gons", "Radians", "Quadrant bearing"}, {});
    addVerb("Convert", "convert", [this]() -> Result<std::string> {
        const auto conversion = cad::convertAngle(
            text(angle_), static_cast<cad::AngleInputUnit>(unit_->currentIndex()));
        if (!conversion) {
            return conversion.error();
        }
        return cad::formatAngleConversion(*conversion);
    });
    resize(640, 320);
}

// ---- traverse -------------------------------------------------------------------------------

namespace {

// In the order of the Kind choice.
constexpr survey::TraverseKind kKinds[] = {survey::TraverseKind::ClosedLoop,
                                           survey::TraverseKind::Link,
                                           survey::TraverseKind::Open};
// In the order of the Method choice.
constexpr cad::TraverseMethod kMethods[] = {cad::TraverseMethod::Compass,
                                            cad::TraverseMethod::Transit,
                                            cad::TraverseMethod::LeastSquares,
                                            cad::TraverseMethod::None};

// A reference mark (a position) or an azimuth; the mark wins, as
// cad::TraverseOrientation says. Neither is refused rather than taken as
// north: an orientation nobody gave would turn the whole traverse.
Result<cad::TraverseOrientation> orientation(const cad::Document& document, const QLineEdit* mark,
                                             const QLineEdit* azimuth, const char* what)
{
    cad::TraverseOrientation result;
    if (!text(mark).empty()) {
        const auto position =
            inField(what, cad::parseSurveyPosition(document, text(mark)));
        if (!position) {
            return position.error();
        }
        result.referenceMark = position->point;
        return result;
    }
    if (!text(azimuth).empty()) {
        const auto direction = inField(what, cad::parseSurveyDirection(text(azimuth)));
        if (!direction) {
            return direction.error();
        }
        result.azimuth = *direction;
        return result;
    }
    return makeError(ErrorCode::InvalidArgument,
                     std::string(what) + ": give a reference mark or an azimuth");
}

} // namespace

SurveyTraverseDialog::SurveyTraverseDialog(SurveyDialogContext context, QWidget* parent)
    : SurveyToolDialog(
          std::move(context), "surveyTraverseDialog", "Traverse",
          "A traverse from known control: the angles turned clockwise from the back station "
          "and the horizontal distances, one leg per line (station angle distance). A closed "
          "loop returns to its start; a link closes on a second known station (and on a "
          "reference direction there, if a closing angle is given); an open traverse has no "
          "check at all. Positions are E,N or a point id. Add to Drawing puts the computed "
          "stations and the traverse line on the current layer as one undoable step.",
          parent)
{
    name_ = addField("Name:", "name", "Traverse", {});
    kind_ = addChoice("Kind:", "kind", {"Closed loop", "Link", "Open"},
                      "Closed loop: back to the start. Link: from one known station to another. "
                      "Open: nothing checks it.");
    start_ = addField("Start station:", "start", "E,N or point id",
                      "Known coordinates of the first leg's station");
    startMark_ = makeField("startMark", "E,N or point id",
                           "The backsight - for a loop, the station the first leg runs to");
    startAzimuth_ = makeField("startAzimuth", "azimuth or bearing",
                              "Used when no reference mark is given");
    addRow("Start reference mark:",
           {startMark_, new QLabel("or its azimuth", this), startAzimuth_});
    legs_ = addTextField("Legs:", "legs", "A  286d53m11.63s  200.000\nB  249:23:26.4  250.000",
                         "One leg per line: station, angle turned there (no blanks inside it), "
                         "distance to the next station. '#' starts a comment.");
    endStation_ = makeField("endStation", "its id",
                            "The station the last leg ends on (link and open)");
    end_ = makeField("end", "E,N or point id", "Known coordinates of the closing station (link)");
    addRow("End station:", {endStation_, new QLabel("at", this), end_});
    closingAngle_ = addField("Closing angle:", "closingAngle", "optional",
                             "The angle turned at the end station from the last station to "
                             "the closing reference - the angular check of a link");
    closingMark_ = makeField("closingMark", "E,N or point id", {});
    closingAzimuth_ = makeField("closingAzimuth", "azimuth or bearing", {});
    addRow("Closing reference mark:",
           {closingMark_, new QLabel("or its azimuth", this), closingAzimuth_});
    method_ = addChoice("Adjustment:", "method",
                        {"Bowditch (compass)", "Transit", "Least squares", "None"},
                        "Least squares weighs the observations by the precisions below");
    balanceAngles_ = new QCheckBox("Balance the angular misclosure first", this);
    balanceAngles_->setObjectName("balanceAngles");
    balanceAngles_->setChecked(true);
    form()->addRow(QString(), balanceAngles_);

    // The weights least squares uses, filled with the library's defaults (a
    // common 5" construction instrument) to be replaced with the instrument's.
    const survey::TraverseStochasticModel defaults = cad::defaultTraversePrecision();
    angleSigma_ = makeField("angleSigma", {}, "Standard deviation of an angle, seconds");
    angleSigma_->setText(QString::number(survey::radiansToArcSeconds(defaults.angleSigma)));
    distanceSigma_ = makeField("distanceSigma", {}, "Standard deviation of a distance: mm");
    distanceSigma_->setText(QString::number(defaults.distanceSigmaConstant * 1000.0));
    distancePpm_ = makeField("distancePpm", {}, "... plus parts per million of its length");
    distancePpm_->setText(QString::number(defaults.distanceSigmaPpm));
    addRow("Least-squares precision:",
           {angleSigma_, new QLabel("\" angles;", this), distanceSigma_,
            new QLabel("mm +", this), distancePpm_, new QLabel("ppm distances", this)});

    addVerb("Compute", "compute", [this] { return compute(false); });
    addVerb("Add to Drawing", "addToDrawing", [this] { return compute(true); });
    connect(kind_, &QComboBox::currentIndexChanged, this, [this] { kindChanged(); });
    connect(method_, &QComboBox::currentIndexChanged, this, [this] { kindChanged(); });
    kindChanged();
    resize(900, 820);
}

// Only the fields the chosen kind and method read are enabled, so a value
// typed where it would be ignored cannot look as if it counted.
void SurveyTraverseDialog::kindChanged()
{
    const survey::TraverseKind kind = kKinds[kind_->currentIndex()];
    const bool link = kind == survey::TraverseKind::Link;
    endStation_->setEnabled(kind != survey::TraverseKind::ClosedLoop);
    for (QLineEdit* field : {end_, closingAngle_, closingMark_, closingAzimuth_}) {
        field->setEnabled(link);
    }
    const bool leastSquares =
        kMethods[method_->currentIndex()] == cad::TraverseMethod::LeastSquares;
    for (QLineEdit* field : {angleSigma_, distanceSigma_, distancePpm_}) {
        field->setEnabled(leastSquares);
    }
    balanceAngles_->setEnabled(!leastSquares);
}

Result<std::string> SurveyTraverseDialog::compute(bool add)
{
    cad::Document& doc = document();
    cad::TraverseSpec spec;
    if (const std::string name = text(name_); !name.empty()) {
        spec.name = name;
    }
    spec.kind = kKinds[kind_->currentIndex()];
    const auto start = inField("Start station", cad::parseSurveyPosition(doc, text(start_)));
    if (!start) {
        return start.error();
    }
    spec.start = start->point;
    const auto startOrientation = orientation(doc, startMark_, startAzimuth_,
                                              spec.kind == survey::TraverseKind::ClosedLoop
                                                  ? "First-leg direction"
                                                  : "Backsight");
    if (!startOrientation) {
        return startOrientation.error();
    }
    spec.startOrientation = *startOrientation;
    auto legs = inField("Legs", cad::parseTraverseLegs(text(legs_)));
    if (!legs) {
        return legs.error();
    }
    spec.legs = std::move(*legs);

    if (spec.kind != survey::TraverseKind::ClosedLoop) {
        spec.endStation = text(endStation_);
        if (spec.endStation.empty()) {
            return makeError(ErrorCode::InvalidArgument,
                             "End station: name the station the last leg ends on");
        }
    }
    if (spec.kind == survey::TraverseKind::Link) {
        const auto end = inField("End station position", cad::parseSurveyPosition(doc, text(end_)));
        if (!end) {
            return end.error();
        }
        spec.end = end->point;
        if (!text(closingAngle_).empty()) {
            const auto angle = inField("Closing angle", cad::parseSurveyAngle(text(closingAngle_)));
            if (!angle) {
                return angle.error();
            }
            spec.closingAngle = *angle;
            const auto closing =
                orientation(doc, closingMark_, closingAzimuth_, "Closing reference");
            if (!closing) {
                return closing.error();
            }
            spec.closingOrientation = *closing;
        }
    }
    spec.method = kMethods[method_->currentIndex()];
    spec.balanceAngles = balanceAngles_->isChecked();
    if (spec.method == cad::TraverseMethod::LeastSquares) {
        const auto angle = cad::parseSurveyNumber(text(angleSigma_), "the angle precision");
        const auto constant =
            cad::parseSurveyNumber(text(distanceSigma_), "the distance precision");
        const auto ppm = cad::parseSurveyNumber(text(distancePpm_), "the distance ppm");
        for (const auto* value : {&angle, &constant, &ppm}) {
            if (!*value) {
                return value->error();
            }
        }
        spec.precision.angleSigma = survey::arcSecondsToRadians(*angle);
        spec.precision.distanceSigmaConstant = *constant / 1000.0; // mm to metres
        spec.precision.distanceSigmaPpm = *ppm;
    }

    const auto result = cad::computeTraverseTool(spec);
    if (!result) {
        return result.error();
    }
    std::string report = cad::formatTraverseReport(*result);
    if (!add) {
        return report;
    }
    auto command = cad::traverseCommand(doc, *result);
    if (!command) {
        return command.error();
    }
    if (auto status = doc.execute(std::move(*command)); !status) {
        return status.error();
    }
    frameViews();
    return report + "\n  Added to layer " + doc.currentLayer() + ": " +
           std::to_string(doc.lastCreatedEntities().size()) +
           " entities - the computed stations and the traverse line";
}

// ---- level book -----------------------------------------------------------------------------

namespace {

// In the order of the Adjustment choice.
constexpr survey::LevelAdjustment kAdjustments[] = {survey::LevelAdjustment::None,
                                                    survey::LevelAdjustment::BySetups,
                                                    survey::LevelAdjustment::ByDistance};

} // namespace

SurveyLevelBookDialog::SurveyLevelBookDialog(SurveyDialogContext context, QWidget* parent)
    : SurveyToolDialog(
          std::move(context), "surveyLevelBookDialog", "Level Book",
          "Reduces a level book by height of collimation. One line per reading: point, "
          "backsight, intersight, foresight and - on a foresight line - the length levelled in "
          "that setup, with '-' for an empty column; a change point carries its foresight and "
          "its backsight on one line. The allowable misclosure is k x sqrt(K km), stated only "
          "when every setup has a length; 12 mm is the common third-order figure - use your "
          "specification's.",
          parent)
{
    name_ = addField("Name:", "name", "Level run", {});
    startLevel_ =
        addField("Opening benchmark RL:", "startLevel", "reduced level of the first point", {});
    closingLevel_ = addField("Closing benchmark RL:", "closingLevel", "optional",
                             "Known level of the last point; without it the run is not checked");
    book_ = addTextField("Book:", "book",
                         "BM1  1.500  -      -\nA    -      2.000  -\n"
                         "CP1  2.250  -      0.500  100\nBM2  -      -      3.260  150",
                         "point BS IS FS [distance], '-' for an empty column, '#' a comment");
    adjustment_ = addChoice("Adjustment:", "adjustment",
                            {"None", "Equally between setups", "By distance levelled"},
                            "How the misclosure is shared out; needs a closing benchmark");
    allowance_ = addField("Allowance k (mm per \xE2\x88\x9Akm):", "allowance", {}, {});
    allowance_->setText(QString::number(cad::kThirdOrderLevelAllowance * 1000.0));
    addVerb("Reduce", "reduce", [this]() -> Result<std::string> {
        cad::LevelBookSpec spec;
        if (const std::string name = text(name_); !name.empty()) {
            spec.name = name;
        }
        const auto start = cad::parseSurveyNumber(text(startLevel_), "the opening benchmark RL");
        if (!start) {
            return start.error();
        }
        spec.startLevel = *start;
        const auto closing = optionalNumber(closingLevel_, "the closing benchmark RL");
        if (!closing) {
            return closing.error();
        }
        spec.closingLevel = *closing;
        auto lines = inField("Book", cad::parseLevelBook(text(book_)));
        if (!lines) {
            return lines.error();
        }
        spec.lines = std::move(*lines);
        spec.adjustment = kAdjustments[adjustment_->currentIndex()];
        const auto allowance = cad::parseSurveyNumber(text(allowance_), "the allowance k");
        if (!allowance) {
            return allowance.error();
        }
        spec.allowanceK = *allowance / 1000.0; // mm to metres
        const auto result = cad::computeLevelBook(spec);
        if (!result) {
            return result.error();
        }
        return cad::formatLevelBookReport(*result);
    });
    resize(820, 720);
}

// ---- coordinate converter -------------------------------------------------------------------

SurveyConverterDialog::SurveyConverterDialog(SurveyDialogContext context, QWidget* parent)
    : SurveyToolDialog(
          std::move(context), "surveyCoordinateConverterDialog", "Coordinate Converter",
          "Converts coordinates between two coordinate systems named by EPSG code (4326, "
          "28356, ...) through PROJ, with the grid scale factor and convergence on each "
          "projected side. One point per line: [label] easting northing for a projected system, "
          "[label] latitude longitude for a geographic one, an angle written without blanks "
          "inside it (-33:51:24.5 or -33d51m24.5s). A datum change PROJ could only do "
          "approximately is refused. It reports; it does NOT move anything in the drawing - "
          "converting drawing entities is not part of this tool - and heights are not converted.",
          parent)
{
    source_ = addField("From system:", "source", "EPSG code, e.g. 4326", {});
    target_ = addField("To system:", "target", "EPSG code, e.g. 28356", {});
    points_ = addTextField("Points:", "points", "CP1  -33.8568  151.2153",
                           "One point per line; Use Selection fills it from the selected points");
    addFiller("Use Selection", "useSelection", [this]() -> Status {
        const auto lines = cad::conversionLinesForSelection(document(), text(source_));
        if (!lines) {
            return lines.error();
        }
        points_->setPlainText(QString::fromStdString(*lines));
        return {};
    });
    addVerb("Convert", "convert", [this]() -> Result<std::string> {
        const auto conversion =
            cad::convertCoordinates(text(source_), text(target_), text(points_));
        if (!conversion) {
            return conversion.error();
        }
        return cad::formatCoordinateConversion(*conversion);
    });
    resize(900, 560);
}

} // namespace katana::qt
