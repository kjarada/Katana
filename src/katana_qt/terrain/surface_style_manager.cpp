// Terrain > Surface Styles... (terrain/surface_style_manager.hpp,
// docs/surface_display.md "In the window").

#include "terrain/surface_style_manager.hpp"

#include <QCheckBox>
#include <QComboBox>
#include <QDoubleSpinBox>
#include <QFormLayout>
#include <QHBoxLayout>
#include <QLabel>
#include <QLineEdit>
#include <QListWidget>
#include <QPainter>
#include <QPushButton>
#include <QSignalBlocker>
#include <QSpinBox>
#include <QStringList>
#include <QTabWidget>
#include <QVBoxLayout>
#include <QWidget>

#include <algorithm>
#include <cmath>
#include <memory>
#include <string>
#include <utility>
#include <vector>

#include "command_word.hpp"
#include "customisation/document_watcher.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/terrain/surface_colouring.hpp"
#include "katana/cad/terrain/surface_plan.hpp"
#include "katana/cad/view_transform.hpp"
#include "katana/core/text.hpp"
#include "katana/terrain/tin_surface.hpp"
#include "terrain/surface_plan_painter.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace cad = katana::cad;
using cad::SurfaceDisplayStyle;
using cad::SurfaceFaces;
using cad::SurfaceStyle;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::Color;

namespace {

QString qs(std::string_view text) { return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size())); }

// ---- the words of the SURFACESTYLE verbs --------------------------------------------------

constexpr std::array<SurfaceFaces, 5> kPlanFaces{SurfaceFaces::None, SurfaceFaces::Flat,
                                                 SurfaceFaces::Elevation, SurfaceFaces::Slope,
                                                 SurfaceFaces::Aspect};
// faces3d= takes no "none": a surface in 3D always has faces (or is hidden by
// its shading).
constexpr std::array<SurfaceFaces, 4> kFaces3d{SurfaceFaces::Flat, SurfaceFaces::Elevation,
                                               SurfaceFaces::Slope, SurfaceFaces::Aspect};
constexpr std::array<SurfaceStyle, 5> kShadings{SurfaceStyle::Automatic, SurfaceStyle::Shaded,
                                                SurfaceStyle::ShadedWithEdges, SurfaceStyle::Wireframe,
                                                SurfaceStyle::Hidden};

QString shadingWord(SurfaceStyle shading)
{
    switch (shading) {
    case SurfaceStyle::Automatic:
        return "automatic";
    case SurfaceStyle::Shaded:
        return "shaded";
    case SurfaceStyle::ShadedWithEdges:
        return "edges";
    case SurfaceStyle::Wireframe:
        return "wireframe";
    case SurfaceStyle::Hidden:
        return "hidden";
    }
    return "automatic";
}

QString colourWord(const Color& colour) { return qs(colour.toHex()); }

QString onOff(bool on) { return on ? QStringLiteral("on") : QStringLiteral("off"); }

// "v:#RRGGBB;v:#RRGGBB", the stops= value.
QString stopsWord(const std::vector<cad::ColourStop>& stops)
{
    QStringList parts;
    for (const cad::ColourStop& stop : stops) {
        parts << exactNumber(stop.value) + ":" + colourWord(stop.colour);
    }
    return parts.join(';');
}

Result<std::vector<cad::ColourStop>> readStops(const QString& text)
{
    std::vector<cad::ColourStop> stops;
    const QString trimmed = text.trimmed();
    if (trimmed.isEmpty()) {
        return stops;
    }
    for (const QString& part : trimmed.split(';', Qt::SkipEmptyParts)) {
        const qsizetype colon = part.indexOf(':');
        const auto value = colon < 0 ? std::optional<double>{}
                                     : katana::core::parseFiniteDouble(part.left(colon).trimmed().toStdString());
        auto colour = colon < 0 ? Result<Color>(makeError(ErrorCode::ParseFailure, "no colour"))
                                : Color::fromHex(part.mid(colon + 1).trimmed().toStdString());
        if (!value || !colour) {
            return makeError(ErrorCode::ParseFailure,
                             "a stop is value:#RRGGBB, stops apart by ';'", part.toStdString());
        }
        stops.push_back({*value, *colour});
    }
    return stops;
}

// ---- the sample the preview draws -----------------------------------------------------------

