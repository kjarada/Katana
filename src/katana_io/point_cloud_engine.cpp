#include "katana/pointcloud/point_cloud_engine.hpp"

#include <algorithm>
#include <limits>
#include <string>

#include "katana/core/text.hpp"
#include "proj_search_paths.hpp"

#include <pdal/io/BufferReader.hpp>
#include <pdal/Dimension.hpp>
#include <pdal/Options.hpp>
#include <pdal/PipelineManager.hpp>
#include <pdal/PointTable.hpp>
#include <pdal/PointView.hpp>
#include <pdal/StageFactory.hpp>
#include <pdal/pdal_features.hpp>

namespace katana::pointcloud {
namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

// PDAL reports failures by throwing. The rest of Katana reports them with
// Result (PLAN.MD section 36), so every entry point into PDAL is wrapped here
// and nothing throws across the interface.
template <typename Callable>
auto guarded(Callable&& callable, ErrorCode code, const char* what)
    -> decltype(callable())
{
    try {
        katana::io_detail::pointGdalAtProjData();
        return callable();
    } catch (const std::exception& error) {
        return makeError(code, what, error.what());
    } catch (...) {
        return makeError(code, what, "unknown PDAL failure");
    }
}

std::string inferDriver(const std::filesystem::path& path, bool forWriting)
{
    pdal::StageFactory factory;
    return forWriting ? pdal::StageFactory::inferWriterDriver(path.string())
                      : pdal::StageFactory::inferReaderDriver(path.string());
}

// ASPRS LAS 1.4 R15 stores X, Y and Z as 32-bit integers, each multiplied by
// the public header's scale factor and added to its offset, so the scale IS the
// resolution of every coordinate in the file. PDAL's writers default it to
// 0.01 - a centimetre - which quantised every point written here by up to 5 mm
// (audit IO-17). 0.001 is the millimetre survey coordinates are quoted to.
//
// With that resolution an int32 spans +/-2 147 km, less than a projected
// northing of 7 410 km, so the offset must move the origin to the data: "auto"
// has PDAL take it from the points' minimum.
constexpr const char* kLasScale = "0.001";
constexpr const char* kLasOffset = "auto";

void addMillimetreScale(pdal::Options& options)
{
    for (const char* axis : {"x", "y", "z"}) {
        options.add(std::string("scale_") + axis, kLasScale);
        options.add(std::string("offset_") + axis, kLasOffset);
    }
}

void growBounds(PointCloudBounds& bounds, double x, double y, double z)
{
    bounds.minX = std::min(bounds.minX, x);
    bounds.minY = std::min(bounds.minY, y);
    bounds.minZ = std::min(bounds.minZ, z);
    bounds.maxX = std::max(bounds.maxX, x);
    bounds.maxY = std::max(bounds.maxY, y);
    bounds.maxZ = std::max(bounds.maxZ, z);
}

PointCloudBounds invertedBounds()
{
    PointCloudBounds bounds;
    bounds.minX = bounds.minY = bounds.minZ = std::numeric_limits<double>::max();
    bounds.maxX = bounds.maxY = bounds.maxZ = std::numeric_limits<double>::lowest();
    return bounds;
}

} // namespace

std::uint32_t PointCloudEngine::decimationForBudget(std::uint64_t sourceCount,
                                                    std::uint64_t budget)
{
    if (budget == 0 || sourceCount <= budget) {
        return 1;
    }
    // Round up, so the result is genuinely at or under the budget rather than
    // just over it.
    const std::uint64_t step = (sourceCount + budget - 1) / budget;
    return static_cast<std::uint32_t>(
        std::min<std::uint64_t>(step, std::numeric_limits<std::uint32_t>::max()));
}

Result<PointCloudHeader> PointCloudEngine::readHeader(const std::filesystem::path& path) const
{
    std::error_code existsError;
    if (!std::filesystem::exists(path, existsError)) {
        return Result<PointCloudHeader>(
            makeError(ErrorCode::NotFound, "file does not exist", path.string()));
    }

    return guarded(
        [&]() -> Result<PointCloudHeader> {
            const std::string driver = inferDriver(path, false);
            if (driver.empty()) {
                return makeError(ErrorCode::Unsupported,
                                 "no PDAL reader is registered for this extension", path.string());
            }
            pdal::StageFactory factory;
            pdal::Stage* reader = factory.createStage(driver);
            if (reader == nullptr) {
                return makeError(ErrorCode::Unsupported, "PDAL could not create reader", driver);
            }
            pdal::Options options;
            options.add("filename", path.string());
            reader->setOptions(options);

            // preview() reads the header only, so this stays cheap no matter
            // how large the file is - which is what lets the importer size a
            // decimation step before committing to a read.
            const pdal::QuickInfo info = reader->preview();

            PointCloudHeader header;
            header.pointCount = info.m_pointCount;
            if (info.m_bounds.valid()) {
                header.bounds.minX = info.m_bounds.minx;
                header.bounds.minY = info.m_bounds.miny;
                header.bounds.minZ = info.m_bounds.minz;
                header.bounds.maxX = info.m_bounds.maxx;
                header.bounds.maxY = info.m_bounds.maxy;
                header.bounds.maxZ = info.m_bounds.maxz;
            }
            if (!info.m_srs.empty()) {
                header.projectionWkt = info.m_srs.getWKT();
            }
            header.hasColor =
                std::find(info.m_dimNames.begin(), info.m_dimNames.end(), "Red") !=
                info.m_dimNames.end();
            return header;
        },
        ErrorCode::FileImportFailure, "PDAL could not read the point-cloud header");
}

