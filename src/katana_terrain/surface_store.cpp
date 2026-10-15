#include "katana/terrain/surface_store.hpp"

#include <algorithm>
#include <utility>

#include "katana/core/text.hpp"

namespace katana::terrain {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

Status SurfaceStore::add(NamedSurface surface)
{
    if (surface.name.empty()) {
        return makeError(ErrorCode::InvalidArgument, "a surface needs a name");
    }
    if (!surface.surface) {
        return makeError(ErrorCode::InvalidArgument, "no surface to keep", surface.name);
    }
    if (find(surface.name) != nullptr) {
        return makeError(ErrorCode::AlreadyExists, "a surface already has that name",
                         surface.name);
    }
    surfaces_.push_back(std::move(surface));
    ++revision_;
    return {};
}

const NamedSurface* SurfaceStore::find(std::string_view name) const
{
    const auto found = std::ranges::find_if(surfaces_, [&](const NamedSurface& surface) {
        return katana::core::equalsIgnoringCase(surface.name, name);
    });
    return found == surfaces_.end() ? nullptr : &*found;
}

bool SurfaceStore::remove(std::string_view name)
{
    const auto removed = std::erase_if(surfaces_, [&](const NamedSurface& surface) {
        return katana::core::equalsIgnoringCase(surface.name, name);
    });
    if (removed == 0) {
        return false;
    }
    ++revision_;
    return true;
}

namespace {

NamedSurface* findIn(std::vector<NamedSurface>& surfaces, std::string_view name)
{
    const auto found = std::ranges::find_if(surfaces, [&](const NamedSurface& surface) {
        return katana::core::equalsIgnoringCase(surface.name, name);
    });
    return found == surfaces.end() ? nullptr : &*found;
}

} // namespace

Status SurfaceStore::replace(std::string_view name, std::shared_ptr<const TinSurface> surface)
{
    if (!surface) {
        return makeError(ErrorCode::InvalidArgument, "no surface to keep", std::string(name));
    }
    NamedSurface* named = findIn(surfaces_, name);
    if (named == nullptr) {
        return makeError(ErrorCode::NotFound, "no surface of that name", std::string(name));
    }
    named->surface = std::move(surface);
    ++revision_;
    return {};
}

Status SurfaceStore::rename(std::string_view name, std::string_view newName)
{
    if (newName.empty()) {
        return makeError(ErrorCode::InvalidArgument, "a surface needs a name");
    }
    NamedSurface* named = findIn(surfaces_, name);
    if (named == nullptr) {
        return makeError(ErrorCode::NotFound, "no surface of that name", std::string(name));
    }
    const NamedSurface* other = find(newName);
    if (other != nullptr && other != named) {
        return makeError(ErrorCode::AlreadyExists, "a surface already has that name",
                         std::string(newName));
    }
    named->name = std::string(newName);
    ++revision_;
    return {};
}

Status SurfaceStore::setStyle(std::string_view name, std::string_view style)
{
    NamedSurface* named = findIn(surfaces_, name);
    if (named == nullptr) {
        return makeError(ErrorCode::NotFound, "no surface of that name", std::string(name));
    }
    named->style = std::string(style);
    ++revision_;
    return {};
}

Status SurfaceStore::setVisible(std::string_view name, bool visible)
{
    NamedSurface* named = findIn(surfaces_, name);
    if (named == nullptr) {
        return makeError(ErrorCode::NotFound, "no surface of that name", std::string(name));
    }
    named->visible = visible;
    ++revision_;
    return {};
}

void SurfaceStore::clear()
{
    surfaces_.clear();
    ++revision_;
}

std::string SurfaceStore::uniqueName(std::string_view base) const
{
    std::string name(base.empty() ? std::string_view("surface") : base);
    for (int copy = 2; find(name) != nullptr; ++copy) {
        name = std::string(base.empty() ? std::string_view("surface") : base) + " (" +
               std::to_string(copy) + ")";
    }
    return name;
}

} // namespace katana::terrain
