#pragma once

// Colour and depth target for the software renderer (PLAN.MD Phase 15).
//
// LAYOUT. Pixels are row-major and contiguous, not swizzled into tile-local
// blocks. A swizzled layout gives a rasteriser marginally better locality, but
// every frame here ends up in a QImage, and row-major is the one layout that
// can be handed over without a copy. A 64x64 tile still touches only 64 rows of
// a single buffer, so the locality that matters - staying inside L2 while a
// tile is being shaded - is preserved either way. Measure before changing this.
//
// COLOUR is 0xAARRGGBB packed in a std::uint32_t, which is byte-for-byte
// QImage::Format_ARGB32 on a little-endian machine.
//
// DEPTH is normalised [0, 1] and REVERSED: 1 at the near plane, 0 at the far
// (camera.hpp says why: it keeps a float's precision where the perspective
// divide needs it). Larger is nearer and the buffer is cleared to 0; the test
// is strictly-greater, so of two primitives at exactly the same depth the
// first rasterised wins, which combined with a fixed binning order is what
// makes a frame reproducible (Rule 7).

#include <cstddef>
#include <cstdint>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::render {

using Rgba = std::uint32_t;

[[nodiscard]] constexpr Rgba rgba(std::uint8_t r, std::uint8_t g, std::uint8_t b,
                                  std::uint8_t a = 255)
{
    return (static_cast<Rgba>(a) << 24) | (static_cast<Rgba>(r) << 16) |
           (static_cast<Rgba>(g) << 8) | static_cast<Rgba>(b);
}

[[nodiscard]] constexpr std::uint8_t redOf(Rgba c) { return static_cast<std::uint8_t>(c >> 16); }
[[nodiscard]] constexpr std::uint8_t greenOf(Rgba c) { return static_cast<std::uint8_t>(c >> 8); }
[[nodiscard]] constexpr std::uint8_t blueOf(Rgba c) { return static_cast<std::uint8_t>(c); }
[[nodiscard]] constexpr std::uint8_t alphaOf(Rgba c) { return static_cast<std::uint8_t>(c >> 24); }

// Scales the three colour channels by `factor` (clamped to [0, 1] by the
// caller's lighting), leaving alpha alone. Used to bake flat shading into
// vertex colours before anything reaches the rasteriser.
[[nodiscard]] Rgba shade(Rgba color, double factor);

// A rectangle of pixels, in pixel coordinates with the origin at the top-left.
struct TileRect {
    int x0 = 0;
    int y0 = 0;
    int x1 = 0; // exclusive
    int y1 = 0; // exclusive

    [[nodiscard]] constexpr int width() const { return x1 - x0; }
    [[nodiscard]] constexpr int height() const { return y1 - y0; }
    [[nodiscard]] constexpr bool empty() const { return x1 <= x0 || y1 <= y0; }
};

class Framebuffer {
  public:
    // Tiles are 64x64: 4096 pixels of colour (16 KiB) plus depth (16 KiB) fits
    // in a typical 256 KiB L2 many times over, leaving room for the primitive
    // list and the vertex data a tile touches.
    static constexpr int kTileSize = 64;

    Framebuffer() = default;

    // Fails with InvalidArgument for a non-positive or absurd size. The cap is
    // deliberately generous (64 megapixels) and exists so a corrupt widget size
    // cannot ask for an allocation that takes the process down.
    [[nodiscard]] static katana::core::Result<Framebuffer> create(int width, int height);

    // Reuses the existing allocation when the size is unchanged.
    [[nodiscard]] katana::core::Status resize(int width, int height);

    [[nodiscard]] int width() const { return width_; }
    [[nodiscard]] int height() const { return height_; }
    [[nodiscard]] bool empty() const { return width_ <= 0 || height_ <= 0; }

    [[nodiscard]] int tilesAcross() const { return tilesAcross_; }
    [[nodiscard]] int tilesDown() const { return tilesDown_; }
    [[nodiscard]] std::size_t tileCount() const
    {
        return static_cast<std::size_t>(tilesAcross_) * static_cast<std::size_t>(tilesDown_);
    }
    // Clipped to the framebuffer, so the right and bottom tiles are partial.
    [[nodiscard]] TileRect tile(std::size_t index) const;

    [[nodiscard]] const std::vector<Rgba>& color() const { return color_; }
    [[nodiscard]] std::vector<Rgba>& color() { return color_; }
    [[nodiscard]] const std::vector<float>& depth() const { return depth_; }
    [[nodiscard]] std::vector<float>& depth() { return depth_; }

    [[nodiscard]] Rgba colorAt(int x, int y) const { return color_[index(x, y)]; }
    [[nodiscard]] float depthAt(int x, int y) const { return depth_[index(x, y)]; }

    // Fills colour with `background` and depth with 0 (the far plane).
    void clear(Rgba background);
    // Same, but only inside `rect`. Used when a tile is rendered on its own.
    void clearTile(const TileRect& rect, Rgba background);

  private:
    [[nodiscard]] std::size_t index(int x, int y) const
    {
        return static_cast<std::size_t>(y) * static_cast<std::size_t>(width_) +
               static_cast<std::size_t>(x);
    }

    int width_ = 0;
    int height_ = 0;
    int tilesAcross_ = 0;
    int tilesDown_ = 0;
    std::vector<Rgba> color_;
    std::vector<float> depth_;
};

} // namespace katana::render
