// The road model as one: bim::bimModel, made once per change of what it is
// made from and shared (include/katana/cad/bim/bim_object.hpp), and how the
// views show it (include/katana/cad/bim/bim_view.hpp).

#include "katana/cad/bim/bim_object.hpp"

#include <algorithm>
#include <array>
#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/cad/bim/bim_view.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/road/corridor_model.hpp"
#include "katana/cad/road/road_objects.hpp"
#include "katana/render/framebuffer.hpp"
#include "katana/terrain/surface_store.hpp"

namespace katana::cad {

// What the road model was last made from, and what was made. Two halves,
// each kept on what it is made from, because they are made from different
// things (docs/road_design.md, "The cache"):
//   the corridors - the templates and the corridors (their text), the
//   alignments, and the surfaces the corridors TARGET (a corridor's `surface`,
//   its batters' daylight and its earthworks' ground; it reads no other);
//   the placed objects - the `road_objects` key (their text), the alignments
//   and the surfaces they NAME (RoadObjectPlacement::surface; a line they
//   follow was copied in when they were placed). Every other command - a line
//   drawn, a layer, a surface no one names - leaves both halves as they were.
// A surface is told apart by identity, not by its name or the store's
// generation: an edit of a surface replaces the object (SurfaceStore::replace),
// so a corridor's ground that is the same object is the same ground, and one
// that is not is not, whatever else in the store moved. An undo that puts back
// the old object is a change too. The cache holds each as a weak_ptr, which
// keeps the control block - and so the identity - alive after the surface
// itself is gone, so a new surface cannot be taken for the one it replaced by
// being made at the same address.
// A half made again that equals what it replaced keeps the old one, so the
// model a view holds stays the same object until something in it changes.
using Grounds = std::vector<std::weak_ptr<const katana::terrain::TinSurface>>;

struct Document::BimCache {
    std::array<std::optional<std::string>, 2> corridorTexts; // road_templates, corridors
    std::uint64_t alignments = 0;
    Grounds corridorGrounds;
    std::shared_ptr<const bim::BimModel> corridors;

    std::optional<std::string> objectsText; // road_objects
    std::uint64_t placedAlignments = 0;
    Grounds placedGrounds;
    std::shared_ptr<const bim::BimModel> placed;

    std::shared_ptr<const bim::BimModel> combined;

    // Times each half was made (bim::bimBuildCounts).
    bim::BimBuildCounts built;
};

namespace bim {

namespace {

// The metadata keys the two halves are defined under (road_document.cpp, and
// the placed objects' cad/road/road_objects.hpp).
constexpr std::string_view kTemplatesKey = "road_templates";
constexpr std::string_view kCorridorsKey = "corridors";
constexpr std::string_view kObjectsKey = "road_objects";

const std::string* keyText(const Document& document, std::string_view key)
{
    const auto& keys = document.metadata().unknownKeys;
    const auto found = keys.find(std::string(key));
    return found == keys.end() ? nullptr : &found->second;
}

// Whether `seen` is what the key holds now; set to it when not.
bool same(std::optional<std::string>& seen, const std::string* now)
{
    if (now == nullptr) {
        const bool was = !seen.has_value();
        seen.reset();
        return was;
    }
    if (seen && *seen == *now) {
        return true;
    }
    seen = *now;
    return false;
}

// The surface the drawing has under `name` (found as the verbs find it,
// ignoring case), or none for an empty name and one the drawing lacks.
void addGround(const Document& document, std::string_view name, Grounds& grounds)
{
    const katana::terrain::NamedSurface* found =
        name.empty() ? nullptr : document.surfaces().find(name);
    grounds.emplace_back(found != nullptr ? found->surface
                                          : std::shared_ptr<const katana::terrain::TinSurface>{});
}

// One for each corridor, in order: the surface it targets.
Grounds corridorGrounds(const Document& document)
{
    Grounds grounds;
    for (const road::Corridor& corridor : document.corridors().corridors) {
        addGround(document, corridor.surface, grounds);
    }
    return grounds;
}

// One for each placed object, in order: the surface it names.
Grounds placedGrounds(const Document& document)
{
    Grounds grounds;
    for (const road::RoadObject& object : document.roadObjects().objects) {
        addGround(document, object.placement.surface, grounds);
    }
    return grounds;
}

// The same surfaces: the same objects, or none in the same places.
bool sameGrounds(const Grounds& a, const Grounds& b)
{
    return std::ranges::equal(a, b, [](const auto& x, const auto& y) {
        return !x.owner_before(y) && !y.owner_before(x);
    });
}

} // namespace

class BimCacheAccess {
  public:
    static std::shared_ptr<const BimModel> model(const Document& document)
    {
        Document::BimCache& cache = slotOf(document);

        // Every comparison is made, so each key is brought up to date.
        bool corridorsFresh = cache.corridors != nullptr;
        corridorsFresh =
            same(cache.corridorTexts[0], keyText(document, kTemplatesKey)) && corridorsFresh;
        corridorsFresh =
            same(cache.corridorTexts[1], keyText(document, kCorridorsKey)) && corridorsFresh;
        const std::uint64_t alignments = document.model().alignments.revision();
        Grounds targets = corridorGrounds(document);
        corridorsFresh = corridorsFresh && cache.alignments == alignments &&
                         sameGrounds(cache.corridorGrounds, targets);
        bool changed = false;
        if (!corridorsFresh) {
            // Made again, but kept as the old one when it came out the same: an
            // alignment no corridor uses moves the revision, and a view that is
            // handed a different object for an unchanged model makes every 3D
            // view build its terrain again (RoadWorkbench).
            BimModel made = corridorObjects(document);
            ++cache.built.corridors;
            if (cache.corridors == nullptr || !(*cache.corridors == made)) {
                cache.corridors = std::make_shared<const BimModel>(std::move(made));
                changed = true;
            }
            cache.alignments = alignments;
            cache.corridorGrounds = std::move(targets);
        }

        bool placedFresh = cache.placed != nullptr;
        placedFresh = same(cache.objectsText, keyText(document, kObjectsKey)) && placedFresh;
        Grounds named = placedGrounds(document);
        placedFresh = placedFresh && cache.placedAlignments == alignments &&
                      sameGrounds(cache.placedGrounds, named);
        if (!placedFresh) {
            BimModel made = placedObjects(document);
            ++cache.built.placed;
            if (cache.placed == nullptr || !(*cache.placed == made)) {
                cache.placed = std::make_shared<const BimModel>(std::move(made));
                changed = true;
            }
            cache.placedAlignments = alignments;
            cache.placedGrounds = std::move(named);
        }

        if (changed || cache.combined == nullptr) {
            auto both = std::make_shared<BimModel>(*cache.corridors);
            both->objects.insert(both->objects.end(), cache.placed->objects.begin(),
                                 cache.placed->objects.end());
            both->problems.insert(both->problems.end(), cache.placed->problems.begin(),
                                  cache.placed->problems.end());
            cache.combined = std::move(both);
        }
        return cache.combined;
    }

