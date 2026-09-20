#include "katana/render/framebuffer.hpp"

#include <algorithm>
#include <cmath>

namespace katana::render {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

// 64 megapixels. Far beyond any real display wall, and small enough that the
// colour plus depth allocation (512 MiB) still fails cleanly rather than
// taking the process out.
constexpr std::size_t kMaxPixels = 64u * 1024u * 1024u;

} // namespace

Rgba shade(Rgba color, double factor)
{
    const double f = std::clamp(factor, 0.0, 1.0);
    const auto channel = [f](std::uint8_t value) {
        // +0.5 then truncate: round-to-nearest without a library call, and
        // monotone in `value`, so a brighter input never shades darker.
        return static_cast<std::uint8_t>(static_cast<double>(value) * f + 0.5);
    };
    return (color & 0xFF00'0000u) | (static_cast<Rgba>(channel(redOf(color))) << 16) |
           (static_cast<Rgba>(channel(greenOf(color))) << 8) |
           static_cast<Rgba>(channel(blueOf(color)));
}

Result<Framebuffer> Framebuffer::create(int width, int height)
{
    Framebuffer framebuffer;
    if (auto status = framebuffer.resize(width, height); !status) {
        return status.error();
    }
    return framebuffer;
}

Status Framebuffer::resize(int width, int height)
{
    if (width <= 0 || height <= 0) {
        return makeError(ErrorCode::InvalidArgument, "framebuffer size must be positive");
    }
    const std::size_t pixels =
        static_cast<std::size_t>(width) * static_cast<std::size_t>(height);
    if (pixels > kMaxPixels) {
        return makeError(ErrorCode::InvalidArgument, "framebuffer is larger than 64 megapixels");
    }
    if (width == width_ && height == height_) {
        return {}; // the allocation is already the right size
    }

    width_ = width;
    height_ = height;
    tilesAcross_ = (width + kTileSize - 1) / kTileSize;
    tilesDown_ = (height + kTileSize - 1) / kTileSize;
    color_.assign(pixels, 0u);
    depth_.assign(pixels, 1.0f);
    return {};
}

TileRect Framebuffer::tile(std::size_t index) const
{
    if (index >= tileCount()) {
        return {};
    }
    const int tx = static_cast<int>(index % static_cast<std::size_t>(tilesAcross_));
    const int ty = static_cast<int>(index / static_cast<std::size_t>(tilesAcross_));
    TileRect rect;
    rect.x0 = tx * kTileSize;
    rect.y0 = ty * kTileSize;
    rect.x1 = std::min(rect.x0 + kTileSize, width_);
    rect.y1 = std::min(rect.y0 + kTileSize, height_);
    return rect;
}

void Framebuffer::clear(Rgba background)
{
    std::fill(color_.begin(), color_.end(), background);
    std::fill(depth_.begin(), depth_.end(), 1.0f);
}

void Framebuffer::clearTile(const TileRect& rect, Rgba background)
{
    for (int y = rect.y0; y < rect.y1; ++y) {
        const std::size_t row = index(0, y);
        std::fill(color_.begin() + static_cast<std::ptrdiff_t>(row + static_cast<std::size_t>(rect.x0)),
                  color_.begin() + static_cast<std::ptrdiff_t>(row + static_cast<std::size_t>(rect.x1)),
                  background);
        std::fill(depth_.begin() + static_cast<std::ptrdiff_t>(row + static_cast<std::size_t>(rect.x0)),
                  depth_.begin() + static_cast<std::ptrdiff_t>(row + static_cast<std::size_t>(rect.x1)),
                  1.0f);
    }
}

} // namespace katana::render
