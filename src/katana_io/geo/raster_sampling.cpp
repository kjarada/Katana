// A raster's value at any point (raster_sampling.hpp).

#include "katana/gis/raster_sampling.hpp"

#include <algorithm>
#include <cmath>
#include <string>
#include <system_error>

#include <cpl_error.h>
#include <gdal.h>

#include "gdal_registry.hpp"
#include "katana/core/text.hpp"
#include "katana/gis/formats.hpp"

namespace katana::gis {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

std::string utf8Of(const std::filesystem::path& path)
{
    const std::u8string text = path.generic_u8string();
    return std::string(text.begin(), text.end());
}

GDALRIOResampleAlg gdalResampling(Resampling resampling)
{
    switch (resampling) {
    case Resampling::Nearest:
        return GRIORA_NearestNeighbour;
    case Resampling::Bilinear:
        return GRIORA_Bilinear;
    case Resampling::Cubic:
        return GRIORA_Cubic;
    case Resampling::CubicSpline:
        return GRIORA_CubicSpline;
    }
    return GRIORA_Bilinear;
}

} // namespace

Result<Resampling> resamplingNamed(std::string_view name)
{
    const std::string word = katana::core::lowered(name);
    if (word == "nearest") {
        return Resampling::Nearest;
    }
    if (word == "bilinear") {
        return Resampling::Bilinear;
    }
    if (word == "cubic") {
        return Resampling::Cubic;
    }
    if (word == "cubicspline") {
        return Resampling::CubicSpline;
    }
    return makeError(ErrorCode::InvalidArgument,
                     "method is bilinear, nearest, cubic or cubicspline", std::string(name));
}

const char* toString(Resampling resampling)
{
    switch (resampling) {
    case Resampling::Nearest:
        return "nearest";
    case Resampling::Bilinear:
        return "bilinear";
    case Resampling::Cubic:
        return "cubic";
    case Resampling::CubicSpline:
        return "cubicspline";
    }
    return "bilinear";
}

Result<std::unique_ptr<RasterSampler>> RasterSampler::open(const std::filesystem::path& path,
                                                           int band)
{
    detail::ensureGdalRegistered();
    std::error_code existsError;
    if (!isVirtualPath(path) && !std::filesystem::exists(path, existsError)) {
        return makeError(ErrorCode::NotFound, "file does not exist", utf8Of(path));
    }
    CPLErrorReset();
    GDALDatasetH dataset =
        GDALOpenEx(utf8Of(path).c_str(), GDAL_OF_READONLY | GDAL_OF_RASTER, nullptr, nullptr, nullptr);
    if (dataset == nullptr) {
        return makeError(ErrorCode::FileImportFailure,
                         "GDAL could not open '" + utf8Of(path) + "' as a raster",
                         CPLGetLastErrorMsg());
    }
    std::unique_ptr<RasterSampler> sampler(new RasterSampler());
    sampler->dataset_ = dataset;
    if (band < 1 || band > GDALGetRasterCount(dataset)) {
        return makeError(ErrorCode::InvalidArgument, "the raster has no such band",
                         std::to_string(band));
    }
    sampler->band_ = GDALGetRasterBand(dataset, band);
    RasterInfo& info = sampler->info_;
    info.width = GDALGetRasterXSize(dataset);
    info.height = GDALGetRasterYSize(dataset);
    info.bandCount = GDALGetRasterCount(dataset);
    if (const char* wkt = GDALGetProjectionRef(dataset); wkt != nullptr) {
        info.projectionWkt = wkt;
    }
    info.hasGeotransform = GDALGetGeoTransform(dataset, info.geotransform.data()) == CE_None;
    int hasNoData = 0;
    const double noData = GDALGetRasterNoDataValue(sampler->band_, &hasNoData);
    if (hasNoData != 0) {
        info.noDataValue = noData;
    }
    // A raster without a geotransform is placed by GDAL's default at the
    // origin, a unit per pixel: sampled so, a height would come from a place
    // on the ground the raster never described.
    if (!info.hasGeotransform) {
        return makeError(ErrorCode::InvalidArgument,
                         "the raster has no georeferencing, so no cell has a place on the ground",
                         utf8Of(path));
    }
    if (GDALInvGeoTransform(info.geotransform.data(), sampler->inverse_) == 0) {
        return makeError(ErrorCode::InvalidArgument, "the raster's geotransform cannot be inverted",
                         utf8Of(path));
    }
    return sampler;
}

RasterSampler::~RasterSampler()
{
    if (dataset_ != nullptr) {
        GDALClose(static_cast<GDALDatasetH>(dataset_));
    }
}

std::optional<double> RasterSampler::at(double x, double y, Resampling resampling) const
{
    if (!std::isfinite(x) || !std::isfinite(y)) {
        return std::nullopt;
    }
    const double pixel = inverse_[0] + x * inverse_[1] + y * inverse_[2];
    const double line = inverse_[3] + x * inverse_[4] + y * inverse_[5];
    // Off the raster, asked here rather than of GDAL: its refusal would
    // raise an error per point of a line that runs off the data.
    if (!(pixel >= 0.0 && line >= 0.0 && pixel <= info_.width && line <= info_.height)) {
        return std::nullopt;
    }
    double real = 0.0;
    double imaginary = 0.0;
    if (GDALRasterInterpolateAtPoint(static_cast<GDALRasterBandH>(band_), pixel, line,
                                     gdalResampling(resampling), &real,
                                     &imaginary) != CE_None) {
        return std::nullopt;
    }
    // GDAL refuses a bilinear or cubic window that touches no-data; nearest
    // hands the cell back, which may be the no-data value itself.
    if (!std::isfinite(real) || (info_.noDataValue && real == *info_.noDataValue)) {
        return std::nullopt;
    }
    return real;
}

double RasterSampler::cellSize() const
{
    const auto& gt = info_.geotransform;
    return std::max(std::hypot(gt[1], gt[4]), std::hypot(gt[2], gt[5]));
}

} // namespace katana::gis
