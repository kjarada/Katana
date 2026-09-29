// The plan view's paint and plot, on the generated survey drawing
// (survey_drawing.hpp): what a frame of the plan view costs at zoom extents
// and zoomed in, what a mouse move over an unchanged drawing costs, and what
// one A1 plot costs.
//
// Driven through ViewportWidget's own public surface - render() into a QImage
// and plotToPdf() - so that the SAME source measures the widget before the
// plan painter was taken out of it and after, and tools/compare_benchmarks.py
// --alternate can run the two builds side by side.
//
// A frame is forced to be a real frame by panning one pixel each way between
// iterations: the view has changed, so nothing drawn for the last frame can be
// reused. The cursor-move benchmark does the opposite - the drawing and the
// view stay as they are and only the mouse moves - which is what the cached
// drawing layer is for.

#include <benchmark/benchmark.h>

#include <cstdio>
#include <filesystem>
#include <memory>
#include <string>
#include <vector>

#include <QByteArray>
#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QImage>
#include <QMouseEvent>
#include <QPainter>

#include "katana/archive12d/customisation.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/layer_overrides.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/selection.hpp"
#include "katana/cad/view_set.hpp"
#include "katana/entity/model.hpp"
#include "katana/interop/archive12d.hpp"
#include "survey_drawing.hpp"
#include "viewport_widget.hpp"

namespace {

using katana::geometry::Box2;
using katana::geometry::Point2;

constexpr int kWidth = 1600;
constexpr int kHeight = 1000;

// The drawing, built once per process: thirty thousand entities take a
// second or two to make, and every benchmark here only reads them.
struct Fixture {
    katana::cad::Document document;
    katana::bench::SurveyDrawingSummary summary;
    std::filesystem::path project;
    bool ok = false;
};

Fixture& fixture()
{
    static std::unique_ptr<Fixture> made = [] {
        auto f = std::make_unique<Fixture>();
        f->document.setStyleLibrary(katana::archive12d::builtinCustomisation().library);
        auto built = katana::bench::buildSurveyDrawing(f->document);
        if (!built) {
            std::fprintf(stderr, "survey drawing: %s\n", built.error().describe().c_str());
            return f;
        }
        f->summary = *built;
        // Saved and opened again, so the spatial index is the one an opened
        // project has: one built by an in-session import keeps the default
        // cell and queries far slower (docs/performance.md), which would
        // measure the index rather than the paint.
        f->project = std::filesystem::temp_directory_path() /
                     ("katana_plan_bench_" + std::to_string(QCoreApplication::applicationPid()));
        std::error_code ignored;
        std::filesystem::remove_all(f->project, ignored);
        if (auto saved = f->document.saveAs(f->project); !saved) {
            std::fprintf(stderr, "save: %s\n", saved.error().describe().c_str());
            return f;
        }
        if (auto opened = f->document.open(f->project); !opened) {
            std::fprintf(stderr, "open: %s\n", opened.error().describe().c_str());
            return f;
        }
        f->ok = true;
        return f;
    }();
    return *made;
}

// A plan view of the fixture, `zoom` times closer than the drawing's extent,
// centred on `centreFraction` of the way across it.
struct View {
    katana::cad::ViewSet views;
    katana::cad::ViewState* state = nullptr;
    std::unique_ptr<katana::qt::ViewportWidget> widget;
    QImage image{kWidth, kHeight, QImage::Format_ARGB32_Premultiplied};

    View(double zoom, Point2 centreFraction)
    {
        Fixture& f = fixture();
        state = &views.add(katana::cad::ViewKind::Plan);
        const Box2 extent = f.summary.extent;
        state->plan.resize(kWidth, kHeight);
        state->plan.fit(extent, 0.02);
        state->plan.scale *= zoom;
        state->plan.center = Point2(extent.min.x + centreFraction.x * (extent.max.x - extent.min.x),
                                    extent.min.y + centreFraction.y * (extent.max.y - extent.min.y));
        state->planFramed = true; // this view, not a frame of the drawing
        widget = std::make_unique<katana::qt::ViewportWidget>(f.document, *state);
        widget->resize(kWidth, kHeight);
        widget->setGridVisible(false);
    }

