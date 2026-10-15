#pragma once

// What a leader reads beyond the entity its tip is on, and the leaders made
// for the questions a drawing is asked most (docs/annotation.md, "What a
// leader can say" and "Presets").
//
// leaderContext is the drawing's surfaces and alignments as the entity
// layer's LeaderContext (entity/leader_values.hpp: {surface.NAME},
// {dsurface.NAME}, {cut.NAME}, {fill.NAME}, {station.NAME}, {offset.NAME},
// {profile.NAME}, {dprofile.NAME}). Every caller that shows what a leader
// says - the plan painter and so plots and sheets, LIST and LEADER, the
// property panel, the Leaders manager, the tools, the DXF export and the IFC
// export (app::setLeaderNotes) - asks it of the one Document, so they cannot
// say different things.
//
// A PRESET is a named template: the level of a point, its height above a
// surface, the cut or the fill to one, its name, its code, its details, its
// station and offset. LEADER preset=, LEADER FOR preset=, the Annotate
// menu's preset leader tools and the Leaders manager all make one through
// resolveLeaderPreset, so a tool clicked and a line typed make the same
// leader, and what the note says stays live like any smart leader's.

#include <cstddef>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/cad/scope_verbs.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/leader_values.hpp"
#include "katana/entity/model.hpp"
#include "katana/terrain/surface_store.hpp"

namespace katana::cad {
class Document;
}

