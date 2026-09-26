// Terrain > DEM > DEM Tools (dem_tools_dialog.hpp).

#include "geo/dem_tools_dialog.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPushButton>
#include <QRadioButton>
#include <QSignalBlocker>
#include <QTabWidget>
#include <QVBoxLayout>

#include <algorithm>
#include <string>
#include <utility>

#include "customisation/document_watcher.hpp"
#include "geo/geo_workbench.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "katana/gis/processing.hpp"
#include "katana/interop/reference_data.hpp"

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

// The prefix of a tab's object names: demMosaic, demClip ...
QString prefixOf(DemTool tool)
{
    const QString word = QString(demToolWord(tool)).toLower();
    return "dem" + word.left(1).toUpper() + word.mid(1);
}

// The options whose value must read as a number.
bool numeric(const QString& key)
{
    return key == "distance" || key == "smoothing" || key == "cell";
}

// GDAL's own choices for an argument, so the dialog offers what this GDAL
// takes; a blank first, which leaves GDAL's default.
QStringList choicesOf(const std::vector<std::string>& path, const std::string& argument)
{
    QStringList choices{QString()};
    if (const auto spec = gp::describe(path)) {
        for (const gp::ArgSpec& arg : spec->args) {
            if (arg.name == argument) {
                for (const std::string& choice : arg.choices) {
                    choices << QString::fromStdString(choice);
                }
            }
        }
    }
    return choices;
}

} // namespace

const char* demToolWord(DemTool tool)
{
    switch (tool) {
    case DemTool::Mosaic:
        return "MOSAIC";
    case DemTool::Clip:
        return "CLIP";
    case DemTool::Fill:
        return "FILL";
    case DemTool::Reproject:
        return "REPROJECT";
    case DemTool::Footprint:
        return "FOOTPRINT";
    case DemTool::Difference:
        return "DIFFERENCE";
    }
    return "";
}

Result<QString> demToolCommandLine(const DemToolForm& form)
{
    QString line = QString("RASTER ") + demToolWord(form.tool);
    if (form.sources.isEmpty()) {
        return invalid("choose the raster first");
    }
    for (const QString& source : form.sources) {
        if (source.trimmed().isEmpty()) {
            return invalid("choose the raster first");
        }
        line += " " + source.trimmed();
    }
    if (!form.regionError.isEmpty()) {
        return invalid(form.regionError);
    }
    if (form.tool == DemTool::Clip && form.region.trimmed().isEmpty()) {
        return invalid("choose what to clip to: a box, or the closed boundaries of a scope");
    }
    if (!form.region.trimmed().isEmpty()) {
        line += " " + form.region.trimmed();
    }
    for (const auto& [key, text] : form.options) {
        const QString value = text.trimmed();
        if (value.isEmpty()) {
            continue;
        }
        if (std::any_of(value.begin(), value.end(), [](QChar c) { return c.isSpace(); }) ||
            value.contains('=') || value.contains('"')) {
            return invalid(key + " is one word with no blank, '=' or double quote: '" + value + "'");
        }
        if (numeric(key) && !katana::core::parseFiniteDouble(value.toStdString())) {
            return invalid(key + " must be a number, not '" + value + "'");
        }
        line += " " + key + "=" + value;
    }
    if (!form.save.trimmed().isEmpty()) {
        auto word = lineWord(form.save.trimmed(), "the file SAVE writes");
        if (!word) {
            return word.error();
        }
        line += " SAVE " + *word;
    }
    if (!form.name.trimmed().isEmpty()) {
        auto word = lineWord(form.name.trimmed(), "the name");
        if (!word) {
            return word.error();
        }
        line += " NAME " + *word;
    }
    if (!form.layer.trimmed().isEmpty()) {
        auto word = lineWord(form.layer.trimmed(), "the layer");
        if (!word) {
            return word.error();
        }
        line += " TO LAYER " + *word;
    }
    return line;
}