// A 120 x 80 m piece of ground, 24 x 16 cells: a hill, a hollow and a gentle
// fall across it, so every kind of face shows - bands of height, steep and
// flat slopes, every aspect - with a breakline across its middle row so a
// style's breaklines show too.
std::shared_ptr<const katana::terrain::TinSurface> previewSurface()
{
    static const std::shared_ptr<const katana::terrain::TinSurface> sample = [] {
        constexpr int kColumns = 24;
        constexpr int kRows = 16;
        constexpr double kCell = 5.0;
        constexpr int kBreakRow = 8;
        std::vector<katana::geometry::Point3> vertices;
        for (int j = 0; j <= kRows; ++j) {
            for (int i = 0; i <= kColumns; ++i) {
                const double x = i * kCell;
                const double y = j * kCell;
                const double hill = 6.0 * std::exp(-((x - 40.0) * (x - 40.0) + (y - 48.0) * (y - 48.0)) / 400.0);
                const double hollow = 4.0 * std::exp(-((x - 92.0) * (x - 92.0) + (y - 24.0) * (y - 24.0)) / 300.0);
                vertices.emplace_back(x, y, 20.0 + hill - hollow + 0.04 * x);
            }
        }
        const auto at = [](int i, int j) { return static_cast<std::uint32_t>(j * (kColumns + 1) + i); };
        std::vector<katana::terrain::TinTriangle> triangles;
        std::vector<std::uint8_t> constrained;
        for (int j = 0; j < kRows; ++j) {
            for (int i = 0; i < kColumns; ++i) {
                // (i,j) (i+1,j) (i+1,j+1), then (i,j) (i+1,j+1) (i,j+1): the
                // first's edge 0 runs along row j, the second's edge 1 along
                // row j + 1 - the breakline's two sides.
                triangles.push_back({at(i, j), at(i + 1, j), at(i + 1, j + 1)});
                constrained.push_back(j == kBreakRow ? 0b001 : 0);
                triangles.push_back({at(i, j), at(i + 1, j + 1), at(i, j + 1)});
                constrained.push_back(j + 1 == kBreakRow ? 0b010 : 0);
            }
        }
        auto made = katana::terrain::TinSurface::create(std::move(vertices), std::move(triangles),
                                                        std::move(constrained));
        return made ? std::make_shared<const katana::terrain::TinSurface>(std::move(*made))
                    : std::make_shared<const katana::terrain::TinSurface>();
    }();
    return sample;
}

// The form's style drawn on the sample, as the plan view would draw it,
// with its legend.
class SurfaceStylePreview final : public QWidget {
  public:
    explicit SurfaceStylePreview(QWidget* parent) : QWidget(parent)
    {
        setObjectName("surfaceStylePreview");
        setMinimumSize(320, 220);
        setToolTip("A sample surface drawn in the style as the form says it, as the plan view draws it");
    }

    void show(std::optional<SurfaceDisplayStyle> style)
    {
        style_ = std::move(style);
        update();
    }

    // What the last paint drew of the sample, for the tests.
    [[nodiscard]] const cad::SurfacePlanStats& lastStats() const { return stats_; }

  protected:
    void paintEvent(QPaintEvent*) override
    {
        QPainter painter(this);
        painter.fillRect(rect(), theme::viewport());
        stats_ = {};
        const auto sample = previewSurface();
        if (!style_.has_value() || sample->empty()) {
            painter.setPen(theme::textMuted());
            painter.drawText(rect(), Qt::AlignCenter, "Choose a style to see it here");
            return;
        }
        cad::ViewTransform view;
        view.resize(width(), height());
        view.fit(sample->bounds(), 0.06);
        const katana::geometry::Box2 visible = view.visibleWorldBounds();
        cad::SurfacePlanView plan;
        plan.visible = visible;
        plan.unitsPerPixel = 1.0 / view.scale;
        const double ratio = std::max(devicePixelRatioF(), 1.0);
        const cad::SurfacePlanImageGrid grid =
            cad::planImageGrid(visible, sample->bounds(), 1.0 / (view.scale * ratio), 4'000'000);
        plan.imageBox = grid.box;
        plan.imageWidth = grid.width;
        plan.imageHeight = grid.height;
        // A label every 160 px: the sample is small, and the plan view's
        // spacing would give it one label at most.
        plan.labelSpacingPixels = 160.0;
        const katana::terrain::NamedSurface named{"sample", sample, "preview"};
        const cad::SurfacePlan drawn = cad::surfacePlan(named, *style_, plan, cache_);
        stats_ = drawn.stats;
        painter.setRenderHint(QPainter::Antialiasing, true);
        SurfacePaintOptions options;
        options.deviceRatio = ratio;
        options.halo = theme::viewport();
        paintSurfacePlan(painter, drawn, view, options);
        if (drawn.legend.has_value()) {
            QFont font = painter.font();
            font.setPixelSize(10);
            QColor backing = theme::viewport();
            backing.setAlpha(215);
            (void)paintSurfaceLegend(painter, *drawn.legend, qs(style_->name),
                                     QPointF(width() - 6.0, 6.0), font, theme::text(), backing);
        }
    }

  private:
    std::optional<SurfaceDisplayStyle> style_;
    cad::SurfacePlanCache cache_;
    cad::SurfacePlanStats stats_{};
};

// A colour field: #RRGGBB as typed, read through Color::fromHex.
Result<Color> readColour(const QLineEdit& field, const QString& what)
{
    auto colour = Color::fromHex(field.text().trimmed().toStdString());
    if (!colour) {
        return makeError(ErrorCode::ParseFailure, what.toStdString() + " is #RRGGBB",
                         field.text().toStdString());
    }
    return *colour;
}

Result<double> readReal(const QLineEdit& field, const QString& what)
{
    const auto value = katana::core::parseFiniteDouble(field.text().trimmed().toStdString());
    if (!value) {
        return makeError(ErrorCode::ParseFailure, what.toStdString() + " is a number",
                         field.text().toStdString());
    }
    return *value;
}

Result<std::optional<double>> readOptionalReal(const QLineEdit& field, const QString& what)
{
    if (field.text().trimmed().isEmpty()) {
        return std::optional<double>{};
    }
    auto value = readReal(field, what);
    if (!value) {
        return value.error();
    }
    return std::optional<double>{*value};
}

} // namespace

// ---- the words of an edit -----------------------------------------------------------------

