#include "katana/terrain/surface_store.hpp"

#include <algorithm>
#include <utility>

#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"

namespace katana::terrain {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

namespace {

// Free text a surface carries - its source and the style it names - is
// refused when it is not UTF-8, for the reason the name is (checkName). The
// text is not echoed: it is the bytes that are wrong, and a message holding
// them reaches the same JSON writers.
Status checkText(std::string_view field, std::string_view text)
{
    if (katana::core::isValidUtf8(text)) {
        return {};
    }
    return makeError(ErrorCode::InvalidArgument,
                     "a surface's " + std::string(field) + " is not valid UTF-8",
                     "field=" + std::string(field));
}

} // namespace

Status SurfaceStore::checkName(std::string_view name)
{
    if (name.empty()) {
        return makeError(ErrorCode::InvalidArgument, "a surface needs a name");
    }
    // The ONE place a surface name is judged to be text. Project storage and
    // every JSON reply write it, and nlohmann's writer THROWS on a byte that
    // is not UTF-8 (core/text_encoding.hpp) - out of a save, which promises a
    // Status. The bytes are not echoed, as the layers' refusal does not.
    if (auto text = checkText("name", name); !text) {
        return text;
    }
    // A line names a surface in quotes and has no way to type a quote inside
    // them (the tokenizer has no escape); a line break would end the line. A
    // name holding either could be listed but never addressed again.
    if (name.find_first_of("\"\r\n") != std::string_view::npos) {
        return makeError(ErrorCode::InvalidArgument,
                         "a surface's name holds a double quote or a line break, which a "
                         "command line cannot name",
                         "field=name");
    }
    return {};
}

std::string SurfaceStore::nameable(std::string_view name)
{
    std::string text = katana::core::utf8OrWindows1252(name);
    for (char& c : text) {
        if (c == '"') {
            c = '\'';
        } else if (c == '\r' || c == '\n') {
            c = ' ';
        }
    }
    return text;
}

Status SurfaceStore::add(NamedSurface surface)
{
    if (auto status = checkName(surface.name); !status) {
        return status;
    }
    if (auto status = checkText("source", surface.source); !status) {
        return status;
    }
    if (auto status = checkText("style", surface.style); !status) {
        return status;
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
    if (auto status = checkName(newName); !status) {
        return status;
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
    if (auto status = checkText("style", style); !status) {
        return status;
    }
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
    // Made nameable first: the name an importer reads out of a file is not the
    // person's to choose, and refusing the whole import for a quote in a TIN's
    // name would be the wrong answer to it.
    const std::string stem = base.empty() ? std::string("surface") : nameable(base);
    std::string name = stem;
    for (int copy = 2; find(name) != nullptr; ++copy) {
        name = stem + " (" + std::to_string(copy) + ")";
    }
    return name;
}

} // namespace katana::terrain
