#pragma once

// Section / profile viewport (PLAN.MD Phases 14 and 21).
//
// Drawn with QPainter rather than through the 3D rasteriser, deliberately: a
// section is a measured DRAWING - axes, a datum, gridlines, station labels,
// annotated crossings - not a view of a model. Text and hairlines are what
// QPainter is good at, and a section never has enough geometry for the software
// rasteriser to be worth its setup.
//
// The section itself is computed by katana::cad::extractSection and is not
// recomputed while the user pans or zooms: this widget only maps station and
// elevation to pixels.

#include <functional>
#include <optional>
#include <string>

#include <QPoint>
#include <QRectF>
#include <QSize>
#include <QWidget>

#include "katana/cad/document.hpp"
#include "katana/cad/section.hpp"
#include "katana/cad/view_set.hpp"

namespace katana::qt {

class SectionViewWidget final : public QWidget {
  public:
    // `state` belongs to the workspace's ViewSet and outlives this widget. The
    // section and its exaggeration live there (ViewState::section and
    // sectionExaggeration), so changing the view to another kind and back
    // shows the same section again.
    explicit SectionViewWidget(katana::cad::ViewState& state, QWidget* parent = nullptr);

    [[nodiscard]] katana::cad::ViewState& state() const { return state_; }

    // The drawing whose selection this view marks on its crossings, each of
    // which names the entity it cuts: a selected one in the selection colour,
    // and one on a layer this view hides as a ghost (cad/selection_style.hpp).
    // Repaints on every change of it, which Qt folds into one paint. Without
    // one (the default) every crossing is drawn as it always was.
    void setDocument(katana::cad::Document* document);

    // Replaces what is shown. An empty section draws the "no section" message
    // rather than an empty grid, so it is obvious nothing has been cut yet.
    void setSection(katana::cad::Section section);
    void clearSection();
    [[nodiscard]] bool hasSection() const { return state_.section.has_value(); }
    [[nodiscard]] const std::optional<katana::cad::Section>& section() const
    {
        return state_.section;
    }

    // Vertical exaggeration. A profile at 1:1 over a kilometre is a flat line,
    // so 10 is the usual default for a long section.
    void setVerticalExaggeration(double factor);
    [[nodiscard]] double verticalExaggeration() const { return state_.sectionExaggeration; }

    void zoomExtents();
    // Zooms by `factor` (above 1 closer) about `position`, in this widget's
    // pixels, keeping the station and elevation there where they are: the
    // wheel's zoom, and ZOOM IN and OUT's about plotCentre. A factor that is
    // not finite and above 0 does nothing.
    void zoomAt(const QPointF& position, double factor);

    // Station and elevation under a point of this widget, in its own pixels:
    // the live readout, and what a headless check asks to prove that a resize
    // kept the drawing where the user had put it.
    [[nodiscard]] QPointF stationElevationAt(const QPointF& pixel) const;
    // The middle of the plot area - the part inside the axes - in pixels.
    [[nodiscard]] QPointF plotCentre() const;

    // Crossings the last paint drew: those whose layer this view does not
    // hide and whose station was in view. For the headless checks, as
    // ViewportWidget::lastDrawnEntityCount is.
    [[nodiscard]] std::size_t lastDrawnCrossingCount() const { return lastDrawnCrossings_; }
    // Crossings the last paint left out because this view hides their layer
    // - the layer their entity is on now (layerOf).
    [[nodiscard]] std::size_t lastHiddenCrossingCount() const { return lastHiddenCrossings_; }
    // Crossings the last paint drew as selected: a selected entity this
    // view draws (cad::isDrawn).
    [[nodiscard]] std::size_t lastSelectedCrossingCount() const { return lastSelectedCrossings_; }
    // Crossings the last paint drew as ghosts (ViewState::selectionGhosts): a
    // selected entity the document draws and this view hides (cad::isGhost).
    [[nodiscard]] std::size_t lastGhostCrossingCount() const { return lastGhostCrossings_; }