DemToolsDialog::DemToolsDialog(GeoDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("demToolsDialog");
    setWindowTitle("DEM Tools");
    setModal(false);
    resize(1000, 860);

    const auto field = [this](const QString& name, const QString& placeholder, const QString& tip) {
        auto* edit = new QLineEdit(this);
        edit->setObjectName(name);
        edit->setPlaceholderText(placeholder);
        edit->setToolTip(tip);
        connect(edit, &QLineEdit::textChanged, this, [this] { refresh(); });
        return edit;
    };
    const auto note = [](const QString& text, QWidget* owner) {
        auto* label = new QLabel(text, owner);
        label->setWordWrap(true);
        return label;
    };

    tabs_ = new QTabWidget(this);
    tabs_->setObjectName("demToolsTabs");
    const unsigned rasters = BindRaster | BindSurface | BindFile;
    for (int i = 0; i < kDemToolCount; ++i) {
        const auto tool = static_cast<DemTool>(i);
        const QString prefix = prefixOf(tool);
        auto* page = new QWidget(tabs_);
        page->setObjectName("demTools" + prefix.mid(3));
        auto* layout = new QVBoxLayout(page);
        // A mosaic's tiles are rasters or files, several (a surface is no
        // tile); every other tool reads one raster.
        QWidget* picked = nullptr;
        if (tool == DemTool::Mosaic) {
            mosaicTiles_ = new BindingList(prefix, BindRaster | BindFile, context_, "Tiles", page);
            mosaicTiles_->onChanged = [this] { refresh(); };
            sources_[i] = &mosaicTiles_->picker();
            picked = mosaicTiles_;
        } else {
            sources_[i] = new BindingPicker(prefix, rasters, context_, page);
            sources_[i]->onChanged = [this] { refresh(); };
            picked = sources_[i];
        }
        panels_[i] = new GeoRunPanel(prefix, context_, true, page);
        GeoRunPanel* panel = panels_[i];
        sources_[i]->say = [panel](const QString& text) { panel->setStatus(text, true); };
        panel->line = [this, tool] { return demToolCommandLine(form(tool)); };
        // A result may be a raster the other tabs can take next.
        panel->onFinished = [this](const VerbOutcome&) { reload(); };
        if (tool != DemTool::Footprint) {
            names_[i] = field(prefix + "Name", "default: " + QString(demToolWord(tool)).toLower(),
                              "NAME: the reference raster's name; a name taken becomes <name>-2");
        }
        auto* options = new QGroupBox(this);
        options->setObjectName(prefix + "Options");
        auto* grid = new QFormLayout(options);
        auto* sourceBox = new QGroupBox(tool == DemTool::Difference ? "First (minus the second)"
                                                                    : "Raster",
                                        page);
        auto* sourceLayout = new QVBoxLayout(sourceBox);
        sourceLayout->addWidget(picked);

        switch (tool) {
        case DemTool::Mosaic: {
            grid->addRow(note("Tiles joined into one DEM, kept as a reference raster reading them "
                              "where they are (a VRT); SAVE writes the mosaic to a file of its "
                              "own and keeps that. A folder is every raster in it, a pattern "
                              "(tiles/*.tif) every file it matches.",
                              options));
            resolution_ = new QComboBox(options);
            resolution_->setObjectName("demMosaicResolution");
            resolution_->setEditable(true);
            // GDAL's words for raster mosaic --resolution (its documentation):
            // or <x>,<y> typed.
            resolution_->addItems({QString(), "same", "highest", "lowest", "average"});
            resolution_->setToolTip("resolution=: the mosaic's cell when the tiles' differ - same "
                                    "(they must agree), highest, lowest, average, or x,y");
            connect(resolution_, &QComboBox::currentTextChanged, this, [this] { refresh(); });
            grid->addRow("Resolution:", resolution_);
            save_ = field("demMosaicSave", "keep it virtual (a VRT)",
                          "SAVE: write the mosaic to this file and keep that file as the "
                          "reference raster");
            grid->addRow("Save to:", save_);
            break;
        }
        case DemTool::Clip: {
            grid->addRow(note("The raster cut to a box, or to the closed boundaries a scope takes "
                              "(a cell is kept when its centre is inside).",
                              options));
            clipByArea_ = new QRadioButton("To a box:", options);
            clipByArea_->setObjectName("demClipByArea");
            clipByArea_->setChecked(true);
            clipArea_ = field("demClipArea", "x0,y0,x1,y1", "AREA: the box, in the drawing's units");
            clipByBoundaries_ = new QRadioButton("To the closed boundaries of:", options);
            clipByBoundaries_->setObjectName("demClipByBoundaries");
            clipBoundaries_ = new ScopeFilterWidget("demClipBoundary", options);
            clipBoundaries_->views = context_.views;
            clipBoundaries_->setChoice(ScopeChoice::Selection);
            clipBoundaries_->onChanged = [this] { refresh(); };
            connect(clipByArea_, &QRadioButton::toggled, this, [this] { refresh(); });
            grid->addRow(clipByArea_, clipArea_);
            grid->addRow(clipByBoundaries_);
            grid->addRow(clipBoundaries_);
            break;
        }
        case DemTool::Fill: {
            grid->addRow(note("The raster's cells with no value filled by inverse-distance "
                              "weighting from the cells round them; the reply counts what was "
                              "empty and what was filled.",
                              options));
            fillDistance_ = field("demFillDistance", "GDAL's default, 100",
                                  "distance=: how far to look for values, in cells");
            fillSmoothing_ = field("demFillSmoothing", "none",
                                   "smoothing=: passes of a 3 x 3 average over what was filled");
            fillStrategy_ = new QComboBox(options);
            fillStrategy_->setObjectName("demFillStrategy");
            fillStrategy_->addItems(choicesOf({"raster", "fill-nodata"}, "strategy"));
            fillStrategy_->setToolTip("strategy=: GDAL's own choices; blank for its default");
            connect(fillStrategy_, &QComboBox::currentTextChanged, this, [this] { refresh(); });
            grid->addRow("Search distance (cells):", fillDistance_);
            grid->addRow("Smoothing passes:", fillSmoothing_);
            grid->addRow("Strategy:", fillStrategy_);
            break;
        }
        case DemTool::Reproject: {
            grid->addRow(note("The raster into the project's coordinate system, another (an "
                              "EPSG code), or another raster's grid. A raster that says nothing "
                              "of where it is is refused: say it with From.",
                              options));
            reprojectCrs_ = field("demReprojectCrs", "the project's coordinate system",
                                  "crs=: EPSG:28356, or any system GDAL reads");
            reprojectLike_ = new QComboBox(options);
            reprojectLike_->setObjectName("demReprojectLike");
            reprojectLike_->setToolTip("like=: onto this reference raster's grid - its system, "
                                       "extent and cells");
            connect(reprojectLike_, &QComboBox::currentIndexChanged, this, [this] { refresh(); });
            reprojectFrom_ = field("demReprojectFrom", "what the raster says",
                                   "from=: the raster's own system, when it does not say");
            reprojectResampling_ = new QComboBox(options);
            reprojectResampling_->setObjectName("demReprojectResampling");
            reprojectResampling_->addItems(choicesOf({"raster", "reproject"}, "resampling"));
            reprojectResampling_->setToolTip("resampling=: GDAL's own choices; blank for its "
                                             "default (nearest)");
            connect(reprojectResampling_, &QComboBox::currentTextChanged, this,
                    [this] { refresh(); });
            reprojectCell_ = field("demReprojectCell", "as GDAL chooses",
                                   "cell=: the result's cell, in the target system's units");
            grid->addRow("To system:", reprojectCrs_);
            grid->addRow("Or onto the grid of:", reprojectLike_);
            grid->addRow("From system:", reprojectFrom_);
            grid->addRow("Resampling:", reprojectResampling_);
            grid->addRow("Cell:", reprojectCell_);
            break;
        }
        case DemTool::Footprint: {
            grid->addRow(note("Where the raster has data, drawn as closed polylines on a layer, "
                              "as one undo step.",
                              options));
            footprintLayer_ = field("demFootprintLayer", "gis/footprint",
                                    "TO LAYER: the layer the footprint is drawn on");
            grid->addRow("Layer:", footprintLayer_);
            break;
        }
        case DemTool::Difference: {
            grid->addRow(note("The first minus the second, the second aligned to the first's "
                              "grid where they differ: a raster of the difference, and the "
                              "volumes of fill (the first above) and cut (below) by the grid "
                              "method.",
                              options));
            auto* secondBox = new QGroupBox("Second", options);
            auto* secondLayout = new QVBoxLayout(secondBox);
            differenceSecond_ =
                new BindingPicker("demDifferenceSecond", rasters, context_, secondBox);
            differenceSecond_->onChanged = [this] { refresh(); };
            differenceSecond_->say = [panel](const QString& text) { panel->setStatus(text, true); };
            secondLayout->addWidget(differenceSecond_);
            grid->addRow(secondBox);
            differenceLimit_ =
                new QCheckBox("Only within the closed boundaries of:", options);
            differenceLimit_->setObjectName("demDifferenceLimit");
            differenceWithin_ = new ScopeFilterWidget("demDifferenceWithin", options);
            differenceWithin_->views = context_.views;
            differenceWithin_->setChoice(ScopeChoice::Selection);
            differenceWithin_->onChanged = [this] { refresh(); };
            connect(differenceLimit_, &QCheckBox::toggled, this, [this] { refresh(); });
            grid->addRow(differenceLimit_);
            grid->addRow(differenceWithin_);
            differenceResampling_ = new QComboBox(options);
            differenceResampling_->setObjectName("demDifferenceResampling");
            differenceResampling_->addItems(choicesOf({"raster", "reproject"}, "resampling"));
            differenceResampling_->setToolTip("resampling=: how the second is aligned when the "
                                              "grids differ; blank for bilinear");
            connect(differenceResampling_, &QComboBox::currentTextChanged, this,
                    [this] { refresh(); });
            grid->addRow("Alignment:", differenceResampling_);
            break;
        }
        }
        if (names_[i] != nullptr) {
            grid->addRow("Name:", names_[i]);
        }
        auto* top = new QHBoxLayout;
        top->addWidget(sourceBox, 2);
        top->addWidget(options, 3);
        layout->addLayout(top, 3);
        layout->addWidget(panel, 2);
        tabs_->addTab(page, QString(demToolWord(tool)).left(1) +
                                QString(demToolWord(tool)).mid(1).toLower());
    }
    connect(tabs_, &QTabWidget::currentChanged, this, [this] { refresh(); });
    auto* layout = new QVBoxLayout(this);
    layout->addWidget(tabs_);

    if (context_.document != nullptr) {
        watcher_ = std::make_unique<DocumentWatcher>(
            *context_.document, [this](const DocumentChanges& changes) {
                if (changes.model) {
                    reload();
                }
            });
    }
    reload();
}

