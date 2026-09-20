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

#include <QPoint>
#include <QWidget>

#include "katana/cad/section.hpp"

namespace katana::qt {

class SectionViewWidget final : public QWidget {
  public:
    explicit SectionViewWidget(QWidget* parent = nullptr);

    // Replaces what is shown. An empty section draws the "no section" message
    // rather than an empty grid, so it is obvious nothing has been cut yet.
    void setSection(katana::cad::Section section);
    void clearSection();
    [[nodiscard]] bool hasSection() const { return section_.has_value(); }
    [[nodiscard]] const std::optional<katana::cad::Section>& section() const { return section_; }

    // Vertical exaggeration. A profile at 1:1 over a kilometre is a flat line,
    // so 10 is the usual default for a long section.
    void setVerticalExaggeration(double factor);
    [[nodiscard]] double verticalExaggeration() const { return exaggeration_; }

    void zoomExtents();

    std::function<void()> onActivated;
    std::function<void(const QString&)> onStatus;

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
    // horizontally; vertically it is scale_ * exaggeration_.
    [[nodiscard]] QPointF toScreen(double station, double elevation) const;
    [[nodiscard]] double stationAt(double x) const;
    [[nodiscard]] double elevationAt(double y) const;
    // A round number of model units that is at least `minimumPixels` apart on
    // screen: 1, 2, 5, 10, 20, 50 ... so gridlines land on values a surveyor
    // would actually write down.
    [[nodiscard]] double niceStep(double minimumPixels, bool vertical) const;

    void drawGrid(QPainter& painter) const;
    void drawSurfaces(QPainter& painter) const;
    void drawCrossings(QPainter& painter) const;
    void drawLegend(QPainter& painter) const;

    std::optional<katana::cad::Section> section_;
    double exaggeration_ = 10.0;

    double scale_ = 1.0;          // pixels per model unit, horizontally
    double originStation_ = 0.0;  // station at the left edge of the plot area
    double originElevation_ = 0.0; // elevation at the bottom edge
    bool framed_ = false;

    bool panning_ = false;
    QPoint lastMouse_;
};

} // namespace katana::qt