Result<QString> surfaceStyleSetWords(const SurfaceDisplayStyle& from, const SurfaceDisplayStyle& to)
{
    QStringList words;
    const auto add = [&words](const QString& key, const QString& value) { words << key + "=" + value; };
    if (from.description != to.description) {
        auto word = commandWord(qs(to.description), "the description");
        if (!word) {
            return word.error();
        }
        add("description", *word);
    }
    if (from.showTriangles != to.showTriangles) {
        add("triangles", onOff(to.showTriangles));
    }
    if (from.triangleColour != to.triangleColour) {
        add("trianglecolour", colourWord(to.triangleColour));
    }
    if (from.showBoundary != to.showBoundary) {
        add("boundary", onOff(to.showBoundary));
    }
    if (from.boundaryColour != to.boundaryColour) {
        add("boundarycolour", colourWord(to.boundaryColour));
    }
    if (from.showPoints != to.showPoints) {
        add("points", onOff(to.showPoints));
    }
    if (from.pointColour != to.pointColour) {
        add("pointcolour", colourWord(to.pointColour));
    }
    if (from.showBreaklines != to.showBreaklines) {
        add("breaklines", onOff(to.showBreaklines));
    }
    if (from.breaklineColour != to.breaklineColour) {
        add("breaklinecolour", colourWord(to.breaklineColour));
    }
    if (from.planFaces != to.planFaces) {
        add("faces", qs(cad::surfaceFacesWord(to.planFaces)));
    }
    if (from.planOpacity != to.planOpacity) {
        add("opacity", exactNumber(to.planOpacity));
    }
    if (from.contours.show != to.contours.show) {
        add("contours", onOff(to.contours.show));
    }
    if (from.contours.interval != to.contours.interval) {
        add("interval", exactNumber(to.contours.interval));
    }
    if (from.contours.majorEvery != to.contours.majorEvery) {
        add("major", QString::number(to.contours.majorEvery));
    }
    if (from.contours.base != to.contours.base) {
        add("base", exactNumber(to.contours.base));
    }
    if (from.contours.minorColour != to.contours.minorColour) {
        add("minorcolour", colourWord(to.contours.minorColour));
    }
    if (from.contours.majorColour != to.contours.majorColour) {
        add("majorcolour", colourWord(to.contours.majorColour));
    }
    if (from.contours.labelMajor != to.contours.labelMajor) {
        add("labels", onOff(to.contours.labelMajor));
    }
    if (from.ramp != to.ramp) {
        auto word = commandWord(qs(to.ramp), "the ramp");
        if (!word) {
            return word.error();
        }
        add("ramp", *word);
    }
    if (from.banded != to.banded) {
        add("banded", onOff(to.banded));
    }
    if (from.bands != to.bands) {
        add("bands", QString::number(to.bands));
    }
    if (from.stops != to.stops) {
        add("stops", stopsWord(to.stops));
    }
    if (from.rangeMin != to.rangeMin) {
        add("min", to.rangeMin.has_value() ? exactNumber(*to.rangeMin) : QString());
    }
    if (from.rangeMax != to.rangeMax) {
        add("max", to.rangeMax.has_value() ? exactNumber(*to.rangeMax) : QString());
    }
    if (from.flatColour != to.flatColour) {
        add("flatcolour", colourWord(to.flatColour));
    }
    if (from.shading != to.shading) {
        add("shading", shadingWord(to.shading));
    }
    if (from.faces3d != to.faces3d) {
        add("faces3d", qs(cad::surfaceFacesWord(to.faces3d)));
    }
    return words.join(' ');
}

// ---- the dialog ----------------------------------------------------------------------------

struct SurfaceStyleManagerDialog::Impl {
    SurfaceStyleManagerDialog* dialog = nullptr;
    SurfaceStyleManagerContext context;
    std::unique_ptr<QObject> guard = std::make_unique<QObject>();
    std::unique_ptr<DocumentWatcher> watcher;

    QListWidget* list = nullptr;
    QLabel* defaultNote = nullptr;
    QLineEdit* name = nullptr;
    QPushButton* newStyle = nullptr;
    QPushButton* copy = nullptr;
    QPushButton* rename = nullptr;
    QPushButton* remove = nullptr;
    QPushButton* setDefault = nullptr;
    QTabWidget* tabs = nullptr;
    // Plan
    QLineEdit* description = nullptr;
    QCheckBox* triangles = nullptr;
    QLineEdit* triangleColour = nullptr;
    QCheckBox* boundary = nullptr;
    QLineEdit* boundaryColour = nullptr;
    QCheckBox* points = nullptr;
    QLineEdit* pointColour = nullptr;
    QCheckBox* breaklines = nullptr;
    QLineEdit* breaklineColour = nullptr;
    QComboBox* faces = nullptr;
    QDoubleSpinBox* opacity = nullptr;
    // Colours
    QComboBox* ramp = nullptr;
    QCheckBox* banded = nullptr;
    QSpinBox* bands = nullptr;
    QLineEdit* stops = nullptr;
    QLineEdit* minimum = nullptr;
    QLineEdit* maximum = nullptr;
    QLineEdit* flatColour = nullptr;
    // Contours
    QCheckBox* contours = nullptr;
    QLineEdit* interval = nullptr;
    QSpinBox* major = nullptr;
    QLineEdit* base = nullptr;
    QLineEdit* minorColour = nullptr;
    QLineEdit* majorColour = nullptr;
    QCheckBox* labels = nullptr;
    // 3D
    QComboBox* shading = nullptr;
    QComboBox* faces3d = nullptr;

