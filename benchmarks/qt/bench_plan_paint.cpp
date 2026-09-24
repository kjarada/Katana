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

#include <QCoreApplication>
#include <QDir>
#include <QFile>
#include <QImage>
#include <QMouseEvent>
#include <QPainter>

#include "katana/archive12d/customisation.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/view_set.hpp"
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

// A whole frame at zoom extents: every entity drawn.
void BM_PlanPaintExtents(benchmark::State& state)
{
    if (!fixture().ok) {
        state.SkipWithError("the survey drawing could not be built");
        return;
    }
    View view(1.0, Point2(0.5, 0.5));
    double direction = 1.0;
    for (auto _ : state) {
        view.state->plan.panByPixels(direction, 0.0); // a changed view: nothing reusable
        direction = -direction;
        view.frame();
    }
    setCounters(state, *view.widget);
}
BENCHMARK(BM_PlanPaintExtents)->Unit(benchmark::kMillisecond)->UseRealTime();

// Zoomed in five times, a fifth of the site across: the contours crossing
// the view run far beyond it on both sides, which is the case clipping to the
// view is for (measured on a real archive at this zoom: 159 partly visible
// strings, 221 ms).
void BM_PlanPaintZoomed(benchmark::State& state)
{
    if (!fixture().ok) {
        state.SkipWithError("the survey drawing could not be built");
        return;
    }
    View view(5.0, Point2(0.45, 0.55));
    double direction = 1.0;
    for (auto _ : state) {
        view.state->plan.panByPixels(direction, 0.0);
        direction = -direction;
        view.frame();
    }
    setCounters(state, *view.widget);
}
BENCHMARK(BM_PlanPaintZoomed)->Unit(benchmark::kMillisecond)->UseRealTime();

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
}
BENCHMARK(BM_PlanPaintCursorMove)->Unit(benchmark::kMillisecond)->UseRealTime();

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
