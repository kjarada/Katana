#include "style_preview.hpp"

#include <algorithm>
#include <cmath>
#include <limits>

#include <QPainter>
#include <QPainterPath>
#include <QStringList>

#include "katana/cad/plot.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/display.hpp"
#include "katana/entity/model.hpp"
#include "name_picker.hpp"
#include "style_painter.hpp"
#include "theme.hpp"

namespace katana::qt {

namespace {

using katana::geometry::Box2;
using katana::geometry::Point2;

// The paper the preview prints on: only the colour rule is read from it.
const katana::cad::PlotSettings& paperSettings()
{
    static const katana::cad::PlotSettings settings{};
    return settings;
}

// The entity pen on screen, as the viewport's plain line and the thumbnails'
// ink (definition_thumbnails.cpp, kInkPixels).
constexpr double kScreenInkPixels = 1.5;
// The crosshair's arms, and the warning mark's size, in pixels.
constexpr int kCrosshairArmPixels = 10;
constexpr int kWarningPixels = 16;

[[nodiscard]] QString fromName(const std::string& name) { return QString::fromStdString(name); }

// A plain line by name - the linetype names that never name a pattern.
[[nodiscard]] bool plainLinetypeName(std::string_view name)
{
    if (name.empty() || katana::entity::isByLayer(name)) {
        return true;
    }
    const std::string lowered = katana::core::lowered(name);
    return lowered == katana::entity::kContinuousLinetype || lowered == "0" || lowered == "1";
}

// The longest 1-2-5 length (…, 0.5, 1, 2, 5, 10, …) of at most `limit`.
[[nodiscard]] double niceLengthAtMost(double limit)
{
    if (!(limit > 0.0) || !std::isfinite(limit)) {
        return 0.0;
    }
    const double decade = std::pow(10.0, std::floor(std::log10(limit)));
    for (const double step : {5.0, 2.0, 1.0}) {
        if (step * decade <= limit) {
            return step * decade;
        }
    }
    return decade;
}

[[nodiscard]] QString metresText(double metres)
{
    if (metres >= 1.0) {
        return QStringLiteral("%1 m").arg(metres, 0, 'g', 6);
    }
    return QStringLiteral("%1 mm").arg(metres * 1000.0, 0, 'g', 6);
}

} // namespace

StylePreview::StylePreview(katana::cad::Document& document, QWidget* parent)
    : QWidget(parent), document_(&document)
{
    setObjectName(QStringLiteral("stylePreview"));
    setMinimumSize(160, 90);
    // A library loaded, a linetype edited: repaint - through update(), which
    // Qt coalesces into one paint from the event loop, never a paint inside
    // the Document's notification.
    registration_ = document.addListener([this] { update(); });
}

void StylePreview::setStyle(const katana::entity::Style& style)
{
    showing_ = Showing::Style;
    style_ = style;
    update();
}

void StylePreview::setSymbol(const std::string& name, double size)
{
    showing_ = Showing::Symbol;
    name_ = name;
    symbolSize_ = size;
    update();
}

void StylePreview::setLinestyle(const std::string& name)
{
    showing_ = Showing::Linestyle;
    name_ = name;
    update();
}

void StylePreview::clear()
{
    showing_ = Showing::Nothing;
    update();
}

void StylePreview::setScaleDenominator(int denominator)
{
    if (denominator > 0) {
        denominator_ = denominator;
        update();
    }
}

void StylePreview::setGround(PreviewGround ground)
{
    ground_ = ground;
    update();
}

QColor StylePreview::groundColour() const
{
    return ground_ == PreviewGround::Paper ? QColor(Qt::white) : theme::viewport();
}

QColor StylePreview::inkColour(const std::optional<katana::entity::Color>& colour) const
{
    // ByLayer: a new layer's white (the screen's default ink).
    katana::entity::Color ink = colour.value_or(katana::entity::Color{});
    if (ground_ == PreviewGround::Paper) {
        ink = katana::cad::paperColour(ink, paperSettings());
    }
    return QColor(ink.r, ink.g, ink.b, ink.a);
}

QColor StylePreview::insertionMarkColour() { return theme::accent(); }

katana::cad::StyleDrawing StylePreview::layOutLine(QSize area)
{
    // Printed size: a plot millimetre is kPixelsPerPaperMillimetre pixels
    // and covers paperScale() metres.
    const double pixelsPerMetre = kPixelsPerPaperMillimetre / paperScale();
    const double usableWidth = std::max(area.width() - 2 * kMarginPixels, 1);
    const double usableHeight = std::max(area.height() - 2 * kMarginPixels, 1);
    // styleSamplePath(w) fills a box w wide and w / 4 high.
    const double samplePixels = std::min(usableWidth, 4.0 * usableHeight);
    const double width = samplePixels / pixelsPerMetre;
    view_.resize(area.width(), area.height());
    view_.scale = pixelsPerMetre;
    view_.center = Point2(width / 2.0, width / 8.0);
    const katana::cad::StyleSamplePath sample = katana::cad::styleSamplePath(width);
    katana::cad::DashOptions dashes;
    dashes.viewScale = pixelsPerMetre;

    const katana::entity::StyleLibrary& library = document_->styleLibrary();
    if (showing_ == Showing::Style) {
        const katana::entity::Model& model = document_->model();
        QStringList notes;
        const std::string& linetype = style_.linetype;
        const bool definedLinetype = plainLinetypeName(linetype) || linetype == style_.symbol ||
                                     model.linetypes.contains(linetype) ||
                                     library.contains(linetype);
        if (!definedLinetype) {
            notes << tr("Linetype \"%1\" is not defined: drawn as a solid line.")
                         .arg(fromName(linetype));
        }
        if (!style_.symbol.empty()) {
            const katana::cad::ResolvedSymbol symbol =
                katana::cad::resolveSymbol(library, style_.symbol);
            if (symbol.kind == katana::cad::SymbolKind::BuiltInFallback) {
                notes << tr("Symbol \"%1\" is not defined: drawn as the built-in \"%2\".")
                             .arg(fromName(style_.symbol),
                                  QString::fromUtf8(symbol.builtIn.data(),
                                                    static_cast<qsizetype>(symbol.builtIn.size())));
            }
        }
        notice_ = notes.join(QLatin1Char(' '));
        return katana::cad::styleSampleDrawing(model, library, style_, sample, paperScale(),
                                               dashes);
    }

    // One library definition as a linestyle: what a line whose linetype
    // names it draws, with no model linetype of the same name in the way -
    // this is a picture of the LIBRARY's definition (as the thumbnails are).
    static const katana::entity::Model noModel{};
    const katana::cad::ResolvedLinetype resolved =
        katana::cad::resolveLinetype(noModel, library, name_);
    if (resolved.kind != katana::cad::LinetypeKind::LibraryDefinition) {
        notice_ = library.contains(name_)
                      ? tr("\"%1\" is an `at vertices` symbol, not a linestyle: a line naming it "
                           "is drawn plain.")
                            .arg(fromName(name_))
                      : tr("\"%1\" is not defined: drawn as a solid line.").arg(fromName(name_));
    }
    katana::entity::Style linestyle;
    linestyle.name = "linestyle";
    linestyle.linetype = name_;
    return katana::cad::styleSampleDrawing(noModel, library, linestyle, sample, paperScale(),
                                           dashes);
}

katana::cad::StyleDrawing StylePreview::layOutSymbol(QSize area, const QFont& font)
{
    const katana::entity::StyleLibrary& library = document_->styleLibrary();
    const katana::cad::ResolvedSymbol resolved = katana::cad::resolveSymbol(library, name_);
    if (resolved.kind == katana::cad::SymbolKind::BuiltInFallback) {
        notice_ = tr("\"%1\" is not defined: drawn as the built-in \"%2\".")
                      .arg(fromName(name_),
                           QString::fromUtf8(resolved.builtIn.data(),
                                             static_cast<qsizetype>(resolved.builtIn.size())));
    }
    // A built-in shape with no size of its own is given 2 mm of paper - any
    // size would do, since the pane is fitted to the symbol.
    katana::cad::StyleDrawing drawing = katana::cad::pointSymbolDrawing(
        definitions_, library, document_->libraryGeneration(), name_, Point2(0.0, 0.0),
        symbolSize_, 0.0, paperScale(), 2.0 * paperScale());

    // Centred on the insertion point, and scaled so the symbol's reach from
    // it fits either way: the crosshair is always the pane's centre, and an
    // off-centre symbol shows as off-centre.
    view_.resize(area.width(), area.height());
    view_.center = Point2(0.0, 0.0);
    view_.scale = kPixelsPerPaperMillimetre / paperScale(); // printed size, if nothing drawn
    const Box2 extent = paintedExtent(drawing, font);
    originOutside_ = false;
    if (!extent.empty()) {
        const double reachX = std::max(std::abs(extent.min.x), std::abs(extent.max.x));
        const double reachY = std::max(std::abs(extent.min.y), std::abs(extent.max.y));
        const double usableWidth = std::max(area.width() - 2 * kMarginPixels, 1);
        const double usableHeight = std::max(area.height() - 2 * kMarginPixels, 1);
        double scale = std::numeric_limits<double>::infinity();
        if (reachX > 0.0) {
            scale = std::min(scale, usableWidth / (2.0 * reachX));
        }
        if (reachY > 0.0) {
            scale = std::min(scale, usableHeight / (2.0 * reachY));
        }
        if (std::isfinite(scale)) {
            view_.scale = std::clamp(scale, katana::cad::ViewTransform::kMinimumScale,
                                     katana::cad::ViewTransform::kMaximumScale);
        }
        originOutside_ = extent.min.x > 0.0 || extent.max.x < 0.0 || extent.min.y > 0.0 ||
                         extent.max.y < 0.0;
    }
    const Point2 origin = view_.worldToScreen(Point2(0.0, 0.0));
    insertion_ = QPointF(origin.x, origin.y);
    return drawing;
}

void StylePreview::paintEvent(QPaintEvent* /*event*/)
{
    QPainter painter(this);
    painter.fillRect(rect(), groundColour());
    insertion_.reset();
    originOutside_ = false;
    scaleBarMetres_ = 0.0;
    notice_.clear();
    // The Document may have gone before this pane (a dialog closing with
    // the application): nothing of it may be read.
    if (showing_ == Showing::Nothing || !registration_.active()) {
        return;
    }
    painter.setRenderHint(QPainter::Antialiasing, true);
    painter.setRenderHint(QPainter::TextAntialiasing, true);

    const QSize area(width(), std::max(height() - kScaleBarPixels, 1));
    const katana::cad::StyleDrawing drawing = showing_ == Showing::Symbol
                                                  ? layOutSymbol(area, painter.font())
                                                  : layOutLine(area);

    const bool paper = ground_ == PreviewGround::Paper;
    StylePaintTarget target;
    target.view = view_;
    const std::optional<katana::entity::Color> colour =
        showing_ == Showing::Style ? style_.color : std::nullopt;
    // On paper a line weight is a printed width: the style's own, or for a
    // bare definition the weight a new style starts with.
    const double lineWeight =
        showing_ == Showing::Style ? style_.lineWeight : katana::entity::Style{}.lineWeight;
    const double inkPixels =
        paper ? std::max(1.0, lineWeight * kPixelsPerPaperMillimetre) : kScreenInkPixels;
    target.entityPen = QPen(inkColour(colour), inkPixels);
    // Flat, as the viewport and the plot paint a definition: a dash is its
    // own length, not a pen width longer.
    target.entityPen.setCapStyle(Qt::FlatCap);
    target.paper = paper ? &paperSettings() : nullptr;
    // A pen inside the definition is the colour the drawing resolves its
    // name to: the session's customisation, then the standard names.
    target.colours = &document_->customisationState().colours;
    paintStyleDrawing(painter, drawing, target);

    if (showing_ == Showing::Symbol) {
        paintInsertionMark(painter);
    }
    scaleBarMetres_ = paintScaleBar(painter, QRect(0, area.height(), width(), kScaleBarPixels));
    if (!notice_.isEmpty()) {
        painter.setPen(kUndefinedNameColour);
        painter.drawText(QRect(kMarginPixels / 2, 2, width() - kMarginPixels, area.height()),
                         Qt::AlignLeft | Qt::AlignTop | Qt::TextWordWrap, notice_);
    }
}

void StylePreview::paintInsertionMark(QPainter& painter) const
{
    if (!insertion_) {
        return;
    }
    painter.save();
    // Crisp and on whole pixels: a mark, not a drawing, and the one thing in
    // the pane a person lines things up against.
    painter.setRenderHint(QPainter::Antialiasing, false);
    painter.setPen(QPen(insertionMarkColour(), 1));
    const int x = static_cast<int>(std::lround(insertion_->x()));
    const int y = static_cast<int>(std::lround(insertion_->y()));
    painter.drawLine(x - kCrosshairArmPixels, y, x + kCrosshairArmPixels, y);
    painter.drawLine(x, y - kCrosshairArmPixels, x, y + kCrosshairArmPixels);
    if (originOutside_) {
        // A warning triangle in the top-right corner: the symbol will sit
        // away from the point it is put on.
        const QPointF corner(width() - kMarginPixels / 2.0 - kWarningPixels, 2.0);
        QPainterPath triangle;
        triangle.moveTo(corner + QPointF(kWarningPixels / 2.0, 0.0));
        triangle.lineTo(corner + QPointF(kWarningPixels, kWarningPixels));
        triangle.lineTo(corner + QPointF(0.0, kWarningPixels));
        triangle.closeSubpath();
        painter.setRenderHint(QPainter::Antialiasing, true);
        painter.fillPath(triangle, kUndefinedNameColour);
        painter.setPen(Qt::black);
        painter.drawText(QRectF(corner, QSizeF(kWarningPixels, kWarningPixels + 2.0)),
                         Qt::AlignCenter, QStringLiteral("!"));
    }
    painter.restore();
}

double StylePreview::paintScaleBar(QPainter& painter, QRect area) const
{
    // About a quarter of the pane, in a 1-2-5 length of ground metres.
    const double pixelsPerMetre = view_.scale;
    const double metres = niceLengthAtMost(width() / 4.0 / pixelsPerMetre);
    if (!(metres > 0.0)) {
        return 0.0;
    }
    const double length = metres * pixelsPerMetre;
    painter.save();
    painter.setRenderHint(QPainter::Antialiasing, false);
    const QColor ink = inkColour(std::nullopt);
    painter.setPen(QPen(ink, 1));
    const int left = kMarginPixels;
    const int right = kMarginPixels + static_cast<int>(std::lround(length));
    const int y = area.top() + area.height() / 2;
    painter.drawLine(left, y, right, y);
    painter.drawLine(left, y - 4, left, y + 4);
    painter.drawLine(right, y - 4, right, y + 4);
    painter.drawText(QRect(right + 6, area.top(), width() - right - 6, area.height()),
                     Qt::AlignLeft | Qt::AlignVCenter,
                     tr("%1   at 1:%2").arg(metresText(metres)).arg(denominator_));
    painter.restore();
    return metres;
}

} // namespace katana::qt
