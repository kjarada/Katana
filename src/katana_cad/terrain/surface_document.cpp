// Document's surface members (include/katana/cad/document.hpp; docs/terrain.md,
// "Surfaces in the drawing"): the drawing's one store of named TIN surfaces and
// their display styles, each change ONE undoable step, saved with the project.
// Kept here, beside the rest of the surface code, as the sheets are kept in
// plotting/sheet_store.cpp: document.cpp only calls in at new, open and save.

#include <algorithm>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/storage/project_store.hpp"
#include "katana/terrain/tin_codec.hpp"

namespace katana::cad {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::terrain::NamedSurface;
using katana::terrain::SurfaceStore;

namespace {

// The metadata key the styles are kept under, as versioned JSON - the
// sheets' arrangement (plotting/sheet_store.cpp): the storage layer carries
// a key it does not read, so the styles need no schema change. Written by
// every save, empty for the built-in set, so a set put back to the built-ins
// overwrites the row a custom one left (the metadata table is never emptied:
// ProjectStore::save).
constexpr std::string_view kStylesKey = "surface_styles";

// One change of the surfaces, the styles or both: the state before and after,
// put back by the document. A snapshot rather than an inverse operation,
// because a copy of a store is a pointer per surface (every surface is shared
// and immutable) and a style set is a few hundred bytes - and a snapshot
// cannot undo to anything but exactly what was there.
class SurfaceStateCommand final : public katana::commands::Command {
  public:
    // Called with true to install the state after the change, false before.
    using Apply = std::function<void(bool after)>;

    SurfaceStateCommand(std::string name, Apply apply)
        : name_(std::move(name)), apply_(std::move(apply))
    {
    }

    [[nodiscard]] std::string_view name() const override { return name_; }
    [[nodiscard]] Status validate(const katana::commands::CommandContext&) const override
    {
        return {};
    }
    [[nodiscard]] Status execute(katana::commands::CommandContext&) override
    {
        apply_(true);
        return {};
    }
    [[nodiscard]] Status undo(katana::commands::CommandContext&) override
    {
        apply_(false);
        return {};
    }
    [[nodiscard]] Status redo(katana::commands::CommandContext&) override
    {
        apply_(true);
        return {};
    }

