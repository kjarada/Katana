// Terrain > DEM > Grid Points to DEM (grid_dem_dialog.hpp).

#include "geo/grid_dem_dialog.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QFormLayout>
#include <QGroupBox>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QRegularExpression>
#include <QSignalBlocker>
#include <QVBoxLayout>

#include <algorithm>
#include <set>
#include <string>
#include <variant>
#include <vector>

#include "customisation/document_watcher.hpp"
#include "geo/geo_workbench.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
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

bool readsAsNumber(const QString& text)
{
    return katana::core::parseFiniteDouble(text.toStdString()).has_value();
}

// " key=value" for a number given, nothing for a blank field.
Result<QString> numberOption(const QString& key, const QString& text, const QString& field)
{
    const QString value = text.trimmed();
    if (value.isEmpty()) {
        return QString();
    }
    if (!readsAsNumber(value)) {
        return invalid(field + " must be a number, not '" + value + "'");
    }
    return " " + key + "=" + value;
}

// An option's value is one word with no '=' and no quote: a blank would end
// it and a quoted word is never an option.
Result<QString> wordOption(const QString& key, const QString& text, const QString& field)
{
    const QString value = text.trimmed();
    if (value.isEmpty()) {
        return QString();
    }
    if (std::any_of(value.begin(), value.end(), [](QChar c) { return c.isSpace(); }) ||
        value.contains('=') || value.contains('"')) {
        return invalid(field + " is one word with no blank, '=' or double quote: '" + value + "'");
    }
    return " " + key + "=" + value;
}

// GDAL's gridding methods: the leaves of `vector grid`, in its order, read at
// run time, so a method GDAL adds is offered with no change here - as the
// verb itself reads them.
QStringList gridMethods()
{
    QStringList methods;
    for (const gp::AlgorithmInfo& info : gp::catalogue()) {
        if (!info.container && info.path.size() == 3 && info.path[0] == "vector" &&
            info.path[1] == "grid") {
            methods << QString::fromStdString(info.path[2]);
        }
    }
    return methods;
}

bool methodTakes(const QString& method, const std::string& argument)
{
    const auto spec = gp::describe({"vector", "grid", method.toStdString()});
    return spec.ok() && std::ranges::any_of(spec->args, [&](const gp::ArgSpec& arg) {
               return arg.name == argument;
           });
}

bool isNumber(const katana::entity::PropertyValue& value)
{
    if (std::holds_alternative<double>(value) || std::holds_alternative<std::int64_t>(value)) {
        return true;
    }
    const auto* text = std::get_if<std::string>(&value);
    return text != nullptr &&
           katana::core::parseFiniteDouble(katana::core::trimmed(*text)).has_value();
}

} // namespace

Result<QString> gridDemCommandLine(const GridDemForm& form)
{
    if (!form.scopeError.isEmpty()) {
        return invalid("the points' scope: " + form.scopeError);
    }
    const QString scope = form.scope.trimmed();
    if (scope.isEmpty()) {
        return invalid("choose which points to grid first");
    }
    const QString z = form.z.trimmed();
    const bool zFromGeometry = z.isEmpty() || z.compare("geometry", Qt::CaseInsensitive) == 0;
    const Result<QString> parts[] = {
        wordOption("method", form.method, "the method"),
        numberOption("cell", form.cell, "the cell size"),
        wordOption("size", form.size, "the grid's size"),
        zFromGeometry ? Result<QString>(QString()) : wordOption("z", z, "the height property"),
        wordOption("extent", form.extent, "the extent"),
        numberOption("power", form.power, "the power"),
        numberOption("radius", form.radius, "the radius"),
    };
    QString line = "RASTER GRID " + scope;
    for (const Result<QString>& part : parts) {
        if (!part) {
            return part.error();
        }
        line += *part;
    }
    const QString size = form.size.trimmed();
    if (!size.isEmpty()) {
        const QStringList sides = size.split(QRegularExpression("[xX,]"));
        if (sides.size() != 2 || !readsAsNumber(sides[0]) || !readsAsNumber(sides[1])) {
            return invalid("the grid's size is columns x rows, two whole numbers: 400x300");
        }
    }
    const QString extent = form.extent.trimmed();
    if (!extent.isEmpty()) {
        const QStringList corners = extent.split(',');
        if (corners.size() != 4 ||
            !std::ranges::all_of(corners, [](const QString& c) { return readsAsNumber(c); })) {
            return invalid("the extent is four numbers, x0,y0,x1,y1");
        }
    }
    const QString name = form.name.trimmed();
    if (!name.isEmpty()) {
        const auto word = lineWord(name, "the name");
        if (!word) {
            return word.error();
        }
        line += " NAME " + *word;
    }
    if (form.toSurface) {
        const auto word = lineWord(name.isEmpty() ? QString("dem") : name, "the surface's name");
        if (!word) {
            return word.error();
        }
        line += " TO SURFACE " + *word;
    }
    return line;
}

