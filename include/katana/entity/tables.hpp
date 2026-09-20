#pragma once

// Named resource tables that entities refer to by name (PLAN.MD Phase 05):
// LayerDatabase, StyleDatabase, PropertyDatabase.
//
// Tables are ordered by name for deterministic iteration and are not
// thread-safe. Cross-table rules (e.g. "a layer in use cannot be removed") are
// enforced by commands, which can see every table.

#include <map>
#include <utility>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/layer_path.hpp"
#include "katana/entity/entity.hpp"

namespace katana::entity {


inline constexpr std::string_view kDefaultLayerName = "0";
inline constexpr std::string_view kContinuousLinetype = "continuous";

struct Layer {
    std::string name{kDefaultLayerName};
    Color color{};
    bool visible = true;
    bool locked = false; // locked layers are drawn but their entities cannot be edited
    std::string linetype{kContinuousLinetype};
    double lineWeight = 0.25; // millimetres on paper

    friend bool operator==(const Layer&, const Layer&) = default;
};

// Always contains layer "0", which can be modified but never removed or renamed.
//
// Layer names are PATHS: "design/surface/tin1". See layer_path.hpp for why the
// tree is derived from the names rather than stored. Two consequences shape
// this interface:
//
//   * Adding "design/surface/tin1" also creates "design" and "design/surface"
//     if they are missing, so every node in the tree is a real layer with its
//     own colour, visibility and lock rather than a placeholder the user
//     cannot select or set.
//   * Visibility and lock INHERIT down the tree (see effectivelyVisible and
//     effectivelyLocked): turning off "design" turns off everything under it,
//     which is the entire point of grouping. Colour and linetype do not
//     inherit - ByLayer already resolves those, and a second inheritance rule
//     layered on top would make the resolved colour impossible to predict.
//
// Because the map is keyed by full path, its ascending order IS a pre-order
// walk of the tree, and a subtree is a contiguous range rather than a search.
class LayerDatabase {
  public:
    LayerDatabase();

    // Creates `layer` and any missing ancestors (with default properties).
    // Fails with AlreadyExists when the layer itself is already present, and
    // with InvalidArgument when the name is not a valid path.
    [[nodiscard]] katana::core::Status add(Layer layer);
    // Same, but succeeds and changes nothing when the layer already exists.
    // What an importer wants: "make sure this path exists".
    [[nodiscard]] katana::core::Status ensure(std::string_view name);
    [[nodiscard]] katana::core::Status update(const Layer& layer); // matched by name
    // Removes ONE layer. Fails with InvalidArgument when it still has
    // children: silently orphaning or deleting them is the kind of quiet
    // destruction section 36 forbids. Use removeSubtree to take the branch.
    [[nodiscard]] katana::core::Result<Layer> remove(std::string_view name);
    // Removes `name` and everything under it, deepest first, and returns what
    // was removed in that order so an undo can put it back parents-first by
    // walking the result backwards.
    [[nodiscard]] katana::core::Result<std::vector<Layer>> removeSubtree(std::string_view name);
    // Restores the pristine state: only the default layer.
    void reset();

    // Renames `from` and every descendant to sit under `to`. Returns the
    // (old, new) pairs so the caller can move the entities that reference
    // them; nothing is changed if any target name is already taken, so a
    // collision cannot leave the tree half renamed.
    //
    // Fails with NotFound, InvalidArgument (bad path, renaming layer "0", or
    // moving a layer inside its own subtree) or AlreadyExists.
    [[nodiscard]] katana::core::Result<std::vector<std::pair<std::string, std::string>>>
    renameSubtree(std::string_view from, std::string_view to);

    [[nodiscard]] const Layer* find(std::string_view name) const;
    [[nodiscard]] bool contains(std::string_view name) const { return find(name) != nullptr; }
    [[nodiscard]] std::vector<std::string> names() const; // ascending
    [[nodiscard]] std::vector<Layer> all() const;          // ascending by name
    [[nodiscard]] std::size_t size() const { return layers_.size(); }

    // ---- the tree ---------------------------------------------------------

    // Top-level layers, ascending.
    [[nodiscard]] std::vector<std::string> roots() const;
    // Immediate children of `name`, ascending. Empty for a leaf or a missing
    // layer. Pass an empty name for the roots.
    [[nodiscard]] std::vector<std::string> children(std::string_view name) const;
    // `name` and everything beneath it, in pre-order. Empty when absent.
    [[nodiscard]] std::vector<std::string> subtree(std::string_view name) const;
    [[nodiscard]] bool hasChildren(std::string_view name) const;

    // Visible only when it and every ancestor is visible; locked when it or
    // any ancestor is locked. False / true respectively for a missing layer,
    // so an entity on a layer that has gone is not silently drawn or edited.
    [[nodiscard]] bool effectivelyVisible(std::string_view name) const;
    [[nodiscard]] bool effectivelyLocked(std::string_view name) const;

  private:
    std::map<std::string, Layer, std::less<>> layers_;
};

struct Style {
    std::string name{};
    std::optional<Color> color{}; // empty: ByLayer
    double lineWeight = 0.25;     // millimetres on paper
    std::string linetype{kContinuousLinetype};

    friend bool operator==(const Style&, const Style&) = default;
};

class StyleDatabase {
  public:
    [[nodiscard]] katana::core::Status add(Style style);
    [[nodiscard]] katana::core::Status update(const Style& style);
    [[nodiscard]] katana::core::Result<Style> remove(std::string_view name);
    void reset() { styles_.clear(); }

    [[nodiscard]] const Style* find(std::string_view name) const;
    [[nodiscard]] std::vector<Style> all() const; // ascending by name
    [[nodiscard]] std::size_t size() const { return styles_.size(); }

  private:
    std::map<std::string, Style, std::less<>> styles_;
};

enum class PropertyType { Boolean, Integer, Real, Text };

[[nodiscard]] constexpr PropertyType typeOf(const PropertyValue& value)
{
    return static_cast<PropertyType>(value.index());
}

// Schema entry for an entity property. Defining a property makes it typed:
// values of the wrong type are rejected before they reach an entity.
struct PropertyDefinition {
    std::string name{};
    PropertyType type = PropertyType::Text;
    std::string description{};
    std::optional<PropertyValue> defaultValue{};

    friend bool operator==(const PropertyDefinition&, const PropertyDefinition&) = default;
};

// Registry of property definitions. Properties without a definition are
// free-form and always accepted.
class PropertyDatabase {
  public:
    [[nodiscard]] katana::core::Status define(PropertyDefinition definition);
    [[nodiscard]] katana::core::Result<PropertyDefinition> undefine(std::string_view name);
    void reset() { definitions_.clear(); }

    [[nodiscard]] const PropertyDefinition* find(std::string_view name) const;
    [[nodiscard]] std::vector<PropertyDefinition> all() const; // ascending by name

    // Ok when `name` is undefined, or defined with the type of `value`.
    [[nodiscard]] katana::core::Status validate(std::string_view name,
                                                const PropertyValue& value) const;

  private:
    std::map<std::string, PropertyDefinition, std::less<>> definitions_;
};

} // namespace katana::entity