    static BimBuildCounts counts(const Document& document) { return slotOf(document).built; }

  private:
    static Document::BimCache& slotOf(const Document& document)
    {
        std::shared_ptr<Document::BimCache>& slot = document.bimCache_;
        if (!slot) {
            slot = std::make_shared<Document::BimCache>();
        }
        return *slot;
    }
};

std::shared_ptr<const BimModel> bimModel(const Document& document)
{
    return BimCacheAccess::model(document);
}

BimBuildCounts bimBuildCounts(const Document& document)
{
    return BimCacheAccess::counts(document);
}

// ---- in the views ----------------------------------------------------------------------

bool isShown(const BimObject& object, const katana::entity::Model& model,
             const LayerOverrides& view)
{
    if (!object.layer.empty() && view.hides(object.layer)) {
        return false;
    }
    std::string_view path = object.layer;
    while (!path.empty()) {
        if (model.layers.contains(path)) {
            return model.layers.resolve(path).shown;
        }
        const auto slash = path.rfind('/');
        if (slash == std::string_view::npos) {
            break;
        }
        path = path.substr(0, slash);
    }
    return true;
}

std::vector<SceneMesh> sceneMeshes(const BimModel& objects, const katana::entity::Model& model,
                                   const LayerOverrides& view)
{
    std::vector<SceneMesh> meshes;
    for (const BimObject& object : objects.objects) {
        if (object.mesh.empty() || !isShown(object, model, view)) {
            continue;
        }
        SceneMesh mesh;
        mesh.name = object.name;
        mesh.mesh = &object.mesh;
        mesh.style = SurfaceStyle::Shaded;
        mesh.flatColor = katana::render::rgba(object.colour.r, object.colour.g, object.colour.b);
        mesh.planFootprint = false;
        mesh.layer = object.layer;
        meshes.push_back(std::move(mesh));
    }
    return meshes;
}

katana::geometry::Box2 planBounds(const BimModel& objects, const katana::entity::Model& model,
                                  const LayerOverrides& view)
{
    katana::geometry::Box2 box;
    for (const BimObject& object : objects.objects) {
        if (!isShown(object, model, view)) {
            continue;
        }
        if (object.plan.empty()) {
            const katana::math::AABB space = object.mesh.bounds();
            if (!space.empty()) {
                box.expand(katana::geometry::Point2(space.min.x, space.min.y));
                box.expand(katana::geometry::Point2(space.max.x, space.max.y));
            }
            continue;
        }
        for (const katana::geometry::Polyline2& line : object.plan) {
            box.expand(line.boundingBox());
        }
    }
    return box;
}

} // namespace bim

} // namespace katana::cad
