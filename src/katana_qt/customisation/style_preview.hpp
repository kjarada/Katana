#pragma once

// A picture of what a style, a 12d linestyle or a 12d symbol draws, at a
// plot scale, on paper or on the screen - the preview pane of the style
// manager and the symbol library.
//
// Everything is painted through customisation/style_painter.hpp from the one
// resolver (cad/style_resolver.hpp), so the preview draws what the viewport
// and the plot draw; it computes nothing of its own about a definition.
//
// WHAT IS SHOWN
//   * a Style (setStyle): cad::styleSampleDrawing along cad::styleSamplePath
//     - a straight run, a sharp corner and an arc - with the style's symbol at
//     every vertex (decision D8). A style whose linetype is ByLayer has no
//     layer here and is drawn as a plain line.
//   * one library definition as a LINESTYLE (setLinestyle): laid along the
//     same sample, as the resolver lays it for a line whose linetype names it
//     - a `mode vertex` definition or an unknown name is a plain line, said
//     in the notice.
//   * one definition as a SYMBOL (setSymbol): cad::pointSymbolDrawing at the
//     model origin - the insertion point - which is marked with a crosshair,
//     with a warning when the origin lies outside what the symbol draws (it
//     will sit away from the point it is put on).
//
// SCALE. Lines are shown at their PRINTED size: a plot millimetre is
// kPixelsPerPaperMillimetre pixels (a 96-dpi screen), and the model units one
// plot millimetre covers are N / 1000 at 1:N (paperScale) - Katana drawings
// are in METRES. So a 12d `paperstyle` looks the same at every scale while a
// `worldstyle` or a model linetype shrinks as N grows, which is exactly what
// choosing a scale is for. A symbol is fitted to the pane instead (at 96 dpi
// a 3 mm symbol would be 11 pixels), centred on its insertion point; the
// scale still sizes a paper symbol on the ground, and the scale bar, always
// in ground metres, says how big it is.
//
// GROUND. Paper is white, and a white or near-white pen prints black there
// (cad::paperColour, decision D7) - the entity pen and every 12d pen. The
// screen is the viewport's dark ground. With no colour of its own (ByLayer)
// the entity pen is a new layer's white: white on screen, black on paper.
//
// Setters only, no signals: the preview reports nothing. It repaints when
// the Document notifies (a coalesced update(), never a paint inside the
// listener), and draws nothing once the Document has gone.

#include <array>
#include <optional>
#include <string>

#include <QColor>
#include <QString>
#include <QWidget>

#include "katana/cad/document.hpp"
#include "katana/cad/style_resolver.hpp"
#include "katana/cad/view_transform.hpp"
#include "katana/entity/tables.hpp"

namespace katana::qt {

enum class PreviewGround {
    Paper,  // white, pens by the paper colour rule
    Screen, // the viewport's dark ground
};

class StylePreview : public QWidget {
  public:
    // The plot scales offered, as the N of 1:N.
    static constexpr std::array<int, 7> kPlotScales{100, 200, 250, 500, 1000, 2000, 5000};
    // A plot millimetre on screen: 96 dpi, the logical resolution Qt gives a
    // desktop screen, so "printed size" is the same on every machine.
    static constexpr double kPixelsPerPaperMillimetre = 96.0 / 25.4;
    // Room kept round the drawing, and below it for the scale bar, in pixels.
    static constexpr int kMarginPixels = 12;
    static constexpr int kScaleBarPixels = 24;

    // The Document supplies the model (for a style's model linetype) and the
    // library. It may be destroyed before the preview.
    explicit StylePreview(katana::cad::Document& document, QWidget* parent = nullptr);

    // What to show; each replaces the last. The style is copied: a form
    // previews its working copy before anything is applied.
    void setStyle(const katana::entity::Style& style);
    // `size` is Style::symbolSize - a width in model units, 0 for the
    // definition's own.
    void setSymbol(const std::string& name, double size = 0.0);
    void setLinestyle(const std::string& name);
    void clear();

    // 1:N. Any positive N is accepted; kPlotScales are the ones a dialog
    // offers. Anything else is ignored.
    void setScaleDenominator(int denominator);
    [[nodiscard]] int scaleDenominator() const { return denominator_; }
    // Model units (metres) per plot millimetre: N / 1000.
    [[nodiscard]] double paperScale() const { return denominator_ / 1000.0; }
    void setGround(PreviewGround ground);
    [[nodiscard]] PreviewGround ground() const { return ground_; }

    // ---- the last layout: where things were painted, for a dialog's status
    // line and for tests. Worked out by paintEvent (so after a grab() or a
    // show), for the size painted at.

    // Model to widget pixels, over the area above the scale bar.
    [[nodiscard]] const katana::cad::ViewTransform& view() const { return view_; }
    // Symbol only: where the insertion point (the model origin) is painted.
    [[nodiscard]] std::optional<QPointF> insertionPoint() const { return insertion_; }
    // Symbol only: the origin lies outside the painted extent, and the
    // warning mark is shown.
    [[nodiscard]] bool originOutsideExtent() const { return originOutside_; }
    // The scale bar's length in ground metres; 0 when none was drawn.
    [[nodiscard]] double scaleBarMetres() const { return scaleBarMetres_; }
    // What the pane says about what it drew in place of the name, if
    // anything: "Not defined - drawn as ...". Empty when it drew the name.
    [[nodiscard]] QString notice() const { return notice_; }

    // The ground and the ink, as painted.
    [[nodiscard]] QColor groundColour() const;
    [[nodiscard]] QColor inkColour(const std::optional<katana::entity::Color>& colour) const;
    // The insertion point's crosshair.
    [[nodiscard]] static QColor insertionMarkColour();

  protected:
    void paintEvent(QPaintEvent* event) override;

  private:
    enum class Showing { Nothing, Style, Symbol, Linestyle };

    [[nodiscard]] katana::cad::StyleDrawing layOutLine(QSize area);
    [[nodiscard]] katana::cad::StyleDrawing layOutSymbol(QSize area, const QFont& font);
    void paintInsertionMark(QPainter& painter) const;
    // Returns the bar's length in ground metres, 0 when none fits.
    [[nodiscard]] double paintScaleBar(QPainter& painter, QRect area) const;

    katana::cad::Document* document_ = nullptr;
    Showing showing_ = Showing::Nothing;
    katana::entity::Style style_{};
    std::string name_{};
    double symbolSize_ = 0.0;
    int denominator_ = 500;
    PreviewGround ground_ = PreviewGround::Paper;

    katana::cad::DefinitionCache definitions_{};
    katana::cad::ViewTransform view_{};
    std::optional<QPointF> insertion_{};
    bool originOutside_ = false;
    double scaleBarMetres_ = 0.0;
    QString notice_{};

    // Last, so it goes first: no notification reaches a half-destroyed
    // preview. Also how paintEvent knows the Document is still there.
    katana::cad::Document::ListenerHandle registration_{};
};

} // namespace katana::qt