    SurfaceStylePreview* preview = nullptr;
    QLabel* line = nullptr;
    QPushButton* apply = nullptr;
    QPushButton* revert = nullptr;
    QComboBox* surface = nullptr;
    QPushButton* assign = nullptr;
    QLabel* status = nullptr;

    // The drawing's style the form was filled from: what Apply changes, and
    // what a reload compares with to keep unapplied edits.
    std::optional<SurfaceDisplayStyle> loaded;
    // Filling the form: no field's change is an edit.
    bool filling = false;
    // What the opacity box showed when filled, rounded to its decimals.
    double shownOpacity = 0.0;
    // The name to choose at the next reload (a style just made or renamed).
    QString pending;

    [[nodiscard]] bool alive() const { return watcher != nullptr && watcher->documentAlive(); }
    [[nodiscard]] katana::cad::Document& document() const { return *context.document; }

    template <typename Sender, typename Signal, typename Slot>
    void connect(Sender* sender, Signal signal, Slot slot)
    {
        QObject::connect(sender, signal, guard.get(), std::move(slot));
    }

    void setStatus(const QString& text, bool isError)
    {
        status->setStyleSheet(
            QString("color: %1").arg((isError ? theme::error() : theme::textMuted()).name()));
        status->setText(text);
    }

    [[nodiscard]] const SurfaceDisplayStyle* chosen() const
    {
        if (!alive() || list->currentItem() == nullptr) {
            return nullptr;
        }
        return document().surfaceStyles().find(list->currentItem()->text().toStdString());
    }

    QLineEdit* colourField(const char* objectName, const QString& tip)
    {
        auto* field = new QLineEdit(dialog);
        field->setObjectName(objectName);
        field->setPlaceholderText("#RRGGBB");
        field->setToolTip(tip);
        field->setMaximumWidth(110);
        return field;
    }

    QCheckBox* checkBox(const char* objectName, const QString& text)
    {
        auto* box = new QCheckBox(text, dialog);
        box->setObjectName(objectName);
        return box;
    }

    QPushButton* button(const char* objectName, const QString& text, const QString& tip)
    {
        auto* pushButton = new QPushButton(text, dialog);
        pushButton->setObjectName(objectName);
        pushButton->setToolTip(tip);
        pushButton->setAutoDefault(false);
        return pushButton;
    }

    QWidget* row(QWidget* first, QWidget* second)
    {
        auto* holder = new QWidget(dialog);
        auto* layout = new QHBoxLayout(holder);
        layout->setContentsMargins(0, 0, 0, 0);
        layout->addWidget(first);
        layout->addWidget(second);
        layout->addStretch(1);
        return holder;
    }

