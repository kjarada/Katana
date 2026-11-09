// PDAL's primitives behind a C ABI, for the pointcloud layer's Rust
// (src/ffi.rs declares this file's functions; src/point_cloud_engine.rs calls
// them). PDAL has only a C++ interface, so something C++ must stand between
// it and Rust; this is all of it (docs/rust.md, "Third-party libraries").
//
// The shim decides nothing. Every choice src/katana_io/point_cloud_engine.cpp
// makes - which stages, with which options and their text, how many points,
// how a colour is scaled, the error codes and messages - is made in Rust and
// handed down: a pipeline is a list of stages, each a driver name with
// options as name and value text; the points of each view are read into a
// plain struct in whatever ranges the Rust asks for; a write takes such
// structs. What PDAL is asked, and in what order, is what the C++ engine asks.
//
// Errors. PDAL reports failures by throwing, and nothing may unwind into
// Rust: every function that can fail catches every exception and returns a
// code, with the exception's what() copied into *message (pdal::pdal_error is
// a std::exception). A copy is malloc'd and handed to Rust, which gives it
// back to katana_pdal_free. The shim calls no Rust.
//
// Threads. No lock here: PDAL guards its process-wide registries itself
// (PluginManager's and StageExtensions' mutexes; StageFactory documents stage
// creation as thread-safe), each call builds and owns its own stages, and the
// C++ engine is run from the window's job threads with no lock either.

#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <memory>
#include <new>
#include <stdexcept>
#include <string>
#include <vector>

#include <pdal/Dimension.hpp>
#include <pdal/Options.hpp>
#include <pdal/PipelineManager.hpp>
#include <pdal/PointTable.hpp>
#include <pdal/PointView.hpp>
#include <pdal/StageFactory.hpp>
#include <pdal/io/BufferReader.hpp>
#include <pdal/pdal_features.hpp>

// ---- the C ABI's types (src/ffi.rs declares the same, field for field) ---------------------

extern "C" {

struct katana_pdal_option {
    const char* name;
    const char* value;
};

// A stage of a pipeline: a reader first, then filters, then at most a writer,
// each taking the stage before it as its input.
struct katana_pdal_stage {
    int role;             // 0 reader, 1 filter, 2 writer
    const char* driver;   // "readers.las", "filters.crop", ...
    const char* filename; // the reader's input, the writer's output; unread for a filter
    const katana_pdal_option* options;
    std::size_t option_count;
};

// What Stage::preview() reports of a file before any point is read.
struct katana_pdal_quick_info {
    std::uint64_t point_count;
    double min_x;
    double min_y;
    double min_z;
    double max_x;
    double max_y;
    double max_z;
    int bounds_valid;
    int has_red; // a dimension named "Red" is listed
};

// One view of a pipeline that has run: its size and the dimensions its
// layout has. is_null for a view PDAL gave as a null pointer.
struct katana_pdal_view_info {
    std::uint64_t size;
    int is_null;
    int has_red;
    int has_green;
    int has_blue;
    int has_intensity;
    int has_classification;
};

// One point as PointView::getFieldAs reads it and setField writes it: the
// colour as LAS's 16 bits; a field the layout lacks is zero.
struct katana_pdal_point {
    double x;
    double y;
    double z;
    double intensity;
    std::uint16_t red;
    std::uint16_t green;
    std::uint16_t blue;
    std::uint8_t classification;
};

} // extern "C"

// A pipeline that has run, kept for its views until Rust frees it.
struct katana_pdal_pipeline {
    pdal::PipelineManager manager;
    std::vector<pdal::PointViewPtr> views; // in the order PDAL's view set iterates them
};

// The layouts src/ffi.rs asserts too.
static_assert(sizeof(katana_pdal_option) == 2 * sizeof(void*), "katana_pdal_option");
static_assert(sizeof(void*) != 8 || sizeof(katana_pdal_stage) == 40, "katana_pdal_stage");
static_assert(sizeof(void*) != 8 || offsetof(katana_pdal_stage, driver) == 8, "katana_pdal_stage");
static_assert(sizeof(katana_pdal_quick_info) == 64, "katana_pdal_quick_info");
static_assert(offsetof(katana_pdal_quick_info, bounds_valid) == 56, "katana_pdal_quick_info");
static_assert(sizeof(katana_pdal_view_info) == 32, "katana_pdal_view_info");
static_assert(offsetof(katana_pdal_view_info, is_null) == 8, "katana_pdal_view_info");
static_assert(sizeof(katana_pdal_point) == 40, "katana_pdal_point");
static_assert(offsetof(katana_pdal_point, red) == 32, "katana_pdal_point");
static_assert(offsetof(katana_pdal_point, classification) == 38, "katana_pdal_point");
static_assert(sizeof(int) == 4, "the C ABI's int is Rust's c_int, 32 bits");

