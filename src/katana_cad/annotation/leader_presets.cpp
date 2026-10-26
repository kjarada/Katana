#include "katana/cad/annotation/leader_presets.hpp"

#include <algorithm>
#include <array>
#include <map>
#include <memory>
#include <mutex>
#include <utility>

#include <nlohmann/json.hpp>

#include "katana/cad/annotation/leader_edit.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/anchor.hpp"
#include "katana/entity/tables.hpp"
#include "katana/entity/text_block.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"

namespace katana::cad::annotation {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::entity::EntityId;
using katana::entity::LeaderContext;
using katana::entity::LeaderGeometry;
using katana::geometry::Point2;
using Json = nlohmann::json;

namespace {

// The presets. Each template is a choice, written once here: a level to the
// millimetre as a survey plan writes one ("RL 12.345"); a difference with
// its sign always written, so above and below read apart at a glance; cut
// and fill on two lines of which only the one that holds is said; a station
// in chainage form and its offset signed, left positive (leader_values.hpp).
constexpr std::array kPresets = {
    LeaderPresetInfo{LeaderPreset::Level, "level", "Level", PresetReads::Target, "RL {rl:.3f}",
                     "annotate.leader.level", "the level (z) of the point the tip is on"},
    LeaderPresetInfo{LeaderPreset::Surface, "surface", "Surface Difference", PresetReads::Surface,
                     "{dsurface.NAME:+.3f}", "annotate.leader.surface",
                     "how far the point is above (+) or below (-) the surface"},
    LeaderPresetInfo{LeaderPreset::CutFill, "cutfill", "Cut/Fill", PresetReads::Surface,
                     "CUT {cut.NAME:.3f}\nFILL {fill.NAME:.3f}", "annotate.leader.cutfill",
                     "the cut down to the point where it is below the surface, else the fill "
                     "up to it"},
    LeaderPresetInfo{LeaderPreset::Name, "name", "Point Name", PresetReads::Target, "{point}",
                     "annotate.leader.name", "the point's name (its point number)"},
    LeaderPresetInfo{LeaderPreset::Code, "code", "Code", PresetReads::Target, "{code}",
                     "annotate.leader.code", "the survey code of what the tip is on"},
    LeaderPresetInfo{LeaderPreset::Details, "details", "Point Details", PresetReads::Target,
                     "{point}\n{code}\nRL {rl:.3f}", "",
                     "the point's name, code and level, each line only where it has one"},
    LeaderPresetInfo{LeaderPreset::Station, "station", "Station", PresetReads::Alignment,
                     "CH {station.NAME:ch}\nOS {offset.NAME:+.3f}", "annotate.leader.station",
                     "the station along the alignment and the offset from it, left positive"},
};

constexpr std::string_view kName = "NAME";

// `pattern` with each NAME replaced by `name`.
std::string instantiate(std::string_view pattern, std::string_view name)
{
    std::string text;
    std::size_t from = 0;
    while (true) {
        const std::size_t at = pattern.find(kName, from);
        if (at == std::string_view::npos) {
            text += pattern.substr(from);
            return text;
        }
        text += pattern.substr(from, at - from);
        text += name;
        from = at + kName.size();
    }
}

// The alignments of a model, each solved once - when a leader first asks
// for it - and kept. Shared by every copy of the context the one function
// is copied into, and locked: a painter may read leaders on more than one
// thread.
class AlignmentReader {
  public:
    explicit AlignmentReader(const katana::entity::AlignmentDatabase& table)
    {
        table.forEach([&](const katana::entity::Alignment& alignment) {
            definitions_.emplace(alignment.name, alignment);
        });
    }

    std::optional<katana::entity::AlignmentPlace> place(std::string_view name, const Point2& at)
    {
        const Solved* solved = solve(name);
        if (solved == nullptr) {
            return std::nullopt;
        }
        const auto where = solved->horizontal.stationOffset(at);
        if (!where) {
            return std::nullopt;
        }
        katana::entity::AlignmentPlace place{where->station, where->offset, std::nullopt};
        if (solved->profile) {
            place.profile = solved->profile->elevationAt(where->station);
        }
        return place;
    }

  private:
    struct Solved {
        katana::geometry::SolvedAlignment horizontal;
        std::optional<katana::geometry::SolvedProfile> profile;
    };

