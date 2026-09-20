#pragma once

// One implementation of "a table of things looked up by name".
//
// Linetypes, dimension styles and styles are the same container: an ordered map
// from name to record, with add / update / remove / find / all, `AlreadyExists`
// and `NotFound` reported the same way, and in two of the three a built-in
// entry that cannot be removed. They were three hand-written copies of that,
// about 200 lines, and the next such table would have been a fourth. CLAUDE.md
// section 6: a second way of doing something that already has a first way is a
// defect, not a shortcut.
//
// What a table is allowed to differ in is expressed as a policy, so that the
// differences are visible in one place instead of buried in three
// near-identical bodies:
//
//   * the noun used in its error messages, so they still read as English;
//   * what makes a record valid;
//   * an extra rule for updates only (the continuous linetype may be renamed
//     in every respect except being given a pattern);
//   * a built-in name that `remove` refuses;
//   * what `reset` seeds.
//
// `LayerDatabase` is deliberately NOT built on this. Layer names are `/`-
// separated paths and the table derives a tree from them - `children`,
// `subtree`, `removeSubtree`, `rename` - so it is a different data structure
// that happens to be keyed by name. Forcing it in here would mean a policy
// with more behaviour than the template. See `layer_path.hpp`.
//
// The map is `std::map` with a transparent comparator, not `unordered_map`, so
// that `all()` and `names()` come back in name order without sorting. Callers
// depend on that: the project store writes tables in that order, and a stable
// order is what makes a saved file diffable.

#include <map>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/core/error.hpp"

namespace katana::entity {

// The map every table is built on; a policy's `seed` receives one.
template <class T> using NamedMap = std::map<std::string, T, std::less<>>;

// Defaults a policy inherits and overrides only where it differs. A table with
// no built-in entry and no update-only rule says nothing about either.
template <class T> struct NamedTablePolicy {
    static katana::core::Status checkUpdate(const T&) { return {}; }
    static bool isProtected(std::string_view) { return false; }
    static void seed(NamedMap<T>&) {}
};

template <class T, class Policy> class NamedTable {
  public:
    NamedTable() { reset(); }

    void reset()
    {
        items_.clear();
        Policy::seed(items_);
    }

    [[nodiscard]] katana::core::Status add(T item)
    {
        if (auto status = Policy::validate(item); !status) {
            return status;
        }
        if (contains(item.name)) {
            return katana::core::makeError(katana::core::ErrorCode::AlreadyExists,
                                           std::string(Policy::kNoun) + " already exists",
                                           item.name);
        }
        std::string name = item.name;
        items_.emplace(std::move(name), std::move(item));
        return {};
    }

    [[nodiscard]] katana::core::Status update(const T& item)
    {
        const auto found = items_.find(item.name);
        if (found == items_.end()) {
            return katana::core::makeError(katana::core::ErrorCode::NotFound,
                                           std::string(Policy::kNoun) + " does not exist",
                                           item.name);
        }
        // The update-only rule runs before validation, so that a record which
        // is both invalid and forbidden reports the more specific reason.
        if (auto status = Policy::checkUpdate(item); !status) {
            return status;
        }
        if (auto status = Policy::validate(item); !status) {
            return status;
        }
        found->second = item;
        return {};
    }

    [[nodiscard]] katana::core::Result<T> remove(std::string_view name)
    {
        if (Policy::isProtected(name)) {
            return katana::core::makeError(katana::core::ErrorCode::InvalidArgument,
                                           std::string("the built-in ") +
                                               std::string(Policy::kNoun) + " cannot be removed",
                                           std::string(name));
        }
        const auto found = items_.find(name);
        if (found == items_.end()) {
            return katana::core::makeError(katana::core::ErrorCode::NotFound,
                                           std::string(Policy::kNoun) + " does not exist",
                                           std::string(name));
        }
        T removed = std::move(found->second);
        items_.erase(found);
        return removed;
    }

    [[nodiscard]] const T* find(std::string_view name) const
    {
        const auto found = items_.find(name);
        return found == items_.end() ? nullptr : &found->second;
    }

    [[nodiscard]] bool contains(std::string_view name) const { return find(name) != nullptr; }

    [[nodiscard]] std::vector<std::string> names() const // ascending
    {
        std::vector<std::string> result;
        result.reserve(items_.size());
        for (const auto& entry : items_) {
            result.push_back(entry.first);
        }
        return result;
    }

    [[nodiscard]] std::vector<T> all() const // ascending by name
    {
        std::vector<T> result;
        result.reserve(items_.size());
        for (const auto& entry : items_) {
            result.push_back(entry.second);
        }
        return result;
    }

    [[nodiscard]] std::size_t size() const { return items_.size(); }
    [[nodiscard]] bool empty() const { return items_.empty(); }

  private:
    NamedMap<T> items_;
};

} // namespace katana::entity
