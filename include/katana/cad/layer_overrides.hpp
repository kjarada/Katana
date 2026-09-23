#pragma once

// Per-view layer visibility (the user's request of 2026-09-23: "per view
// controls to add and remove layers").
//
// A layer's visibility is document state: it is saved, it is undoable, and
// every view obeys it. A view may additionally hide layers of its own - a plan
// with the survey hidden beside a 3D view that shows it - and that is VIEW
// state: not saved in the project, not undoable, and never seen by another
// view. This class is that second half.
//
// It is SUBTRACTIVE. A view can hide what the document shows; it can never
// show what the document hides. That keeps the one visibility rule in
// selection.hpp a plain conjunction - drawn = document says drawn AND this
// view does not hide it - and means a layer the user switched off in the
// Layers panel cannot reappear in some view they forgot about.
//
// Hiding a path hides everything beneath it in that view, exactly as the
// document rule does (PLAN.MD 5.1): hiding "design" hides "design/surface/tin1".
// The test walks the path's ancestors, which is a string_view slice per
// segment and a set lookup each - no allocation, and no LayerDatabase needed,
// which is what lets every consumer ask it without being handed the model.
// docs/model.md records why the document rule does NOT walk ancestors per
// query (it stores them resolved); a view's set is a handful of paths, and
// resolving it per view would need a notification the document does not send.

#include <cstddef>
#include <functional>
#include <set>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::entity {
class LayerDatabase;
}

namespace katana::cad {

class LayerOverrides {
  public:
    // Hides `path` and everything beneath it in this view. False when it was
    // already hidden directly, so a caller can tell whether to repaint.
    bool hide(std::string_view path);
    // Removes this view's hide of exactly `path`. An ANCESTOR hidden in this
    // view still hides it; showing a child of a hidden parent is the same
    // impossibility the Layers panel greys out for the document rule.
    bool show(std::string_view path);
    // Shows everything the document shows.
    void clear() { hidden_.clear(); }

    // Hides everything that is not `path`, one of its ancestors, or beneath
    // it, using the fewest entries: at each level of `path`, the siblings of
    // the next segment. `allPaths` is every layer name in the document (the
    // tree is derived from the names, include/katana/entity/layer_path.hpp).
    // Replaces whatever this view hid before. Fails with NotFound when no
    // layer is `path` or lies beneath it - isolating nothing would hide the
    // whole drawing, which is never what was meant.
    [[nodiscard]] katana::core::Status isolate(std::string_view path,
                                               const std::vector<std::string>& allPaths);

    // Drops entries that name no layer: neither a layer of that name nor any
    // layer beneath it. Document listeners carry no payload, so a view cannot
    // follow a rename or a delete; pruning after every change is what stops a
    // later layer that happens to reuse the name being born hidden here.
    // Returns how many entries were dropped.
    std::size_t pruneMissing(const katana::entity::LayerDatabase& layers);

    // True when `path` or any ancestor of it is hidden in this view. Returns at
    // once for an empty set, which is every view the user has not touched -
    // this is asked once per entity per frame.
    [[nodiscard]] bool hides(std::string_view path) const;
    // True only when `path` itself is an entry, as a check box shows it.
    [[nodiscard]] bool hidesDirectly(std::string_view path) const;

    [[nodiscard]] bool empty() const { return hidden_.empty(); }
    [[nodiscard]] std::size_t size() const { return hidden_.size(); }
    [[nodiscard]] const std::set<std::string, std::less<>>& hidden() const { return hidden_; }

    friend bool operator==(const LayerOverrides&, const LayerOverrides&) = default;

  private:
    // std::less<> for heterogeneous lookup: hides() asks with string_view
    // slices of the path and must not build a std::string to do it.
    std::set<std::string, std::less<>> hidden_;
};

// What a caller passes when it means the document rule alone: Surface From
// Drawing, the command-line SELECT and the section cut are shared results and
// must not depend on which view happened to be active.
inline const LayerOverrides kNoLayerOverrides{};

} // namespace katana::cad