Result<PointCloud> PointCloudEngine::read(const std::filesystem::path& path,
                                          const PointCloudReadOptions& options) const
{
    if (options.decimationStep == 0) {
        return Result<PointCloud>(makeError(ErrorCode::InvalidArgument,
                                            "decimation step must be at least 1"));
    }
    std::error_code existsError;
    if (!std::filesystem::exists(path, existsError)) {
        return Result<PointCloud>(
            makeError(ErrorCode::NotFound, "file does not exist", path.string()));
    }

    return guarded(
        [&]() -> Result<PointCloud> {
            const std::string driver = inferDriver(path, false);
            if (driver.empty()) {
                return makeError(ErrorCode::Unsupported,
                                 "no PDAL reader is registered for this extension", path.string());
            }

            pdal::PipelineManager pipeline;
            pdal::Options readerOptions;
            if (options.resolution.has_value()) {
                // Only readers.copc understands a resolution. Any other reader
                // would ignore the option and return the whole file, which is
                // exactly the silent failure a caller asking for a coarse
                // level of detail cannot afford (PLAN.MD section 36).
                if (driver != "readers.copc") {
                    return makeError(ErrorCode::InvalidArgument,
                                     "a resolution can only be asked of a COPC file; convert it "
                                     "with convertToCopc first",
                                     path.string());
                }
                if (!(*options.resolution > 0.0)) {
                    return makeError(ErrorCode::InvalidArgument,
                                     "the resolution must be positive");
                }
                readerOptions.add("resolution", *options.resolution);
            }
            pdal::Stage* stage = &pipeline.makeReader(path.string(), driver, readerOptions);

            if (options.clip.has_value()) {
                const PointCloudBounds& box = *options.clip;
                pdal::Options cropOptions;
                cropOptions.add("bounds", "([" + std::to_string(box.minX) + "," +
                                              std::to_string(box.maxX) + "],[" +
                                              std::to_string(box.minY) + "," +
                                              std::to_string(box.maxY) + "],[" +
                                              std::to_string(box.minZ) + "," +
                                              std::to_string(box.maxZ) + "])");
                stage = &pipeline.makeFilter("filters.crop", *stage, cropOptions);
            }
            if (options.classification.has_value()) {
                const std::string value = std::to_string(*options.classification);
                pdal::Options rangeOptions;
                rangeOptions.add("limits", "Classification[" + value + ":" + value + "]");
                stage = &pipeline.makeFilter("filters.range", *stage, rangeOptions);
            }
            if (options.decimationStep > 1) {
                pdal::Options decimationOptions;
                decimationOptions.add("step", options.decimationStep);
                stage = &pipeline.makeFilter("filters.decimation", *stage, decimationOptions);
            }
            // `stage` is the tail of the pipeline; PipelineManager executes the
            // stage graph it has accumulated, so the tail is what gets run.
            (void)stage;

            pipeline.execute();

            PointCloud cloud;
            cloud.bounds = invertedBounds();
            bool truncated = false;

            for (const pdal::PointViewPtr& view : pipeline.views()) {
                if (view == nullptr) {
                    continue;
                }
                const pdal::PointLayoutPtr layout = view->layout();
                const bool hasColor = layout->hasDim(pdal::Dimension::Id::Red) &&
                                      layout->hasDim(pdal::Dimension::Id::Green) &&
                                      layout->hasDim(pdal::Dimension::Id::Blue);
                const bool hasIntensity = layout->hasDim(pdal::Dimension::Id::Intensity);
                const bool hasClassification =
                    layout->hasDim(pdal::Dimension::Id::Classification);

                for (pdal::PointId index = 0; index < view->size(); ++index) {
                    if (options.maxPoints != 0 && cloud.points.size() >= options.maxPoints) {
                        truncated = true;
                        break;
                    }
                    PointCloudPoint point;
                    point.x = view->getFieldAs<double>(pdal::Dimension::Id::X, index);
                    point.y = view->getFieldAs<double>(pdal::Dimension::Id::Y, index);
                    point.z = view->getFieldAs<double>(pdal::Dimension::Id::Z, index);
                    if (hasIntensity) {
                        point.intensity =
                            view->getFieldAs<double>(pdal::Dimension::Id::Intensity, index);
                    }
                    if (hasClassification) {
                        point.classification = view->getFieldAs<std::uint8_t>(
                            pdal::Dimension::Id::Classification, index);
                    }
                    if (hasColor) {
                        // LAS stores colour as 16-bit; scale to 8-bit for display.
                        const auto scale = [](std::uint16_t value) {
                            return static_cast<std::uint8_t>(value > 255 ? value >> 8 : value);
                        };
                        point.red =
                            scale(view->getFieldAs<std::uint16_t>(pdal::Dimension::Id::Red, index));
                        point.green = scale(
                            view->getFieldAs<std::uint16_t>(pdal::Dimension::Id::Green, index));
                        point.blue = scale(
                            view->getFieldAs<std::uint16_t>(pdal::Dimension::Id::Blue, index));
                        point.hasColor = true;
                    }
                    growBounds(cloud.bounds, point.x, point.y, point.z);
                    cloud.points.push_back(point);
                }
                if (truncated) {
                    break;
                }
                if (!view->spatialReference().empty() && cloud.projectionWkt.empty()) {
                    cloud.projectionWkt = view->spatialReference().getWKT();
                }
            }

            if (cloud.points.empty()) {
                cloud.bounds = PointCloudBounds{};
            }

            // Report what the FILE holds, not what survived filtering, so the
            // caller can say "2 000 000 of 418 000 000" honestly.
            auto header = readHeader(path);
            cloud.sourcePointCount = header.ok() ? header->pointCount : cloud.points.size();
            if (cloud.projectionWkt.empty() && header.ok()) {
                cloud.projectionWkt = header->projectionWkt;
            }
            return cloud;
        },
        ErrorCode::FileImportFailure, "PDAL could not read the point cloud");
}

