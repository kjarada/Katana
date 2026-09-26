// GIS > Processing - GDAL > GDAL Toolbox (gdal_toolbox_dialog.hpp).

#include "geo/gdal_toolbox_dialog.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QDesktopServices>
#include <QFormLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QHeaderView>
#include <QLabel>
#include <QLineEdit>
#include <QPushButton>
#include <QScrollArea>
#include <QSettings>
#include <QSignalBlocker>
#include <QSplitter>
#include <QStandardItemModel>
#include <QTabWidget>
#include <QTreeWidget>
#include <QUrl>
#include <QVBoxLayout>

#include <algorithm>
#include <functional>
#include <map>
#include <set>
#include <utility>

#include "customisation/document_watcher.hpp"
#include "geo/argument_form.hpp"
#include "geo/geo_workbench.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"

namespace katana::qt {

namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

// Where the last algorithm chosen is kept, per user.
constexpr const char* kLastAlgorithmKey = "geoprocessing/toolboxAlgorithm";

// The arguments the output's target says instead of the form: its format and
// whether a file already there is replaced or added to.
const std::set<std::string> kSaidByTheTarget{"output-format", "overwrite", "append",
                                             "update",        "overwrite-layer", "upsert"};

katana::core::Error invalid(const QString& message)
{
    return makeError(ErrorCode::InvalidArgument, message.toStdString());
}

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

QString pathText(const std::vector<std::string>& path)
{
    return qs(gp::pathText(path));
}

// The output's default name for a kind: gis/<last word> for a layer, the
// last word for a raster or a surface; none for a file, which must be named.
QString defaultName(const std::vector<std::string>& path, const QString& kind)
{
    if (path.empty() || kind == "FILE") {
        return {};
    }
    const QString last = qs(path.back());
    return kind == "LAYER" ? "gis/" + last : last;
}

} // namespace

