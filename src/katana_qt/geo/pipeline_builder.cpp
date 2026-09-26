// The GDAL Toolbox's Pipeline tab (pipeline_builder.hpp).

#include "geo/pipeline_builder.hpp"

#include <QComboBox>
#include <QDir>
#include <QFile>
#include <QFileDialog>
#include <QFormLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPushButton>
#include <QRegularExpression>
#include <QSignalBlocker>
#include <QSplitter>
#include <QVBoxLayout>

#include <algorithm>
#include <set>
#include <string_view>
#include <utility>

#include "geo/argument_form.hpp"
#include "katana/gis/processing.hpp"

namespace katana::qt {

namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

katana::core::Error invalid(const QString& message)
{
    return makeError(ErrorCode::InvalidArgument, message.toStdString());
}

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

// The kinds the first dataset input reads and the output makes.
std::pair<unsigned, unsigned> kindsOf(const gp::AlgorithmSpec& spec)
{
    unsigned reads = 0;
    unsigned makes = 0;
    for (const gp::ArgSpec& arg : spec.args) {
        if (arg.isDataset() && arg.isInput && !arg.isOutput && reads == 0) {
            reads = arg.datasetKinds;
        }
        if (arg.isDataset() && arg.isOutput && makes == 0) {
            makes = arg.datasetKinds;
        }
    }
    return {reads, makes};
}

const PipelineStepKind* stepAt(const std::vector<std::string>& path)
{
    for (const PipelineStepKind& step : pipelineSteps()) {
        if (step.path == path) {
            return &step;
        }
    }
    return nullptr;
}

// A step's words into its form: --name=value, --name for a flag.
bool loadWords(ArgumentForm& form, const QStringList& words, QString* why)
{
    for (const QString& word : words) {
        const QString body = word.startsWith("--") ? word.mid(2) : word;
        const qsizetype equals = body.indexOf('=');
        const QString name = equals < 0 ? body : body.left(equals);
        const QString value = equals < 0 ? QString("true") : body.mid(equals + 1);
        if (!form.setValue(name.toStdString(), value)) {
            if (why != nullptr) {
                *why = "'" + value + "' does not fit " + name;
            }
            return false;
        }
    }
    return true;
}

// The argument of `spec` a word names: a long name, an alias or a short name.
const gp::ArgSpec* argumentNamed(const gp::AlgorithmSpec& spec, const QString& name)
{
    const std::string wanted = name.toStdString();
    for (const gp::ArgSpec& arg : spec.args) {
        if (arg.name == wanted || (!arg.shortName.empty() && arg.shortName == wanted) ||
            std::ranges::find(arg.aliases, wanted) != arg.aliases.end()) {
            return &arg;
        }
    }
    return nullptr;
}

QString kindWord(unsigned kind)
{
    if (kind == gp::DatasetKind::Raster) {
        return "a raster";
    }
    if (kind == gp::DatasetKind::Vector) {
        return "features";
    }
    return "data";
}

// GDAL's usage JSON with its non-JSON numbers - a default of Infinity (vector
// grid's radius), NaN - made null: no JSON reader takes them, and a reader
// that stops there sees no steps at all.
QByteArray strictJson(const std::string& text)
{
    QByteArray out;
    out.reserve(static_cast<qsizetype>(text.size()));
    bool inString = false;
    for (std::size_t i = 0; i < text.size(); ++i) {
        const char c = text[i];
        if (inString) {
            out += c;
            if (c == '\\' && i + 1 < text.size()) {
                out += text[++i];
            } else if (c == '"') {
                inString = false;
            }
            continue;
        }
        if (c == '"') {
            inString = true;
            out += c;
            continue;
        }
        bool replaced = false;
        for (const std::string_view word : {"-Infinity", "Infinity", "NaN"}) {
            if (text.compare(i, word.size(), word) == 0) {
                out += "null";
                i += word.size() - 1;
                replaced = true;
                break;
            }
        }
        if (!replaced) {
            out += c;
        }
    }
    return out;
}

} // namespace

std::vector<std::string> pipelineStepNames()
{
    std::vector<std::string> names;
    const auto spec = gp::describe({"pipeline"});
    if (!spec) {
        return names;
    }
    const QJsonDocument usage = QJsonDocument::fromJson(strictJson(spec->usageJson));
    for (const QJsonValue step : usage.object().value("pipeline_algorithms").toArray()) {
        names.push_back(step.toObject().value("name").toString().toStdString());
    }
    return names;
}