    void build()
    {
        auto* outer = new QVBoxLayout(dialog);
        auto* columns = new QHBoxLayout;
        outer->addLayout(columns, 1);

        // ---- the styles
        auto* left = new QVBoxLayout;
        list = new QListWidget(dialog);
        list->setObjectName("surfaceStyleList");
        list->setToolTip("The drawing's surface display styles");
        left->addWidget(list, 1);
        defaultNote = new QLabel(dialog);
        defaultNote->setObjectName("surfaceStyleDefaultNote");
        left->addWidget(defaultNote);
        name = new QLineEdit(dialog);
        name->setObjectName("surfaceStyleName");
        name->setPlaceholderText("Name for New, Copy or Rename");
        left->addWidget(name);
        auto* names = new QHBoxLayout;
        newStyle = button("surfaceStyleNew", "New", "A new style of that name, with the built-in defaults");
        copy = button("surfaceStyleCopy", "Copy", "A new style of that name, a copy of the chosen one");
        rename = button("surfaceStyleRename", "Rename", "The chosen style renamed; surfaces drawn in it follow");
        names->addWidget(newStyle);
        names->addWidget(copy);
        names->addWidget(rename);
        left->addLayout(names);
        auto* more = new QHBoxLayout;
        remove = button("surfaceStyleDelete", "Delete", "Delete the chosen style (refused while a surface uses it)");
        setDefault = button("surfaceStyleSetDefault", "Set Default",
                            "Draw every surface with no style of its own in the chosen style");
        more->addWidget(remove);
        more->addWidget(setDefault);
        left->addLayout(more);
        columns->addLayout(left, 1);

        // ---- its fields
        tabs = new QTabWidget(dialog);
        tabs->setObjectName("surfaceStyleTabs");
        auto* planPage = new QWidget(tabs);
        auto* plan = new QFormLayout(planPage);
        description = new QLineEdit(planPage);
        description->setObjectName("surfaceStyleDescription");
        plan->addRow("Description", description);
        triangles = checkBox("surfaceStyleTriangles", "Triangles");
        triangleColour = colourField("surfaceStyleTriangleColour", "The triangles' colour");
        plan->addRow(row(triangles, triangleColour));
        boundary = checkBox("surfaceStyleBoundary", "Boundary");
        boundaryColour = colourField("surfaceStyleBoundaryColour", "The boundary's colour");
        plan->addRow(row(boundary, boundaryColour));
        breaklines = checkBox("surfaceStyleBreaklines", "Breaklines");
        breaklineColour = colourField("surfaceStyleBreaklineColour", "The breaklines' colour");
        plan->addRow(row(breaklines, breaklineColour));
        points = checkBox("surfaceStylePoints", "Points");
        pointColour = colourField("surfaceStylePointColour", "The points' colour");
        plan->addRow(row(points, pointColour));
        faces = new QComboBox(planPage);
        faces->setObjectName("surfaceStyleFaces");
        for (const SurfaceFaces kind : kPlanFaces) {
            faces->addItem(qs(cad::surfaceFacesWord(kind)));
        }
        faces->setToolTip("What colours the faces in plan: nothing, one colour, the elevation, the "
                          "slope or the aspect");
        plan->addRow("Faces", faces);
        opacity = new QDoubleSpinBox(planPage);
        opacity->setObjectName("surfaceStyleOpacity");
        opacity->setRange(0.0, 1.0);
        opacity->setSingleStep(0.05);
        opacity->setDecimals(2);
        opacity->setToolTip("How much the faces cover what is beneath them, 0 to 1");
        plan->addRow("Opacity", opacity);
        tabs->addTab(planPage, "Plan");

        auto* colourPage = new QWidget(tabs);
        auto* colours = new QFormLayout(colourPage);
        ramp = new QComboBox(colourPage);
        ramp->setObjectName("surfaceStyleRamp");
        for (const std::string_view rampName : cad::surfaceRampNames()) {
            ramp->addItem(qs(rampName));
        }
        colours->addRow("Ramp", ramp);
        banded = checkBox("surfaceStyleBanded", "Banded");
        bands = new QSpinBox(colourPage);
        bands->setObjectName("surfaceStyleBands");
        bands->setRange(2, 100);
        bands->setToolTip("How many bands of equal width the range is cut into");
        colours->addRow(row(banded, bands));
        stops = new QLineEdit(colourPage);
        stops->setObjectName("surfaceStyleStops");
        stops->setPlaceholderText("value:#RRGGBB; value:#RRGGBB ... (empty: the ramp)");
        stops->setToolTip("Colours at values, in order; they replace the ramp. Banded, each is the "
                          "lower edge of a band");
        colours->addRow("Stops", stops);
        minimum = new QLineEdit(colourPage);
        minimum->setObjectName("surfaceStyleMin");
        minimum->setPlaceholderText("the surface's own");
        maximum = new QLineEdit(colourPage);
        maximum->setObjectName("surfaceStyleMax");
        maximum->setPlaceholderText("the surface's own");
        colours->addRow("Range from", minimum);
        colours->addRow("Range to", maximum);
        flatColour = colourField("surfaceStyleFlatColour", "The colour of flat faces, and of a level face by aspect");
        colours->addRow("Flat colour", flatColour);
        tabs->addTab(colourPage, "Colours");

        auto* contourPage = new QWidget(tabs);
        auto* contourForm = new QFormLayout(contourPage);
        contours = checkBox("surfaceStyleContours", "Contours");
        contourForm->addRow(contours);
        interval = new QLineEdit(contourPage);
        interval->setObjectName("surfaceStyleInterval");
        contourForm->addRow("Interval", interval);
        major = new QSpinBox(contourPage);
        major->setObjectName("surfaceStyleMajor");
        major->setRange(0, 100);
        major->setToolTip("Every Nth contour is a major; 0 for none");
        contourForm->addRow("Major every", major);
        base = new QLineEdit(contourPage);
        base->setObjectName("surfaceStyleBase");
        contourForm->addRow("Base level", base);
        minorColour = colourField("surfaceStyleMinorColour", "The minor contours' colour");
        majorColour = colourField("surfaceStyleMajorColour", "The major contours' colour");
        contourForm->addRow("Minor colour", minorColour);
        contourForm->addRow("Major colour", majorColour);
        labels = checkBox("surfaceStyleLabels", "Label the majors");
        contourForm->addRow(labels);
        tabs->addTab(contourPage, "Contours");

        auto* threePage = new QWidget(tabs);
        auto* three = new QFormLayout(threePage);
        shading = new QComboBox(threePage);
        shading->setObjectName("surfaceStyleShading");
        for (const SurfaceStyle kind : kShadings) {
            shading->addItem(shadingWord(kind));
        }
        shading->setToolTip("How the 3D view shades it: automatic draws edges up to 50 000 triangles");
        three->addRow("Shading", shading);
        faces3d = new QComboBox(threePage);
        faces3d->setObjectName("surfaceStyleFaces3d");
        for (const SurfaceFaces kind : kFaces3d) {
            faces3d->addItem(qs(cad::surfaceFacesWord(kind)));
        }
        three->addRow("Faces", faces3d);
        tabs->addTab(threePage, "3D");

        auto* middle = new QVBoxLayout;
        middle->addWidget(tabs);
        line = new QLabel(dialog);
        line->setObjectName("surfaceStyleLine");
        line->setWordWrap(true);
        line->setTextFormat(Qt::PlainText);
        line->setTextInteractionFlags(Qt::TextSelectableByMouse);
        middle->addWidget(line);
        auto* applying = new QHBoxLayout;
        apply = button("surfaceStyleApply", "Apply", "Run the SURFACESTYLE SET line: one undo step");
        revert = button("surfaceStyleRevert", "Revert", "The form back to the drawing's style");
        applying->addWidget(apply);
        applying->addWidget(revert);
        applying->addStretch(1);
        middle->addLayout(applying);
        columns->addLayout(middle, 2);

        preview = new SurfaceStylePreview(dialog);
        columns->addWidget(preview, 2);

        // ---- a surface's style
        auto* assigning = new QHBoxLayout;
        assigning->addWidget(new QLabel("Draw surface", dialog));
        surface = new QComboBox(dialog);
        surface->setObjectName("surfaceStyleSurface");
        surface->setToolTip("The drawing's surfaces");
        assigning->addWidget(surface, 1);
        assign = button("surfaceStyleAssign", "In This Style", "Run SURFACE STYLE: the surface drawn in the chosen style");
        assigning->addWidget(assign);
        outer->addLayout(assigning);

        auto* foot = new QHBoxLayout;
        status = new QLabel(dialog);
        status->setObjectName("surfaceStyleStatus");
        status->setWordWrap(true);
        status->setTextFormat(Qt::PlainText);
        foot->addWidget(status, 1);
        auto* close = button("surfaceStyleClose", "Close", "Close the window; unapplied edits are kept");
        foot->addWidget(close);
        outer->addLayout(foot);

        // ---- what each does
        connect(list, &QListWidget::currentRowChanged, [this](int) { chooseStyle(); });
        connect(name, &QLineEdit::textChanged, [this](const QString&) { enable(); });
        connect(newStyle, &QPushButton::clicked, [this] { makeStyle(false); });
        connect(copy, &QPushButton::clicked, [this] { makeStyle(true); });
        connect(rename, &QPushButton::clicked, [this] { renameStyle(); });
        connect(remove, &QPushButton::clicked, [this] { deleteStyle(); });
        connect(setDefault, &QPushButton::clicked, [this] { defaultStyle(); });
        connect(apply, &QPushButton::clicked, [this] { applyForm(); });
        connect(revert, &QPushButton::clicked, [this] {
            if (const SurfaceDisplayStyle* style = chosen()) {
                fill(*style);
            }
        });
        connect(assign, &QPushButton::clicked, [this] { assignStyle(); });
        connect(surface, &QComboBox::currentIndexChanged, [this](int) { enable(); });
        connect(close, &QPushButton::clicked, [this] { dialog->close(); });
        for (QLineEdit* field : {description, triangleColour, boundaryColour, pointColour, breaklineColour,
                                 stops, minimum, maximum, flatColour, interval, base, minorColour, majorColour}) {
            connect(field, &QLineEdit::textChanged, [this](const QString&) { edited(); });
        }
        for (QCheckBox* box : {triangles, boundary, points, breaklines, banded, contours, labels}) {
            connect(box, &QCheckBox::toggled, [this](bool) { edited(); });
        }
        for (QComboBox* box : {faces, ramp, shading, faces3d}) {
            connect(box, &QComboBox::currentIndexChanged, [this](int) { edited(); });
        }
        connect(opacity, &QDoubleSpinBox::valueChanged, [this](double) { edited(); });
        connect(bands, &QSpinBox::valueChanged, [this](int) { edited(); });
        connect(major, &QSpinBox::valueChanged, [this](int) { edited(); });
    }