    void frame()
    {
        widget->render(&image);
    }
};

void setCounters(benchmark::State& state, const katana::qt::ViewportWidget& widget)
{
    state.counters["entitiesDrawn"] = static_cast<double>(widget.lastDrawnEntityCount());
    state.counters["entities"] = static_cast<double>(fixture().summary.entities);
}

// Frames of a view `zoom` times closer than the extent, each panned a pixel
// from the last so that nothing drawn for one can be reused by the next.
void panningFrames(benchmark::State& state, double zoom, Point2 centreFraction)
{
    if (!fixture().ok) {
        state.SkipWithError("the survey drawing could not be built");
        return;
    }
    View view(zoom, centreFraction);
    double direction = 1.0;
    for (auto _ : state) {
        view.state->plan.panByPixels(direction, 0.0); // a changed view: nothing reusable
        direction = -direction;
        view.frame();
    }
    setCounters(state, *view.widget);
}

// The plan view's lines at the 1.5 px hairline it drew before View > Thin
// screen lines existed, for as long as a benchmark runs: what the speed work
// bought WITHOUT the look changing, measured against a build from before it.
struct ThickLines {
    bool was = katana::qt::ViewportWidget::thinScreenLines();
    ThickLines() { katana::qt::ViewportWidget::setThinScreenLines(false); }
    ~ThickLines() { katana::qt::ViewportWidget::setThinScreenLines(was); }
    ThickLines(const ThickLines&) = delete;
    ThickLines& operator=(const ThickLines&) = delete;
};

// A whole frame at zoom extents: every entity drawn, in the product's
// default look (thin screen lines).
void BM_PlanPaintExtents(benchmark::State& state)
{
    panningFrames(state, 1.0, Point2(0.5, 0.5));
}
BENCHMARK(BM_PlanPaintExtents)->Unit(benchmark::kMillisecond)->UseRealTime();

void BM_PlanPaintExtentsThickLines(benchmark::State& state)
{
    const ThickLines thick;
    panningFrames(state, 1.0, Point2(0.5, 0.5));
}
BENCHMARK(BM_PlanPaintExtentsThickLines)->Unit(benchmark::kMillisecond)->UseRealTime();

// Zoomed in five times, a fifth of the site across: the contours crossing
// the view run far beyond it on both sides, which is the case clipping to the
// view is for (measured on a real archive at this zoom: 159 partly visible
// strings, 221 ms).
void BM_PlanPaintZoomed(benchmark::State& state)
{
    panningFrames(state, 5.0, Point2(0.45, 0.55));
}
BENCHMARK(BM_PlanPaintZoomed)->Unit(benchmark::kMillisecond)->UseRealTime();

void BM_PlanPaintZoomedThickLines(benchmark::State& state)
{
    const ThickLines thick;
    panningFrames(state, 5.0, Point2(0.45, 0.55));
}
BENCHMARK(BM_PlanPaintZoomedThickLines)->Unit(benchmark::kMillisecond)->UseRealTime();

// The mouse moving over an unchanged drawing at zoom extents: what every
// cursor move, snap marker and rubber band costs.
void BM_PlanPaintCursorMove(benchmark::State& state)
{
    if (!fixture().ok) {
        state.SkipWithError("the survey drawing could not be built");
        return;
    }
    View view(1.0, Point2(0.5, 0.5));
    view.frame(); // the first frame, which any view pays once
    int x = 0;
    for (auto _ : state) {
        const QPointF at(400.0 + (x++ % 200), 300.0);
        QMouseEvent move(QEvent::MouseMove, at, view.widget->mapToGlobal(at), Qt::NoButton,
                         Qt::NoButton, Qt::NoModifier);
        QCoreApplication::sendEvent(view.widget.get(), &move);
        view.frame();
    }
    setCounters(state, *view.widget);
    // How many times the drawing itself was painted over the run: once, the
    // first frame, when the kept drawing layer does its job.
    state.counters["drawingPaints"] = static_cast<double>(view.widget->drawingPaintCount());
}
BENCHMARK(BM_PlanPaintCursorMove)->Unit(benchmark::kMillisecond)->UseRealTime();

// Two plan views of the fixture in one ViewSet, linked (cad/view_link.hpp):
// a design view and an as-built view that pan and zoom together. The second
// starts elsewhere and is brought to the first by the link, as VIEWS LINK
// brings it.
struct LinkedPair {
    katana::cad::ViewSet views;
    katana::cad::ViewState* leader = nullptr;
    katana::cad::ViewState* follower = nullptr;
    std::unique_ptr<katana::qt::ViewportWidget> leaderWidget;
    std::unique_ptr<katana::qt::ViewportWidget> followerWidget;
    QImage leaderImage{kWidth, kHeight, QImage::Format_ARGB32_Premultiplied};
    QImage followerImage{kWidth, kHeight, QImage::Format_ARGB32_Premultiplied};