GridDemDialog::GridDemDialog(GeoDialogContext context, QWidget* parent)
    : QDialog(parent), context_(std::move(context))
{
    setObjectName("gridDemDialog");
    setWindowTitle("Grid Points to DEM");
    // Non-modal: kept open beside the drawing, which a run changes.
    setModal(false);
    resize(1000, 820);

    const auto field = [this](const char* name, const QString& placeholder, const QString& tip) {
        auto* edit = new QLineEdit(this);
        edit->setObjectName(name);
        edit->setPlaceholderText(placeholder);
        edit->setToolTip(tip);
        return edit;
    };

    // The points: Global Modify's own scope and filter controls.
    scope_ = new ScopeFilterWidget("grid", this);
    scope_->views = context_.views;
    scope_->setChoice(ScopeChoice::Drawing);
    scope_->onChanged = [this] { refresh(); };

    method_ = new QComboBox(this);
    method_->setObjectName("gridMethod");
    method_->addItems(gridMethods());
    method_->setCurrentText("linear");
    method_->setToolTip("GDAL's gridding method (method=): linear interpolates over a "
                        "triangulation of the points and reproduces a plane exactly; invdist and "
                        "invdistnn weight by inverse distance; nearest takes the nearest point; "
                        "average, minimum, maximum, range and count summarise the points in a "
                        "search ellipse");
    cell_ = field("gridCell", "suggested for the extent",
                  "cell=: each cell's side, metres. The extent grows outwards to whole cells, so "
                  "every cell is this size and grids of one cell size line up");
    size_ = field("gridSize", "or columns x rows: 400x300",
                  "size=: the grid's columns and rows over the extent as it is, instead of a "
                  "cell");
    z_ = new QComboBox(this);
    z_->setObjectName("gridZ");
    z_->setEditable(true);
    z_->setToolTip("z=: where each point's height comes from - the geometry's own heights, or "
                   "a property holding a number. A point without one is left out and counted, "
                   "never read as 0");
    extent_ = field("gridExtent", "the scope's bounds",
                    "extent=x0,y0,x1,y1: the area gridded; blank for the bounds of the points "
                    "used, outwards to whole cells");
    power_ = field("gridPower", "GDAL's default",
                   "power=: the weighting power of the inverse-distance methods");
    radius_ = field("gridRadius", "GDAL's default",
                    "radius=: how far from a cell a point may be and count, metres");
    name_ = field("gridName", "dem",
                  "NAME: the reference raster's name; a name taken becomes <name>-2");
    toSurface_ = new QCheckBox("Keep as a named surface (TO SURFACE)", this);
    toSurface_->setObjectName("gridToSurface");
    // TO SURFACE is answered by the terrain session's surface store (its block
    // in the executor's bindings); until that is built the executor refuses
    // it, so it is not offered here either.
    toSurface_->setEnabled(false);
    toSurface_->setToolTip("A raster result kept as a surface arrives with the terrain session "
                           "(SURFACE verbs); the DEM is kept as a reference raster meanwhile");

    auto* options = new QGroupBox("Grid", this);
    options->setObjectName("gridOptions");
    auto* grid = new QFormLayout(options);
    grid->addRow("Method:", method_);
    auto* sizeRow = new QHBoxLayout;
    sizeRow->addWidget(cell_, 1);
    sizeRow->addWidget(new QLabel("or size:", options));
    sizeRow->addWidget(size_, 1);
    grid->addRow("Cell (m):", sizeRow);
    grid->addRow("Heights from:", z_);
    grid->addRow("Extent:", extent_);
    grid->addRow("Power:", power_);
    grid->addRow("Search radius (m):", radius_);
    grid->addRow("Name:", name_);
    grid->addRow(toSurface_);
    auto* note = new QLabel(
        "The points in the scope, and the vertices of its lines and areas, gridded by GDAL into a "
        "DEM kept as a reference raster (REFS lists it). Cells no point reaches hold no data, "
        "never 0.",
        options);
    note->setWordWrap(true);
    grid->addRow(note);

    panel_ = new GeoRunPanel("grid", context_, true, this);
    panel_->line = [this] { return gridDemCommandLine(form()); };

    auto* middle = new QHBoxLayout;
    middle->addWidget(scope_, 2);
    middle->addWidget(options, 3);
    auto* layout = new QVBoxLayout(this);
    layout->addLayout(middle);
    layout->addWidget(panel_, 1);

    for (QLineEdit* edit : {cell_, size_, extent_, power_, radius_, name_}) {
        connect(edit, &QLineEdit::textChanged, this, [this] { refresh(); });
    }
    connect(method_, &QComboBox::currentTextChanged, this, [this] { methodChosen(); });
    connect(z_, &QComboBox::currentTextChanged, this, [this] { refresh(); });
    connect(toSurface_, &QCheckBox::toggled, this, [this] { refresh(); });

    if (context_.document != nullptr) {
        // The layers and properties follow the drawing.
        watcher_ = std::make_unique<DocumentWatcher>(
            *context_.document, [this](const DocumentChanges& changes) {
                if (changes.model) {
                    reload();
                }
            });
    }
    methodChosen();
    reload();
}

