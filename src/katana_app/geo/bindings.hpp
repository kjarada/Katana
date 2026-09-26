#pragma once

// What every geoprocessing verb shares (docs/geoprocessing.md, "Bindings"):
// the ONE reader of a source clause and the ONE reader of a target clause,
// drawing data bound through the shared scope and filter, rasters and
// surfaces bound as datasets, and a run's outputs applied to the drawing.
//
//   <source> := <scope> | RASTER <id|name> | SURFACE <name> [CELL <m>]
//             | FILE <path> [LAYER <name>]
//   <scope>  := what cad::parseScopeWords reads (scope_verbs.hpp): SELECTION |
//               DRAWING | VIEW [<id>] [EXTENTS] | AREA x0,y0,x1,y1 |
//               LAYERS a,b [ONLY], then [WHERE key=value ...]
//   <target> := LAYER <path> | REFERENCE [<name>] | FILE <path> [FORMAT <driver>]
//             | SURFACE <name> | SELECTION | REPORT
//
// A curated verb (the lanes' SURFACE, CONTOUR, GIS BUFFER ...) is a thin
// module over these: it reads its own words, builds RunRequests and chooses a
// ResultMode; it never reads a scope or applies a result another way.

#include <cstddef>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "geo_verbs.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/gis/processing.hpp"
#include "katana/interop/geo/drawing_dataset.hpp"

namespace katana::app::geo {

// A line split into words as the interpreter splits it (blanks, double quotes
// grouping and removed), and which words were quoted: a quoted "FROM" is a
// value, never the keyword.
struct Tokens {
    std::vector<std::string> words;
    std::vector<bool> quoted;

    [[nodiscard]] std::size_t size() const { return words.size(); }
    [[nodiscard]] const std::string& operator[](std::size_t i) const { return words[i]; }
    // `words[i]` is `keyword`, unquoted, in any case.
    [[nodiscard]] bool is(std::size_t i, std::string_view keyword) const;
};

// ParseFailure for a quote never closed.
[[nodiscard]] katana::core::Result<Tokens> tokenize(std::string_view line);

struct Source {
    enum class Kind { Drawing, Raster, Surface, File };
    Kind kind = Kind::Drawing;
    katana::cad::ScopeWords scope; // Drawing
    std::string raster;            // Raster: an id or a name
    std::string surface;           // Surface
    std::string path, layer;       // File
    std::optional<double> cell;    // Surface: CELL
};

struct Target {
    enum class Kind { Default, Layer, Reference, File, Surface, Selection, Report };
    Kind kind = Kind::Default;
    std::string name;   // Layer: the layer; Reference, Surface: the name; File: the path
    std::string format; // File: FORMAT
};

// The words of a clause keyword, for the errors that name what was expected.
[[nodiscard]] bool isSourceKeyword(const Tokens& tokens, std::size_t i);

// Reads one source from `at`, leaving `at` after it. InvalidArgument (or the
// scope parser's ParseFailure) naming the word, for a clause that is none.
[[nodiscard]] katana::core::Result<Source> parseSource(const Tokens& tokens, std::size_t& at);
// Reads one target from `at`, leaving `at` after it.
[[nodiscard]] katana::core::Result<Target> parseTarget(const Tokens& tokens, std::size_t& at);

// What a scope took, as the feature set an algorithm reads.
struct BoundDrawing {
    katana::cad::ScopeMatch match;
    katana::interop::geo::DrawingDataset dataset;
};

// The scope resolved and matched by the shared matcher (VIEW through the
// interpreter's scope context, refused headless naming AREA), then converted.
// A scope that takes nothing is not a failure: the dataset is empty and the
// record says matched=0.
[[nodiscard]] katana::core::Result<BoundDrawing>
bindDrawing(Context& context, const katana::cad::ScopeWords& scope,
            const katana::interop::geo::DrawingDatasetOptions& options);

// A dataset made on the worker, from what was copied at prepare: a surface
// is sampled there, since sampling is work and the surface is shared and
// immutable.
using DeferredDataset = std::function<katana::core::Result<katana::gis::processing::DatasetValue>()>;

// A raster, surface or file source as the dataset GDAL reads. RASTER binds
// the reference raster's file, read at full precision - never its display
// copy; SURFACE the surface sampled at cell centres at CELL, or at a cell
// suggested for its extent; FILE the path as it is (a /vsi path or a URL
// passes through). NotFound for a raster or surface of no such id or name;
// Unsupported for a reference raster with no file behind it.
[[nodiscard]] katana::core::Result<DeferredDataset> bindRaster(Context& context, const Source& source);

// The record of one input: `input arg=input source=surface name=ground
// cell=1` (docs/geoprocessing.md, "Replies").
[[nodiscard]] std::string inputRecord(std::string_view arg, const Source& source);

// The record of what a scope took, the shared scope record in the middle:
//   scope arg=input scope=drawing matched=12 used=10 points=0 lines=4
//   polygons=6 skipped.text=2
[[nodiscard]] std::string scopeRecord(std::string_view arg, const BoundDrawing& bound);

// The project's coordinate system, for the tables a scope becomes.
[[nodiscard]] std::string projectCrs(const Context& context);

// Where a derived raster goes: the project's cache, or the scratch folder
// (persisted=no) when the drawing has no project.
[[nodiscard]] std::filesystem::path derivedFolder(const Context& context);

// What an apply needs to know besides the outputs.
struct ApplyRequest {
    Target target;
    std::string arg = "output";
    // The layer a vector result goes to when no TO says: gis/<algorithm's
    // last word>. The reference name a raster result takes likewise.
    std::string defaultName;
    katana::interop::geo::ResultOptions result;
};

// The outputs of a run, applied: a vector result as ONE command on the
// drawing (resultCommand), a raster as a derived reference raster, a file
// named, text printed. The records, one per line. Unsupported for an output
// the target cannot take (a vector into REFERENCE, a raster into a LAYER),
// and for the targets whose packages are not built yet.
[[nodiscard]] katana::core::Result<std::string>
applyOutputs(Context& context, const ApplyRequest& request,
             katana::gis::processing::RunOutputs&& outputs);

// For an in-place apply (UpdateGeometry, SetProperties, REPLACE): the
// entities as they were when the line was prepared, compared with the
// drawing now. InvalidState "the drawing changed while the job ran" when any
// differs or is gone: overwriting a concurrent edit silently would be worse
// than refusing.
[[nodiscard]] katana::core::Status
unchangedSince(const Context& context, const std::vector<katana::entity::Entity>& copies);

} // namespace katana::app::geo