Result<bool> PointCloudEngine::isCopc(const std::filesystem::path& path) const
{
    std::error_code existsError;
    if (!std::filesystem::exists(path, existsError)) {
        return Result<bool>(makeError(ErrorCode::NotFound, "file does not exist", path.string()));
    }
    if (inferDriver(path, false) != "readers.copc") {
        return false;
    }
    // A .copc.laz that is not actually COPC makes readers.copc throw on its
    // header. "Will PDAL read it as COPC" is the question, so that is false,
    // not an error.
    const auto probe = guarded(
        [&]() -> Status {
            pdal::StageFactory factory;
            pdal::Stage* reader = factory.createStage("readers.copc");
            if (reader == nullptr) {
                return makeError(ErrorCode::Unsupported, "PDAL could not create readers.copc");
            }
            pdal::Options options;
            options.add("filename", path.string());
            reader->setOptions(options);
            (void)reader->preview();
            return {};
        },
        ErrorCode::FileImportFailure, "not COPC");
    return probe.ok();
}

Status PointCloudEngine::convertToCopc(const std::filesystem::path& source,
                                       const std::filesystem::path& destination) const
{
    std::error_code existsError;
    if (!std::filesystem::exists(source, existsError)) {
        return makeError(ErrorCode::NotFound, "file does not exist", source.string());
    }
    // ASCII folding, whatever the process locale (core/text.hpp).
    const std::string name = katana::core::lowered(destination.filename().string());
    if (!name.ends_with(".copc.laz")) {
        // The extension is what makes every later read infer readers.copc;
        // a COPC file called .laz would be read as plain LAZ and could never
        // answer a resolution query.
        return makeError(ErrorCode::InvalidArgument,
                         "a COPC file must be named .copc.laz", destination.string());
    }
    const std::string driver = inferDriver(source, false);
    if (driver.empty()) {
        return makeError(ErrorCode::Unsupported,
                         "no PDAL reader is registered for this extension", source.string());
    }
    return guarded(
        [&]() -> Status {
            pdal::PipelineManager pipeline;
            pdal::Stage& reader = pipeline.makeReader(source.string(), driver);
            pdal::Options writerOptions;
            writerOptions.add("filename", destination.string());
            // The conversion must not coarsen the data it indexes (audit
            // IO-17): writers.copc, like writers.las, defaults to a 0.01
            // scale. From a LAS source the source's own scale and offset are
            // forwarded - a 0.1 mm scan stays 0.1 mm - with its header fields
            // and VLRs. Any other format has no LAS header to forward, so it
            // gets the millimetre, stated explicitly. Never both: an explicit
            // scale OVERRIDES a forwarded one (measured with PDAL 2.10.2: a
            // 0.0001-scale LAS converted with forward=all kept 0.0001, and
            // with forward=all plus scale_x=0.001 came out at 0.001).
            const bool lasSource = driver == "readers.las" || driver == "readers.copc";
            if (lasSource) {
                writerOptions.add("forward", "all");
            } else {
                addMillimetreScale(writerOptions);
            }
            pipeline.makeWriter(destination.string(), "writers.copc", reader, writerOptions);
            pipeline.execute();
            return {};
        },
        ErrorCode::FileExportFailure, "PDAL could not write the COPC file");
}