    LinkedPair(double zoom, Point2 centreFraction)
    {
        Fixture& f = fixture();
        const Box2 extent = f.summary.extent;
        for (katana::cad::ViewState** made : {&leader, &follower}) {
            katana::cad::ViewState& view = views.add(katana::cad::ViewKind::Plan);
            view.plan.resize(kWidth, kHeight);
            view.plan.fit(extent, 0.02);
            view.planFramed = true;
            *made = &view;
        }
        leader->plan.scale *= zoom;
        leader->plan.center =
            Point2(extent.min.x + centreFraction.x * (extent.max.x - extent.min.x),
                   extent.min.y + centreFraction.y * (extent.max.y - extent.min.y));
        const katana::cad::ViewId both[] = {leader->id, follower->id};
        (void)views.link(both, leader->id);
        leaderWidget = std::make_unique<katana::qt::ViewportWidget>(f.document, *leader);
        followerWidget = std::make_unique<katana::qt::ViewportWidget>(f.document, *follower);
        for (katana::qt::ViewportWidget* widget : {leaderWidget.get(), followerWidget.get()}) {
            widget->resize(kWidth, kHeight);
            widget->setGridVisible(false);
        }
    }
};

// One step of a middle-drag pan in a view with a second linked to it: the
// pan, the link's follow (ViewSet::follow, what ViewWorkspace::viewMoved
// runs) and both views drawn. Against BM_PlanPaintZoomed, the same pan of
// the same view at the same zoom on its own, it is what linking costs: the
// second view drawn whole, since its centre moved (docs/plan_view.md,
// "Linked views"). Measured, not optimised.
void BM_PlanLinkedPan(benchmark::State& state)
{
    if (!fixture().ok) {
        state.SkipWithError("the survey drawing could not be built");
        return;
    }
    LinkedPair pair(5.0, Point2(0.45, 0.55));
    double direction = 1.0;
    for (auto _ : state) {
        pair.leader->plan.panByPixels(direction, 0.0);
        direction = -direction;
        (void)pair.views.follow(pair.leader->id);
        pair.leaderWidget->render(&pair.leaderImage);
        pair.followerWidget->render(&pair.followerImage);
    }
    setCounters(state, *pair.leaderWidget);
    state.counters["followerDrawn"] =
        static_cast<double>(pair.followerWidget->lastDrawnEntityCount());
}
BENCHMARK(BM_PlanLinkedPan)->Unit(benchmark::kMillisecond)->UseRealTime();

// One A1 sheet at 1 : 2500 about the drawing's centre, at 300 dpi.
void BM_PlanPlotA1(benchmark::State& state)
{
    if (!fixture().ok) {
        state.SkipWithError("the survey drawing could not be built");
        return;
    }
    View view(1.0, Point2(0.5, 0.5));
    katana::cad::PlotSettings settings;
    settings.paper = katana::cad::PaperSize::A1;
    settings.landscape = true;
    settings.scaleDenominator = 2500.0;
    const Box2 extent = fixture().summary.extent;
    settings.center = Point2(0.5 * (extent.min.x + extent.max.x), 0.5 * (extent.min.y + extent.max.y));
    const QString path =
        QDir::temp().filePath(QString("katana_plan_bench_%1.pdf").arg(QCoreApplication::applicationPid()));
    for (auto _ : state) {
        if (auto status = view.widget->plotToPdf(path, settings); !status) {
            state.SkipWithError(status.error().describe().c_str());
            break;
        }
    }
    QFile::remove(path);
    setCounters(state, *view.widget);
}
BENCHMARK(BM_PlanPlotA1)->Unit(benchmark::kMillisecond)->UseRealTime();

} // namespace