const std::vector<PipelineStepKind>& pipelineSteps()
{
    static const std::vector<PipelineStepKind> steps = [] {
        // A pipeline's own ends, a step that runs a program, and one that is
        // a pipeline of its own are not steps to offer.
        const std::set<std::string> notOffered{"read", "write", "external", "tee"};
        std::vector<PipelineStepKind> made;
        std::set<std::string> seen;
        for (const std::string& name : pipelineStepNames()) {
            if (notOffered.contains(name) || !seen.insert(name).second) {
                continue;
            }
            for (const char* group : {"raster", "vector"}) {
                const std::vector<std::string> path{group, name};
                const bool leaf = std::ranges::any_of(gp::catalogue(), [&](const gp::AlgorithmInfo& info) {
                    return !info.container && info.path == path;
                });
                if (!leaf) {
                    continue;
                }
                const auto spec = gp::describe(path);
                if (!spec) {
                    continue;
                }
                const auto [reads, makes] = kindsOf(*spec);
                // A step that prints (info) ends a pipeline without a write,
                // and this builder always writes.
                if (makes == 0) {
                    continue;
                }
                made.push_back({name, path, reads, makes, qs(spec->info.description)});
            }
        }
        return made;
    }();
    return steps;
}

Result<QString> pipelineText(const std::vector<PipelineStep>& steps)
{
    QString text = "read";
    for (const PipelineStep& step : steps) {
        text += " ! " + qs(step.path.back());
        for (const QString& word : step.words) {
            if (word.contains('"') || word.contains(' ')) {
                return invalid(qs(step.path.back()) + ": " + word.section('=', 0, 0).remove('"') +
                               " holds a blank, which a pipeline on one line cannot carry");
            }
            text += " " + word;
        }
    }
    return text + " ! write";
}

Result<std::vector<PipelineStep>> parsePipeline(const QString& text, unsigned reads)
{
    QStringList parts;
    for (const QString& part : text.split('!')) {
        parts << part.trimmed();
    }
    if (parts.size() < 2 || parts.front().section(' ', 0, 0) != "read" ||
        parts.back().section(' ', 0, 0) != "write") {
        return invalid("a pipeline is read ! <steps> ! write");
    }
    if (parts.front() != "read" || parts.back() != "write") {
        return invalid("read and write take their datasets from the source and the output, so "
                       "the text names none");
    }
    std::vector<PipelineStep> steps;
    unsigned kind = reads;
    static const QRegularExpression blanks("\\s+");
    for (qsizetype p = 1; p + 1 < parts.size(); ++p) {
        const QStringList words = parts[p].split(blanks, Qt::SkipEmptyParts);
        if (words.isEmpty()) {
            return invalid("a step between two ! is empty");
        }
        const QString name = words.front();
        if (name == "external") {
            return invalid("external runs a program and is refused");
        }
        const PipelineStepKind* chosen = nullptr;
        for (const PipelineStepKind& step : pipelineSteps()) {
            if (qs(step.name) == name && (kind == 0 || (step.reads & kind) != 0)) {
                chosen = &step;
                break;
            }
        }
        if (chosen == nullptr) {
            return invalid("no pipeline step " + name + " reads " + kindWord(kind) + " here");
        }
        const auto spec = gp::describe(chosen->path);
        if (!spec) {
            return spec.error();
        }
        ArgumentForm form("gdalScratch.", "gdalScratchAdvanced");
        form.setAlgorithm(*spec, {}, true);
        std::vector<std::string> positional;
        for (const gp::ArgSpec& arg : spec->args) {
            if (arg.positional && !arg.isDataset() && form.control(arg.name) != nullptr) {
                positional.push_back(arg.name);
            }
        }
        std::size_t nextPositional = 0;
        for (qsizetype w = 1; w < words.size(); ++w) {
            const QString& word = words[w];
            QString argName;
            QString value;
            bool hasValue = false;
            if (word.startsWith("--") || (word.size() == 2 && word[0] == '-' && word[1].isLetter())) {
                const QString body = word.mid(word.startsWith("--") ? 2 : 1);
                const qsizetype equals = body.indexOf('=');
                argName = equals < 0 ? body : body.left(equals);
                if (equals >= 0) {
                    value = body.mid(equals + 1);
                    hasValue = true;
                }
            } else {
                if (nextPositional >= positional.size()) {
                    return invalid(name + " takes no value by position here: " + word);
                }
                argName = qs(positional[nextPositional++]);
                value = word;
                hasValue = true;
            }
            const gp::ArgSpec* arg = argumentNamed(*spec, argName);
            if (arg == nullptr || form.control(arg->name) == nullptr) {
                return invalid(name + " takes no argument " + argName + " in a pipeline");
            }
            if (!hasValue) {
                if (arg->type == gp::ArgType::Boolean) {
                    value = "true";
                } else if (w + 1 < words.size()) {
                    value = words[++w];
                } else {
                    return invalid(name + ": " + argName + " needs a value");
                }
            }
            if (!form.setValue(arg->name, value)) {
                return invalid(name + ": '" + value + "' does not fit " + qs(arg->name));
            }
        }
        auto checked = form.words();
        if (!checked) {
            return invalid(name + ": " + qs(checked.error().message));
        }
        steps.push_back({chosen->path, *checked});
        kind = chosen->makes;
    }
    return steps;
}

