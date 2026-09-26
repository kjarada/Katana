// What the terrain analysis verbs of T4 and T5 share (analysis_support.hpp).

#include "analysis_support.hpp"

#include <algorithm>
#include <cctype>
#include <filesystem>
#include <variant>

#include "katana/core/text.hpp"
#include "replies.hpp"

namespace katana::app::geo::analysis {

namespace gp = katana::gis::processing;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Point2;

namespace {

std::string upper(std::string_view text)
{
    std::string out(text);
    for (char& c : out) {
        c = static_cast<char>(std::toupper(static_cast<unsigned char>(c)));
    }
    return out;
}

std::filesystem::path pathOf(const std::string& utf8)
{
    return std::filesystem::path(std::u8string(utf8.begin(), utf8.end()));
}

} // namespace

Result<Ground> bindGround(Context& context, const Tokens& tokens, std::size_t& at,
                          std::string_view verb)
{
    if (at >= tokens.size() ||
        (!tokens.is(at, "RASTER") && !tokens.is(at, "SURFACE") && !tokens.is(at, "FILE"))) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string(verb) +
                             " reads heights from a SURFACE <name>, a RASTER <id|name> or a FILE "
                             "<path>; a scope of the drawing comes after it",
                         at < tokens.size() ? tokens[at] : std::string());
    }
    auto source = parseSource(tokens, at);
    if (!source) {
        return source.error();
    }
    Ground ground;
    ground.source = std::move(source).value();
    if (ground.source.kind == Source::Kind::Surface) {
        if (ground.source.cell) {
            return makeError(ErrorCode::InvalidArgument,
                             "a surface is read on its own triangles; CELL is for a grid of it",
                             "CELL");
        }
        const katana::terrain::NamedSurface* named = context.surfaces.find(ground.source.surface);
        if (named == nullptr) {
            return makeError(ErrorCode::NotFound, "no surface has that name; SURFACE LIST names them",
                             ground.source.surface);
        }
        ground.surface = named->surface;
        ground.record = "input arg=input source=surface name=" + value(named->name) +
                        " read=triangles";
        return ground;
    }
    // A raster's or a file's path, which the bindings read from the
    // reference raster (its source file, never its display copy): nothing
    // is opened here.
    auto deferred = bindRaster(context, ground.source);
    if (!deferred) {
        return deferred.error();
    }
    auto dataset = (*deferred)();
    if (!dataset) {
        return dataset.error();
    }
    const auto* file = std::get_if<gp::DatasetPath>(&*dataset);
    if (file == nullptr) {
        return makeError(ErrorCode::Unsupported, "the raster has no file to sample",
                         ground.source.raster);
    }
    ground.path = file->path;
    ground.record = inputRecord("input", ground.source);
    return ground;
}

Result<OpenGround> openGround(const Ground& ground, katana::gis::Resampling method)
{
    OpenGround open;
    if (ground.surface) {
        const std::shared_ptr<const katana::terrain::TinSurface> surface = ground.surface;
        open.at = [surface](const Point2& at) { return surface->elevationAt(at); };
        return open;
    }
    auto sampler = katana::gis::RasterSampler::open(pathOf(ground.path));
    if (!sampler) {
        return sampler.error();
    }
    // Shared, so the callback can be copied; still used on this one thread.
    const std::shared_ptr<const katana::gis::RasterSampler> shared = std::move(sampler).value();
    open.spacing = shared->cellSize() / 2.0;
    open.at = [shared, method](const Point2& at) { return shared->at(at.x, at.y, method); };
    return open;
}

std::optional<Point2> pointOf(std::string_view text)
{
    const std::size_t comma = text.find(',');
    if (comma == std::string_view::npos || text.find(',', comma + 1) != std::string_view::npos) {
        return std::nullopt;
    }
    const auto x = katana::core::parseFiniteDouble(katana::core::trimmed(text.substr(0, comma)));
    const auto y = katana::core::parseFiniteDouble(katana::core::trimmed(text.substr(comma + 1)));
    if (!x || !y) {
        return std::nullopt;
    }
    return Point2(*x, *y);
}

std::string pointText(const Point2& point)
{
    return katana::core::formatExactReal(point.x) + "," + katana::core::formatExactReal(point.y);
}

Result<KeywordValues> takeKeywordValues(const Tokens& tokens, std::size_t begin,
                                        const std::vector<std::string>& keywords)
{
    KeywordValues out;
    for (std::size_t i = begin; i < tokens.size(); ++i) {
        const std::string word = tokens.quoted[i] ? std::string() : upper(tokens[i]);
        if (!word.empty() && std::ranges::find(keywords, word) != keywords.end()) {
            if (i + 1 >= tokens.size()) {
                return makeError(ErrorCode::InvalidArgument, word + " needs a value after it",
                                 tokens[i]);
            }
            out.taken.emplace_back(word, tokens[i + 1]);
            ++i;
            continue;
        }
        out.rest.words.push_back(tokens.words[i]);
        out.rest.quoted.push_back(tokens.quoted[i]);
    }
    return out;
}

} // namespace katana::app::geo::analysis
