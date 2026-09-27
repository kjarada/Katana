#pragma once

// A raster's value at any point (docs/terrain.md, "Sampling and drape"):
// what RASTER SAMPLE reports, DRAPE sets on the drawing's points and
// vertices, and LOS walks along a sight line. GDAL interpolates
// (GDALRasterInterpolateAtPoint), so the value between cell centres is
// GDAL's bilinear - or nearest, cubic, cubic spline - and not a second
// interpolation of Katana's.
//
// Absent is not zero: a point off the raster, or one whose interpolation
// window touches a no-data cell, has no value, and the caller says so
// rather than reading 0.
//
// A sampler holds its own open dataset and is used on one thread: the job's
// worker opens it and drops it (never a dataset shared between runs,
// docs/geoprocessing.md "Risks").

#include <filesystem>
#include <memory>
#include <optional>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"
#include "katana/gis/gdal_adapter.hpp"

namespace katana::gis {

enum class Resampling { Nearest, Bilinear, Cubic, CubicSpline };

// "nearest", "bilinear", "cubic", "cubicspline" (any case); InvalidArgument
// naming the word and the choices otherwise.
[[nodiscard]] katana::core::Result<Resampling> resamplingNamed(std::string_view name);
[[nodiscard]] const char* toString(Resampling resampling);

class RasterSampler {
  public:
    // Band `band` (1-based) of the raster at `path` - a file, a /vsi path, a
    // URL - read at full precision. NotFound for a local file that is not
    // there; FileImportFailure with GDAL's message when GDAL cannot open it
    // as a raster; InvalidArgument for a band it does not have, or a raster
    // with no georeferencing (its cells have no place on the ground).
    [[nodiscard]] static katana::core::Result<std::unique_ptr<RasterSampler>>
    open(const std::filesystem::path& path, int band = 1);

    ~RasterSampler();
    RasterSampler(const RasterSampler&) = delete;
    RasterSampler& operator=(const RasterSampler&) = delete;

    // The value at the ground point (x, y) by `resampling`; none off the
    // raster, on or beside a no-data cell (GDAL refuses an interpolation
    // window that touches one), or where GDAL gives no finite number.
    [[nodiscard]] std::optional<double> at(double x, double y, Resampling resampling) const;

    [[nodiscard]] const RasterInfo& info() const { return info_; }
    // The longer side of a cell, in ground units: what a sight line steps by.
    [[nodiscard]] double cellSize() const;

  private:
    RasterSampler() = default;

    void* dataset_ = nullptr;
    void* band_ = nullptr;
    RasterInfo info_;
    // The inverse of info_.geotransform: ground to pixel and line.
    double inverse_[6] = {0, 1, 0, 0, 0, 1};
};

} // namespace katana::gis