Result<QString> pipelineLine(const PipelineForm& form)
{
    const QString text = form.text.trimmed();
    if (text.isEmpty()) {
        return invalid("the pipeline has no steps");
    }
    if (text.contains('"')) {
        return invalid("the pipeline holds a double quote, which a command line cannot carry");
    }
    if (auto refused = gp::checkTokens({"pipeline"}, {text.toStdString()}); !refused) {
        return invalid(qs(refused.error().message));
    }
    if (!form.sourceError.isEmpty()) {
        return invalid("the source: " + form.sourceError);
    }
    if (form.source.trimmed().isEmpty()) {
        return invalid("choose what the pipeline reads");
    }
    QString line = "GDAL pipeline \"" + text + "\" FROM input " + form.source.trimmed();
    const QString name = form.outputName.trimmed();
    if (form.outputKind == "LAYER" || form.outputKind == "FILE") {
        if (name.isEmpty()) {
            return invalid(form.outputKind == "LAYER" ? QString("give the layer's path")
                                                      : QString("give the file's path"));
        }
        auto word = lineWord(name, "the output");
        if (!word) {
            return word.error();
        }
        line += " TO " + form.outputKind + " " + *word;
    } else if (form.outputKind == "REFERENCE") {
        line += " TO REFERENCE";
        if (!name.isEmpty()) {
            auto word = lineWord(name, "the raster's name");
            if (!word) {
                return word.error();
            }
            line += " " + *word;
        }
    }
    return line;
}