    // The list and the surfaces from the drawing, keeping what was chosen.
    void reload()
    {
        if (!alive()) {
            return;
        }
        const cad::SurfaceStyleSet& styles = document().surfaceStyles();
        const QString wanted = !pending.isEmpty()
                                   ? pending
                                   : (list->currentItem() != nullptr ? list->currentItem()->text()
                                                                     : qs(styles.defaultStyle));
        pending.clear();
        {
            const QSignalBlocker block(list);
            list->clear();
            for (const SurfaceDisplayStyle& style : styles.styles) {
                auto* item = new QListWidgetItem(qs(style.name), list);
                item->setToolTip(qs(style.description));
                if (katana::core::equalsIgnoringCase(style.name, styles.defaultStyle)) {
                    QFont bold = item->font();
                    bold.setBold(true);
                    item->setFont(bold);
                }
            }
            int row = 0;
            for (int i = 0; i < list->count(); ++i) {
                if (list->item(i)->text().compare(wanted, Qt::CaseInsensitive) == 0) {
                    row = i;
                    break;
                }
            }
            if (list->count() > 0) {
                list->setCurrentRow(row);
            }
        }
        defaultNote->setText("Default: " + qs(styles.defaultStyle) +
                             " - a surface with no style of its own is drawn in it");
        {
            const QSignalBlocker block(surface);
            const QString was = surface->currentText();
            surface->clear();
            for (const katana::terrain::NamedSurface& named : document().surfaces().all()) {
                surface->addItem(qs(named.name));
            }
            const int at = surface->findText(was);
            surface->setCurrentIndex(at >= 0 ? at : (surface->count() > 0 ? 0 : -1));
        }
        // Keep unapplied edits while the style they were made to is as it
        // was; else the form shows the drawing's style.
        const SurfaceDisplayStyle* style = chosen();
        if (style == nullptr) {
            loaded.reset();
            preview->show(std::nullopt);
        } else if (!loaded.has_value() || !(*loaded == *style)) {
            fill(*style);
        }
        enable();
    }

    void chooseStyle()
    {
        if (const SurfaceDisplayStyle* style = chosen()) {
            fill(*style);
        } else {
            loaded.reset();
            preview->show(std::nullopt);
        }
        enable();
    }

    static void choose(QComboBox* box, const QString& text)
    {
        const int at = box->findText(text, Qt::MatchFixedString);
        box->setCurrentIndex(at >= 0 ? at : 0);
    }