    // Raised when this view is clicked or the user moves the keyboard focus
    // into it (view_focus.hpp).
    std::function<void()> onActivated;
    // Messages the user must see. Nothing here raises it today; it stays for
    // the window's wiring and for a failure this view may one day report.
    std::function<void(const QString&)> onStatus;
    // What the view shows now: after every paint the length and the crossings
    // ("Section  length 120.00  7 crossings, 2 hidden in this view, 1
    // selected" - a ghost counts as selected), and on a
    // mouse move the station and elevation under the cursor. Transient text,
    // kept apart from onStatus so that it never writes over a prompt or an
    // error in the status bar.
    std::function<void(const QString&)> onFrameStats;

  protected:
    void paintEvent(QPaintEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;
    void mouseMoveEvent(QMouseEvent* event) override;
    void mouseReleaseEvent(QMouseEvent* event) override;
    void mouseDoubleClickEvent(QMouseEvent* event) override;
    void wheelEvent(QWheelEvent* event) override;

  private:
    // Station/elevation -> pixels. `scale_` is pixels per model unit
    // horizontally; vertically it is scale_ times the exaggeration.
    [[nodiscard]] QPointF toScreen(double station, double elevation) const;
    [[nodiscard]] double stationAt(double x) const;
    [[nodiscard]] double elevationAt(double y) const;
    // A round number of model units that is at least `minimumPixels` apart on
    // screen: 1, 2, 5, 10, 20, 50 ... so gridlines land on values a surveyor
    // would actually write down.
    [[nodiscard]] double niceStep(double minimumPixels, bool vertical) const;

    [[nodiscard]] double exaggeration() const { return state_.sectionExaggeration; }
    // zoomExtents without asking for a repaint, for the paint that frames.
    void frameExtents();

    void drawGrid(QPainter& painter) const;
    void drawSurfaces(QPainter& painter) const;
    void drawCrossings(QPainter& painter) const;
    // The selected entities' crossings and their ghosts, after the surfaces
    // so that no line is drawn over them.
    void drawSelectedCrossings(QPainter& painter) const;
    void drawLegend(QPainter& painter) const;
    // How this view marks a crossing: its entity selected and drawn here, a
    // ghost of one selected where only this view hides it (with the view's
    // switch on), or not at all.
    enum class Mark { None, Selected, Ghost };
    [[nodiscard]] Mark markOf(const katana::cad::SectionCrossing& crossing) const;
    // The layer a crossing is on NOW: its entity's, which markOf reads too,
    // so whether this view hides a crossing and how it marks its selection
    // go by one layer - the cut's, read by the one and not the other, drew
    // an entity moved onto a hidden layer both plain and as a ghost. The
    // cut's when the entity has gone since, or there is no document.
    [[nodiscard]] const std::string& layerOf(const katana::cad::SectionCrossing& crossing) const;
    // Inside the axes and their labels, in pixels.
    [[nodiscard]] QRectF plotRect() const;

    katana::cad::ViewState& state_;

    // The pan and zoom. Widget state, not view state: ViewState has no place
    // for them yet, so a change of kind and back frames the section afresh.
    // Moving these four into the state is what would keep them.
    double scale_ = 1.0;          // pixels per model unit, horizontally
    double originStation_ = 0.0;  // station at the left edge of the plot area
    double originElevation_ = 0.0; // elevation at the bottom edge
    // False until the section has been framed once at a real size. The first
    // paint frames it. After that a resize frames it again at the new size
    // until the user pans or zooms (userMoved_), and from then on keeps their
    // scale and the middle of what they were looking at.
    bool framed_ = false;
    bool userMoved_ = false;
    // The size the last resize left this widget at, which a resize delivered
    // with no old size (one made while the widget was hidden) measures from.
    QSize lastSize_;
    mutable std::size_t lastDrawnCrossings_ = 0;
    mutable std::size_t lastHiddenCrossings_ = 0;
    mutable std::size_t lastSelectedCrossings_ = 0;
    mutable std::size_t lastGhostCrossings_ = 0;

    katana::cad::Document* document_ = nullptr;
    katana::cad::Document::ListenerHandle documentListener_;

    bool panning_ = false;
    QPoint lastMouse_;
};

} // namespace katana::qt
