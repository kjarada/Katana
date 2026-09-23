#include "definition_thumbnails.hpp"

#include <algorithm>
#include <utility>

#include <QPainter>
#include <QPen>

#include "katana/cad/document.hpp"
#include "katana/cad/plot.hpp"
#include "katana/cad/style_drawing.hpp"
#include "katana/cad/view_transform.hpp"
#include "katana/entity/model.hpp"
#include "style_painter.hpp"

namespace katana::qt {

namespace {

using katana::geometry::Point2;

// The entity pen's width in a picture: the viewport's own screen width, so a
// thumbnail's strokes weigh what the drawing's do.
constexpr double kInkPixels = 1.5;
// What a built-in shape with no size of its own is drawn at before the
// picture is fitted to it. Any length does, since the fit scales it away.
constexpr double kBuiltInHalfWidth = 1.0;
// Repeats of a linestyle's period along the sample path, and how many times
// its reach across the line the sample is wide: enough of the pattern to
// recognise, and room for a fence's ticks to stand clear of the next ones.
constexpr double kSamplePeriods = 4.0;
constexpr double kSampleReaches = 8.0;
// The share of each side left clear, so a pen at the edge is not cut in half.
constexpr double kMarginFraction = 0.12;

// On a light ground the picture is on paper, and a light ground is one a
// light pen would vanish into.
[[nodiscard]] bool paperGround(const QColor& ground) { return ground.lightness() >= 128; }

// The linestyle `name` laid along the sample path, or the plain sample path
// when the library does not hold it as a line pattern. The same choice and the
// same laying as cad::styleSampleDrawing makes for a style naming it, without
// a Model: this is a picture of the LIBRARY's definition, keyed on the
// library's generation, and a project's linetype of the same name is not.
[[nodiscard]] std::pair<katana::cad::StyleDrawing, bool>
linestylePicture(katana::cad::DefinitionCache& definitions,
                 const katana::entity::StyleLibrary& library, std::uint64_t generation,
                 std::string_view name)
{
    // Only a NON-vertex definition is a line pattern (D2, D8); resolveLinetype
    // with an empty model answers exactly that, and never ModelLinetype for a
    // name the empty model does not hold.
    static const katana::entity::Model noModel{};
    const katana::cad::ResolvedLinetype resolved =
        katana::cad::resolveLinetype(noModel, library, name);
    const auto flat = resolved.kind == katana::cad::LinetypeKind::LibraryDefinition
                          ? definitions.find(library, generation, name)
                          : nullptr;
    double width = 1.0;
    if (flat != nullptr) {
        const double period = flat->length > 0.0 ? flat->length : flat->naturalPeriod();
        width = std::max({kSamplePeriods * period, kSampleReaches * flat->reachAcross, width});
    }
    const katana::cad::StyleSamplePath sample = katana::cad::styleSamplePath(width);
    if (flat != nullptr) {
        // Laid at any size (no view scale): a picture this small would call
        // most patterns too fine, and the point of it is to show the pattern.
        katana::cad::LinestyleLayout laid =
            katana::cad::styleDrawing(*flat, sample.path, katana::cad::LinestyleOptions{});
        if (laid.outcome == katana::cad::LinestyleLayout::Outcome::Laid) {
            return {std::move(laid.drawing), false};
        }
    }
    katana::cad::StyleDrawing plain;
    plain.strokes.push_back(katana::cad::StyleStroke{sample.path, {}});
    return {std::move(plain), true};
}

} // namespace

DefinitionThumbnail paintDefinitionThumbnail(katana::cad::DefinitionCache& definitions,
                                             const katana::entity::StyleLibrary& library,
                                             std::uint64_t generation, ThumbnailKind kind,
                                             std::string_view name, QSize size, QColor ground)
{
    DefinitionThumbnail result;
    if (size.isEmpty()) {
        return result;
    }
    katana::cad::StyleDrawing drawing;
    if (kind == ThumbnailKind::Symbol) {
        result.standIn = katana::cad::resolveSymbol(library, name).kind ==
                         katana::cad::SymbolKind::BuiltInFallback;
        drawing = katana::cad::pointSymbolDrawing(definitions, library, generation, name,
                                                  Point2(0.0, 0.0), 0.0, 0.0, 1.0,
                                                  kBuiltInHalfWidth);
    } else {
        auto [laid, standIn] = linestylePicture(definitions, library, generation, name);
        drawing = std::move(laid);
        result.standIn = standIn;
    }

    result.image = QImage(size, QImage::Format_ARGB32_Premultiplied);
    result.image.fill(ground);
    if (drawing.empty()) {
        return result; // kNoSymbol, or an empty name: the ground alone
    }
    const bool paper = paperGround(ground);
    static const katana::cad::PlotSettings paperSettings{};
    QPainter painter(&result.image);
    painter.setRenderHint(QPainter::Antialiasing, true);
    painter.setRenderHint(QPainter::TextAntialiasing, true);
    StylePaintTarget target;
    target.view.resize(size.width(), size.height());
    // Fitted to what is PAINTED, texts measured in their font: the culling
    // extent over-estimates a label threefold and left a labelled symbol a
    // third of the picture.
    target.view.fit(paintedExtent(drawing, painter.font()), kMarginFraction);
    // A definition is painted with a flat cap, as the viewport paints it, so
    // a dash is its own length and not a pen width longer.
    target.entityPen = QPen(paper ? QColor(Qt::black) : QColor(224, 224, 224), kInkPixels);
    target.entityPen.setCapStyle(Qt::FlatCap);
    target.paper = paper ? &paperSettings : nullptr;
    paintStyleDrawing(painter, drawing, target);
    return result;
}

DefinitionThumbnail DefinitionThumbnails::thumbnail(const katana::entity::StyleLibrary& library,
                                                    std::uint64_t generation, ThumbnailKind kind,
                                                    std::string_view name, QSize size,
                                                    QColor ground)
{
    if (generation_ != generation) {
        images_.clear(); // every picture of the replaced library, at once
        generation_ = generation;
    }
    Key key{static_cast<int>(kind), std::string(name), size.width(), size.height(),
            static_cast<unsigned int>(ground.rgba())};
    if (const auto found = images_.find(key); found != images_.end()) {
        return found->second; // QImage is shared, not copied
    }
    if (images_.size() >= kMaximumEntries) {
        images_.clear();
    }
    DefinitionThumbnail painted =
        paintDefinitionThumbnail(definitions_, library, generation, kind, name, size, ground);
    images_.emplace(std::move(key), painted);
    return painted;
}

DefinitionThumbnail DefinitionThumbnails::thumbnail(const katana::cad::Document& document,
                                                    ThumbnailKind kind, std::string_view name,
                                                    QSize size, QColor ground)
{
    return thumbnail(document.styleLibrary(), document.libraryGeneration(), kind, name, size,
                     ground);
}

void DefinitionThumbnails::clear()
{
    images_.clear();
    generation_.reset();
    definitions_.clear();
}

} // namespace katana::qt
