#pragma once

// Nested layer names (PLAN.MD Phase 05 / Phase 09).
//
// A layer name is a PATH: "design/surface/tin1", "asbuilt/road/kerb". The
// separator is '/' and the segments form a tree, exactly as a survey or civil
// job is actually organised.
//
// WHY THE TREE IS DERIVED, NOT STORED. Entities reference a layer by its full
// name, and that string is what a project database, a DXF export and an undo
// record all carry. If the tree were the primary structure, every node would
// need an identity of its own, a rename of "design" would have to rewrite the
// node, every descendant AND every entity, and any of those going wrong would
// leave entities pointing at a layer that no longer exists. Keeping the full
// path as the single identity means the tree is a VIEW of the names: there is
// one source of truth and it is the one already persisted.
//
// std::map ordering over full paths puts every parent before its children (a
// proper prefix sorts first), and everything under "a" is one contiguous
// range, the keys from "a/" to "a0" - so listing a subtree is a range scan, not
// a graph walk. That is the other reason the flat representation stays. It is
// NOT a strict pre-order walk, which this comment used to claim: a sibling
// whose name continues with a character below '/' ("a 2", "a-old", "a.bak")
// sorts between "a" and "a/b", so "the keys straight after a layer" are not
// its children (see LayerDatabase::hasChildren).

#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::entity {

inline constexpr char kLayerSeparator = '/';

// Bounded so that a tree widget, a recursive walk and a database column all
// have something finite to work with, and so that a generated name cannot grow
// without limit. Sixteen levels is far past any real drawing organisation.
inline constexpr std::size_t kMaximumLayerDepth = 16;
inline constexpr std::size_t kMaximumLayerNameLength = 512;

// Checks `name` as a layer path. Fails with InvalidArgument and says which
// rule was broken - an empty segment, a leading or trailing separator, a
// segment that is only whitespace, "." or "..", a control character, or a path
// past the depth or length limit.
//
// "." and ".." are refused because layer names end up in file names (a plot per
// layer, an export per layer) and a name that means "the parent directory"
// there is a path traversal waiting to happen.
[[nodiscard]] katana::core::Status validateLayerPath(std::string_view name);

// Splits on the separator. No validation: use on a path that already passed
// validateLayerPath, or to report what is wrong with one that did not.
[[nodiscard]] std::vector<std::string_view> layerSegments(std::string_view name);

// "design/surface/tin1" -> "tin1". The whole name when it has no separator.
[[nodiscard]] std::string_view layerLeaf(std::string_view name);

// "design/surface/tin1" -> "design/surface"; empty for a root layer.
[[nodiscard]] std::string_view layerParent(std::string_view name);

// Every proper ancestor, outermost first: "design", "design/surface".
[[nodiscard]] std::vector<std::string> layerAncestors(std::string_view name);

// Number of separators plus one. "0" is depth 1.
[[nodiscard]] std::size_t layerDepth(std::string_view name);

// True when `name` is `ancestor` itself or lies beneath it. Compares whole
// segments, so "designs/x" is NOT under "design" - a plain prefix test would
// say it was, and would hide or lock the wrong layers.
[[nodiscard]] bool isLayerUnder(std::string_view name, std::string_view ancestor);

// Joins segments with the separator, skipping empty ones.
[[nodiscard]] std::string joinLayerPath(std::string_view parent, std::string_view child);

// `name` with the `from` prefix replaced by `to`, for renaming a subtree.
// Returns `name` unchanged when it is not under `from`.
[[nodiscard]] std::string rewriteLayerPrefix(std::string_view name, std::string_view from,
                                             std::string_view to);

} // namespace katana::entity