namespace {

// The codes every fallible function returns.
constexpr int kOk = 0;
constexpr int kNoStage = 1;       // the factory made no stage of that driver
constexpr int kThrew = 2;         // PDAL threw a std::exception; *message holds its what()
constexpr int kThrewUnknown = 3;  // PDAL threw something else; *message is NULL

constexpr int kReader = 0;
constexpr int kFilter = 1;
constexpr int kWriter = 2;

// A copy Rust owns, NUL-terminated; std::bad_alloc when there is no memory,
// which the caller's catch reports like any other failure.
char* copied(const std::string& text)
{
    char* out = static_cast<char*>(std::malloc(text.size() + 1));
    if (out == nullptr) {
        throw std::bad_alloc();
    }
    std::memcpy(out, text.data(), text.size());
    out[text.size()] = '\0';
    return out;
}

// A failure's message, which never throws: NULL when even that cannot be
// allocated.
int threw(char** message, const char* what) noexcept
{
    const std::size_t size = std::strlen(what);
    char* out = static_cast<char*>(std::malloc(size + 1));
    if (out != nullptr) {
        std::memcpy(out, what, size + 1);
    }
    *message = out;
    return kThrew;
}

// `body` with every exception caught: its code, or a failure's.
template <typename Body>
int guarded(char** message, Body&& body) noexcept
{
    *message = nullptr;
    try {
        return body();
    } catch (const std::exception& error) {
        return threw(message, error.what());
    } catch (...) {
        return kThrewUnknown;
    }
}

pdal::Options optionsOf(const katana_pdal_option* options, std::size_t count)
{
    pdal::Options out;
    for (std::size_t i = 0; i < count; ++i) {
        out.add(std::string(options[i].name), std::string(options[i].value));
    }
    return out;
}

const pdal::PointViewPtr& viewOf(const katana_pdal_pipeline* pipeline, std::size_t index)
{
    return pipeline->views.at(index);
}

} // namespace