    void fill(const SurfaceDisplayStyle& style)
    {
        filling = true;
        description->setText(qs(style.description));
        triangles->setChecked(style.showTriangles);
        triangleColour->setText(colourWord(style.triangleColour));
        boundary->setChecked(style.showBoundary);
        boundaryColour->setText(colourWord(style.boundaryColour));
        points->setChecked(style.showPoints);
        pointColour->setText(colourWord(style.pointColour));
        breaklines->setChecked(style.showBreaklines);
        breaklineColour->setText(colourWord(style.breaklineColour));
        choose(faces, qs(cad::surfaceFacesWord(style.planFaces)));
        opacity->setValue(style.planOpacity);
        shownOpacity = opacity->value();
        choose(ramp, qs(style.ramp));
        banded->setChecked(style.banded);
        bands->setValue(style.bands);
        stops->setText(stopsWord(style.stops));
        minimum->setText(style.rangeMin.has_value() ? exactNumber(*style.rangeMin) : QString());
        maximum->setText(style.rangeMax.has_value() ? exactNumber(*style.rangeMax) : QString());
        flatColour->setText(colourWord(style.flatColour));
        contours->setChecked(style.contours.show);
        interval->setText(exactNumber(style.contours.interval));
        major->setValue(style.contours.majorEvery);
        base->setText(exactNumber(style.contours.base));
        minorColour->setText(colourWord(style.contours.minorColour));
        majorColour->setText(colourWord(style.contours.majorColour));
        labels->setChecked(style.contours.labelMajor);
        choose(shading, shadingWord(style.shading));
        choose(faces3d, qs(cad::surfaceFacesWord(style.faces3d == SurfaceFaces::None ? SurfaceFaces::Flat
                                                                                     : style.faces3d)));
        loaded = style;
        filling = false;
        edited();
    }

    // The form as a style, or why it is not one.
    [[nodiscard]] Result<SurfaceDisplayStyle> read() const
    {
        if (!loaded.has_value()) {
            return makeError(ErrorCode::InvalidState, "choose a style");
        }
        SurfaceDisplayStyle style = *loaded;
        style.description = description->text().toStdString();
        style.showTriangles = triangles->isChecked();
        style.showBoundary = boundary->isChecked();
        style.showPoints = points->isChecked();
        style.showBreaklines = breaklines->isChecked();
        style.planFaces = kPlanFaces[static_cast<std::size_t>(std::max(faces->currentIndex(), 0))];
        // Exactly what the drawing holds unless the spin box was moved: it
        // shows two decimals, and read back as it shows it, 0.625 would be
        // written over as 0.63. Moved is: not what filling it showed.
        if (opacity->value() != shownOpacity) {
            style.planOpacity = opacity->value();
        }
        style.ramp = ramp->currentText().toStdString();
        style.banded = banded->isChecked();
        style.bands = bands->value();
        style.contours.show = contours->isChecked();
        style.contours.majorEvery = major->value();
        style.contours.labelMajor = labels->isChecked();
        style.shading = kShadings[static_cast<std::size_t>(std::max(shading->currentIndex(), 0))];
        style.faces3d = kFaces3d[static_cast<std::size_t>(std::max(faces3d->currentIndex(), 0))];
        const std::pair<QLineEdit*, Color*> colours[] = {
            {triangleColour, &style.triangleColour},   {boundaryColour, &style.boundaryColour},
            {pointColour, &style.pointColour},         {breaklineColour, &style.breaklineColour},
            {flatColour, &style.flatColour},           {minorColour, &style.contours.minorColour},
            {majorColour, &style.contours.majorColour}};
        for (const auto& [field, colour] : colours) {
            auto read = readColour(*field, field->toolTip());
            if (!read) {
                return read.error();
            }
            *colour = *read;
        }
        auto readInterval = readReal(*interval, "the contour interval");
        if (!readInterval) {
            return readInterval.error();
        }
        style.contours.interval = *readInterval;
        auto readBase = readReal(*base, "the base level");
        if (!readBase) {
            return readBase.error();
        }
        style.contours.base = *readBase;
        auto low = readOptionalReal(*minimum, "the range's start");
        if (!low) {
            return low.error();
        }
        style.rangeMin = *low;
        auto high = readOptionalReal(*maximum, "the range's end");
        if (!high) {
            return high.error();
        }
        style.rangeMax = *high;
        auto readStopsField = readStops(stops->text());
        if (!readStopsField) {
            return readStopsField.error();
        }
        style.stops = *readStopsField;
        return style;
    }

    // A field changed: the preview and the line follow the form.
    void edited()
    {
        if (filling) {
            return;
        }
        auto style = read();
        preview->show(style ? std::optional<SurfaceDisplayStyle>(*style) : loaded);
        line->setText(lineText());
        enable();
    }

    [[nodiscard]] QString applyLine() const
    {
        auto style = read();
        if (!style || !loaded.has_value()) {
            return {};
        }
        auto words = surfaceStyleSetWords(*loaded, *style);
        auto styleWord = commandWord(qs(loaded->name), "the style's name");
        if (!words || !styleWord || words->isEmpty()) {
            return {};
        }
        return "SURFACESTYLE SET " + *styleWord + " " + *words;
    }

    [[nodiscard]] QString lineText() const
    {
        auto style = read();
        if (!style) {
            return "Cannot apply: " + qs(style.error().describe());
        }
        auto words = surfaceStyleSetWords(*loaded, *style);
        if (!words) {
            return "Cannot apply: " + qs(words.error().describe());
        }
        if (words->isEmpty()) {
            return "The form says what the drawing holds: nothing to apply.";
        }
        return applyLine();
    }