    // Null for no alignment of that name, or one that does not solve (a
    // stored one always does: the table validates it on the way in).
    const Solved* solve(std::string_view name)
    {
        const std::lock_guard lock(mutex_);
        if (const auto found = solved_.find(name); found != solved_.end()) {
            return found->second ? &*found->second : nullptr;
        }
        std::optional<Solved> made;
        if (const auto definition = definitions_.find(name); definition != definitions_.end()) {
            if (auto horizontal = katana::geometry::solveAlignment(definition->second.horizontal)) {
                made = Solved{std::move(*horizontal), std::nullopt};
                const auto& vertical = definition->second.vertical;
                if (vertical) {
                    if (auto profile = katana::geometry::solveProfile(*vertical)) {
                        made->profile = std::move(*profile);
                    }
                }
            }
        }
        // A map's nodes stay where they are, so the pointer handed out is
        // good for the reader's life.
        const auto inserted = solved_.emplace(std::string(name), std::move(made)).first;
        return inserted->second ? &*inserted->second : nullptr;
    }

    std::map<std::string, katana::entity::Alignment, std::less<>> definitions_;
    std::mutex mutex_;
    std::map<std::string, std::optional<Solved>, std::less<>> solved_;
};

} // namespace

LeaderContext leaderContext(const katana::entity::Model& model,
                            const katana::terrain::SurfaceStore& surfaces)
{
    LeaderContext context;
    for (const katana::terrain::NamedSurface& named : surfaces.all()) {
        context.surfaces.push_back(named.name);
    }
    context.alignments = model.alignments.names();
    // A copy: what the context reads cannot change or go under it.
    context.surfaceLevel = [store = surfaces](std::string_view name,
                                              const Point2& at) -> std::optional<double> {
        const katana::terrain::NamedSurface* found = store.find(name);
        if (found == nullptr || !found->surface) {
            return std::nullopt;
        }
        return found->surface->elevationAt(at);
    };
    context.alignmentPlace = [reader = std::make_shared<AlignmentReader>(model.alignments)](
                                 std::string_view name, const Point2& at) {
        return reader->place(name, at);
    };
    return context;
}

LeaderContext leaderContext(const Document& document)
{
    return leaderContext(document.model(), document.surfaces());
}

std::span<const LeaderPresetInfo> leaderPresets()
{
    return kPresets;
}

const LeaderPresetInfo& presetInfo(LeaderPreset preset)
{
    for (const LeaderPresetInfo& info : kPresets) {
        if (info.preset == preset) {
            return info;
        }
    }
    return kPresets.front();
}

Result<LeaderPreset> leaderPresetNamed(std::string_view word)
{
    const std::string lower = katana::core::lowered(word);
    std::string words;
    for (const LeaderPresetInfo& info : kPresets) {
        if (info.word == lower) {
            return info.preset;
        }
        words += words.empty() ? "" : "|";
        words += info.word;
    }
    return makeError(ErrorCode::InvalidArgument, "no such leader preset (preset=" + words + ")",
                     std::string(word));
}

Result<std::string> presetTemplate(LeaderPreset preset, std::string_view name)
{
    const LeaderPresetInfo& info = presetInfo(preset);
    if (info.reads == PresetReads::Target) {
        return std::string(info.pattern);
    }
    if (name.empty() || name.find_first_of(":{}") != std::string_view::npos) {
        return makeError(ErrorCode::InvalidArgument,
                         std::string("a template cannot name a ") +
                             (info.reads == PresetReads::Surface ? "surface" : "alignment") +
                             " that is unnamed or whose name holds ':', '{' or '}': rename it",
                         std::string(name));
    }
    return instantiate(info.pattern, name);
}

Result<ResolvedPreset> resolveLeaderPreset(const Document& document, LeaderPreset preset,
                                           const std::optional<std::string>& surface,
                                           const std::optional<std::string>& alignment)
{
    const LeaderPresetInfo& info = presetInfo(preset);
    const std::string word(info.word);
    if (surface && info.reads != PresetReads::Surface) {
        return makeError(ErrorCode::InvalidArgument,
                         "surface= is for preset=surface and preset=cutfill",
                         "preset=" + word);
    }
    if (alignment && info.reads != PresetReads::Alignment) {
        return makeError(ErrorCode::InvalidArgument, "alignment= is for preset=station",
                         "preset=" + word);
    }
    ResolvedPreset resolved;
    resolved.preset = preset;
    if (info.reads == PresetReads::Surface) {
        const katana::terrain::SurfaceStore& store = document.surfaces();
        if (surface) {
            const katana::terrain::NamedSurface* found = store.find(*surface);
            if (found == nullptr) {
                return makeError(ErrorCode::NotFound,
                                 "the drawing has no surface of that name (SURFACE LIST lists "
                                 "them)",
                                 *surface);
            }
            resolved.reads = found->name;
        } else {
            const auto& all = store.all();
            const auto visible = std::find_if(all.begin(), all.end(),
                                              [](const auto& named) { return named.visible; });
            if (visible == all.end()) {
                return makeError(ErrorCode::InvalidState,
                                 all.empty() ? "preset=" + word +
                                                   " reads a surface and the drawing has none "
                                                   "(SURFACE FROM makes one)"
                                             : "preset=" + word +
                                                   " reads a surface and every surface is "
                                                   "hidden: name one with surface=");
            }
            resolved.reads = visible->name;
            resolved.chosen = "first visible surface";
        }
    } else if (info.reads == PresetReads::Alignment) {
        const auto& table = document.model().alignments;
        if (alignment) {
            if (!table.contains(*alignment)) {
                return makeError(ErrorCode::NotFound,
                                 "the drawing has no alignment of that name (ALIGN LIST lists "
                                 "them)",
                                 *alignment);
            }
            resolved.reads = *alignment;
        } else {
            const std::vector<std::string> names = table.names();
            if (names.empty()) {
                return makeError(ErrorCode::InvalidState,
                                 "preset=station reads an alignment and the drawing has none");
            }
            resolved.reads = names.front();
            resolved.chosen = "first alignment";
        }
    }
    auto text = presetTemplate(preset, resolved.reads);
    if (!text) {
        return text.error();
    }
    resolved.templateText = std::move(*text);
    return resolved;
}

std::optional<RecognisedPreset> recognisePreset(std::string_view templateText)
{
    for (const LeaderPresetInfo& info : kPresets) {
        if (info.reads == PresetReads::Target) {
            if (templateText == info.pattern) {
                return RecognisedPreset{info.preset, {}};
            }
            continue;
        }
        // The name between what comes before the first NAME and what comes
        // after it, checked by making the template again from it: a pattern
        // naming it twice (cut and fill) must name the same one twice.
        const std::size_t at = info.pattern.find(kName);
        const std::string_view before = info.pattern.substr(0, at);
        const std::string_view after = info.pattern.substr(at + kName.size());
        const std::size_t end = templateText.find(after.substr(0, after.find('\n')),
                                                  before.size());
        if (!templateText.starts_with(before) || end == std::string_view::npos ||
            end <= before.size()) {
            continue;
        }
        const std::string name(templateText.substr(before.size(), end - before.size()));
        if (instantiate(info.pattern, name) == templateText) {
            return RecognisedPreset{info.preset, name};
        }
    }
    return std::nullopt;
}

Result<katana::cad::ScopeMatch> matchLeaders(const Document& document,
                                             const katana::cad::ScopeWords& words,
                                             const katana::cad::ScopeViewProvider& views)
{
    auto match = katana::cad::matchScope(document, words, views);
    if (!match) {
        return match.error();
    }
    const auto& entities = document.model().entities;
    std::erase_if(match->matched, [&](EntityId id) {
        const katana::entity::Entity* entity = entities.find(id);
        return entity == nullptr || !std::holds_alternative<LeaderGeometry>(entity->geometry);
    });
    return match;
}

LeadersReport leadersReport(const Document& document,
                            const std::optional<std::vector<EntityId>>& only,
                            std::optional<EntityId> target)
{
    const auto& model = document.model();
    const LeaderContext context = leaderContext(document);
    LeadersReport report;
    model.entities.forEach([&](const katana::entity::Entity& entity) {
        const auto* leader = std::get_if<LeaderGeometry>(&entity.geometry);
        if (leader == nullptr ||
            (only && std::find(only->begin(), only->end(), entity.id) == only->end()) ||
            (target && leader->tipRef.entity != *target)) {
            return;
        }
        LeaderReport record;
        record.id = entity.id;
        record.layer = entity.layer;
        if (!leader->vertices.empty()) {
            record.tip = leader->vertices.front();
        }
        record.vertices = leader->vertices.size();
        if (leader->tipRef.associated()) {
            record.target = leader->tipRef.entity;
            record.anchor = katana::entity::describe(leader->tipRef);
        }
        record.smart = katana::entity::isSmart(*leader);
        if (leader->fields) {
            record.templateText = leader->text;
        }
        if (!leader->labelStyle.empty()) {
            record.labelStyle = leader->labelStyle;
        }
        const auto recognised = leader->fields ? recognisePreset(leader->text) : std::nullopt;
        if (recognised) {
            record.preset = std::string(presetInfo(recognised->preset).word);
            if (!recognised->reads.empty()) {
                record.reads = recognised->reads;
            }
        }
        record.arrow = std::string(katana::entity::toString(leader->arrow));
        record.callout = std::string(katana::entity::toString(leader->callout));
        record.landingAngle = leader->landingAngle;
        record.points = leader->vertices;
        record.textStyle = leader->style;
        record.paperHeight = leader->paperHeight;
        record.arrowSize = leader->arrowSize;
        record.landing = leader->landing;
        record.text = leaderSays(model, *leader, context);
        if (!record.text.empty()) {
            for (const std::string_view line : katana::entity::textLines(record.text)) {
                record.lines.emplace_back(line);
            }
        }
        report.leaders.push_back(std::move(record));
    });
    report.surfaces = context.surfaces;
    report.alignments = context.alignments;
    return report;
}

std::string leadersJson(const Document& document, const std::vector<EntityId>& only,
                        std::optional<EntityId> target)
{
    return leadersJsonOf(leadersReport(
        document, only.empty() ? std::nullopt : std::optional<std::vector<EntityId>>(only),
        target));
}

std::string leadersJsonOf(const LeadersReport& report)
{
    const auto optionalText = [](const std::optional<std::string>& text) {
        return text ? Json(*text) : Json(nullptr);
    };
    Json leaders = Json::array();
    for (const LeaderReport& leader : report.leaders) {
        Json record;
        record["id"] = leader.id;
        record["layer"] = leader.layer;
        record["tip"] = leader.tip ? Json::array({leader.tip->x, leader.tip->y}) : Json(nullptr);
        record["vertices"] = leader.vertices;
        record["target"] = leader.target ? Json(*leader.target) : Json(nullptr);
        record["anchor"] = optionalText(leader.anchor);
        record["smart"] = leader.smart;
        record["template"] = optionalText(leader.templateText);
        record["labelstyle"] = optionalText(leader.labelStyle);
        record["preset"] = optionalText(leader.preset);
        record["reads"] = optionalText(leader.reads);
        record["arrow"] = leader.arrow;
        record["callout"] = leader.callout;
        record["text"] = leader.text;
        record["lines"] = leader.lines;
        record["points"] = Json::array();
        for (const auto& point : leader.points) {
            record["points"].push_back(Json::array({point.x, point.y}));
        }
        record["text_style"] = leader.textStyle;
        record["paper_height"] = leader.paperHeight;
        record["arrow_size"] = leader.arrowSize;
        record["landing"] = leader.landing;
        if (leader.landingAngle) {
            record["landing_angle"] = *leader.landingAngle;
        }
        leaders.push_back(std::move(record));
    }
    Json presets = Json::array();
    for (const LeaderPresetInfo& info : kPresets) {
        presets.push_back(Json{
            {"name", std::string(info.word)},
            {"title", std::string(info.title)},
            {"reads", info.reads == PresetReads::Target
                          ? "target"
                          : (info.reads == PresetReads::Surface ? "surface" : "alignment")},
            {"template", std::string(info.pattern)},
            {"tool", info.tool.empty() ? Json(nullptr) : Json(std::string(info.tool))},
            {"says", std::string(info.says)}});
    }
    Json result;
    result["leaders"] = std::move(leaders);
    result["presets"] = std::move(presets);
    result["surfaces"] = report.surfaces;
    result["alignments"] = report.alignments;
    if (report.scope) {
        result["scope"] = Json{{"record", report.scope->record}, {"matched", report.scope->matched}};
    }
    // A reply, not a stored value, so U+FFFD for a byte that is not UTF-8
    // (docs/cad.md: a name that is not UTF-8 is written so, not thrown).
    // nlohmann's default throws, and nothing up the call chain of a verb
    // catches: katana_cli and the window ended on a surface name typed in the
    // ANSI code page. The stores now refuse such a name (SurfaceStore::
    // checkName); this keeps a name that reached the drawing another way from
    // taking LEADER LIST down with it.
    return result.dump(2, ' ', false, Json::error_handler_t::replace);
}

} // namespace katana::cad::annotation