PipelineBuilder::PipelineBuilder(GeoDialogContext context, QWidget* parent)
    : QWidget(parent), context_(std::move(context))
{
    setObjectName("gdalPipeline");
    source_ = new BindingPicker("gdalPipeline", BindDrawing | BindRaster | BindSurface | BindFile,
                                context_, this);
    source_->onChanged = [this] { rebuild(false); };
    auto* sourceBox = new QGroupBox("Read", this);
    auto* sourceLayout = new QVBoxLayout(sourceBox);
    sourceLayout->addWidget(source_);

    list_ = new QListWidget(this);
    list_->setObjectName("gdalPipelineSteps");
    list_->setToolTip("read, the steps in order, write");
    add_ = new QComboBox(this);
    add_->setObjectName("gdalPipelineAdd");
    add_->setToolTip("GDAL's pipeline steps that read what the pipeline makes at that point");
    up_ = new QPushButton("Up", this);
    up_->setObjectName("gdalPipelineUp");
    down_ = new QPushButton("Down", this);
    down_->setObjectName("gdalPipelineDown");
    remove_ = new QPushButton("Remove", this);
    remove_->setObjectName("gdalPipelineRemove");
    for (QPushButton* button : {up_, down_, remove_}) {
        button->setAutoDefault(false);
    }
    auto* buttons = new QHBoxLayout;
    buttons->addWidget(add_, 1);
    buttons->addWidget(up_);
    buttons->addWidget(down_);
    buttons->addWidget(remove_);
    auto* stepsBox = new QGroupBox("Steps", this);
    auto* stepsLayout = new QVBoxLayout(stepsBox);
    stepsLayout->addWidget(list_, 1);
    stepsLayout->addLayout(buttons);

    stepForm_ = new ArgumentForm("gdalStep.", "gdalPipelineAdvanced", this);
    stepForm_->onChanged = [this] { stepEdited(); };
    auto* formBox = new QGroupBox("The step's arguments", this);
    auto* formLayout = new QVBoxLayout(formBox);
    formLayout->addWidget(stepForm_);

    text_ = new QLineEdit(this);
    text_->setObjectName("gdalPipelineText");
    text_->setToolTip("The pipeline as GDAL reads it; an edit is read back into the steps");
    outputKind_ = new QComboBox(this);
    outputKind_->setObjectName("gdalPipelineOutputKind");
    outputName_ = new QLineEdit(this);
    outputName_->setObjectName("gdalPipelineOutputName");
    script_ = new QLineEdit(this);
    script_->setObjectName("gdalPipelineScript");
    script_->setPlaceholderText("a script (.kcs) to keep the line in");
    auto* saveButton = new QPushButton("Save to Script", this);
    saveButton->setObjectName("gdalPipelineSave");
    saveButton->setAutoDefault(false);
    saveButton->setToolTip("Adds the line to the script; File > Run Script replays it");
    auto* lower = new QFormLayout;
    lower->addRow("Pipeline:", text_);
    auto* outputRow = new QHBoxLayout;
    outputRow->addWidget(outputKind_);
    outputRow->addWidget(outputName_, 1);
    lower->addRow("Write to:", outputRow);
    auto* scriptRow = new QHBoxLayout;
    scriptRow->addWidget(script_, 1);
    scriptRow->addWidget(saveButton);
    lower->addRow("Recipe:", scriptRow);

    panel_ = new GeoRunPanel("gdalPipeline", context_, true, this);
    panel_->line = [this] { return pipelineLine(form()); };
    panel_->onFinished = [this](const VerbOutcome&) { reload(); };
    source_->say = [this](const QString& text) { panel_->setStatus(text, true); };

    auto* top = new QSplitter(this);
    top->addWidget(sourceBox);
    top->addWidget(stepsBox);
    top->addWidget(formBox);
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(top, 3);
    layout->addLayout(lower);
    layout->addWidget(panel_, 2);

    connect(list_, &QListWidget::currentRowChanged, this, [this] {
        showStep();
        offerSteps();
    });
    connect(add_, &QComboBox::currentIndexChanged, this, [this](int index) {
        if (updating_ || index <= 0) {
            return;
        }
        const QString name = add_->itemText(index);
        addStep(name);
    });
    connect(up_, &QPushButton::clicked, this, [this] { move(-1); });
    connect(down_, &QPushButton::clicked, this, [this] { move(1); });
    connect(remove_, &QPushButton::clicked, this, [this] {
        const int row = list_->currentRow();
        if (row >= 1 && row <= static_cast<int>(steps_.size())) {
            steps_.erase(steps_.begin() + (row - 1));
            typed_.clear();
            rebuild(true);
            select(std::min(row, static_cast<int>(steps_.size())));
        }
    });
    // Any edit of the text - typed, or filled by a headless run - but not
    // the builder's own writing of it.
    connect(text_, &QLineEdit::textChanged, this, [this](const QString& text) {
        if (!writingText_) {
            textEdited(text);
        }
    });
    connect(outputKind_, &QComboBox::currentIndexChanged, this, [this] {
        if (updating_) {
            return;
        }
        const QString kind = outputKind_->currentData().toString();
        outputName_->setText(kind == "LAYER" ? QString("gis/pipeline")
                             : kind == "REFERENCE" ? QString("pipeline")
                                                   : QString());
        panel_->refresh();
    });
    connect(outputName_, &QLineEdit::textChanged, this, [this] { panel_->refresh(); });
    connect(saveButton, &QPushButton::clicked, this, [this] { save(); });
    rebuild(true);
    select(0);
}

unsigned PipelineBuilder::sourceKind() const
{
    switch (source_->kind()) {
    case BindDrawing:
        return gp::DatasetKind::Vector;
    case BindRaster:
    case BindSurface:
        return gp::DatasetKind::Raster;
    case BindFile:
    case BindNone:
        break;
    }
    return 0; // a file may be either
}