    void enable()
    {
        const bool styleChosen = chosen() != nullptr;
        const bool named = !name->text().trimmed().isEmpty();
        newStyle->setEnabled(named);
        copy->setEnabled(named && styleChosen);
        rename->setEnabled(named && styleChosen);
        remove->setEnabled(styleChosen);
        setDefault->setEnabled(styleChosen);
        apply->setEnabled(!applyLine().isEmpty());
        revert->setEnabled(styleChosen);
        assign->setEnabled(styleChosen && surface->currentIndex() >= 0);
    }

    bool run(const QString& text)
    {
        if (!alive()) {
            setStatus("The drawing this dialog edited is gone.", true);
            return false;
        }
        if (!context.run) {
            setStatus("Nothing here can run " + text + ": the dialog has no command line.", true);
            return false;
        }
        const VerbOutcome outcome = context.run(text);
        if (outcome.ok) {
            setStatus(outcome.reply.isEmpty() ? text : outcome.reply, false);
        } else {
            setStatus(outcome.error.isEmpty() ? "Refused: " + text : outcome.error, true);
        }
        reload();
        return outcome.ok;
    }

    // A word for a name, or the status says why there is none.
    std::optional<QString> word(const QString& text, const QString& what)
    {
        auto result = commandWord(text, what);
        if (!result) {
            setStatus(qs(result.error().describe()), true);
            return std::nullopt;
        }
        return *result;
    }

    void makeStyle(bool fromChosen)
    {
        const QString newName = name->text().trimmed();
        const auto newWord = word(newName, "the new style's name");
        if (!newWord) {
            return;
        }
        QString text = "SURFACESTYLE NEW " + *newWord;
        if (fromChosen) {
            const SurfaceDisplayStyle* style = chosen();
            const auto fromWord = style != nullptr ? word(qs(style->name), "the style's name") : std::nullopt;
            if (!fromWord) {
                return;
            }
            text += " FROM " + *fromWord;
        }
        pending = newName;
        if (!run(text)) {
            pending.clear();
        }
    }

    void renameStyle()
    {
        const SurfaceDisplayStyle* style = chosen();
        const QString newName = name->text().trimmed();
        const auto fromWord = style != nullptr ? word(qs(style->name), "the style's name") : std::nullopt;
        const auto toWord = word(newName, "the new name");
        if (!fromWord || !toWord) {
            return;
        }
        pending = newName;
        if (!run("SURFACESTYLE RENAME " + *fromWord + " " + *toWord)) {
            pending.clear();
        }
    }

    void deleteStyle()
    {
        const SurfaceDisplayStyle* style = chosen();
        if (const auto styleWord = style != nullptr ? word(qs(style->name), "the style's name") : std::nullopt) {
            run("SURFACESTYLE DELETE " + *styleWord);
        }
    }

    void defaultStyle()
    {
        const SurfaceDisplayStyle* style = chosen();
        if (const auto styleWord = style != nullptr ? word(qs(style->name), "the style's name") : std::nullopt) {
            run("SURFACESTYLE DEFAULT " + *styleWord);
        }
    }

    void applyForm()
    {
        const QString text = applyLine();
        if (text.isEmpty()) {
            setStatus(lineText(), true);
            return;
        }
        run(text);
    }

    void assignStyle()
    {
        const SurfaceDisplayStyle* style = chosen();
        const auto surfaceWord = word(surface->currentText(), "the surface's name");
        const auto styleWord = style != nullptr ? word(qs(style->name), "the style's name") : std::nullopt;
        if (!surfaceWord || !styleWord) {
            return;
        }
        run("SURFACE STYLE " + *surfaceWord + " " + *styleWord);
    }
};

SurfaceStyleManagerDialog::SurfaceStyleManagerDialog(SurfaceStyleManagerContext context, QWidget* parent)
    : QDialog(parent), impl_(std::make_unique<Impl>())
{
    Impl& impl = *impl_;
    impl.dialog = this;
    impl.context = std::move(context);
    setObjectName("surfaceStyleManagerDialog");
    setWindowTitle("Surface Styles");
    // Non-modal: kept open beside the views while a style is tried on a
    // surface.
    setModal(false);
    impl.build();
    if (impl.context.document != nullptr) {
        impl.watcher = std::make_unique<DocumentWatcher>(
            *impl.context.document, [&impl](const DocumentChanges& changes) {
                if (changes.model) {
                    impl.reload();
                }
            });
    }
    impl.reload();
    resize(1100, 560);
}

SurfaceStyleManagerDialog::~SurfaceStyleManagerDialog()
{
    // The watcher first, then every connection: left to ~QWidget, the
    // children would die after impl_ and could signal into it on the way.
    impl_->watcher.reset();
    impl_->guard.reset();
}

bool SurfaceStyleManagerDialog::selectStyle(const QString& name)
{
    for (int row = 0; row < impl_->list->count(); ++row) {
        if (impl_->list->item(row)->text().compare(name, Qt::CaseInsensitive) == 0) {
            impl_->list->setCurrentRow(row);
            return true;
        }
    }
    return false;
}

QString SurfaceStyleManagerDialog::currentStyle() const
{
    return impl_->list->currentItem() != nullptr ? impl_->list->currentItem()->text() : QString();
}

std::optional<SurfaceDisplayStyle> SurfaceStyleManagerDialog::formStyle() const
{
    auto style = impl_->read();
    return style ? std::optional<SurfaceDisplayStyle>(*style) : std::nullopt;
}

QString SurfaceStyleManagerDialog::applyLine() const { return impl_->applyLine(); }

} // namespace katana::qt