namespace {

// The painter alone (plan_painter.hpp), with each of its screen-speed
// measures switched on or off, so that one binary says what each buys: the
// thin cosmetic pen, clipping lines to the view, and symbol sprites. (The
// once-per-key display resolution and the kept drawing layer cannot be
// switched off; a build from before them measures those.) The counters say
// what the paint did: entities drawn, stamps and how many came from a
// sprite, displays resolved, lines clipped.
void BM_PlanPainter(benchmark::State& state, double zoom, Point2 centreFraction, bool thin,
                    bool clip, bool sprites)
{
    Fixture& f = fixture();
    if (!f.ok) {
        state.SkipWithError("the survey drawing could not be built");
        return;
    }
    const Box2 extent = f.summary.extent;
    katana::qt::PlanFrame frame;
    frame.transform.resize(kWidth, kHeight);
    frame.transform.fit(extent, 0.02);
    frame.transform.scale *= zoom;
    frame.transform.center =
        Point2(extent.min.x + centreFraction.x * (extent.max.x - extent.min.x),
               extent.min.y + centreFraction.y * (extent.max.y - extent.min.y));
    katana::qt::PlanPaintOptions options;
    options.thinLines = thin;
    options.clipLines = clip;
    options.symbolSprites = sprites;
    const katana::qt::PlanSource source = katana::qt::planSourceOf(f.document);
    katana::qt::PlanPaintCache cache;
    QImage image(kWidth, kHeight, QImage::Format_ARGB32_Premultiplied);
    katana::qt::PlanPaintStats stats;
    double direction = 1.0;
    for (auto _ : state) {
        frame.transform.panByPixels(direction, 0.0);
        direction = -direction;
        QPainter painter(&image);
        painter.fillRect(image.rect(), QColor(0x1e, 0x23, 0x29));
        stats = katana::qt::paintPlan(painter, source, frame, options, cache);
    }
    state.counters["entitiesDrawn"] = static_cast<double>(stats.entitiesDrawn);
    state.counters["stamps"] = static_cast<double>(stats.symbolsStamped);
    state.counters["sprites"] = static_cast<double>(stats.spritesDrawn);
    state.counters["displays"] = static_cast<double>(stats.displaysResolved);
    state.counters["clipped"] = static_cast<double>(stats.linesClipped);
}

const Point2 kWhole(0.5, 0.5);
const Point2 kZoomedCentre(0.45, 0.55);

} // namespace

// At extents: nothing on (the 1.5 px pen, every vertex, every stamp
// stroked), each measure alone, and all three.
BENCHMARK_CAPTURE(BM_PlanPainter, extents_none, 1.0, kWhole, false, false, false)
    ->Unit(benchmark::kMillisecond)->UseRealTime();
BENCHMARK_CAPTURE(BM_PlanPainter, extents_clip, 1.0, kWhole, false, true, false)
    ->Unit(benchmark::kMillisecond)->UseRealTime();
BENCHMARK_CAPTURE(BM_PlanPainter, extents_sprites, 1.0, kWhole, false, false, true)
    ->Unit(benchmark::kMillisecond)->UseRealTime();
