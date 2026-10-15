#pragma once

// The named TIN surfaces of a drawing. The Document holds the one store
// (cad::Document::surfaces), so a verb that reads or makes a surface - the
// geoprocessing bindings' SURFACE <name> (docs/geoprocessing.md), the
// surface edits, the volumes, a leader reading a level off a surface - finds
// it by the same name on every front end. A change is made on a copy and
// committed through Document::setSurfaces as one undo step, and the store is
// saved with the project (docs/terrain.md, "Surfaces in the drawing").
//
// A surface is shared, immutable, so a background job can read one while the
// window draws it, and a copy of the store - what an undo step keeps - costs
// a pointer per surface. A change to the store is a new revision, which is
// how a view knows to look again without being told what changed.

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
    // The display style it is drawn in (cad/terrain/surface_style.hpp), by
    // name; empty for the drawing's default style.
    std::string style{};
    // Whether the views draw it. A hidden surface is still sampled, measured
    // and exported: hiding is a matter of the picture, not of the data.
    bool visible = true;

    friend bool operator==(const NamedSurface&, const NamedSurface&) = default;
};

class SurfaceStore {
  public:
    // Whether `name` can be a surface's name - InvalidArgument, with a sentence
    // that does not echo the name, when it cannot:
    //   - empty;
    //   - not UTF-8. Project storage and the JSON replies write the name, and
    //     the JSON writer throws on a byte that is not UTF-8 (core/
    //     text_encoding.hpp), out of a save that promises a Status. So the name
    //     is refused where it enters, as a layer's or an alignment's is;
    //   - holding a double quote or a line break. A command line names a
    //     surface in quotes and cannot carry either (the tokenizer has no
    //     escape), so such a surface could be listed and never addressed again.
    // add and rename apply it, so every verb and dialog that names a surface
    // does, with no check of its own.
    [[nodiscard]] static katana::core::Status checkName(std::string_view name);

    // `name` made into one checkName accepts, for a name that is not the
    // person's to choose - read out of a file, or out of a project an earlier
    // build wrote: bytes that are not UTF-8 are read as Windows-1252 (the ANSI
    // code page of the katana_cli that typed them, core::utf8OrWindows1252), a
    // double quote becomes an apostrophe and a line break a blank. Text that
    // needs none of it comes back unchanged. uniqueName does it for the
    // importers.
    [[nodiscard]] static std::string nameable(std::string_view name);

    // AlreadyExists for a name in use, compared case-insensitively, since a
    // person types SURFACE Ground as readily as SURFACE ground;
    // InvalidArgument for a name checkName refuses, no surface, or a source or
    // style that is not UTF-8.
    katana::core::Status add(NamedSurface surface);

    // Case-insensitive; nullptr when there is none of that name.
    [[nodiscard]] const NamedSurface* find(std::string_view name) const;

    // False when there was none of that name.
    bool remove(std::string_view name);

    // The surface of `name` replaced by `surface`, keeping its place, name,
    // source, style and visibility - what an edit of a surface commits.
    // NotFound for no surface of that name; InvalidArgument for no surface.
    katana::core::Status replace(std::string_view name, std::shared_ptr<const TinSurface> surface);
    // NotFound for no surface of `name`; AlreadyExists when `newName` is
    // another surface's (a change of case alone is allowed); InvalidArgument
    // for a new name checkName refuses.
    katana::core::Status rename(std::string_view name, std::string_view newName);
    // NotFound for no surface of that name; InvalidArgument for a style name
    // that is not UTF-8. Whether the style exists is the Document's to check:
    // the store does not know the styles.
    katana::core::Status setStyle(std::string_view name, std::string_view style);
    katana::core::Status setVisible(std::string_view name, bool visible);
    void clear();

    // In the order they were added.
    [[nodiscard]] const std::vector<NamedSurface>& all() const { return surfaces_; }
    [[nodiscard]] bool empty() const { return surfaces_.empty(); }

    // Bumped by every change, including a clear of an empty store: what tells
    // a staging copy it was changed (the geoprocessing executor's). A view
    // keys on Document::surfacesGeneration instead - an undo puts back a
    // store with the revision it had then.
    [[nodiscard]] std::uint64_t revision() const { return revision_; }

    // `base` (made nameable), or "base (2)", "base (3)" ...: the first no
    // surface has. What a front end names an import whose name is taken, as the
    // session names a second alignment of the same name.
    [[nodiscard]] std::string uniqueName(std::string_view base) const;

  private:
    std::vector<NamedSurface> surfaces_;
    std::uint64_t revision_ = 0;
};

} // namespace katana::terrain
