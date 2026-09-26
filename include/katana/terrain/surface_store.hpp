#pragma once

// The named TIN surfaces a session holds beside its drawing: one store in the
// window (MainWindow, drawn in the 3D and section views) and one per headless
// session (katana_cli, katana_mcp), so a verb that reads or makes a surface -
// the geoprocessing bindings' SURFACE <name> (docs/geoprocessing.md) - finds
// it by the same name on every front end.
//
// Surfaces are session data, as reference rasters are: not entities, not
// undoable, not saved in the project (docs/terrain.md). A surface is shared,
// immutable, so a background job can read one while the window draws it; a
// change to the store is a new revision, which is how a view knows to look
// again without being told what changed.

#include <cstdint>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace katana::terrain {

struct NamedSurface {
    std::string name;
    std::shared_ptr<const TinSurface> surface;
    // Where it came from, for a person: "point cloud site.laz", "raster
    // dem.tif", "drawing (12 entities)".
    std::string source;
};

class SurfaceStore {
  public:
    // AlreadyExists for a name in use, compared case-insensitively, since a
    // person types SURFACE Ground as readily as SURFACE ground;
    // InvalidArgument for an empty name or no surface.
    katana::core::Status add(NamedSurface surface);

    // Case-insensitive; nullptr when there is none of that name.
    [[nodiscard]] const NamedSurface* find(std::string_view name) const;

    // False when there was none of that name.
    bool remove(std::string_view name);
    void clear();

    // In the order they were added.
    [[nodiscard]] const std::vector<NamedSurface>& all() const { return surfaces_; }
    [[nodiscard]] bool empty() const { return surfaces_.empty(); }

    // Bumped by every change, including a clear of an empty store: a view
    // compares it with the revision it last drew.
    [[nodiscard]] std::uint64_t revision() const { return revision_; }

    // `base`, or "base (2)", "base (3)" ...: the first no surface has. What a
    // front end names an import whose name is taken, as the session names a
    // second alignment of the same name.
    [[nodiscard]] std::string uniqueName(std::string_view base) const;

  private:
    std::vector<NamedSurface> surfaces_;
    std::uint64_t revision_ = 0;
};

} // namespace katana::terrain
