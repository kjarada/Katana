#pragma once

// Small pictures of 12d definitions for the pickers and browsers: the symbol
// library, a style's symbol and linestyle choosers, the survey-code manager.
//
// Every picture is painted through the one style painter (style_painter.hpp)
// from the one resolver (cad/style_resolver.hpp), so a thumbnail shows what
// the viewport would draw for the same name - library definition first, the
// built-in shape a symbol name falls back to, the plain line a linestyle name
// falls back to - and says when it is such a stand-in, so a picker can mark a
// name the library does not define rather than drop it (decision D3).
//
// Pictures are cached by (kind, name, pixel size, ground colour, library
// generation). The generation, never a LineStyle*, is what keeps a picture
// honest: Document::setStyleLibrary moves it, and the first request after
// that drops every picture of the old library.
//
// GUI thread only. A QImage could be painted elsewhere, but its fonts and a
// Document may not be reached off the GUI thread, and a picker asks for a few
// dozen pictures at a time - not enough to be worth a farm.

#include <cstddef>
#include <cstdint>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <tuple>

#include <QColor>
#include <QImage>
#include <QSize>

#include "katana/cad/style_resolver.hpp"
#include "katana/entity/style_library.hpp"

namespace katana::cad {
class Document;
}

namespace katana::qt {

enum class ThumbnailKind {
    // The name drawn as a point symbol, centred.
    Symbol,
    // The name drawn as a line's linetype along cad::styleSamplePath.
    Linestyle,
};

struct DefinitionThumbnail {
    QImage image{};
    // What was painted is NOT a library definition of the name: a symbol
    // name's built-in fallback shape, or the plain line a linestyle name the
    // library does not hold (or holds only as a vertex symbol) is drawn as.
    // False for a built-in shape asked for by its own name.
    bool standIn = false;
};

class DefinitionThumbnails {
  public:
    // At most this many pictures are kept; past it the cache starts again.
    // A picker of the reference libraries at two sizes is about 1,600.
    static constexpr std::size_t kMaximumEntries = 4096;

    // A picture `size` device pixels across, on `ground`. The ink follows the
    // ground: on a light ground the picture is "on paper" - the entity pen is
    // black and a white 12d pen prints black (cad::paperColour, decision D7) -
    // and on a dark ground it is the screen, with a light entity pen. An
    // empty size gives a null image.
    [[nodiscard]] DefinitionThumbnail thumbnail(const katana::entity::StyleLibrary& library,
                                                std::uint64_t generation, ThumbnailKind kind,
                                                std::string_view name, QSize size,
                                                QColor ground);
    [[nodiscard]] DefinitionThumbnail thumbnail(const katana::cad::Document& document,
                                                ThumbnailKind kind, std::string_view name,
                                                QSize size, QColor ground);

    [[nodiscard]] std::size_t size() const { return images_.size(); }
    void clear();

  private:
    // (kind, name, width, height, ground as ARGB). The generation is not in
    // the key: pictures of one generation only are ever held.
    using Key = std::tuple<int, std::string, int, int, unsigned int>;
    std::map<Key, DefinitionThumbnail, std::less<>> images_{};
    std::optional<std::uint64_t> generation_{};
    katana::cad::DefinitionCache definitions_{};
};

// The picture itself, uncached: what DefinitionThumbnails::thumbnail keeps.
[[nodiscard]] DefinitionThumbnail paintDefinitionThumbnail(
    katana::cad::DefinitionCache& definitions, const katana::entity::StyleLibrary& library,
    std::uint64_t generation, ThumbnailKind kind, std::string_view name, QSize size,
    QColor ground);

} // namespace katana::qt