BENCHMARK_CAPTURE(BM_PlanPainter, extents_thin, 1.0, kWhole, true, false, false)
    ->Unit(benchmark::kMillisecond)->UseRealTime();
BENCHMARK_CAPTURE(BM_PlanPainter, extents_all, 1.0, kWhole, true, true, true)
    ->Unit(benchmark::kMillisecond)->UseRealTime();
// Zoomed in five times, where clipping is for.
BENCHMARK_CAPTURE(BM_PlanPainter, zoomed_none, 5.0, kZoomedCentre, false, false, false)
    ->Unit(benchmark::kMillisecond)->UseRealTime();
BENCHMARK_CAPTURE(BM_PlanPainter, zoomed_clip, 5.0, kZoomedCentre, false, true, false)
    ->Unit(benchmark::kMillisecond)->UseRealTime();
BENCHMARK_CAPTURE(BM_PlanPainter, zoomed_all, 5.0, kZoomedCentre, true, true, true)
    ->Unit(benchmark::kMillisecond)->UseRealTime();
// Zoomed in twenty-five times: a contour of 400 vertices across the site
// shows about 16 of them, and clipping drops the other 96%.
BENCHMARK_CAPTURE(BM_PlanPainter, deep_none, 25.0, kZoomedCentre, false, false, false)
    ->Unit(benchmark::kMillisecond)->UseRealTime();
BENCHMARK_CAPTURE(BM_PlanPainter, deep_clip, 25.0, kZoomedCentre, false, true, false)
    ->Unit(benchmark::kMillisecond)->UseRealTime();

namespace {

// The selection's ghosts (PlanPaintOptions::selectionGhosts;
// docs/plan_view.md, "Ghosts of the selection"): every entity selected and
// every other layer hidden in the view, painted with the ghosts on and off.
// Off is the paint as it was before there were ghosts, the pass skipped; the
// difference is the pass - a walk of the selection's ids, the half on hidden
// layers drawn dotted.
void BM_PlanPainterGhosts(benchmark::State& state, double zoom, bool ghosts)
{
    Fixture& f = fixture();
    if (!f.ok) {
        state.SkipWithError("the survey drawing could not be built");
        return;
    }
    const Box2 extent = f.summary.extent;
    katana::qt::PlanFrame frame;
    frame.transform.resize(kWidth, kHeight);
    frame.transform.fit(extent, 0.02);
    frame.transform.scale *= zoom;
    frame.transform.center =
        Point2(extent.min.x + kZoomedCentre.x * (extent.max.x - extent.min.x),
               extent.min.y + kZoomedCentre.y * (extent.max.y - extent.min.y));
    katana::cad::LayerOverrides hidden;
    const std::vector<std::string> names = f.document.model().layers.names();
    for (std::size_t i = 1; i < names.size(); i += 2) {
        (void)hidden.hide(names[i]);
    }
    frame.layers = &hidden;
    katana::cad::SelectionSet everything;
    everything.set(f.document.model().entities.ids());
    katana::qt::PlanSource source = katana::qt::planSourceOf(f.document);
    source.selection = &everything;
    katana::qt::PlanPaintOptions options;
    options.selectionGhosts = ghosts;
    katana::qt::PlanPaintCache cache;
    QImage image(kWidth, kHeight, QImage::Format_ARGB32_Premultiplied);
    katana::qt::PlanPaintStats stats;
    double direction = 1.0;
    for (auto _ : state) {
        frame.transform.panByPixels(direction, 0.0);
        direction = -direction;
        QPainter painter(&image);
        painter.fillRect(image.rect(), QColor(0x1e, 0x23, 0x29));
        stats = katana::qt::paintPlan(painter, source, frame, options, cache);
    }
    state.counters["entitiesDrawn"] = static_cast<double>(stats.entitiesDrawn);
    state.counters["ghosts"] = static_cast<double>(stats.ghostsDrawn);
    state.counters["selected"] = static_cast<double>(everything.size());
}

} // namespace