Status PointCloudEngine::write(const std::filesystem::path& path, const PointCloud& cloud) const
{
    if (cloud.points.empty()) {
        return makeError(ErrorCode::InvalidArgument, "refusing to write an empty point cloud",
                         path.string());
    }

    const std::string driver = inferDriver(path, true);
    if (driver.empty()) {
        return makeError(ErrorCode::Unsupported,
                         "no PDAL writer is registered for this extension", path.string());
    }

    return guarded(
        [&]() -> Status {
            // Build a PointView in memory and hand it straight to the writer.
            // The previous implementation staged the points through a temporary
            // CSV file, which lost precision in formatting, left the temporary
            // behind on some failure paths, and could not represent colour.
            pdal::PointTable table;
            pdal::PointLayoutPtr layout = table.layout();
            layout->registerDim(pdal::Dimension::Id::X);
            layout->registerDim(pdal::Dimension::Id::Y);
            layout->registerDim(pdal::Dimension::Id::Z);
            layout->registerDim(pdal::Dimension::Id::Intensity);
            layout->registerDim(pdal::Dimension::Id::Classification);

            const bool writeColor = std::any_of(
                cloud.points.begin(), cloud.points.end(),
                [](const PointCloudPoint& point) { return point.hasColor; });
            if (writeColor) {
                layout->registerDim(pdal::Dimension::Id::Red);
                layout->registerDim(pdal::Dimension::Id::Green);
                layout->registerDim(pdal::Dimension::Id::Blue);
            }

            pdal::PointViewPtr view(new pdal::PointView(table));
            pdal::PointId index = 0;
            for (const PointCloudPoint& point : cloud.points) {
                view->setField(pdal::Dimension::Id::X, index, point.x);
                view->setField(pdal::Dimension::Id::Y, index, point.y);
                view->setField(pdal::Dimension::Id::Z, index, point.z);
                view->setField(pdal::Dimension::Id::Intensity, index, point.intensity);
                view->setField(pdal::Dimension::Id::Classification, index, point.classification);
                if (writeColor) {
                    // Back to the 16-bit range LAS stores.
                    view->setField(pdal::Dimension::Id::Red, index,
                                   static_cast<std::uint16_t>(point.red) << 8);
                    view->setField(pdal::Dimension::Id::Green, index,
                                   static_cast<std::uint16_t>(point.green) << 8);
                    view->setField(pdal::Dimension::Id::Blue, index,
                                   static_cast<std::uint16_t>(point.blue) << 8);
                }
                ++index;
            }

            pdal::BufferReader reader;
            reader.addView(view);

            pdal::StageFactory factory;
            pdal::Stage* writer = factory.createStage(driver);
            if (writer == nullptr) {
                return makeError(ErrorCode::Unsupported, "PDAL could not create writer", driver);
            }
            pdal::Options writerOptions;
            writerOptions.add("filename", path.string());
            // Only the LAS family has a scale and an offset; any other writer
            // would refuse the options as unknown. writers.las compresses a
            // path ending .laz by its name alone (PDAL 2.10.2, although its
            // `compression` option defaults to false), which
            // ALazIsCompressedAndReadsBackAsTheSamePoints holds it to.
            if (driver == "writers.las" || driver == "writers.copc") {
                addMillimetreScale(writerOptions);
            }
            // PointView::setSpatialReference is private, so the CRS is declared
            // to the writer instead - which is also where it belongs, since it
            // is a property of the file being produced.
            if (!cloud.projectionWkt.empty()) {
                writerOptions.add("a_srs", cloud.projectionWkt);
            }
            writer->setOptions(writerOptions);
            writer->setInput(reader);
            writer->prepare(table);
            writer->execute(table);
            return Status{};
        },
        ErrorCode::FileExportFailure, "PDAL could not write the point cloud");
}

std::string pdalVersion()
{
    return pdal::pdalVersion;
}

} // namespace katana::pointcloud