GridDemDialog::~GridDemDialog() = default;

void GridDemDialog::showEvent(QShowEvent* event)
{
    // The views open now, not when the dialog was made.
    reload();
    QDialog::showEvent(event);
}

void GridDemDialog::reload()
{
    const bool alive = context_.document != nullptr && watcher_ != nullptr &&
                       watcher_->documentAlive();
    if (alive) {
        scope_->reload(context_.document->model());
    } else {
        scope_->reloadViews();
    }
    {
        // The drawing's properties that hold a number somewhere; the heights
        // of the geometry are "geometry", not their property.
        const QSignalBlocker quiet(z_);
        const QString kept = z_->currentText();
        std::set<std::string> keys;
        if (alive) {
            context_.document->model().entities.forEach([&keys](const katana::entity::Entity& entity) {
                for (const auto& [key, value] : entity.properties) {
                    if (key != katana::entity::kElevationProperty &&
                        key != katana::entity::kElevationsProperty && isNumber(value)) {
                        keys.insert(key);
                    }
                }
            });
        }
        z_->clear();
        z_->addItem("geometry");
        for (const std::string& key : keys) {
            z_->addItem(QString::fromStdString(key));
        }
        z_->setCurrentText(kept.isEmpty() ? QString("geometry") : kept);
    }
    refresh();
}

GridDemForm GridDemDialog::form() const
{
    GridDemForm form;
    if (const auto words = scope_->verbWords()) {
        form.scope = *words;
    } else {
        form.scopeError = QString::fromStdString(words.error().message);
    }
    form.method = method_->currentText();
    form.cell = cell_->text();
    form.size = size_->text();
    form.z = z_->currentText();
    form.extent = extent_->text();
    form.power = power_->isEnabled() ? power_->text() : QString();
    form.radius = radius_->isEnabled() ? radius_->text() : QString();
    form.name = name_->text();
    form.toSurface = toSurface_->isEnabled() && toSurface_->isChecked();
    return form;
}

void GridDemDialog::methodChosen()
{
    // Only what the method takes, from its own arguments.
    const QString method = method_->currentText();
    power_->setEnabled(methodTakes(method, "power"));
    radius_->setEnabled(methodTakes(method, "radius"));
    refresh();
}

void GridDemDialog::refresh()
{
    // A cell or a size, never both.
    size_->setEnabled(cell_->text().trimmed().isEmpty());
    cell_->setEnabled(size_->text().trimmed().isEmpty());
    panel_->refresh();
}

GridDemDialog& showGridDemDialog(GeoWorkbench& workbench, QWidget& window)
{
    auto& dialog = showKeptDialog<GridDemDialog>(window, "gridDemDialog", [&] {
        return new GridDemDialog(geoDialogContext(workbench), &window);
    });
    dialog.reload();
    return dialog;
}

} // namespace katana::qt