BENCHMARK_CAPTURE(BM_PlanPainterGhosts, extents_off, 1.0, false)
    ->Unit(benchmark::kMillisecond)->UseRealTime();
BENCHMARK_CAPTURE(BM_PlanPainterGhosts, extents_on, 1.0, true)
    ->Unit(benchmark::kMillisecond)->UseRealTime();
BENCHMARK_CAPTURE(BM_PlanPainterGhosts, zoomed_off, 5.0, false)
    ->Unit(benchmark::kMillisecond)->UseRealTime();
BENCHMARK_CAPTURE(BM_PlanPainterGhosts, zoomed_on, 5.0, true)
    ->Unit(benchmark::kMillisecond)->UseRealTime();

namespace {

// One of the owner's real archives, when KATANA_BENCH_ARCHIVE names one: tens
// of megabytes of someone's survey cannot be committed, and the generated
// drawing's 400-vertex contours are not the case clipping was measured on (a
// scratch harness found 221 ms in 159 partly visible strings of the archive
// with a TIN, zoomed to a fifth of its extent). Built straight into a model,
// with no library: the case measured.
struct ArchiveFixture {
    katana::entity::Model model;
    Box2 extent;
    bool ok = false;
    std::string why = "KATANA_BENCH_ARCHIVE is not set";
};

ArchiveFixture& archiveFixture()
{
    static std::unique_ptr<ArchiveFixture> made = [] {
        auto f = std::make_unique<ArchiveFixture>();
        const QByteArray path = qgetenv("KATANA_BENCH_ARCHIVE");
        if (path.isEmpty()) {
            return f;
        }
        auto imported =
            katana::interop::importArchive12d(std::filesystem::path(QString::fromUtf8(path).toStdWString()));
        if (!imported) {
            f->why = imported.error().describe();
            return f;
        }
        for (const katana::entity::Layer& layer : imported->layersNeeded) {
            (void)f->model.layers.add(layer);
        }
        for (const katana::entity::Style& style : imported->stylesNeeded) {
            (void)f->model.styles.add(style);
        }
        for (katana::entity::Entity& entity : imported->entities) {
            (void)f->model.entities.add(std::move(entity));
        }
        f->extent = imported->bounds;
        f->ok = !f->extent.empty();
        return f;
    }();
    return *made;
}

void BM_PlanPainterArchive(benchmark::State& state, double zoom, bool clip)
{
    ArchiveFixture& f = archiveFixture();
    if (!f.ok) {
        state.SkipWithError(f.why.c_str());
        return;
    }
    katana::qt::PlanFrame frame;
    frame.transform.resize(kWidth, kHeight);
    frame.transform.fit(f.extent, 0.02);
    frame.transform.scale *= zoom;
    frame.transform.center = f.extent.center();
    katana::qt::PlanPaintOptions options;
    options.clipLines = clip;
    katana::qt::PlanSource source;
    source.model = &f.model;
    katana::qt::PlanPaintCache cache;
    QImage image(kWidth, kHeight, QImage::Format_ARGB32_Premultiplied);
    katana::qt::PlanPaintStats stats;
    double direction = 1.0;
    for (auto _ : state) {
        frame.transform.panByPixels(direction, 0.0);
        direction = -direction;
        QPainter painter(&image);
        painter.fillRect(image.rect(), QColor(0x1e, 0x23, 0x29));
        stats = katana::qt::paintPlan(painter, source, frame, options, cache);
    }
    state.counters["entitiesDrawn"] = static_cast<double>(stats.entitiesDrawn);
    state.counters["clipped"] = static_cast<double>(stats.linesClipped);
}

} // namespace

BENCHMARK_CAPTURE(BM_PlanPainterArchive, zoomed_none, 5.0, false)
    ->Unit(benchmark::kMillisecond)->UseRealTime();
BENCHMARK_CAPTURE(BM_PlanPainterArchive, zoomed_clip, 5.0, true)
    ->Unit(benchmark::kMillisecond)->UseRealTime();