namespace katana::cad::annotation {

// The drawing's surfaces (Document::surfaces) and alignments (the model's
// table) as a leader reads them. The context keeps a copy of the surface
// store - a pointer a surface, each shared and immutable - and of the
// alignments' definitions, each solved once, when a leader first reads it,
// so it reads what the drawing was when it was made: make it where it is
// used, not once for a session.
[[nodiscard]] katana::entity::LeaderContext leaderContext(const Document& document);
[[nodiscard]] katana::entity::LeaderContext
leaderContext(const katana::entity::Model& model, const katana::terrain::SurfaceStore& surfaces);

enum class LeaderPreset { Level, Surface, CutFill, Name, Code, Details, Station };

// What a preset reads besides its target.
enum class PresetReads { Target, Surface, Alignment };

struct LeaderPresetInfo {
    LeaderPreset preset;
    // As preset= takes it: "level".
    std::string_view word;
    // As the window shows it: "Level".
    std::string_view title;
    PresetReads reads;
    // The template, NAME standing for the surface or the alignment.
    std::string_view pattern;
    // The catalogue tool that makes it (interactive_tool.hpp), empty for
    // none.
    std::string_view tool;
    // One sentence: what the note says.
    std::string_view says;
};

// Every preset, in the order HELP, LEADER PRESETS and the window list them.
[[nodiscard]] std::span<const LeaderPresetInfo> leaderPresets();
[[nodiscard]] const LeaderPresetInfo& presetInfo(LeaderPreset preset);
// By its word, ignoring case; InvalidArgument naming the words for another.
[[nodiscard]] katana::core::Result<LeaderPreset> leaderPresetNamed(std::string_view word);

// The template of `preset` reading `name` - the surface or the alignment;
// ignored for a preset that reads only its target. InvalidArgument for a
// name the template language cannot write: empty, or holding ':', '{' or
// '}' (leader_values.hpp).
[[nodiscard]] katana::core::Result<std::string> presetTemplate(LeaderPreset preset,
                                                               std::string_view name = {});

// A preset made for a drawing.
struct ResolvedPreset {
    LeaderPreset preset = LeaderPreset::Level;
    std::string templateText;
    // The surface or the alignment it reads, as the drawing names it; empty
    // for a preset that reads only its target.
    std::string reads;
    // Set when none was named and the first visible surface, or the first
    // alignment by name, was taken: "first visible surface". Said in the
    // reply, so a leader is never quietly made against a surface nobody
    // chose.
    std::string chosen;
};

// `preset` for `document`, reading `surface` (for a preset that reads a
// surface) or `alignment` (one that reads an alignment). The name must be
// the drawing's (a surface ignoring case, an alignment exactly, as each is
// found); none given takes the first visible surface, or the first
// alignment by name, and says so. Refused: a name the preset does not read
// (surface= for preset=level), a surface or alignment the drawing has not,
// and a drawing with none to take (or every surface hidden).
[[nodiscard]] katana::core::Result<ResolvedPreset>
resolveLeaderPreset(const Document& document, LeaderPreset preset,
                    const std::optional<std::string>& surface = std::nullopt,
                    const std::optional<std::string>& alignment = std::nullopt);

// The preset `templateText` is, and the name it reads; nullopt for a
// template no preset makes. What LEADER LIST and the Leaders manager say a
// leader is.
struct RecognisedPreset {
    LeaderPreset preset = LeaderPreset::Level;
    std::string reads;
};
[[nodiscard]] std::optional<RecognisedPreset> recognisePreset(std::string_view templateText);

// The leaders a scope and filter take (LEADER LIST, VALUES, FREEZE ... and
// MCP's katana_leaders over `scope`): cad::matchScope - the one scope
// grammar and matcher, so a view, layers, an area, the selection or the
// drawing, and WHERE, mean here what they mean in Global Modify - with what
// it matched narrowed to the leaders among it, so the `matched=` of the
// scope's record counts leaders. A scope that takes none is no failure.
[[nodiscard]] katana::core::Result<katana::cad::ScopeMatch>
matchLeaders(const Document& document, const katana::cad::ScopeWords& words,
             const katana::cad::ScopeViewProvider& views);

// ---- the JSON ------------------------------------------------------------------------------
//
// The report is plain data and the writer a function taking it, so the one
// thing the writer must do - write a byte that is not UTF-8 as U+FFFD
// instead of throwing - can be handed such a byte by a test: nothing in a
// Document can hold one any more (SurfaceStore::checkName and the repair on
// open), so no drawing could reach the guard.

// One leader as the JSON says it.
struct LeaderReport {
    katana::entity::EntityId id = 0;
    std::string layer{};
    // The first vertex; absent for a leader with none.
    std::optional<katana::geometry::Point2> tip{};
    std::size_t vertices = 0;
    // The entity the tip is on and where on it, absent for a plain leader.
    std::optional<katana::entity::EntityId> target{};
    std::optional<std::string> anchor{};
    bool smart = false;
    // Absent for a leader whose note is plain text.
    std::optional<std::string> templateText{};
    std::optional<std::string> labelStyle{};
    // The preset the template is, and the surface or alignment it reads.
    std::optional<std::string> preset{};
    std::optional<std::string> reads{};
    std::string arrow{};
    std::string callout{};
    // What the note says now, whole and line by line.
    std::string text{};
    std::vector<std::string> lines{};
};

// The scope the leaders were chosen by: its record (cad::scopeRecord, as the
// verbs reply it) and how many leaders it took.
struct LeadersScopeReport {
    std::string record{};
    std::size_t matched = 0;
};

struct LeadersReport {
    std::vector<LeaderReport> leaders{};
    std::vector<std::string> surfaces{};
    std::vector<std::string> alignments{};
    std::optional<LeadersScopeReport> scope{};
};

// The drawing's leaders as a report: `only` the leaders of those ids when
// set - and then NONE when it is empty, which is what a scope that took
// nothing must give - and, when `target` is given, only those on it.
[[nodiscard]] LeadersReport
leadersReport(const Document& document,
              const std::optional<std::vector<katana::entity::EntityId>>& only = std::nullopt,
              std::optional<katana::entity::EntityId> target = std::nullopt);

// The report as JSON (LEADER LIST JSON, MCP's katana_leaders):
// {"leaders": [{id, layer, tip, vertices, target, anchor, smart, template,
// labelstyle, preset, reads, arrow, callout, text, lines}], "presets":
// [{name, title, reads, template, tool, says}], "surfaces": [...],
// "alignments": [...]} and, when the leaders were chosen by a scope,
// "scope": {"record": "scope=drawing matched=3", "matched": 3}. Absent
// members are null. A reply, not a stored value, so U+FFFD for a byte that
// is not UTF-8 and never an exception (docs/cad.md).
[[nodiscard]] std::string leadersJsonOf(const LeadersReport& report);

// leadersReport then leadersJsonOf, for the drawing's leaders `only` those of
// the ids when not empty (an empty list is all of them, as it always was) and
// only those on `target` when given.
[[nodiscard]] std::string leadersJson(const Document& document,
                                      const std::vector<katana::entity::EntityId>& only = {},
                                      std::optional<katana::entity::EntityId> target = std::nullopt);

} // namespace katana::cad::annotation
