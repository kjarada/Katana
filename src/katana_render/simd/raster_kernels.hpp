#pragma once

// Entry points of the AVX2 rasteriser kernels, compiled from raster_avx2.cpp.
// Declarations only; src/katana_core/simd/text_kernels.hpp says why they have C
// linkage and a katana_avx2_ prefix.
//
// Each entry gives exactly the bits of its scalar reference in rasterizer.cpp
// (transformVertex and shadePixel): every lane does the reference's operations
// in the reference's order, and nothing is fused.

#include <cstddef>
#include <cstdint>

extern "C" {

// Stage 1 for `count` vertices. `positions` holds x, y, z doubles per vertex;
// `mvp` is a row-major 4x4 matrix of doubles.
//
// For vertex i, clip[5 i .. 5 i + 3] receive the float clip coordinates
// x, y, z, w - the matrix applied in double as math::Mat4::multiply does,
// (((m0 x + m1 y) + m2 z) + m3), then rounded to float - and clip[5 i + 4] the
// colour word colors[i] (white, 0xFFFFFFFF, from i = colorCount on).
// codes[i] has bit p set when the vertex is outside clip plane p (the planes of
// rasterizer.cpp, judged in double from the float coordinates; a NaN distance
// is outside). screen[5 i .. 5 i + 3] receive the projected x, y, depth and 1/w
// in float for a viewport of `width` x `height` pixels, and screen[5 i + 4] the
// colour word - meaningful only where the code is 0, but written for every
// vertex. Both output arrays are 5 words per vertex (the layouts of
// Rasterizer::ClipVertex and its projected twin).
void katana_avx2_transform_vertices(const double* mvp, const double* positions,
                                    std::size_t count, const std::uint32_t* colors,
                                    std::size_t colorCount, float width, float height,
                                    float* clip, std::uint8_t* codes, float* screen);

// Stage 3 for one screen triangle over some rows of one tile. `triangle` is
// x[3], y[3], z[3], invW[3] as floats followed by color[3] as 32-bit words and
// the depth bias, the layout of Rasterizer::ScreenTriangle. `invArea` is 1
// over its signed doubled area, computed as the reference computes it.
//
// Row firstRow + r covers the pixels spans[2 r] .. spans[2 r + 1] inclusive
// (an empty row has first > last). Each pixel is shaded exactly as the scalar
// reference shades it; where `covered` is non-zero every pixel of every span
// is known to be inside the triangle, and the three edge tests are skipped
// (they would all pass). Colour and, when depthWrite is non-zero, depth are
// written through the row-major buffers of `stride` pixels a row. Returns how
// many pixels were written.
std::size_t katana_avx2_shade_rows(const float* triangle, float invArea, const int* spans,
                                   int firstRow, int rowCount, int covered, int depthWrite,
                                   std::uint32_t* color, float* depth, std::size_t stride);

} // extern "C"