Result<QString> gdalToolboxLine(const GdalToolboxForm& form)
{
    if (form.path.empty()) {
        return invalid("choose an algorithm first");
    }
    if (!form.argumentsError.isEmpty()) {
        return invalid(form.argumentsError);
    }
    QString line = "GDAL " + pathText(form.path);
    for (const QString& word : form.arguments) {
        line += " " + word;
    }
    for (const GdalToolboxForm::Input& input : form.inputs) {
        const QString arg = qs(input.arg);
        if (!input.error.isEmpty()) {
            return invalid(arg + ": " + input.error);
        }
        if (input.sources.isEmpty()) {
            if (input.required) {
                return invalid("choose where " + arg + " comes from");
            }
            continue;
        }
        // The first required input is the one a FROM without a name binds;
        // several datasets, or any other input, name their argument.
        const bool unnamed = input.firstRequired && input.sources.size() == 1;
        for (const QString& source : input.sources) {
            line += " FROM " + (unnamed ? QString() : arg + " ") + source;
        }
    }
    const QString name = form.outputName.trimmed();
    if (form.outputKind == "LAYER" || form.outputKind == "FILE" || form.outputKind == "SURFACE") {
        if (name.isEmpty()) {
            return invalid(form.outputKind == "LAYER"  ? QString("give the layer's path")
                           : form.outputKind == "FILE" ? QString("give the file's path")
                                                       : QString("give the surface's name"));
        }
        auto word = lineWord(name, "the output");
        if (!word) {
            return word.error();
        }
        line += " TO " + form.outputKind + " " + *word;
        if (form.outputKind == "FILE" && !form.outputFormat.trimmed().isEmpty()) {
            auto format = lineWord(form.outputFormat.trimmed(), "the format");
            if (!format) {
                return format.error();
            }
            line += " FORMAT " + *format;
        }
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
    if (form.needsConfirm) {
        if (!form.confirm) {
            return invalid("tick Confirm: " + pathText(form.path) +
                           " changes or removes data that already exists");
        }
        line += " CONFIRM";
    }
    if (form.overwrite) {
        line += " OVERWRITE";
    }
    return line;
}

GdalToolboxDialog::GdalToolboxDialog(GeoDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("gdalToolboxDialog");
    setWindowTitle("GDAL Toolbox");
    setModal(false);
    resize(1180, 900);

    tabs_ = new QTabWidget(this);
    tabs_->setObjectName("gdalToolboxTabs");
    auto* page = new QWidget(tabs_);
    page->setObjectName("gdalToolboxAlgorithm");

    // ---- the catalogue ----
    search_ = new QLineEdit(page);
    search_->setObjectName("gdalToolboxSearch");
    search_->setPlaceholderText("search: a name, an alias or a word of the description");
    search_->setClearButtonEnabled(true);
    tree_ = new QTreeWidget(page);
    tree_->setObjectName("gdalToolboxTree");
    tree_->setColumnCount(2);
    tree_->setHeaderLabels({"Algorithm", ""});
    tree_->header()->setStretchLastSection(false);
    tree_->header()->setSectionResizeMode(0, QHeaderView::Stretch);
    tree_->header()->setSectionResizeMode(1, QHeaderView::ResizeToContents);
    auto* left = new QWidget(page);
    auto* leftLayout = new QVBoxLayout(left);
    leftLayout->setContentsMargins(0, 0, 0, 0);
    leftLayout->addWidget(search_);
    leftLayout->addWidget(tree_, 1);

    // ---- the algorithm ----
    auto* right = new QWidget(page);
    auto* rightLayout = new QVBoxLayout(right);
    title_ = new QLabel("Choose an algorithm on the left.", right);
    title_->setObjectName("gdalToolboxTitle");
    title_->setWordWrap(true);
    title_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    description_ = new QLabel(right);
    description_->setObjectName("gdalToolboxDescription");
    description_->setWordWrap(true);
    description_->setTextInteractionFlags(Qt::TextSelectableByMouse);
    help_ = new QPushButton("GDAL's Documentation", right);
    help_->setObjectName("gdalToolboxHelp");
    help_->setAutoDefault(false);
    help_->setEnabled(false);
    auto* titleRow = new QHBoxLayout;
    titleRow->addWidget(title_, 1);
    titleRow->addWidget(help_);
    rightLayout->addLayout(titleRow);
    rightLayout->addWidget(description_);

    auto* body = new QWidget(right);
    auto* bodyLayout = new QVBoxLayout(body);
    inputsBox_ = new QGroupBox("Inputs", body);
    inputsBox_->setObjectName("gdalToolboxInputs");
    inputsLayout_ = new QVBoxLayout(inputsBox_);
    auto* argumentsBox = new QGroupBox("Arguments", body);
    argumentsBox->setObjectName("gdalToolboxArguments");
    arguments_ = new ArgumentForm("gdalArg.", "gdalToolboxAdvanced", argumentsBox);
    arguments_->onChanged = [this] { refresh(); };
    auto* argumentsLayout = new QVBoxLayout(argumentsBox);
    argumentsLayout->addWidget(arguments_);

    outputBox_ = new QGroupBox("Output", body);
    outputBox_->setObjectName("gdalToolboxOutput");
    outputKind_ = new QComboBox(outputBox_);
    outputKind_->setObjectName("gdalOutput.kind");
    outputKind_->setToolTip("Where the result goes: drawn on a layer (one undo step), kept as a "
                            "reference raster, written to a file, or kept as a surface");
    outputName_ = new QLineEdit(outputBox_);
    outputName_->setObjectName("gdalOutput.name");
    outputName_->setToolTip("The layer's path, the reference raster's name, or the file's path");
    outputFormat_ = new QComboBox(outputBox_);
    outputFormat_->setObjectName("gdalOutput.format");
    outputFormat_->setEditable(true);
    outputFormat_->setToolTip("FORMAT: the file's driver; blank to choose it from the extension");
    overwrite_ = new QCheckBox("Replace a file already there (OVERWRITE)", outputBox_);
    overwrite_->setObjectName("gdalOutput.overwrite");
    auto* outputForm = new QFormLayout(outputBox_);
    outputForm->addRow("To:", outputKind_);
    outputForm->addRow("Name:", outputName_);
    outputForm->addRow("Format:", outputFormat_);
    outputForm->addRow(overwrite_);
    confirm_ = new QCheckBox("Confirm: this changes or removes data that already exists", body);
    confirm_->setObjectName("gdalToolboxConfirm");
    confirm_->setVisible(false);
    bodyLayout->addWidget(inputsBox_);
    bodyLayout->addWidget(argumentsBox);
    bodyLayout->addWidget(outputBox_);
    bodyLayout->addWidget(confirm_);
    bodyLayout->addStretch(1);
    auto* scroll = new QScrollArea(right);
    scroll->setWidgetResizable(true);
    scroll->setWidget(body);
    rightLayout->addWidget(scroll, 1);

    auto* splitter = new QSplitter(page);
    splitter->addWidget(left);
    splitter->addWidget(right);
    splitter->setStretchFactor(0, 1);
    splitter->setStretchFactor(1, 3);

    panel_ = new GeoRunPanel("gdalToolbox", context_, true, page);
    panel_->line = [this] { return gdalToolboxLine(form()); };
    // What a run made - a reference raster, a surface - is offered next.
    panel_->onFinished = [this](const VerbOutcome&) { reload(); };

    auto* pageLayout = new QVBoxLayout(page);
    pageLayout->addWidget(splitter, 3);
    pageLayout->addWidget(panel_, 1);
    tabs_->addTab(page, "Algorithm");
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(tabs_);

    buildTree();
    connect(search_, &QLineEdit::textChanged, this, [this](const QString& text) { filter(text); });
    connect(tree_, &QTreeWidget::currentItemChanged, this,
            [this](QTreeWidgetItem* item) { algorithmChosen(item); });
    connect(help_, &QPushButton::clicked, this, [this] { showHelp(); });
    connect(outputKind_, &QComboBox::currentIndexChanged, this, [this] {
        // A name left as the last kind's default follows the kind; one typed
        // is kept.
        const QString kind = outputKind_->currentData().toString();
        const QString name = outputName_->text().trimmed();
        if (name.isEmpty() || name == defaultName(spec_.info.path, lastKind_)) {
            outputName_->setText(defaultName(spec_.info.path, kind));
        }
        lastKind_ = kind;
        outputFormat_->setEnabled(kind == "FILE");
        overwrite_->setEnabled(kind == "FILE");
        refresh();
    });
    for (QLineEdit* edit : {outputName_}) {
        connect(edit, &QLineEdit::textChanged, this, [this] { refresh(); });
    }
    connect(outputFormat_, &QComboBox::currentTextChanged, this, [this] { refresh(); });
    connect(overwrite_, &QCheckBox::toggled, this, [this] { refresh(); });
    connect(confirm_, &QCheckBox::toggled, this, [this] { refresh(); });

    if (context_.document != nullptr) {
        watcher_ = std::make_unique<DocumentWatcher>(
            *context_.document, [this](const DocumentChanges& changes) {
                if (changes.model) {
                    reload();
                }
            });
    }
    refresh();
}

GdalToolboxDialog::~GdalToolboxDialog() = default;

void GdalToolboxDialog::showEvent(QShowEvent* event)
{
    reload();
    QDialog::showEvent(event);
}

void GdalToolboxDialog::buildTree()
{
    std::map<std::string, QTreeWidgetItem*> items;
    for (const gp::AlgorithmInfo& info : gp::catalogue()) {
        std::vector<std::string> parentPath(info.path.begin(), info.path.end() - 1);
        QTreeWidgetItem* parent =
            parentPath.empty() ? nullptr : items[gp::pathText(parentPath)];
        auto* item = parent != nullptr ? new QTreeWidgetItem(parent) : new QTreeWidgetItem(tree_);
        item->setText(0, qs(info.path.back()));
        item->setToolTip(0, pathText(info.path) + ": " + qs(info.description));
        if (!info.container) {
            item->setData(0, Qt::UserRole, pathText(info.path));
            if (info.policy == gp::Policy::Confirm) {
                item->setText(1, "confirm");
                item->setToolTip(1, "Changes or removes data that already exists: runs only when "
                                    "Confirm is ticked");
            }
        }
        items[gp::pathText(info.path)] = item;
    }
}

void GdalToolboxDialog::filter(const QString& text)
{
    const QString wanted = text.trimmed().toLower();
    std::map<std::string, const gp::AlgorithmInfo*> infos;
    for (const gp::AlgorithmInfo& info : gp::catalogue()) {
        infos[gp::pathText(info.path)] = &info;
    }
    const auto matches = [&](const gp::AlgorithmInfo& info) {
        if (wanted.isEmpty()) {
            return true;
        }
        if (pathText(info.path).toLower().contains(wanted) ||
            qs(info.description).toLower().contains(wanted)) {
            return true;
        }
        return std::ranges::any_of(info.aliases, [&](const std::string& alias) {
            return qs(alias).toLower().contains(wanted);
        });
    };
    QTreeWidgetItem* exact = nullptr;
    QTreeWidgetItem* first = nullptr;
    // Depth first, in the catalogue's order; a group shows when any of its
    // algorithms does.
    std::function<bool(QTreeWidgetItem*)> visit = [&](QTreeWidgetItem* item) -> bool {
        bool shown = false;
        const QString path = item->data(0, Qt::UserRole).toString();
        if (!path.isEmpty()) {
            const auto found = infos.find(path.toStdString());
            shown = found != infos.end() && matches(*found->second);
            if (shown) {
                first = first != nullptr ? first : item;
                if (exact == nullptr && item->text(0).toLower() == wanted) {
                    exact = item;
                }
            }
        }
        for (int child = 0; child < item->childCount(); ++child) {
            shown = visit(item->child(child)) || shown;
        }
        item->setHidden(!shown);
        return shown;
    };
    for (int top = 0; top < tree_->topLevelItemCount(); ++top) {
        visit(tree_->topLevelItem(top));
    }
    if (wanted.isEmpty()) {
        tree_->collapseAll();
        return;
    }
    tree_->expandAll();
    // The best match is chosen as the search is typed, so a search is enough
    // to reach an algorithm: the one named exactly so, else the first shown.
    if (QTreeWidgetItem* best = exact != nullptr ? exact : first) {
        tree_->setCurrentItem(best);
    }
}

bool GdalToolboxDialog::choose(const QString& path)
{
    const QList<QTreeWidgetItem*> found =
        tree_->findItems("*", Qt::MatchWildcard | Qt::MatchRecursive);
    for (QTreeWidgetItem* item : found) {
        if (item->data(0, Qt::UserRole).toString() == path.trimmed().toLower()) {
            tree_->setCurrentItem(item);
            return true;
        }
    }
    return false;
}

QString GdalToolboxDialog::chosen() const
{
    return spec_.info.path.empty() ? QString() : pathText(spec_.info.path);
}

void GdalToolboxDialog::algorithmChosen(QTreeWidgetItem* item)
{
    const QString path = item != nullptr ? item->data(0, Qt::UserRole).toString() : QString();
    if (path.isEmpty() || path == chosen()) {
        return;
    }
    std::size_t consumed = 0;
    const QStringList words = path.split(' ');
    std::vector<std::string> parts;
    for (const QString& word : words) {
        parts.push_back(word.toStdString());
    }
    auto resolved = gp::resolve(parts, consumed);
    auto described = resolved ? gp::describe(*resolved) : Result<gp::AlgorithmSpec>(resolved.error());
    if (!described) {
        panel_->setStatus(qs(described.error().describe()), true);
        return;
    }
    choosing_ = true;
    spec_ = std::move(described).value();
    const gp::AlgorithmInfo& info = spec_.info;
    title_->setText("<b>" + pathText(info.path).toHtmlEscaped() + "</b> - " +
                    (info.policy == gp::Policy::Confirm
                         ? QString("changes or removes data that already exists: it runs only "
                                   "when Confirm is ticked")
                         : QString("safe: it changes nothing that exists")));
    description_->setText(qs(spec_.longDescription.empty() ? info.description
                                                           : spec_.longDescription));
    help_->setEnabled(!info.helpUrl.empty());
    help_->setToolTip(qs(info.helpUrl));

    // The datasets it reads: a picker each, offering only what can be one.
    // Emptying the layout deletes the last algorithm's labels and pickers.
    inputs_.clear();
    while (QLayoutItem* old = inputsLayout_->takeAt(0)) {
        delete old->widget();
        delete old;
    }
    for (const gp::ArgSpec& arg : spec_.args) {
        if (!arg.isDataset() || !arg.isInput || arg.isOutput) {
            continue;
        }
        const QString name = qs(arg.name);
        const QString prefix = "gdalInput." + name + ".";
        const unsigned kinds = bindingKindsFor(arg.datasetKinds);
        auto* label = new QLabel("<b>" + name.toHtmlEscaped() + "</b>" +
                                     (arg.required ? QString(" (required)") : QString()) + " - " +
                                     qs(arg.description).toHtmlEscaped(),
                                 inputsBox_);
        label->setWordWrap(true);
        inputsLayout_->addWidget(label);
        InputRow row{arg, nullptr, nullptr};
        if (arg.type == gp::ArgType::DatasetList && arg.maxCount != 1) {
            row.list = new BindingList(prefix, kinds, context_, "List", inputsBox_);
            row.list->onChanged = [this] { refresh(); };
            row.list->picker().say = [this](const QString& text) { panel_->setStatus(text, true); };
            if (!arg.required) {
                row.list->picker().setOptional();
            }
            inputsLayout_->addWidget(row.list);
        } else {
            row.picker = new BindingPicker(prefix, kinds, context_, inputsBox_);
            row.picker->onChanged = [this] { refresh(); };
            row.picker->say = [this](const QString& text) { panel_->setStatus(text, true); };
            if (!arg.required) {
                row.picker->setOptional();
            }
            inputsLayout_->addWidget(row.picker);
        }
        inputs_.push_back(std::move(row));
    }
    inputsBox_->setVisible(!inputs_.empty());

    arguments_->setAlgorithm(spec_, kSaidByTheTarget);

    // Where the output can go, as its kinds allow; a surface once the
    // terrain session's store takes a raster result (TO SURFACE is refused
    // until then, so it is not offered).
    const gp::ArgSpec* output = nullptr;
    for (const gp::ArgSpec& arg : spec_.args) {
        if (arg.isOutput && arg.type == gp::ArgType::Dataset) {
            output = &arg;
            break;
        }
    }
    {
        const QSignalBlocker quiet(outputKind_);
        outputKind_->clear();
        if (output != nullptr) {
            const bool vector = (output->datasetKinds & gp::DatasetKind::Vector) != 0;
            const bool raster = (output->datasetKinds & gp::DatasetKind::Raster) != 0;
            if (vector) {
                outputKind_->addItem("Layer", "LAYER");
            }
            if (raster) {
                outputKind_->addItem("Reference raster", "REFERENCE");
            }
            outputKind_->addItem("File", "FILE");
            if (raster) {
                outputKind_->addItem("Surface", "SURFACE");
                if (auto* model = qobject_cast<QStandardItemModel*>(outputKind_->model())) {
                    model->item(outputKind_->count() - 1)->setEnabled(false);
                    model->item(outputKind_->count() - 1)
                        ->setToolTip("A raster result kept as a surface arrives with the terrain "
                                     "session");
                }
            }
            outputKind_->setCurrentIndex(0);
        }
    }
    outputBox_->setVisible(output != nullptr);
    lastKind_ = outputKind_->currentData().toString();
    outputName_->setText(defaultName(info.path, lastKind_));
    {
        const QSignalBlocker quiet(outputFormat_);
        outputFormat_->clear();
        outputFormat_->addItem(QString());
        if (output != nullptr) {
            for (const std::string& format : gp::suggest(info.path, "output-format", "")) {
                outputFormat_->addItem(qs(format));
            }
        }
    }
    const QString kind = outputKind_->currentData().toString();
    outputFormat_->setEnabled(kind == "FILE");
    overwrite_->setChecked(false);
    overwrite_->setEnabled(kind == "FILE");
    confirm_->setChecked(false);
    confirm_->setVisible(info.policy == gp::Policy::Confirm);
    choosing_ = false;

    if (remember_) {
        QSettings settings;
        if (settings.status() == QSettings::NoError) {
            settings.setValue(kLastAlgorithmKey, pathText(info.path));
        }
    }
    reload();
}

GdalToolboxForm GdalToolboxDialog::form() const
{
    GdalToolboxForm made;
    made.path = spec_.info.path;
    if (auto words = arguments_->words()) {
        made.arguments = *words;
    } else {
        made.argumentsError = qs(words.error().message);
    }
    bool firstRequiredTaken = false;
    for (const InputRow& row : inputs_) {
        GdalToolboxForm::Input input;
        input.arg = row.spec.name;
        input.required = row.spec.required;
        if (row.spec.required && !firstRequiredTaken) {
            input.firstRequired = true;
            firstRequiredTaken = true;
        }
        if (row.list != nullptr) {
            if (auto words = row.list->words()) {
                input.sources = *words;
            } else {
                input.error = qs(words.error().message);
            }
        } else if (auto words = row.picker->words()) {
            if (!words->isEmpty()) {
                input.sources << *words;
            }
        } else {
            input.error = qs(words.error().message);
        }
        made.inputs.push_back(std::move(input));
    }
    made.outputKind = outputKind_->count() > 0 ? outputKind_->currentData().toString() : QString();
    made.outputName = outputName_->text();
    made.outputFormat = outputFormat_->isEnabled() ? outputFormat_->currentText() : QString();
    made.overwrite = overwrite_->isEnabled() && overwrite_->isChecked();
    made.needsConfirm = spec_.info.policy == gp::Policy::Confirm && !spec_.info.path.empty();
    made.confirm = confirm_->isChecked();
    return made;
}

void GdalToolboxDialog::reload()
{
    for (InputRow& row : inputs_) {
        if (row.list != nullptr) {
            row.list->reload();
        } else {
            row.picker->reload();
        }
    }
    refresh();
}

void GdalToolboxDialog::refresh()
{
    if (choosing_) {
        return;
    }
    panel_->refresh();
}

void GdalToolboxDialog::showHelp()
{
    const QString url = qs(spec_.info.helpUrl);
    if (url.isEmpty()) {
        return;
    }
    if (context_.headless && context_.headless()) {
        panel_->setStatus("GDAL's page for " + chosen() + ": " + url +
                          " (a headless session opens no browser)");
        return;
    }
    QDesktopServices::openUrl(QUrl(url));
}

GdalToolboxDialog& showGdalToolboxDialog(GeoWorkbench& workbench, QWidget& window)
{
    auto& dialog = showKeptDialog<GdalToolboxDialog>(window, "gdalToolboxDialog", [&] {
        auto* made = new GdalToolboxDialog(geoDialogContext(workbench), &window);
        // The algorithm last chosen, if this GDAL still has it; a store that
        // cannot be read is no algorithm.
        const QSettings settings;
        if (settings.status() == QSettings::NoError) {
            made->choose(settings.value(kLastAlgorithmKey).toString());
        }
        made->rememberChoices(true);
        return made;
    });
    dialog.reload();
    return dialog;
}

} // namespace katana::qt