unsigned PipelineBuilder::kindAfter(int count) const
{
    unsigned kind = sourceKind();
    for (int i = 0; i < count && i < static_cast<int>(steps_.size()); ++i) {
        if (const PipelineStepKind* step = stepAt(steps_[static_cast<std::size_t>(i)].path)) {
            kind = step->makes;
        }
    }
    return kind;
}

bool PipelineBuilder::addStep(const QString& name)
{
    const int row = list_->currentRow();
    // After the step selected; after the last when read or write is.
    const int at = row >= 1 && row <= static_cast<int>(steps_.size()) ? row
                                                                       : static_cast<int>(steps_.size());
    const unsigned kind = kindAfter(at);
    const PipelineStepKind* chosen = nullptr;
    for (const PipelineStepKind& step : pipelineSteps()) {
        if (qs(step.name) == name && (kind == 0 || (step.reads & kind) != 0)) {
            chosen = &step;
            break;
        }
    }
    if (chosen == nullptr) {
        panel_->setStatus("No pipeline step " + name + " reads " + kindWord(kind) + " there.", true);
        offerSteps();
        return false;
    }
    steps_.insert(steps_.begin() + at, PipelineStep{chosen->path, {}});
    typed_.clear();
    rebuild(true);
    select(at + 1);
    return true;
}

void PipelineBuilder::select(int index)
{
    list_->setCurrentRow(std::clamp(index, 0, list_->count() - 1));
}

void PipelineBuilder::rebuild(bool text)
{
    updating_ = true;
    {
        const QSignalBlocker quiet(list_);
        const int row = list_->currentRow();
        list_->clear();
        list_->addItem("read");
        for (const PipelineStep& step : steps_) {
            QString label = qs(step.path.back());
            if (!step.words.isEmpty()) {
                label += "  " + step.words.join(' ');
            }
            list_->addItem(label);
        }
        list_->addItem("write");
        list_->setCurrentRow(std::clamp(row, 0, list_->count() - 1));
    }
    if (text && typed_.isEmpty()) {
        auto made = pipelineText(steps_);
        writingText_ = true;
        text_->setText(made ? *made : QString());
        writingText_ = false;
        if (!made) {
            panel_->setStatus(qs(made.error().message), true);
        }
    }
    // Where the result can go, as the last step makes it.
    const unsigned kind = kindAfter(static_cast<int>(steps_.size()));
    const QString kept = outputKind_->currentData().toString();
    outputKind_->clear();
    if (kind != gp::DatasetKind::Raster) {
        outputKind_->addItem("Layer", "LAYER");
    }
    if (kind != gp::DatasetKind::Vector) {
        outputKind_->addItem("Reference raster", "REFERENCE");
    }
    outputKind_->addItem("File", "FILE");
    const int index = outputKind_->findData(kept);
    outputKind_->setCurrentIndex(index >= 0 ? index : 0);
    if (index < 0) {
        const QString chosen = outputKind_->currentData().toString();
        outputName_->setText(chosen == "LAYER" ? QString("gis/pipeline")
                             : chosen == "REFERENCE" ? QString("pipeline")
                                                     : QString());
    }
    updating_ = false;
    offerSteps();
    panel_->refresh();
}

void PipelineBuilder::offerSteps()
{
    const bool wasUpdating = updating_;
    updating_ = true;
    const int row = list_->currentRow();
    const int at = row >= 1 && row <= static_cast<int>(steps_.size()) ? row
                                                                       : static_cast<int>(steps_.size());
    const unsigned kind = kindAfter(at);
    add_->clear();
    add_->addItem("Add a step...");
    std::set<std::string> listed;
    for (const PipelineStepKind& step : pipelineSteps()) {
        if ((kind == 0 || (step.reads & kind) != 0) && listed.insert(step.name).second) {
            add_->addItem(qs(step.name), qs(gp::pathText(step.path)));
            add_->setItemData(add_->count() - 1, step.description, Qt::ToolTipRole);
        }
    }
    add_->setCurrentIndex(0);
    const bool middle = row >= 1 && row <= static_cast<int>(steps_.size());
    up_->setEnabled(middle && row > 1);
    down_->setEnabled(middle && row < static_cast<int>(steps_.size()));
    remove_->setEnabled(middle);
    updating_ = wasUpdating;
}