DemToolsDialog::~DemToolsDialog() = default;

void DemToolsDialog::showEvent(QShowEvent* event)
{
    reload();
    QDialog::showEvent(event);
}

void DemToolsDialog::showTool(DemTool tool)
{
    tabs_->setCurrentIndex(static_cast<int>(tool));
}

DemTool DemToolsDialog::tool() const
{
    return static_cast<DemTool>(tabs_->currentIndex());
}

GeoRunPanel& DemToolsDialog::runPanel(DemTool tool) const
{
    return *panels_[static_cast<std::size_t>(tool)];
}

BindingPicker& DemToolsDialog::source(DemTool tool) const
{
    return *sources_[static_cast<std::size_t>(tool)];
}

DemToolForm DemToolsDialog::form(DemTool tool) const
{
    DemToolForm made;
    made.tool = tool;
    const auto index = static_cast<std::size_t>(tool);
    const auto sourceWords = [&made](const BindingPicker& picker) {
        auto words = picker.words();
        if (words) {
            made.sources << *words;
        } else {
            made.regionError = QString::fromStdString(words.error().message);
        }
    };
    if (tool == DemTool::Mosaic) {
        if (auto tiles = mosaicTiles_->words()) {
            made.sources = *tiles;
        } else {
            made.regionError = QString::fromStdString(tiles.error().message);
        }
    } else {
        sourceWords(*sources_[index]);
    }
    if (names_[index] != nullptr) {
        made.name = names_[index]->text();
    }
    switch (tool) {
    case DemTool::Mosaic:
        made.options = {{"resolution", resolution_->currentText()}};
        made.save = save_->text();
        break;
    case DemTool::Clip:
        if (clipByArea_->isChecked()) {
            const QString box = clipArea_->text().trimmed();
            const QStringList corners = box.split(',');
            if (box.isEmpty()) {
                made.regionError = made.regionError.isEmpty() ? QString("give the box, x0,y0,x1,y1")
                                                              : made.regionError;
            } else if (corners.size() != 4 ||
                       !std::ranges::all_of(corners, [](const QString& c) {
                           return katana::core::parseFiniteDouble(c.trimmed().toStdString())
                               .has_value();
                       })) {
                made.regionError = "the box is four numbers, x0,y0,x1,y1";
            } else {
                made.region = "AREA " + box;
            }
        } else if (auto words = clipBoundaries_->verbWords()) {
            made.region = *words;
        } else if (made.regionError.isEmpty()) {
            made.regionError =
                "the boundaries' scope: " + QString::fromStdString(words.error().message);
        }
        break;
    case DemTool::Fill:
        made.options = {{"distance", fillDistance_->text()},
                        {"smoothing", fillSmoothing_->text()},
                        {"strategy", fillStrategy_->currentText()}};
        break;
    case DemTool::Reproject:
        made.options = {{"crs", reprojectCrs_->isEnabled() ? reprojectCrs_->text() : QString()},
                        {"like", reprojectLike_->currentData().toString()},
                        {"from", reprojectFrom_->text()},
                        {"resampling", reprojectResampling_->currentText()},
                        {"cell", reprojectCell_->text()}};
        break;
    case DemTool::Footprint:
        made.layer = footprintLayer_->text();
        break;
    case DemTool::Difference:
        sourceWords(*differenceSecond_);
        if (differenceLimit_->isChecked()) {
            if (auto words = differenceWithin_->verbWords()) {
                made.region = *words;
            } else if (made.regionError.isEmpty()) {
                made.regionError =
                    "the boundaries' scope: " + QString::fromStdString(words.error().message);
            }
        }
        made.options = {{"resampling", differenceResampling_->currentText()}};
        break;
    }
    return made;
}