  private:
    std::string name_;
    Apply apply_;
};

bool sameSurfaces(const SurfaceStore& a, const SurfaceStore& b)
{
    // By what each holds - a surface by its pointer, since a surface is
    // immutable and an edit makes a new one - never by the revision, which
    // counts how a store came to be, not what it is.
    return a.all() == b.all();
}

// A surface names a style the styles do not have. Refused only for a name
// NEW to the drawing: a surface that already named a style since lost (a
// project whose styles a newer Katana wrote) is drawn in the default
// (styleOf) and must not block every later change of the surfaces.
Status checkStyleNames(const SurfaceStore& next, const SurfaceStore& now,
                       const SurfaceStyleSet& styles)
{
    for (const NamedSurface& surface : next.all()) {
        if (surface.name.empty() || !surface.surface) {
            return makeError(ErrorCode::InvalidArgument, "a surface needs a name and a surface",
                             surface.name);
        }
        if (surface.style.empty() || styles.find(surface.style) != nullptr) {
            continue;
        }
        const bool named = std::ranges::any_of(now.all(), [&](const NamedSurface& before) {
            return katana::core::equalsIgnoringCase(before.style, surface.style);
        });
        if (!named) {
            return makeError(ErrorCode::InvalidArgument,
                             "no surface style has that name; SURFACESTYLE LIST names them",
                             "surface=" + surface.name + " style=" + surface.style);
        }
    }
    return {};
}

// A style a surface uses, which `next` no longer has though `now` had it.
Status checkStylesInUse(const SurfaceStyleSet& next, const SurfaceStyleSet& now,
                        const SurfaceStore& surfaces)
{
    for (const NamedSurface& surface : surfaces.all()) {
        if (surface.style.empty() || next.find(surface.style) != nullptr ||
            now.find(surface.style) == nullptr) {
            continue;
        }
        return makeError(ErrorCode::InvalidArgument,
                         "surface " + surface.name + " is drawn in style " + surface.style +
                             "; give it another style first",
                         "style=" + surface.style + " surface=" + surface.name);
    }
    return {};
}

} // namespace

const SurfaceStore& Document::surfaces() const
{
    return surfaces_;
}

std::uint64_t Document::surfacesGeneration() const
{
    return surfacesGeneration_;
}

const SurfaceStyleSet& Document::surfaceStyles() const
{
    return surfaceStyles_;
}

std::uint64_t Document::surfaceStylesGeneration() const
{
    return surfaceStylesGeneration_;
}

Status Document::surfaceStylesStatus() const
{
    return surfaceStylesStatus_;
}

Result<katana::commands::CommandPtr> Document::surfacesCommand(SurfaceStore surfaces,
                                                               std::string stepName) const
{
    if (auto status = checkStyleNames(surfaces, surfaces_, surfaceStyles_); !status) {
        return status.error();
    }
    if (sameSurfaces(surfaces, surfaces_)) {
        return katana::commands::CommandPtr{};
    }
    // Both sides held by the step, so undo and redo swap them back and forth
    // without copying a surface.
    auto states = std::make_shared<std::pair<SurfaceStore, SurfaceStore>>(surfaces_,
                                                                          std::move(surfaces));
    // Making the command changes nothing; running it does, and a command runs
    // only through the non-const execute() of the Document it was made by -
    // so the Document it changes is never a const object.
    Document* self = const_cast<Document*>(this);
    auto apply = [self, states](bool after) {
        self->surfaces_ = after ? states->second : states->first;
        ++self->surfacesGeneration_;
    };
    return katana::commands::CommandPtr(
        std::make_unique<SurfaceStateCommand>(std::move(stepName), std::move(apply)));
}

Status Document::setSurfaces(SurfaceStore surfaces, std::string stepName)
{
    auto command = surfacesCommand(std::move(surfaces), std::move(stepName));
    if (!command) {
        return command.error();
    }
    if (*command == nullptr) {
        return {}; // nothing changed, and nothing to undo
    }
    return execute(std::move(*command));
}

namespace {

// The styles' state as a step keeps it: the set, whether the project's could
// be read, and the key's text when it could not (kept in the metadata, to be
// written back as it was).
struct StylesState {
    SurfaceStyleSet styles;
    Status status;
    std::optional<std::string> kept;
};

} // namespace

Status Document::setSurfacesAndStyles(SurfaceStore surfaces, SurfaceStyleSet styles,
                                      std::string stepName)
{
    if (!surfaceStylesStatus_ && surfaceStylesStatus_.error().code == ErrorCode::Unsupported) {
        return makeError(ErrorCode::CommandRejected,
                         "the surface styles were written by a newer version of Katana; they are "
                         "kept as they are",
                         surfaceStylesStatus_.error().describe());
    }
    if (auto status = checkSurfaceStyleSet(styles); !status) {
        return status;
    }
    // The surfaces are checked against the NEW styles: a rename gives a
    // surface a style name only the new set has.
    if (auto status = checkStyleNames(surfaces, surfaces_, styles); !status) {
        return status;
    }
    if (auto status = checkStylesInUse(styles, surfaceStyles_, surfaces); !status) {
        return status;
    }
    const bool surfacesMove = !sameSurfaces(surfaces, surfaces_);
    const bool stylesMove = !(styles == surfaceStyles_) || !surfaceStylesStatus_;
    if (!surfacesMove && !stylesMove) {
        return {};
    }
    auto before = std::make_shared<StylesState>();
    before->styles = surfaceStyles_;
    before->status = surfaceStylesStatus_;
    if (const auto found = metadata_.unknownKeys.find(std::string(kStylesKey));
        found != metadata_.unknownKeys.end()) {
        before->kept = found->second;
    }
    auto after = std::make_shared<StylesState>();
    after->styles = std::move(styles);
    auto surfaceStates = std::make_shared<std::pair<SurfaceStore, SurfaceStore>>(
        surfaces_, std::move(surfaces));
    auto apply = [this, before, after, surfaceStates, surfacesMove, stylesMove](bool toAfter) {
        if (surfacesMove) {
            surfaces_ = toAfter ? surfaceStates->second : surfaceStates->first;
            ++surfacesGeneration_;
        }
        if (stylesMove) {
            const StylesState& state = toAfter ? *after : *before;
            surfaceStyles_ = state.styles;
            surfaceStylesStatus_ = state.status;
            if (state.kept) {
                metadata_.unknownKeys.insert_or_assign(std::string(kStylesKey), *state.kept);
            } else {
                metadata_.unknownKeys.erase(std::string(kStylesKey));
            }
            ++surfaceStylesGeneration_;
        }
    };
    return execute(std::make_unique<SurfaceStateCommand>(std::move(stepName), std::move(apply)));
}

Status Document::setSurfaceStyles(SurfaceStyleSet styles, std::string stepName)
{
    return setSurfacesAndStyles(surfaces_, std::move(styles), std::move(stepName));
}

// ---- new, open and save -------------------------------------------------------------------

Result<Document::SurfaceState>
Document::readSurfaceState(std::vector<katana::storage::SurfaceRecord>& records,
                           katana::storage::ProjectMetadata& metadata)
{
    SurfaceState state;
    for (katana::storage::SurfaceRecord& record : records) {
        auto surface = katana::terrain::decodeTin(record.data);
        if (!surface) {
            return makeError(surface.error().code,
                             "surface " + record.name +
                                 " in the project cannot be read: " + surface.error().message,
                             surface.error().context);
        }
        // The bytes are done with once decoded: a project's surfaces can be
        // tens of megabytes, held twice for no reason otherwise.
        std::string().swap(record.data);
        NamedSurface named;
        named.name = std::move(record.name);
        // A name this build's door (SurfaceStore::checkName) would refuse but an
        // earlier build let through - the bytes of a katana_cli on Windows
        // typed in the ANSI code page, or a double quote from an importer - is
        // repaired rather than refusing the open: the person could not
        // otherwise reach the project, nor rename the surface, because a line
        // cannot name it. The repair is logged, and visible in SURFACE LIST.
        if (!SurfaceStore::checkName(named.name)) {
            std::string repaired = state.surfaces.uniqueName(named.name);
            state.notes.push_back("surface name repaired on open: \"" + repaired + "\"");
            named.name = std::move(repaired);
        }
        named.surface =
            std::make_shared<const katana::terrain::TinSurface>(std::move(surface).value());
        // Free text, not a key: read as the Windows-1252 it was when it is not
        // UTF-8, so the project's own JSON replies and its next save cannot
        // throw on it.
        named.source = katana::core::utf8OrWindows1252(record.source);
        named.style = katana::core::utf8OrWindows1252(record.style);
        named.visible = record.visible;
        if (auto added = state.surfaces.add(std::move(named)); !added) {
            return makeError(added.error().code,
                             "the project's surfaces cannot be read: " + added.error().message,
                             added.error().context);
        }
    }
    const auto found = metadata.unknownKeys.find(std::string(kStylesKey));
    if (found == metadata.unknownKeys.end()) {
        return state; // the built-ins: a project saved before there were styles
    }
    if (found->second.empty()) {
        metadata.unknownKeys.erase(found); // the built-ins, said so by a save
        return state;
    }
    auto styles = surfaceStylesFromJson(found->second);
    if (!styles) {
        // The built-ins stand in, and the key stays to be written back as it
        // was: a newer Katana's styles are not this build's to lose, and a
        // damaged set is not repaired by guessing.
        state.stylesStatus = styles.error();
        return state;
    }
    state.styles = std::move(*styles);
    metadata.unknownKeys.erase(found);
    return state;
}

void Document::installSurfaceState(SurfaceState state)
{
    surfaces_ = std::move(state.surfaces);
    ++surfacesGeneration_;
    surfaceStyles_ = std::move(state.styles);
    surfaceStylesStatus_ = std::move(state.stylesStatus);
    ++surfaceStylesGeneration_;
}

Status Document::writeSurfaceState(katana::storage::ProjectContents& contents) const
{
    contents.surfaces.reserve(surfaces_.all().size());
    for (const NamedSurface& named : surfaces_.all()) {
        katana::storage::SurfaceRecord record;
        record.name = named.name;
        record.source = named.source;
        record.style = named.style;
        record.visible = named.visible;
        record.data = katana::terrain::encodeTin(*named.surface);
        contents.surfaces.push_back(std::move(record));
    }
    // Styles that could not be read are in the metadata still, and go back
    // as they came. Read ones are the Document's, written every time.
    if (surfaceStylesStatus_) {
        // The set is one checkSurfaceStyleSet accepted on the way in, so this
        // is a guard and not an expected refusal. It is here because a save
        // promises a Status and the JSON writer must never be what ends it:
        // text that is not UTF-8 is a refusal that names the style, rather than
        // either an exception out of the save or a name silently rewritten
        // with U+FFFD in the user's project.
        if (auto status = checkSurfaceStyleSet(surfaceStyles_); !status) {
            return status;
        }
        contents.metadata.unknownKeys.insert_or_assign(
            std::string(kStylesKey),
            surfaceStyles_ == builtInSurfaceStyles() ? std::string()
                                                     : surfaceStylesToJson(surfaceStyles_));
    }
    return {};
}

std::uint32_t Document::surfacePartsChanged()
{
    std::uint32_t parts = 0;
    if (surfacesGeneration_ != surfacesNotified_) {
        parts |= DocumentChange::Surfaces;
        surfacesNotified_ = surfacesGeneration_;
    }
    if (surfaceStylesGeneration_ != surfaceStylesNotified_) {
        parts |= DocumentChange::SurfaceStyles;
        surfaceStylesNotified_ = surfaceStylesGeneration_;
    }
    return parts;
}

} // namespace katana::cad
