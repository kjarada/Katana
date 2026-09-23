// The section view (src/katana_qt/section_view_widget).
//
// Sections are built by hand rather than cut from a surface, so that every
// expected station and elevation can be worked out from the widget's framing
// rule alone. SectionViewWidget::frameExtents fits the tighter of
//   horizontal = plot width  / (station span * 1.04)
//   vertical   = plot height / (elevation span * exaggeration * 1.15)
// and centres the box in what is left over, so a framed section always has
// the middle of its box at the middle of the plot. The plot is the widget less
// 62 px on the left, 12 on the right, 10 at the top and 26 at the bottom.

#include <gtest/gtest.h>

#include <QMouseEvent>
#include <QWheelEvent>

#include "katana/cad/section.hpp"
#include "katana/cad/view_set.hpp"
#include "widget_harness.hpp"

using katana::cad::Section;
using katana::cad::SectionCrossing;
using katana::cad::SectionSample;
using katana::cad::SectionSurface;
using katana::cad::SampleReason;
using katana::cad::ViewKind;
using katana::cad::ViewSet;
using katana::cad::ViewState;
using katana::geometry::Point2;
using katana::qt::SectionViewWidget;
using katana::qt::test::paint;

namespace {

constexpr double kLeft = 62.0;
constexpr double kRight = 12.0;

// Ground running straight from (0, 10) to (length, 20): station span
// `length`, elevation span 10.
Section ground(double length)
{
    Section section;
    section.alignment.vertices = {Point2(0.0, 0.0), Point2(length, 0.0)};
    section.length = length;
    SectionSurface surface;
    surface.name = "ground";
    SectionSample first;
    first.station = 0.0;
    first.elevation = 10.0;
    SectionSample last;
    last.station = length;
    last.plan = Point2(length, 0.0);
    last.elevation = 20.0;
    surface.samples = {first, last};
    surface.reasons = {SampleReason::Start, SampleReason::End};
    surface.minElevation = 10.0;
    surface.maxElevation = 20.0;
    section.surfaces.push_back(std::move(surface));
    return section;
}

// The station at the left and the right edge of the plot.
double leftStation(const SectionViewWidget& view)
{
    return view.stationElevationAt(QPointF(kLeft, 100.0)).x();
}
double rightStation(const SectionViewWidget& view)
{
    return view.stationElevationAt(QPointF(view.width() - kRight, 100.0)).x();
}

// One wheel notch towards the user at `pixel`: a zoom in about that point.
void wheelAt(QWidget& view, const QPointF& pixel)
{
    QWheelEvent event(pixel, view.mapToGlobal(pixel), QPoint(), QPoint(0, 120), Qt::NoButton,
                      Qt::NoModifier, Qt::NoScrollPhase, false);
    QCoreApplication::sendEvent(&view, &event);
}

} // namespace

TEST(SectionView, ASectionStillAsFramedIsFramedAgainWhenItsViewShrinks)
{
    // At 800 px the plot is 726 x 264. A 1000 m section: horizontal
    // 726 / 1040 = 0.698 px/m, vertical 264 / (10 * 10 * 1.15) = 2.30, so
    // the stations decide, and the 1040 m shown are centred on 500: the plot
    // runs from station -20 to 1020. At 400 px the plot is 326 wide and the
    // stations still decide (0.313 px/m), so framing again shows -20..1020
    // again. Keeping the scale instead showed only 326 / 0.698 = 467 m about
    // 500 - 266.5..733.5 - and cut both ends of the section off.
    ViewSet views;
    ViewState& state = views.add(ViewKind::Section);
    SectionViewWidget view(state);
    view.resize(800, 300);
    view.setSection(ground(1000.0));
    paint(view);
    EXPECT_NEAR(leftStation(view), -20.0, 1e-9);
    EXPECT_NEAR(rightStation(view), 1020.0, 1e-9);

    view.resize(400, 300);
    paint(view);
    EXPECT_NEAR(leftStation(view), -20.0, 1e-9);
    EXPECT_NEAR(rightStation(view), 1020.0, 1e-9);
}