void DemToolsDialog::reload()
{
    for (BindingPicker* picker : sources_) {
        picker->reload();
    }
    differenceSecond_->reload();
    const bool alive = context_.document != nullptr && watcher_ != nullptr &&
                       watcher_->documentAlive();
    for (ScopeFilterWidget* scope : {clipBoundaries_, differenceWithin_}) {
        if (alive) {
            scope->reload(context_.document->model());
        } else {
            scope->reloadViews();
        }
    }
    {
        const QSignalBlocker quiet(reprojectLike_);
        const QVariant kept = reprojectLike_->currentData();
        reprojectLike_->clear();
        reprojectLike_->addItem("(none)", QString());
        if (context_.reference != nullptr) {
            for (const katana::interop::RasterOverlay& raster : context_.reference->rasters()) {
                reprojectLike_->addItem(
                    QString("%1  %2").arg(raster.id).arg(QString::fromStdString(raster.name)),
                    QString::number(raster.id));
            }
        }
        const int index = reprojectLike_->findData(kept);
        reprojectLike_->setCurrentIndex(std::max(index, 0));
    }
    refresh();
}

void DemToolsDialog::refresh()
{
    clipArea_->setEnabled(clipByArea_->isChecked());
    clipBoundaries_->setEnabled(clipByBoundaries_->isChecked());
    differenceWithin_->setEnabled(differenceLimit_->isChecked());
    // like= and crs= both say where the raster goes.
    reprojectCrs_->setEnabled(reprojectLike_->currentData().toString().isEmpty());
    for (GeoRunPanel* panel : panels_) {
        panel->refresh();
    }
}

DemToolsDialog& showDemToolsDialog(GeoWorkbench& workbench, QWidget& window)
{
    auto& dialog = showKeptDialog<DemToolsDialog>(window, "demToolsDialog", [&] {
        return new DemToolsDialog(geoDialogContext(workbench), &window);
    });
    dialog.reload();
    return dialog;
}

} // namespace katana::qt
