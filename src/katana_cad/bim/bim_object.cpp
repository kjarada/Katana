// The road model as one: bim::bimModel, made once per change of what it is
// made from and shared (include/katana/cad/bim/bim_object.hpp), and how the
// views show it (include/katana/cad/bim/bim_view.hpp).

#include "katana/cad/bim/bim_object.hpp"

#include <array>
#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>

#include "katana/cad/bim/bim_view.hpp"
#include "katana/cad/document.hpp"
#include "katana/render/framebuffer.hpp"

namespace katana::cad {

// What the road model was last made from, and what was made. Two halves,
// each kept on what it is made from, because they are made from different
// things: the corridors from the templates, the corridors, the alignments and
// the surfaces - so a line drawn does not make them again - and the placed
// objects from every step (modelRevision). That is wider than they need: an
// object copies the line it is placed along and reads only its key, the
// alignments and the surfaces (road_objects.cpp, placementContext), so it is
// a conservative key, not a dependency - docs/road_objects.md, "Not done".
// A half made again that equals what it replaced keeps the old one, so the
// model a view holds stays the same object until something in it changes.
struct Document::BimCache {
    std::array<std::optional<std::string>, 2> corridorTexts; // road_templates, corridors
    std::uint64_t alignments = 0;
    std::uint64_t corridorSurfaces = 0;
    std::shared_ptr<const bim::BimModel> corridors;

    std::optional<std::string> objectsText; // road_objects
    std::uint64_t modelRevision = 0;
    std::uint64_t placedSurfaces = 0;
    std::shared_ptr<const bim::BimModel> placed;

    std::shared_ptr<const bim::BimModel> combined;
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

} // namespace

class BimCacheAccess {
  public:
    static std::shared_ptr<const BimModel> model(const Document& document)
    {
        std::shared_ptr<Document::BimCache>& slot = document.bimCache_;
        if (!slot) {
            slot = std::make_shared<Document::BimCache>();
        }
        Document::BimCache& cache = *slot;

        // Every comparison is made, so each key is brought up to date.
        bool corridorsFresh = cache.corridors != nullptr;
        corridorsFresh =
            same(cache.corridorTexts[0], keyText(document, kTemplatesKey)) && corridorsFresh;
        corridorsFresh =
            same(cache.corridorTexts[1], keyText(document, kCorridorsKey)) && corridorsFresh;
        corridorsFresh = corridorsFresh &&
                         cache.alignments == document.model().alignments.revision() &&
                         cache.corridorSurfaces == document.surfacesGeneration();
        bool changed = false;
        if (!corridorsFresh) {
            cache.corridors = std::make_shared<const BimModel>(corridorObjects(document));
            cache.alignments = document.model().alignments.revision();
            cache.corridorSurfaces = document.surfacesGeneration();
            changed = true;
        }

        bool placedFresh = cache.placed != nullptr;
        placedFresh = same(cache.objectsText, keyText(document, kObjectsKey)) && placedFresh;
        placedFresh = placedFresh && cache.modelRevision == document.modelRevision() &&
                      cache.placedSurfaces == document.surfacesGeneration();
        if (!placedFresh) {
            BimModel made = placedObjects(document);
            if (cache.placed == nullptr || !(*cache.placed == made)) {
                cache.placed = std::make_shared<const BimModel>(std::move(made));
                changed = true;
            }
            cache.modelRevision = document.modelRevision();
            cache.placedSurfaces = document.surfacesGeneration();
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
};

std::shared_ptr<const BimModel> bimModel(const Document& document)
{
    return BimCacheAccess::model(document);
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