TEST(SectionView, AZoomedSectionKeepsItsScaleAndItsMiddleWhenItsViewIsResized)
{
    // A 100 m section at 800 x 300 is framed with its box's middle, (50, 15),
    // at the middle of the plot, (62 + 726 / 2, 10 + 264 / 2) = (425, 142). A
    // wheel notch there zooms about that point, so it stays at the middle.
    // Shrinking the view to 400 wide keeps the scale - the plot's 326 px now
    // cover 326 / 726 of the stations they did - and the middle, now at
    // (62 + 326 / 2, 142) = (225, 142), still reads (50, 15).
    ViewSet views;
    ViewState& state = views.add(ViewKind::Section);
    QWidget window;
    window.resize(900, 400);
    auto* view = new SectionViewWidget(state, &window); // owned by the window
    view->setGeometry(0, 0, 800, 300);
    view->setSection(ground(100.0));
    ASSERT_TRUE(katana::qt::test::showActive(window));
    paint(*view);
    ASSERT_EQ(view->plotCentre(), QPointF(425.0, 142.0));
    wheelAt(*view, view->plotCentre());
    const double spanBefore = rightStation(*view) - leftStation(*view);

    view->resize(400, 300);
    paint(*view);
    ASSERT_EQ(view->plotCentre(), QPointF(225.0, 142.0));
    const QPointF middle = view->stationElevationAt(view->plotCentre());
    EXPECT_NEAR(middle.x(), 50.0, 1e-9);
    EXPECT_NEAR(middle.y(), 15.0, 1e-9);
    EXPECT_NEAR((rightStation(*view) - leftStation(*view)) / spanBefore, 326.0 / 726.0, 1e-12);
}

TEST(SectionView, AZoomedSectionResizedWhileHiddenKeepsItsMiddleWhenShownAgain)
{
    // As above, but the resize happens while the view is hidden - a section
    // tabbed behind another view. Qt holds the resize back and sends it when
    // the widget is shown, without the old size; the middle must still read
    // (50, 15) at the new plot's middle, (225, 142).
    ViewSet views;
    ViewState& state = views.add(ViewKind::Section);
    QWidget window;
    window.resize(900, 400);
    auto* view = new SectionViewWidget(state, &window); // owned by the window
    view->setGeometry(0, 0, 800, 300);
    view->setSection(ground(100.0));
    ASSERT_TRUE(katana::qt::test::showActive(window));
    paint(*view);
    wheelAt(*view, view->plotCentre());

    view->hide();
    view->resize(400, 300);
    view->show();
    paint(*view);
    ASSERT_EQ(view->plotCentre(), QPointF(225.0, 142.0));
    const QPointF middle = view->stationElevationAt(view->plotCentre());
    EXPECT_NEAR(middle.x(), 50.0, 1e-9);
    EXPECT_NEAR(middle.y(), 15.0, 1e-9);
}

TEST(SectionView, ALayerHiddenInTheViewTakesItsCrossingsOutOfIt)
{
    // Two crossings on the 1000 m ground, one on "pipes" at station 200 and
    // one on "kerb" at 600, both inside the -20..1020 the plot shows. Hiding
    // "pipes" in this view leaves one drawn and one hidden.
    ViewSet views;
    ViewState& state = views.add(ViewKind::Section);
    Section section = ground(1000.0);
    SectionCrossing pipe;
    pipe.layer = "pipes";
    pipe.station = 200.0;
    SectionCrossing kerb;
    kerb.layer = "kerb";
    kerb.station = 600.0;
    section.crossings = {pipe, kerb};
    SectionViewWidget view(state);
    view.resize(800, 300);
    view.setSection(std::move(section));
    paint(view);
    EXPECT_EQ(view.lastDrawnCrossingCount(), 2u);
    EXPECT_EQ(view.lastHiddenCrossingCount(), 0u);

    ASSERT_TRUE(state.layers.hide("pipes"));
    paint(view);
    EXPECT_EQ(view.lastDrawnCrossingCount(), 1u);
    EXPECT_EQ(view.lastHiddenCrossingCount(), 1u);
}

TEST(SectionView, MovingTheMouseReportsTheStationAndElevationUnderIt)
{
    // The readout goes out through onFrameStats. At the middle of the plot,
    // (425, 142), a framed 1000 m section reads its box's middle: station
    // 500, elevation (10 + 20) / 2 = 15, at the default exaggeration of 10.
    ViewSet views;
    ViewState& state = views.add(ViewKind::Section);
    SectionViewWidget view(state);
    view.resize(800, 300);
    view.setSection(ground(1000.0));
    paint(view);
    QString readout;
    view.onFrameStats = [&readout](const QString& text) { readout = text; };

    const QPointF middle(425.0, 142.0);
    QMouseEvent move(QEvent::MouseMove, middle, view.mapToGlobal(middle), Qt::NoButton,
                     Qt::NoButton, Qt::NoModifier);
    QCoreApplication::sendEvent(&view, &move);
    EXPECT_EQ(readout, QStringLiteral("Station 500.000   Elevation 15.000   (V x10.0)"));
}
