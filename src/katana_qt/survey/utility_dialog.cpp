#include "survey/utility_dialog.hpp"

#include <QApplication>
#include <QButtonGroup>
#include <QClipboard>
#include <QComboBox>
#include <QDir>
#include <QFile>
#include <QFileDialog>
#include <QFontDatabase>
#include <QFormLayout>
#include <QGridLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QPlainTextEdit>
#include <QPushButton>
#include <QRadioButton>
#include <QSignalBlocker>
#include <QSizePolicy>
#include <QTabWidget>
#include <QVBoxLayout>

#include <algorithm>
#include <initializer_list>
#include <string_view>
#include <utility>
#include <vector>

#include "customisation/document_watcher.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/utilities/utility_drawing.hpp"
#include "katana/core/text.hpp"
#include "katana/survey/subsurface/clearance.hpp"
#include "katana/survey/subsurface/utility_network.hpp"
#include "theme.hpp"

namespace katana::qt {
namespace {

namespace sub = katana::survey::subsurface;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

// What the dialog refuses to write, said in its words.
katana::core::Error invalid(const QString& message)
{
    return makeError(ErrorCode::InvalidArgument, message.toStdString());
}

// A default of the library's own, as a placeholder shows it: 10, 0.3, 2.
QString defaultOf(double value)
{
    return "default " + QString::number(value, 'g', 12);
}

// A word as the command line reads it: double-quoted when it holds a blank,
// which the interpreter's tokenizer would otherwise split it at.
Result<QString> word(const QString& text, const QString& field)
{
    if (text.contains('"')) {
        return invalid(field + " holds a double quote, which a command line cannot carry");
    }
    const bool blank = std::any_of(text.begin(), text.end(), [](QChar c) { return c.isSpace(); });
    return blank ? "\"" + text + "\"" : text;
}

// A file the tool needs, trimmed; InvalidArgument naming it when blank.
Result<QString> requiredFile(const QString& text, const QString& field)
{
    const QString path = text.trimmed();
    if (path.isEmpty()) {
        return invalid("choose the " + field + " first");
    }
    return word(path, "the " + field + " path");
}

// `part` after `prefix`, or its error.
Result<QString> prefixed(const QString& prefix, const Result<QString>& part)
{
    if (!part) {
        return part.error();
    }
    return prefix + *part;
}

// " KEYWORD <number>" for an option that was given, nothing for a blank one.
// The number goes on the line as typed, once it reads as one.
Result<QString> option(const QString& keyword, const QString& text, const QString& field,
                       const QString& unit = "metres")
{
    const QString number = text.trimmed();
    if (number.isEmpty()) {
        return QString();
    }
    if (!katana::core::parseFiniteDouble(number.toStdString())) {
        return invalid(field + " must be a number of " + unit + ", not '" + number + "'");
    }
    return " " + keyword + " " + number;
}

// " KEYWORD <word>" for a word that was given, quoted when it holds a blank;
// nothing for a blank one.
Result<QString> wordOption(const QString& keyword, const QString& text, const QString& field)
{
    const QString given = text.trimmed();
    if (given.isEmpty()) {
        return QString();
    }
    return prefixed(" " + keyword + " ", word(given, field));
}

// The drawing's scope and filter words, or why the controls say none.
Result<QString> scopeWords(const UtilityForm& form)
{
    if (!form.scopeError.isEmpty()) {
        return invalid("the drawing's scope: " + form.scopeError);
    }
    const QString words = form.scope.trimmed();
    if (words.isEmpty()) {
        return invalid("choose what in the drawing to act on first");
    }
    return words;
}

// Where the services come from: the schedule's path, or the scope words.
Result<QString> sourceWords(const UtilityForm& form)
{
    return utilitySourceOf(form) == UtilitySource::Drawing
               ? scopeWords(form)
               : requiredFile(form.schedule, "utility schedule");
}

// "#12" for "#12" or "12": an entity id is a whole number from 1.
Result<QString> entityWord(const QString& text)
{
    QString id = text.trimmed();
    if (id.isEmpty()) {
        return invalid("choose the line or polyline of the proposed works first: its #id, or "
                       "Use Selected");
    }
    if (id.startsWith('#')) {
        id.remove(0, 1);
    }
    const auto number = katana::core::parseInteger(id.toStdString());
    const bool digits = !id.isEmpty() && std::all_of(id.begin(), id.end(), [](QChar c) {
        return c >= '0' && c <= '9';
    });
    if (!digits || !number || *number < 1) {
        return invalid("the design entity must be #<entity id>, not '" + text.trimmed() + "'");
    }
    return "#" + id;
}

// Clearance's works, after the services: the positional design file of a
// schedule file (the form the line always had), else DESIGN and the works.
Result<QString> designWords(const UtilityForm& form)
{
    switch (form.designSource) {
    case UtilityDesignSource::File: {
        const auto path = requiredFile(form.design, "design of the proposed works");
        if (!path) {
            return path.error();
        }
        return utilitySourceOf(form) == UtilitySource::File ? " " + *path : " DESIGN " + *path;
    }
    case UtilityDesignSource::Entity: {
        const auto id = entityWord(form.designEntity);
        if (!id) {
            return id.error();
        }
        const auto level = option("LEVEL", form.designLevel, "The design level");
        if (!level) {
            return level.error();
        }
        return " DESIGN " + *id + *level;
    }
    case UtilityDesignSource::Alignment: {
        const QString name = form.alignment.trimmed();
        if (name.isEmpty()) {
            return invalid("choose the alignment of the proposed works first");
        }
        return prefixed(" DESIGN ALIGNMENT ", word(name, "The alignment's name"));
    }
    }
    return invalid("choose the design of the proposed works first");
}

QLabel* noteLabel(const QString& text, QWidget* parent)
{
    auto* label = new QLabel(text, parent);
    label->setWordWrap(true);
    label->setStyleSheet(QString("color: %1").arg(theme::textMuted().name()));
    return label;
}

} // namespace

const char* utilityVerbWord(UtilityTool tool)
{
    switch (tool) {
    case UtilityTool::Draw:
        return "DRAW";
    case UtilityTool::Report:
        return "REPORT";
    case UtilityTool::Verify:
        return "VERIFY";
    case UtilityTool::Clearance:
        return "CLEARANCE";
    case UtilityTool::Check:
        return "CHECK";
    case UtilityTool::Regrade:
        return "REGRADE";
    case UtilityTool::Schedule:
        return "SCHEDULE";
    }
    return "REPORT";
}

UtilitySource utilitySourceOf(const UtilityForm& form)
{
    switch (form.tool) {
    case UtilityTool::Regrade:
    case UtilityTool::Schedule:
        return UtilitySource::Drawing;
    case UtilityTool::Draw:
    case UtilityTool::Report:
    case UtilityTool::Verify:
    case UtilityTool::Clearance:
    case UtilityTool::Check:
        break;
    }
    return form.source;
}

Result<QString> utilityCommandLine(const UtilityForm& form)
{
    QString line = QString("UTILITY ") + utilityVerbWord(form.tool);
    // SCHEDULE names what it writes before the scope it writes.
    if (form.tool == UtilityTool::Schedule) {
        const auto out = requiredFile(form.scheduleOut, "schedule to write");
        if (!out) {
            return out.error();
        }
        line += " " + *out;
    }
    const auto source = sourceWords(form);
    if (!source) {
        return source.error();
    }
    line += " " + *source;
    // The rest in the order the verb lists it, an option only when given; the
    // first part that cannot be written refuses the whole line.
    std::vector<Result<QString>> parts;
    switch (form.tool) {
    case UtilityTool::Draw:
        // What the geometry in the drawing cannot say for itself; a schedule
        // says it in its columns.
        if (utilitySourceOf(form) == UtilitySource::Drawing) {
            parts.push_back(wordOption("TYPE", form.serviceType, "The type of service"));
            parts.push_back(wordOption("METHOD", form.method, "The location method"));
            parts.push_back(
                option("H_UNC", form.horizontalUncertainty, "The horizontal uncertainty"));
            parts.push_back(option("V_UNC", form.verticalUncertainty, "The vertical uncertainty"));
            parts.push_back(wordOption("HEIGHTS", form.heights, "What the heights are"));
            parts.push_back(wordOption("LEVEL_REF", form.levelReference, "The level reference"));
            parts.push_back(wordOption("PATH", form.path, "The path between vertices"));
            parts.push_back(wordOption("OWNER", form.owner, "The owner"));
            parts.push_back(wordOption("MATERIAL", form.material, "The material"));
            parts.push_back(option("DIAMETER_MM", form.diameter, "The diameter", "millimetres"));
            parts.push_back(wordOption("STATUS", form.status, "The status"));
            parts.push_back(wordOption("FIELDS", form.fields, "The fields"));
        }
        parts.push_back(option("SPACING", form.spacing, "The detected spacing"));
        parts.push_back(option("MINCOVER", form.minCover, "The minimum cover"));
        if (const QString prefix = form.layerPrefix.trimmed(); !prefix.isEmpty()) {
            parts.push_back(prefixed(" LAYER ", word(prefix, "The layer prefix")));
        }
        break;
    case UtilityTool::Report:
        parts.push_back(option("MINCOVER", form.minCover, "The minimum cover"));
        parts.push_back(option("SPACING", form.spacing, "The detected spacing"));
        break;
    case UtilityTool::Verify:
        break;
    case UtilityTool::Clearance:
        parts.push_back(designWords(form));
        parts.push_back(option("WIDTH", form.width, "The works' width"));
        parts.push_back(option("H", form.horizontal, "The horizontal clearance"));
        parts.push_back(option("V", form.vertical, "The vertical clearance"));
        parts.push_back(option("MARGIN", form.margin, "The unverified margin"));
        break;
    case UtilityTool::Check:
        parts.push_back(prefixed(" SCHEMA ", requiredFile(form.schema, "delivery schema")));
        break;
    case UtilityTool::Regrade:
        parts.push_back(option("SPACING", form.spacing, "The detected spacing"));
        parts.push_back(option("MINCOVER", form.minCover, "The minimum cover"));
        break;
    case UtilityTool::Schedule:
        if (!form.scheduleSchema.trimmed().isEmpty()) {
            parts.push_back(
                prefixed(" SCHEMA ", requiredFile(form.scheduleSchema, "delivery schema")));
        }
        break;
    }
    for (const Result<QString>& part : parts) {
        if (!part) {
            return part.error();
        }
        line += *part;
    }
    return line;
}

UtilityToolsDialog::UtilityToolsDialog(UtilityDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("utilityDialog");
    setWindowTitle("Subsurface Utilities (AS 5488)");
    // Non-modal: kept open beside the drawing, which a Draw changes.
    setModal(false);
    resize(1120, 860);

    // The placeholders show the library's own defaults, read from its
    // settings, so what they say cannot drift from what a blank option gets.
    const sub::GradingSettings grading;
    const sub::ClearanceRequirement requirement;
    const auto field = [this](const char* name, const QString& placeholder, const QString& tip) {
        auto* edit = new QLineEdit(this);
        edit->setObjectName(name);
        edit->setPlaceholderText(placeholder);
        edit->setToolTip(tip);
        return edit;
    };
    const auto button = [this](const char* name, const QString& text) {
        auto* made = new QPushButton(text, this);
        made->setObjectName(name);
        made->setAutoDefault(false);
        return made;
    };
    const auto radio = [this](const char* name, const QString& text, QButtonGroup* group) {
        auto* made = new QRadioButton(text, this);
        made->setObjectName(name);
        group->addButton(made);
        return made;
    };
    const auto row = [](std::initializer_list<QWidget*> widgets) {
        auto* layout = new QHBoxLayout;
        bool first = true;
        for (QWidget* widget : widgets) {
            layout->addWidget(widget, first ? 1 : 0);
            first = false;
        }
        return layout;
    };
    const QString csvFilter = "CSV files (*.csv);;All files (*)";

    // ---- where the services come from, above the tabs --------------------------
    auto* sourceBox = new QGroupBox("Services from", this);
    sourceBox->setObjectName("utilitySourceGroup");
    auto* sourceGroup = new QButtonGroup(sourceBox);
    sourceFile_ = radio("utilitySourceFile", "A schedule file", sourceGroup);
    sourceFile_->setToolTip("Draw, Report, Verify, Clearance and Check read the schedule (.csv)");
    sourceDrawing_ = radio("utilitySourceDrawing", "What is drawn", sourceGroup);
    sourceDrawing_->setToolTip(
        "Draw takes the lines, polylines and points a survey or an import left in the drawing "
        "and draws them as services; Report, Verify, Clearance and Check read the services "
        "UTILITY DRAW drew, each line whole. Both by the scope and filter below. Regrade and "
        "Schedule always read what is drawn");
    sourceFile_->setChecked(true);
    schedule_ = field("utilitySchedule", "the utility schedule: a .csv, one row per located vertex",
                      "The schedule of located services (docs/subsurface_utilities.md, \"The "
                      "schedule format\"): rows with the same line id form one service, in order");
    scheduleBrowse_ = button("utilityScheduleBrowse", "Browse...");
    auto* sourceLayout = new QGridLayout(sourceBox);
    sourceLayout->addWidget(sourceFile_, 0, 0);
    sourceLayout->addLayout(row({schedule_, scheduleBrowse_}), 0, 1);
    sourceLayout->addWidget(sourceDrawing_, 1, 0);
    sourceLayout->addWidget(
        noteLabel("in the scope and filter on the left: for Draw, the survey's or an import's "
                  "lines and points, drawn as services; for the others, the lines UTILITY DRAW "
                  "drew, where a scope that takes part of a line takes all of it",
                  sourceBox),
        1, 1);
    sourceLayout->setColumnStretch(1, 1);

    // The drawing's scope and filter: Global Modify's own controls.
    scope_ = new ScopeFilterWidget("utility", this);
    scope_->views = context_.views;
    // What the utility tools usually act on: everything drawn.
    scope_->setChoice(ScopeChoice::Drawing);
    scope_->onChanged = [this] { refreshCommand(); };

    spacing_ = field("utilitySpacing", defaultOf(grading.maximumDetectedSpacing) + " m",
                     "SPACING: the longest segment a detected path may span and stay QL-B; a "
                     "longer one was interpolated, not traced, and grades QL-C. The project "
                     "specification's figure, not the standard's");
    // Shared by Draw, Report and Regrade, as the verb's SPACING and MINCOVER
    // are, so a drawing and a report of one schedule are graded and flagged
    // alike.
    minCover_ = field("utilityMinCover", "none - cover is given, not tested",
                      "MINCOVER: flag every vertex whose cover is less than this - in the "
                      "report's findings, and on each drawn point (utility.cover_below_minimum)");
    auto* common = new QFormLayout;
    common->addRow("Detected spacing (m):", spacing_);
    common->addRow("Minimum cover (m):", minCover_);

    // One tab per form of the verb, in the order of the menu.
    tabs_ = new QTabWidget(this);
    tabs_->setObjectName("utilityTabs");
    // As tall as the tallest tab needs and no taller: the room is the reply's.
    tabs_->setSizePolicy(QSizePolicy::Preferred, QSizePolicy::Maximum);

    auto* drawPage = new QWidget(tabs_);
    auto* drawLayout = new QFormLayout(drawPage);
    drawLayout->addRow(noteLabel(
        "Grades the services - the schedule file's, or the survey's or an import's lines and "
        "points in the drawing - and adds them to the drawing as one undo step: a layer for "
        "each type of service and quality level, with a linetype for each level (QL-A "
        "continuous, QL-B dashed, QL-C dash-dot, QL-D dotted), one polyline for each run at one "
        "level, and a point at every located vertex carrying its whole schedule row - and, with "
        "a minimum cover, whether its cover is below it. A line that cannot be graded refuses "
        "the whole draw. What is drawn already is not drawn again; the survey is left as it "
        "is.",
        drawPage));
    layerPrefix_ = field("utilityLayerPrefix", "default utilities",
                         "LAYER: the layer the drawn services nest under, as "
                         "<prefix>/<type>/QL-A and <prefix>/<type>/points");
    drawLayout->addRow("Layer prefix:", layerPrefix_);

    // What the geometry cannot say for itself. Each choice shows the verb's
    // own word, so the line reads as the choice does; "not given" leaves the
    // option out, for what each line and point says of itself.
    const auto choices = [this](const char* name, const QString& tip,
                                std::initializer_list<std::pair<QString, QString>> items) {
        auto* box = new QComboBox(this);
        box->setObjectName(name);
        box->setToolTip(tip);
        box->addItem("not given", QString());
        for (const auto& [text, explanation] : items) {
            box->addItem(text, text);
            box->setItemData(box->count() - 1, explanation, Qt::ToolTipRole);
        }
        return box;
    };
    geometry_ = new QGroupBox("Drawn geometry as services (Draw of what is drawn)", drawPage);
    geometry_->setObjectName("utilityGeometryGroup");
    auto* geometry = new QGridLayout(geometry_);
    geometry->addWidget(
        noteLabel("Each line or open polyline in the scope is a service, named by its code; a "
                  "point on a vertex gives it its number. What a line or a point says of itself "
                  "(utility.method, utility.depth ... - Format > Global Modify sets them) wins "
                  "over these.",
                  geometry_),
        0, 0, 1, 4);
    serviceType_ = choices("utilityServiceType",
                           "TYPE: the kind of service, where a line does not say (utility.type); "
                           "not given, unknown",
                           {});
    for (const sub::UtilityType type :
         {sub::UtilityType::Water, sub::UtilityType::Electricity,
          sub::UtilityType::Telecommunications, sub::UtilityType::Gas,
          sub::UtilityType::RecycledWater, sub::UtilityType::FireService, sub::UtilityType::Sewer,
          sub::UtilityType::Stormwater, sub::UtilityType::Fuel,
          sub::UtilityType::IntelligentTransport, sub::UtilityType::Other}) {
        const QString typeWord = qs(std::string(katana::cad::utilities::utilityTypeWord(type)));
        serviceType_->addItem(typeWord, typeWord);
    }
    method_ = choices("utilityMethod",
                      "METHOD: how the services were located, where a point or a line does not say "
                      "(utility.method). Needed at every vertex",
                      {{"EML", "electromagnetic location: QL-B at best"},
                       {"GPR", "ground penetrating radar: QL-B at best"},
                       {"geophysical", "other geophysics: QL-B at best"},
                       {"pothole", "non-destructive excavation, the service seen: QL-A at best"},
                       {"trench", "an open trench, the service seen: QL-A at best"},
                       {"surface", "a surveyed pit, valve or marker: QL-C at best"},
                       {"records", "plans and GIS records: QL-D"},
                       {"anecdotal", "site knowledge: QL-D"}});
    horizontalUncertainty_ = field("utilityHUnc", "not assessed",
                                   "H_UNC: the horizontal uncertainty of each located position, "
                                   "+/- metres. Not assessed is not assumed good: a detection "
                                   "without one is QL-C");
    verticalUncertainty_ = field("utilityVUnc", "not assessed",
                                 "V_UNC: the vertical uncertainty of each level or depth, "
                                 "+/- metres");
    heights_ =
        choices("utilityHeights",
                "HEIGHTS: what the heights of the lines and points are; not given, the surface",
                {{"surface", "the ground over the service: marks shot on the surface"},
                 {"service", "the service itself, on its level reference: shot on the pipe"},
                 {"none", "not to be used"}});
    levelReference_ = choices("utilityLevelRef",
                              "LEVEL_REF: the part of the service a level or depth is on; not "
                              "given, the top",
                              {{"top", "the crown, or what is met first above it"},
                               {"centre", "the centre line"},
                               {"invert", "the bottom of the bore"}});
    path_ = choices("utilityPath",
                    "PATH: what is known between each vertex and the next; not given, detected",
                    {{"detected", "traced, or detected at intervals"},
                     {"exposed", "seen all along: an open trench"},
                     {"assumed", "joined up; nothing observed between"}});
    owner_ = field("utilityOwner", "each line's own", "OWNER: the asset owner");
    material_ =
        field("utilityMaterial", "each line's own", "MATERIAL: what the service is made of");
    diameter_ = field("utilityDiameter", "each line's own",
                      "DIAMETER_MM: the outside diameter, millimetres");
    serviceStatus_ =
        choices("utilityServiceStatus", "STATUS: the service's status",
                {{"in service", ""}, {"disused", ""}, {"abandoned", ""}, {"proposed", ""}});
    fields_ = field("utilityFields", "column=property, ... e.g. type=ASSET_TYPE,line=ASSET_ID",
                    "FIELDS: an import's own attributes read as the schedule's columns - a "
                    "shapefile's ASSET_TYPE as the type, its ASSET_ID as the line");
    const auto pair = [this, geometry](int at, int column, const QString& label, QWidget* widget) {
        geometry->addWidget(new QLabel(label, geometry_), at, column * 2);
        geometry->addWidget(widget, at, column * 2 + 1);
    };
    pair(1, 0, "Type:", serviceType_);
    pair(1, 1, "Method:", method_);
    pair(2, 0, "H uncertainty (m):", horizontalUncertainty_);
    pair(2, 1, "V uncertainty (m):", verticalUncertainty_);
    pair(3, 0, "Heights are:", heights_);
    pair(3, 1, "Level reference:", levelReference_);
    pair(4, 0, "Path:", path_);
    pair(4, 1, "Status:", serviceStatus_);
    pair(5, 0, "Owner:", owner_);
    pair(5, 1, "Material:", material_);
    pair(6, 0, "Diameter (mm):", diameter_);
    pair(6, 1, "Fields:", fields_);
    geometry->setColumnStretch(1, 1);
    geometry->setColumnStretch(3, 1);
    drawLayout->addRow(geometry_);
    tabs_->addTab(drawPage, "Draw");

    auto* reportPage = new QWidget(tabs_);
    auto* reportLayout = new QFormLayout(reportPage);
    reportLayout->addRow(noteLabel(
        "The investigation report: every vertex and segment graded by AS 5488 quality level, "
        "the length of each service at each level, depth of cover, and what the schedule "
        "claims better than its evidence supports. Nothing is added to the drawing. On what "
        "is drawn, the reply leads with what the scope took.",
        reportPage));
    tabs_->addTab(reportPage, "Report");

    auto* verifyPage = new QWidget(tabs_);
    auto* verifyLayout = new QVBoxLayout(verifyPage);
    verifyLayout->addWidget(noteLabel(
        "The QL-B detections compared with the QL-A exposures that check them (the schedule's "
        "verifies column): how far each detection was from where the service was seen, in plan "
        "and in level, against QL-B's tolerance. Nothing is added to the drawing.",
        verifyPage));
    verifyLayout->addStretch(1);
    tabs_->addTab(verifyPage, "Verify");

    auto* clearancePage = new QWidget(tabs_);
    auto* clearanceLayout = new QFormLayout(clearancePage);
    clearanceLayout->addRow(noteLabel(
        "Clearance of proposed works from every segment of every service, each widened by the "
        "positional tolerance of its quality level: Conflict, Unconfirmed (QL-C or QL-D nearby: "
        "locate it first), Within tolerance, or Clear. The clearances are the asset owners' "
        "rules, not AS 5488's.",
        clearancePage));
    auto* designGroup = new QButtonGroup(clearancePage);
    designFile_ = radio("utilityDesignFile", "Design file:", designGroup);
    designFile_->setChecked(true);
    design_ = field("utilityDesign", "the proposed works: a .csv of the centre line",
                    "The design centre line (docs/subsurface_utilities.md): easting, northing and "
                    "an optional level at each vertex");
    designBrowse_ = button("utilityDesignBrowse", "Browse...");
    clearanceLayout->addRow(designFile_, row({design_, designBrowse_}));
    designEntity_ = radio("utilityDesignEntity", "Drawn line or polyline:", designGroup);
    designEntityId_ = field("utilityDesignEntityId", "#id of its centre line",
                            "A line or polyline in the drawing as the works' centre line: #id, "
                            "or select it and press Use Selected");
    designUseSelected_ = button("utilityDesignUseSelected", "Use Selected");
    designUseSelected_->setToolTip("The one entity selected in the drawing");
    designLevel_ = field("utilityDesignLevel", "its own heights, if it has them",
                         "LEVEL: the works' level, metres; left blank, the levels the line or "
                         "polyline carries, or none");
    auto* entityRow = row({designEntityId_, designUseSelected_});
    entityRow->addWidget(new QLabel("Level (m):", clearancePage));
    entityRow->addWidget(designLevel_);
    clearanceLayout->addRow(designEntity_, entityRow);
    designAlignment_ = radio("utilityDesignAlignment", "Alignment:", designGroup);
    alignment_ = new QComboBox(this);
    alignment_->setObjectName("utilityDesignAlignmentName");
    alignment_->setToolTip("One of the drawing's alignments: its horizontal geometry, at the "
                           "levels of its design profile where it has one");
    alignment_->setPlaceholderText("the drawing has no alignment");
    clearanceLayout->addRow(designAlignment_, alignment_);
    width_ = field("utilityWidth", "default 0 m - the centre line itself",
                   "WIDTH: the works' width - a pipe's outside diameter, a trench's width");
    clearanceLayout->addRow("Works width (m):", width_);
    horizontal_ = field("utilityH", defaultOf(requirement.horizontal) + " m",
                        "H: the horizontal clearance required, face to face");
    clearanceLayout->addRow("Horizontal clearance (m):", horizontal_);
    vertical_ = field("utilityV", defaultOf(requirement.vertical) + " m",
                      "V: the vertical clearance required, face to face");
    clearanceLayout->addRow("Vertical clearance (m):", vertical_);
    margin_ = field("utilityMargin", defaultOf(requirement.unverifiedMargin) + " m",
                    "MARGIN: a QL-C or QL-D service this close to the works, beyond the "
                    "horizontal clearance, is Unconfirmed - its drawn position is not a "
                    "measurement");
    clearanceLayout->addRow("Unverified margin (m):", margin_);
    tabs_->addTab(clearancePage, "Clearance");

    auto* checkPage = new QWidget(tabs_);
    auto* checkLayout = new QFormLayout(checkPage);
    checkLayout->addRow(noteLabel(
        "The schedule against a client's delivery schema: every mandatory attribute present, "
        "every value from its list, spelt exactly. What is drawn is checked as the schedule "
        "it would write in the schema's words. A schedule with errors still shows the whole "
        "report, and the line fails so that a script can stop on it.",
        checkPage));
    schema_ = field("utilitySchema", "the delivery schema: a .csv of attributes and value lists",
                    "The schema file (include/katana/survey/subsurface/delivery_schema.hpp); "
                    "tools/utility_schema_domains.py makes one from a TfNSW Utility Schema "
                    "workbook");
    auto* schemaBrowse = button("utilitySchemaBrowse", "Browse...");
    checkLayout->addRow("Schema:", row({schema_, schemaBrowse}));
    tabs_->addTab(checkPage, "Check");

    auto* regradePage = new QWidget(tabs_);
    auto* regradeLayout = new QVBoxLayout(regradePage);
    regradeLayout->addWidget(noteLabel(
        "Grades the drawn lines in the scope again from their points as they are now - points "
        "moved, levels or location methods edited - and draws their runs again, as one undo "
        "step. A line whose grading did not change is left alone, and when none did, nothing "
        "is added to the undo history. Always reads what is drawn; the detected spacing and "
        "the minimum cover above apply.",
        regradePage));
    regradeLayout->addStretch(1);
    tabs_->addTab(regradePage, "Regrade");

    auto* schedulePage = new QWidget(tabs_);
    auto* scheduleLayout = new QFormLayout(schedulePage);
    scheduleLayout->addRow(noteLabel(
        "Writes the drawn lines in the scope as a schedule (.csv) that UTILITY reads back as "
        "they are: a drawing edited in CAD made a deliverable. With a schema, in its column "
        "names and spellings. Always reads what is drawn; nothing in the drawing changes.",
        schedulePage));
    scheduleOut_ = field("utilityScheduleOut", "the schedule to write: a .csv",
                         "Where UTILITY SCHEDULE writes the lines in the scope; an existing file "
                         "is replaced");
    auto* scheduleOutBrowse = button("utilityScheduleOutBrowse", "Browse...");
    scheduleLayout->addRow("Write to:", row({scheduleOut_, scheduleOutBrowse}));
    scheduleSchema_ = field("utilityScheduleSchema", "none - the schedule format's own words",
                            "SCHEMA: write in this delivery schema's column names and spellings");
    auto* scheduleSchemaBrowse = button("utilityScheduleSchemaBrowse", "Browse...");
    scheduleLayout->addRow("Schema (optional):", row({scheduleSchema_, scheduleSchemaBrowse}));
    tabs_->addTab(schedulePage, "Schedule");

    // The line, as it will run.
    command_ = new QLineEdit(this);
    command_->setObjectName("utilityCommand");
    command_->setReadOnly(true);
    command_->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    command_->setToolTip("The line Run hands to the command line - type it there, or give it to "
                         "katana_cli, and it does the same");
    run_ = new QPushButton("Run", this);
    run_->setObjectName("utilityRun");
    run_->setDefault(true);
    status_ = new QLabel(this);
    status_->setObjectName("utilityStatus");
    status_->setWordWrap(true);
    status_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    auto* runRow = new QHBoxLayout;
    runRow->addWidget(status_, 1);
    runRow->addWidget(run_);

    output_ = new QPlainTextEdit(this);
    output_->setObjectName("utilityOutput");
    output_->setReadOnly(true);
    output_->setFont(QFontDatabase::systemFont(QFontDatabase::FixedFont));
    // The reports are tables: wrapping would break their columns.
    output_->setLineWrapMode(QPlainTextEdit::NoWrap);
    output_->setPlaceholderText("The reply appears here and in the command log.");
    output_->setMinimumHeight(160);

    copy_ = button("utilityCopy", "Copy");
    save_ = button("utilitySave", "Save As...");
    auto* close = button("utilityClose", "Close");
    copy_->setEnabled(false);
    save_->setEnabled(false);
    auto* buttons = new QHBoxLayout;
    buttons->addWidget(copy_);
    buttons->addWidget(save_);
    buttons->addStretch(1);
    buttons->addWidget(close);

    // The source above everything it feeds; the scope and filter beside the
    // tabs that read them.
    auto* tools = new QVBoxLayout;
    tools->addLayout(common);
    tools->addWidget(tabs_);
    tools->addStretch(1);
    auto* middle = new QHBoxLayout;
    middle->addWidget(scope_, 2);
    middle->addLayout(tools, 3);
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(sourceBox);
    layout->addLayout(middle);
    auto* commandForm = new QFormLayout;
    commandForm->addRow("Command:", command_);
    layout->addLayout(commandForm);
    layout->addLayout(runRow);
    layout->addWidget(output_, 1);
    layout->addLayout(buttons);

    for (QLineEdit* edit : {schedule_,
                            spacing_,
                            layerPrefix_,
                            minCover_,
                            design_,
                            designEntityId_,
                            designLevel_,
                            width_,
                            horizontal_,
                            vertical_,
                            margin_,
                            schema_,
                            scheduleOut_,
                            scheduleSchema_,
                            horizontalUncertainty_,
                            verticalUncertainty_,
                            owner_,
                            material_,
                            diameter_,
                            fields_}) {
        connect(edit, &QLineEdit::textChanged, this, [this] { refreshCommand(); });
    }
    for (QComboBox* box :
         {serviceType_, method_, heights_, levelReference_, path_, serviceStatus_}) {
        connect(box, &QComboBox::currentIndexChanged, this, [this] { refreshCommand(); });
    }
    for (QRadioButton* choice :
         {sourceFile_, sourceDrawing_, designFile_, designEntity_, designAlignment_}) {
        connect(choice, &QRadioButton::toggled, this, [this] { refreshCommand(); });
    }
    connect(alignment_, &QComboBox::currentTextChanged, this, [this] { refreshCommand(); });
    connect(tabs_, &QTabWidget::currentChanged, this, [this] { refreshCommand(); });
    connect(scheduleBrowse_, &QPushButton::clicked, this, [this, csvFilter] {
        browse(*schedule_, "Utility Schedule", csvFilter);
    });
    connect(designBrowse_, &QPushButton::clicked, this,
            [this, csvFilter] { browse(*design_, "Proposed Works", csvFilter); });
    connect(designUseSelected_, &QPushButton::clicked, this, [this] { useSelectedDesign(); });
    connect(schemaBrowse, &QPushButton::clicked, this,
            [this, csvFilter] { browse(*schema_, "Delivery Schema", csvFilter); });
    connect(scheduleOutBrowse, &QPushButton::clicked, this, [this, csvFilter] {
        browseForSave(*scheduleOut_, "Write Utility Schedule", csvFilter);
    });
    connect(scheduleSchemaBrowse, &QPushButton::clicked, this, [this, csvFilter] {
        browse(*scheduleSchema_, "Delivery Schema", csvFilter);
    });
    connect(run_, &QPushButton::clicked, this, [this] { run(); });
    connect(copy_, &QPushButton::clicked, this, [this] {
        QApplication::clipboard()->setText(output_->toPlainText());
        setStatus("Copied.");
    });
    connect(save_, &QPushButton::clicked, this, [this] { saveOutput(); });
    connect(close, &QPushButton::clicked, this, [this] { hide(); });

    if (context_.document != nullptr) {
        // The layers, views and alignments follow the drawing: a DRAW adds
        // layers, an undo takes them away.
        watcher_ = std::make_unique<DocumentWatcher>(
            *context_.document, [this](const DocumentChanges& changes) {
                if (changes.model) {
                    reload();
                }
            });
    }
    reload();
    refreshCommand();
}

UtilityToolsDialog::~UtilityToolsDialog() = default;

bool UtilityToolsDialog::documentAlive() const
{
    return context_.document != nullptr && watcher_ != nullptr && watcher_->documentAlive();
}

void UtilityToolsDialog::showEvent(QShowEvent* event)
{
    // The views open now, not when the dialog was made.
    reload();
    QDialog::showEvent(event);
}

void UtilityToolsDialog::reload()
{
    if (!documentAlive()) {
        // The views are the window's, drawing or none.
        scope_->reloadViews();
        refreshCommand();
        return;
    }
    const katana::entity::Model& model = context_.document->model();
    scope_->reload(model);
    {
        const QSignalBlocker quiet(alignment_);
        const QString kept = alignment_->currentText();
        alignment_->clear();
        for (const std::string& name : model.alignments.names()) {
            alignment_->addItem(qs(name));
        }
        // The one chosen, else the first: a choice with a placeholder is
        // left on none when it is filled, and "none" is no alignment.
        const int index = alignment_->findText(kept);
        alignment_->setCurrentIndex(index >= 0 ? index : (alignment_->count() > 0 ? 0 : -1));
    }
    refreshCommand();
}

void UtilityToolsDialog::showTool(UtilityTool tool)
{
    tabs_->setCurrentIndex(static_cast<int>(tool));
}

UtilityTool UtilityToolsDialog::tool() const
{
    return static_cast<UtilityTool>(tabs_->currentIndex());
}

void UtilityToolsDialog::setSource(UtilitySource source)
{
    (source == UtilitySource::Drawing ? sourceDrawing_ : sourceFile_)->setChecked(true);
}

UtilityForm UtilityToolsDialog::form() const
{
    UtilityForm form;
    form.tool = tool();
    form.source = sourceDrawing_->isChecked() ? UtilitySource::Drawing : UtilitySource::File;
    form.schedule = schedule_->text();
    if (const auto words = scope_->verbWords()) {
        form.scope = *words;
    } else {
        form.scopeError = qs(words.error().message);
    }
    form.designSource = designEntity_->isChecked()      ? UtilityDesignSource::Entity
                        : designAlignment_->isChecked() ? UtilityDesignSource::Alignment
                                                        : UtilityDesignSource::File;
    form.design = design_->text();
    form.designEntity = designEntityId_->text();
    form.designLevel = designLevel_->text();
    form.alignment = alignment_->currentText();
    form.schema = schema_->text();
    form.scheduleOut = scheduleOut_->text();
    form.scheduleSchema = scheduleSchema_->text();
    form.minCover = minCover_->text();
    form.spacing = spacing_->text();
    form.width = width_->text();
    form.horizontal = horizontal_->text();
    form.vertical = vertical_->text();
    form.margin = margin_->text();
    form.layerPrefix = layerPrefix_->text();
    form.serviceType = serviceType_->currentData().toString();
    form.method = method_->currentData().toString();
    form.horizontalUncertainty = horizontalUncertainty_->text();
    form.verticalUncertainty = verticalUncertainty_->text();
    form.heights = heights_->currentData().toString();
    form.levelReference = levelReference_->currentData().toString();
    form.path = path_->currentData().toString();
    form.owner = owner_->text();
    form.material = material_->text();
    form.diameter = diameter_->text();
    form.status = serviceStatus_->currentData().toString();
    form.fields = fields_->text();
    return form;
}

Result<QString> UtilityToolsDialog::command() const
{
    return utilityCommandLine(form());
}

void UtilityToolsDialog::refreshCommand()
{
    const UtilityForm current = form();
    // The detected spacing grades and the minimum cover flags what was
    // graded; Draw, Report and Regrade grade.
    const bool grades = current.tool == UtilityTool::Draw ||
                        current.tool == UtilityTool::Report ||
                        current.tool == UtilityTool::Regrade;
    spacing_->setEnabled(grades);
    minCover_->setEnabled(grades);
    // The source is chosen where a tool reads either; Regrade and Schedule
    // read the drawing whatever is chosen.
    const bool chooses =
        current.tool != UtilityTool::Regrade && current.tool != UtilityTool::Schedule;
    sourceFile_->setEnabled(chooses);
    sourceDrawing_->setEnabled(chooses);
    const bool drawing = utilitySourceOf(current) == UtilitySource::Drawing;
    schedule_->setEnabled(!drawing);
    scheduleBrowse_->setEnabled(!drawing);
    scope_->setEnabled(drawing);
    // What the geometry cannot say is asked only of a draw of the geometry.
    geometry_->setEnabled(drawing && current.tool == UtilityTool::Draw);
    design_->setEnabled(designFile_->isChecked());
    designBrowse_->setEnabled(designFile_->isChecked());
    designEntityId_->setEnabled(designEntity_->isChecked());
    designUseSelected_->setEnabled(designEntity_->isChecked());
    designLevel_->setEnabled(designEntity_->isChecked());
    alignment_->setEnabled(designAlignment_->isChecked());

    const auto line = utilityCommandLine(current);
    command_->setText(line ? *line : QString());
    command_->setPlaceholderText(line ? QString() : "nothing to run yet: " + qs(line.error().message));
}

void UtilityToolsDialog::run()
{
    const auto line = command();
    if (!line) {
        setStatus(qs(line.error().message), true);
        return;
    }
    if (!context_.execute) {
        setStatus("nothing here can run a command", true);
        return;
    }
    const UtilityTool running = tool();
    const auto reply = context_.execute(*line);
    shown_ = running;
    if (!reply) {
        // The whole of it below - a schema check with errors is a whole
        // report - and its first line beside Run.
        showOutput(qs(reply.error().describe()));
        setStatus(qs(reply.error().message).section('\n', 0, 0), true);
        return;
    }
    showOutput(qs(*reply));
    // Said from the reply's own first record, never from the tab alone: a
    // scope that takes no utility line answers with the scope's record, and
    // then nothing was regraded or written, and no step is there to undo.
    const auto leads = [&reply](std::string_view record) { return reply->starts_with(record); };
    switch (running) {
    case UtilityTool::Draw:
        if (leads("utilities drawn ")) {
            setStatus("Drawn as one undo step - Undo removes it all. The records are below and "
                      "in the command log.");
        } else {
            setStatus("Nothing in the scope to draw: what it took is drawn already or cannot be "
                      "a service. Nothing was added to the undo history.");
        }
        break;
    case UtilityTool::Regrade:
        if (leads("utilities regraded changed=0 ")) {
            setStatus("Nothing needed regrading: the drawing is as it was, and nothing was added "
                      "to the undo history.");
        } else if (leads("utilities regraded ")) {
            setStatus("Regraded as one undo step - Undo puts it back. The records are below and "
                      "in the command log.");
        } else {
            setStatus("Nothing in the scope is a utility line: nothing was regraded, and nothing "
                      "was added to the undo history.");
        }
        break;
    case UtilityTool::Schedule:
        setStatus(leads("utilities scheduled ")
                      ? "Written. What was written is below and in the command log."
                      : "Nothing in the scope is a utility line: nothing was written.");
        break;
    case UtilityTool::Report:
    case UtilityTool::Verify:
    case UtilityTool::Clearance:
    case UtilityTool::Check:
        setStatus("Done. The report is below and in the command log.");
        break;
    }
}

bool UtilityToolsDialog::useSelectedDesign()
{
    if (!documentAlive()) {
        setStatus("There is no drawing here to select the proposed works in.", true);
        return false;
    }
    const std::vector<katana::entity::EntityId> selected = context_.document->selection().ids();
    if (selected.size() != 1) {
        setStatus(selected.empty()
                      ? "Select the line or polyline of the proposed works first."
                      : QString("Select just the line or polyline of the proposed works: %1 "
                                "entities are selected.")
                            .arg(selected.size()),
                  true);
        return false;
    }
    designEntityId_->setText("#" + QString::number(selected.front()));
    designEntity_->setChecked(true);
    setStatus("The proposed works: #" + QString::number(selected.front()) +
              ", the entity selected.");
    return true;
}

void UtilityToolsDialog::showOutput(const QString& text)
{
    QString trimmed = text;
    while (trimmed.endsWith('\n')) {
        trimmed.chop(1);
    }
    output_->setPlainText(trimmed);
    copy_->setEnabled(!trimmed.isEmpty());
    save_->setEnabled(!trimmed.isEmpty());
}

void UtilityToolsDialog::browse(QLineEdit& field, const QString& title, const QString& filter)
{
    if (context_.headless && context_.headless()) {
        setStatus("A headless session opens no file dialog: fill " + field.objectName() +
                      " with the path instead.",
                  true);
        return;
    }
    const QString path = QFileDialog::getOpenFileName(this, title, field.text().trimmed(), filter);
    if (!path.isEmpty()) {
        field.setText(QDir::toNativeSeparators(path));
    }
}

void UtilityToolsDialog::browseForSave(QLineEdit& field, const QString& title,
                                       const QString& filter)
{
    if (context_.headless && context_.headless()) {
        setStatus("A headless session opens no file dialog: fill " + field.objectName() +
                      " with the path instead.",
                  true);
        return;
    }
    const QString path = QFileDialog::getSaveFileName(this, title, field.text().trimmed(), filter);
    if (!path.isEmpty()) {
        field.setText(QDir::toNativeSeparators(path));
    }
}

void UtilityToolsDialog::saveOutput()
{
    if (context_.headless && context_.headless()) {
        setStatus("A headless session opens no file dialog; the reply is in the command log.", true);
        return;
    }
    const QString path = QFileDialog::getSaveFileName(this, "Save Utility Output",
                                                      suggestedFileName(),
                                                      "Text files (*.txt);;All files (*)");
    if (path.isEmpty()) {
        return;
    }
    if (const auto saved = saveOutputTo(path); !saved) {
        setStatus(qs(saved.error().describe()), true);
        return;
    }
    setStatus("Saved to " + QDir::toNativeSeparators(path) + ".");
}

QString UtilityToolsDialog::suggestedFileName() const
{
    const UtilityTool named = shown_.value_or(tool());
    return QString("utility_%1.txt").arg(QString(utilityVerbWord(named)).toLower());
}

katana::core::Status UtilityToolsDialog::saveOutputTo(const QString& path)
{
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Truncate)) {
        return makeError(ErrorCode::FileExportFailure, "cannot write " + path.toStdString(),
                         file.errorString().toStdString());
    }
    const QByteArray bytes = (output_->toPlainText() + "\n").toUtf8();
    if (file.write(bytes) != bytes.size()) {
        return makeError(ErrorCode::FileExportFailure, "cannot write " + path.toStdString(),
                         file.errorString().toStdString());
    }
    return {};
}

void UtilityToolsDialog::setStatus(const QString& text, bool isError)
{
    status_->setText(text);
    status_->setStyleSheet(isError ? "color: " + theme::error().name() + ";" : QString());
}

} // namespace katana::qt