extern "C" {

void katana_pdal_free(void* memory) noexcept
{
    std::free(memory);
}

// pdal::pdalVersion, static storage the caller never frees. Nothing to fail.
const char* katana_pdal_version() noexcept
{
    return pdal::pdalVersion.c_str();
}

// The driver PDAL infers from a file name, empty for none, as the C++
// engine's inferDriver asks: a factory made first, then the static inference.
int katana_pdal_infer_driver(const char* filename, int for_writing, char** driver,
                             char** message) noexcept
{
    *driver = nullptr;
    return guarded(message, [&]() -> int {
        pdal::StageFactory factory;
        (void)factory;
        const std::string inferred = for_writing != 0
                                         ? pdal::StageFactory::inferWriterDriver(filename)
                                         : pdal::StageFactory::inferReaderDriver(filename);
        *driver = copied(inferred);
        return kOk;
    });
}

// A stage of `driver` made by a factory, given `options`, and asked for its
// preview(). With `details`, what the preview reported, in the order the C++
// engine's readHeader reads it; without, the preview alone (isCopc's probe).
// *srs_wkt is the spatial reference's WKT when it has one, else empty.
int katana_pdal_preview(const char* driver, const katana_pdal_option* options,
                        std::size_t option_count, int details, katana_pdal_quick_info* info,
                        char** srs_wkt, char** message) noexcept
{
    *info = katana_pdal_quick_info{};
    *srs_wkt = nullptr;
    return guarded(message, [&]() -> int {
        pdal::StageFactory factory;
        pdal::Stage* reader = factory.createStage(driver);
        if (reader == nullptr) {
            return kNoStage;
        }
        reader->setOptions(optionsOf(options, option_count));
        const pdal::QuickInfo quick = reader->preview();
        if (details == 0) {
            return kOk;
        }
        info->point_count = quick.m_pointCount;
        if (quick.m_bounds.valid()) {
            info->bounds_valid = 1;
            info->min_x = quick.m_bounds.minx;
            info->min_y = quick.m_bounds.miny;
            info->min_z = quick.m_bounds.minz;
            info->max_x = quick.m_bounds.maxx;
            info->max_y = quick.m_bounds.maxy;
            info->max_z = quick.m_bounds.maxz;
        }
        std::string wkt;
        if (!quick.m_srs.empty()) {
            wkt = quick.m_srs.getWKT();
        }
        info->has_red = std::find(quick.m_dimNames.begin(), quick.m_dimNames.end(), "Red") !=
                                quick.m_dimNames.end()
                            ? 1
                            : 0;
        *srs_wkt = copied(wkt);
        return kOk;
    });
}

// The stages made in a PipelineManager, each but the first on the one before
// it, and the pipeline executed. *pipeline is Rust's to free with
// katana_pdal_pipeline_free; *view_count is how many views it holds.
int katana_pdal_pipeline_run(const katana_pdal_stage* stages, std::size_t stage_count,
                             katana_pdal_pipeline** pipeline, std::size_t* view_count,
                             char** message) noexcept
{
    *pipeline = nullptr;
    *view_count = 0;
    return guarded(message, [&]() -> int {
        auto made = std::make_unique<katana_pdal_pipeline>();
        pdal::Stage* tail = nullptr;
        for (std::size_t i = 0; i < stage_count; ++i) {
            const katana_pdal_stage& stage = stages[i];
            const pdal::Options options = optionsOf(stage.options, stage.option_count);
            if (stage.role == kReader) {
                tail = &made->manager.makeReader(stage.filename, stage.driver, options);
            } else if (tail == nullptr) {
                throw std::invalid_argument("a filter or a writer needs a stage before it");
            } else if (stage.role == kFilter) {
                tail = &made->manager.makeFilter(stage.driver, *tail, options);
            } else if (stage.role == kWriter) {
                tail = &made->manager.makeWriter(stage.filename, stage.driver, *tail, options);
            } else {
                throw std::invalid_argument("a stage of a role the shim does not know");
            }
        }
        made->manager.execute();
        for (const pdal::PointViewPtr& view : made->manager.views()) {
            made->views.push_back(view);
        }
        *view_count = made->views.size();
        *pipeline = made.release();
        return kOk;
    });
}

void katana_pdal_pipeline_free(katana_pdal_pipeline* pipeline) noexcept
{
    delete pipeline;
}

int katana_pdal_view_info(const katana_pdal_pipeline* pipeline, std::size_t view_index,
                          katana_pdal_view_info* info, char** message) noexcept
{
    // The function's own name hides the struct's here, so the reset names
    // neither: {} value-initialises through the pointer's type.
    *info = {};
    return guarded(message, [&]() -> int {
        const pdal::PointViewPtr& view = viewOf(pipeline, view_index);
        if (view == nullptr) {
            info->is_null = 1;
            return kOk;
        }
        const pdal::PointLayoutPtr layout = view->layout();
        info->has_red = layout->hasDim(pdal::Dimension::Id::Red) ? 1 : 0;
        info->has_green = layout->hasDim(pdal::Dimension::Id::Green) ? 1 : 0;
        info->has_blue = layout->hasDim(pdal::Dimension::Id::Blue) ? 1 : 0;
        info->has_intensity = layout->hasDim(pdal::Dimension::Id::Intensity) ? 1 : 0;
        info->has_classification = layout->hasDim(pdal::Dimension::Id::Classification) ? 1 : 0;
        info->size = view->size();
        return kOk;
    });
}

// Points [first, first + count) of a view into `points`, each field read
// with the C++ engine's getFieldAs type and in its order: X, Y and Z, then
// intensity, classification and the colour when asked for.
int katana_pdal_view_points(const katana_pdal_pipeline* pipeline, std::size_t view_index,
                            std::uint64_t first, std::uint64_t count, int read_intensity,
                            int read_classification, int read_colour, katana_pdal_point* points,
                            char** message) noexcept
{
    return guarded(message, [&]() -> int {
        const pdal::PointViewPtr& view = viewOf(pipeline, view_index);
        if (view == nullptr) {
            throw std::invalid_argument("the points of a null view");
        }
        for (std::uint64_t i = 0; i < count; ++i) {
            const pdal::PointId index = first + i;
            katana_pdal_point& point = points[i];
            point = katana_pdal_point{};
            point.x = view->getFieldAs<double>(pdal::Dimension::Id::X, index);
            point.y = view->getFieldAs<double>(pdal::Dimension::Id::Y, index);
            point.z = view->getFieldAs<double>(pdal::Dimension::Id::Z, index);
            if (read_intensity != 0) {
                point.intensity = view->getFieldAs<double>(pdal::Dimension::Id::Intensity, index);
            }
            if (read_classification != 0) {
                point.classification =
                    view->getFieldAs<std::uint8_t>(pdal::Dimension::Id::Classification, index);
            }
            if (read_colour != 0) {
                point.red = view->getFieldAs<std::uint16_t>(pdal::Dimension::Id::Red, index);
                point.green = view->getFieldAs<std::uint16_t>(pdal::Dimension::Id::Green, index);
                point.blue = view->getFieldAs<std::uint16_t>(pdal::Dimension::Id::Blue, index);
            }
        }
        return kOk;
    });
}

// A view's spatial reference as WKT, empty when it has none.
int katana_pdal_view_srs(const katana_pdal_pipeline* pipeline, std::size_t view_index,
                         char** srs_wkt, char** message) noexcept
{
    *srs_wkt = nullptr;
    return guarded(message, [&]() -> int {
        const pdal::PointViewPtr& view = viewOf(pipeline, view_index);
        if (view == nullptr) {
            throw std::invalid_argument("the spatial reference of a null view");
        }
        std::string wkt;
        if (!view->spatialReference().empty()) {
            wkt = view->spatialReference().getWKT();
        }
        *srs_wkt = copied(wkt);
        return kOk;
    });
}

// The points written by a writer of `driver` given `options`, as the C++
// engine's write does it: X, Y, Z, Intensity and Classification registered
// (and Red, Green and Blue with `with_colour`), one PointView filled, handed
// to the writer through a BufferReader. kNoStage when the factory makes no
// such writer; the view is built first, as the C++ builds it.
int katana_pdal_write(const char* driver, const katana_pdal_option* options,
                      std::size_t option_count, const katana_pdal_point* points,
                      std::size_t point_count, int with_colour, char** message) noexcept
{
    return guarded(message, [&]() -> int {
        pdal::PointTable table;
        pdal::PointLayoutPtr layout = table.layout();
        layout->registerDim(pdal::Dimension::Id::X);
        layout->registerDim(pdal::Dimension::Id::Y);
        layout->registerDim(pdal::Dimension::Id::Z);
        layout->registerDim(pdal::Dimension::Id::Intensity);
        layout->registerDim(pdal::Dimension::Id::Classification);
        if (with_colour != 0) {
            layout->registerDim(pdal::Dimension::Id::Red);
            layout->registerDim(pdal::Dimension::Id::Green);
            layout->registerDim(pdal::Dimension::Id::Blue);
        }

        pdal::PointViewPtr view(new pdal::PointView(table));
        pdal::PointId index = 0;
        for (std::size_t i = 0; i < point_count; ++i) {
            const katana_pdal_point& point = points[i];
            view->setField(pdal::Dimension::Id::X, index, point.x);
            view->setField(pdal::Dimension::Id::Y, index, point.y);
            view->setField(pdal::Dimension::Id::Z, index, point.z);
            view->setField(pdal::Dimension::Id::Intensity, index, point.intensity);
            view->setField(pdal::Dimension::Id::Classification, index, point.classification);
            if (with_colour != 0) {
                // The C++ engine's value is `uint16_t << 8`, which C++ promotes
                // to int, so an int is what PDAL is given here too.
                view->setField(pdal::Dimension::Id::Red, index, static_cast<int>(point.red));
                view->setField(pdal::Dimension::Id::Green, index, static_cast<int>(point.green));
                view->setField(pdal::Dimension::Id::Blue, index, static_cast<int>(point.blue));
            }
            ++index;
        }

        pdal::BufferReader reader;
        reader.addView(view);

        pdal::StageFactory factory;
        pdal::Stage* writer = factory.createStage(driver);
        if (writer == nullptr) {
            return kNoStage;
        }
        writer->setOptions(optionsOf(options, option_count));
        writer->setInput(reader);
        writer->prepare(table);
        writer->execute(table);
        return kOk;
    });
}

} // extern "C"