void PipelineBuilder::showStep()
{
    const int row = list_->currentRow();
    const bool middle = row >= 1 && row <= static_cast<int>(steps_.size());
    stepForm_->setVisible(middle);
    if (!middle) {
        return;
    }
    const PipelineStep& step = steps_[static_cast<std::size_t>(row - 1)];
    const auto spec = gp::describe(step.path);
    if (!spec) {
        panel_->setStatus(qs(spec.error().message), true);
        return;
    }
    updating_ = true;
    stepForm_->setAlgorithm(*spec, {}, true);
    QString why;
    if (!loadWords(*stepForm_, step.words, &why)) {
        panel_->setStatus(why, true);
    }
    updating_ = false;
}

void PipelineBuilder::stepEdited()
{
    if (updating_) {
        return;
    }
    const int row = list_->currentRow();
    if (row < 1 || row > static_cast<int>(steps_.size())) {
        return;
    }
    auto words = stepForm_->words();
    if (!words) {
        panel_->setStatus(qs(steps_[static_cast<std::size_t>(row - 1)].path.back()) + ": " +
                              qs(words.error().message),
                          true);
        typed_.clear();
        writingText_ = true;
        text_->clear();
        writingText_ = false;
        panel_->refresh();
        return;
    }
    steps_[static_cast<std::size_t>(row - 1)].words = *words;
    typed_.clear();
    rebuild(true);
}

void PipelineBuilder::textEdited(const QString& text)
{
    auto parsed = parsePipeline(text, sourceKind());
    if (!parsed) {
        // Run as typed when it can be; the steps stay as they were, and the
        // status says why - and, when the line itself is refused (an
        // external step), that it does not run, beside the disabled Run.
        typed_ = text;
        const auto line = pipelineLine(form());
        panel_->setStatus("The text is not read into steps: " + qs(parsed.error().message) +
                              (line ? QString(". It runs as typed.")
                                    : ". It cannot run: " + qs(line.error().message) + "."),
                          true);
        panel_->refresh();
        return;
    }
    typed_.clear();
    steps_ = std::move(parsed).value();
    rebuild(false);
    panel_->setStatus("The steps follow the text.");
    select(1);
}

void PipelineBuilder::move(int by)
{
    const int row = list_->currentRow();
    const int to = row + by;
    if (row < 1 || to < 1 || row > static_cast<int>(steps_.size()) ||
        to > static_cast<int>(steps_.size())) {
        return;
    }
    std::swap(steps_[static_cast<std::size_t>(row - 1)], steps_[static_cast<std::size_t>(to - 1)]);
    typed_.clear();
    rebuild(true);
    select(to);
}

PipelineForm PipelineBuilder::form() const
{
    PipelineForm made;
    made.text = text_->text();
    if (auto words = source_->words()) {
        made.source = *words;
    } else {
        made.sourceError = qs(words.error().message);
    }
    made.outputKind = outputKind_->currentData().toString();
    made.outputName = outputName_->text();
    return made;
}

katana::core::Status PipelineBuilder::saveTo(const QString& path)
{
    auto line = pipelineLine(form());
    if (!line) {
        return line.error();
    }
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Append | QIODevice::Text)) {
        return makeError(ErrorCode::FileExportFailure, "cannot write " + path.toStdString(),
                         file.errorString().toStdString());
    }
    const QByteArray bytes = (*line + "\n").toUtf8();
    if (file.write(bytes) != bytes.size()) {
        return makeError(ErrorCode::FileExportFailure, "cannot write " + path.toStdString(),
                         file.errorString().toStdString());
    }
    return {};
}

void PipelineBuilder::save()
{
    QString path = script_->text().trimmed();
    if (path.isEmpty()) {
        if (context_.headless && context_.headless()) {
            panel_->setStatus("A headless session opens no file dialog: fill gdalPipelineScript "
                              "with the script's path instead.",
                              true);
            return;
        }
        path = QFileDialog::getSaveFileName(this, "Save the Pipeline to a Script", QString(),
                                            "Katana scripts (*.kcs);;All files (*)");
        if (path.isEmpty()) {
            return;
        }
        script_->setText(QDir::toNativeSeparators(path));
    }
    if (auto saved = saveTo(path); !saved) {
        panel_->setStatus(qs(saved.error().describe()), true);
        return;
    }
    panel_->setStatus("Saved to " + QDir::toNativeSeparators(path) +
                      "; File > Run Script replays it.");
}

void PipelineBuilder::reload()
{
    source_->reload();
    panel_->refresh();
}

} // namespace katana::qt
