#pragma once

// The ONE geoprocessing executor (docs/geoprocessing.md, "The executor"):
// every geoprocessing verb - GDAL, and the curated families the lanes add -
// runs through here, whether it was typed at katana_cli, sent by an agent to
// katana_mcp or run on the window's command line.
//
// A line runs in three phases, so a headless session can run it inline while
// the window runs it as a background job:
//
//   prepare  on the calling thread (the GUI thread in the window): the line is
//            read, the algorithm resolved and its policy checked, the scopes
//            resolved by the shared scope parser and matcher, and everything
//            the work needs COPIED out of the drawing - a feature set, a
//            surface held by a shared pointer, paths. LIST, HELP, VERSION and
//            PREVIEW have nothing to run and answer here.
//   work     on a worker, or inline: the algorithm and the conversions only,
//            never the Document or the reference data. It returns an Apply.
//   apply    on the calling thread again: ONE command on the drawing (one
//            undo step), reference rasters added, and the reply records.
//
// Nothing a run computes is visible until it is applied, so a cancelled or
// failed job leaves no trace (src/katana_qt/jobs.hpp's rule).

#include <filesystem>
#include <functional>
#include <optional>
#include <stop_token>
#include <string>
#include <string_view>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/interop/reference_data.hpp"
#include "katana/terrain/surface_store.hpp"

namespace katana::app::geo {

// What a geoprocessing line runs against: the front end's drawing, its
// interpreter (whose scope context answers VIEW in the window and refuses it
// headless), its reference rasters and its surfaces.
struct Context {
    katana::cad::Document& document;
    katana::cad::CommandInterpreter& interpreter;
    katana::interop::ReferenceData& reference;
    katana::terrain::SurfaceStore& surfaces;
    // Where derived rasters go when the drawing has no project directory.
    std::filesystem::path scratch;
    // Optional: frame what an apply added (the window's views).
    std::function<void(const katana::geometry::Box2&)> frame{};
    // Optional: the reference rasters or the surfaces changed.
    std::function<void()> changed{};
};

using Progress = std::function<void(double)>;
// One undo step at most; the reply records.
using Apply = std::function<katana::core::Result<std::string>(Context&)>;
// Pure: never the Document or the reference data.
using Work =
    std::function<katana::core::Result<Apply>(const std::stop_token&, const Progress&)>;

struct Prepared {
    // For a job's status-bar line: "GDAL raster hillshade".
    std::string title;
    // Set when there is nothing to run: LIST, HELP, VERSION, PREVIEW.
    std::optional<std::string> reply;
    Work work;
};

// Whether a verb of the executor's begins `line`.
[[nodiscard]] bool handles(std::string_view line);

// The first phase. Fails for a line the executor refuses: its grammar, an
// unknown algorithm or argument, a Confirm algorithm without CONFIRM, a file
// in the way without OVERWRITE, VIEW headless.
[[nodiscard]] katana::core::Result<Prepared> prepare(Context& context, std::string_view line);

// The three phases on this thread, one after another: what katana_cli and
// katana_mcp run. The reply records, or the refusal.
[[nodiscard]] katana::core::Result<std::string> runNow(Context& context, std::string_view line,
                                                       const std::stop_token& stop = {},
                                                       const Progress& progress = {});

// The usage of every verb in the table, for HELP, the window's Command
// Reference and katana_help alike.
[[nodiscard]] std::string helpText();

// Where a front end keeps derived rasters for a drawing with no project:
// <temp>/katana-scratch/<process id>-<start>, one folder per process, the
// same whoever asks, so no process - not even a later one given an ended
// one's id - finds another's files there. Nothing is created until a raster
// is written there.
[[nodiscard]] std::filesystem::path defaultScratch();

} // namespace katana::app::geo
