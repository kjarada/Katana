#pragma once

// What a smart leader's note can say about the entity its tip is on
// (docs/annotation.md, "Smart leaders").
//
// A leader whose tip names an entity (LeaderGeometry::tipRef) can have a
// note that is a TEMPLATE - its own (`fields`) or a label style's
// (`labelStyle`) - in the label template language (label_text.hpp). Its
// fields are the values of that entity AT THE TIP: the level of the point it
// touches, the bearing and grade of the segment it lands on, the chainage
// along a pipe to it, the area of the lot it points into, and every
// attribute the entity carries. They are worked out whenever the leader is
// drawn, so the note cannot disagree with the geometry or the attributes
// however either was edited - and, as a label, a line naming a value the
// target lacks is dropped rather than printed as nothing (absent is not
// zero).
//
// In the entity layer beside the label values for the same reason they are
// there: the values are the model's own numbers, and every consumer - the
// painters, the command line, a DXF writer that cannot see katana_cad - must
// read the same ones.

#include <functional>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/label_text.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::entity {

// ---- what a note reads beyond the entity it is on ------------------------------------
//
// A note may also read the drawing's SURFACES and ALIGNMENTS at its tip -
// the ground level under a pit, the cut down to a design point, the station
// of a kerb return - which this layer cannot see (tools/check_layering.cmake:
// entity sees neither terrain nor cad). They come in through a LeaderContext,
// two functions and two lists, which cad::annotation::leaderContext makes
// from a Document; every caller that shows what a leader says passes the
// document's, and one that cannot (the DXF writer's own fallback) passes
// none, so such a line says nothing there rather than something made up.
//
// The values are named KIND.NAME, NAME the surface's or the alignment's name
// as the drawing has it, written as it is, blanks and all:
// {dsurface.Existing Ground:+.3f}. A name holding ':', '{' or '}' cannot be
// written in a template (':' starts a step, the braces end the field).
//
//   surface.NAME    the level of surface NAME at the tip
//   dsurface.NAME   the target's level (z) minus it: positive ABOVE the surface
//   cut.NAME        how far the target is BELOW the surface, present only
//                   there: the cut down to it
//   fill.NAME       how far it is above, present only there (or exactly on
//                   the surface, a fill of nothing): so "CUT {cut.g:.2f}\n
//                   FILL {fill.g:.2f}" says one line, whichever it is
//   station.NAME    the station of the tip along alignment NAME
//   offset.NAME     its offset from the centreline: positive LEFT looking
//                   along increasing station, as a cross section has it
//   profile.NAME    the alignment's design profile level at that station
//   dprofile.NAME   the target's level minus it: positive above the design
//
// Absent is not zero: a tip off the surface (outside it, in a hole), beyond
// the alignment's ends, at a station the profile does not reach, or on a
// target with no level - each leaves the value out, and its line is dropped.

// Where a point is along an alignment, as a context reads it.
struct AlignmentPlace {
    double station = 0.0;
    // Positive LEFT looking along increasing station
    // (geometry::SolvedAlignment::stationOffset).
    double offset = 0.0;
    // The design profile's level at the station; absent with no profile, or
    // at a station it does not reach.
    std::optional<double> profile{};
};

struct LeaderContext {
    // The level of surface `name` at `at`; nullopt when the drawing has no
    // surface of that name or `at` is off it. Unset: no surface is read.
    std::function<std::optional<double>(std::string_view name, const katana::geometry::Point2& at)>
        surfaceLevel{};
    // Where `at` is along alignment `name`; nullopt when the drawing has no
    // such alignment or `at` is beyond its ends. Unset: no alignment is read.
    std::function<std::optional<AlignmentPlace>(std::string_view name,
                                                const katana::geometry::Point2& at)>
        alignmentPlace{};
    // The drawing's surfaces and alignments by name, in its order: what
    // LEADER VALUES and the Leaders manager offer values of, and what a
    // refusal checks a name against.
    std::vector<std::string> surfaces{};
    std::vector<std::string> alignments{};
};

// The KINDs above, in the order LEADER VALUES lists them.
[[nodiscard]] std::vector<std::string_view> leaderContextKinds();

// "dsurface.ground" -> {"dsurface", "ground"}; nullopt for a name that is
// not KIND.NAME with a known kind and a NAME.
struct ContextValueName {
    std::string_view kind;
    std::string_view name;
};
[[nodiscard]] std::optional<ContextValueName> contextValueName(std::string_view value);

// Every value name a leader's template may use besides prop.NAME, in the
// order LEADER VALUES and HELP list them. Which of them a target HAS depends
// on what it is and where the tip is on it (docs/annotation.md has the
// table):
//
//   every target  id layer type code point description, prop.NAME, and x y
//                 easting northing (the tip)
//   Point, Text   z rl (its level); a text's `text`
//   Line          bearing distance length dx dy segment chainage, and z rl
//                 dz grade from its heights
//   Arc           radius diameter length chord delta bearing tangent
//                 chainage, z rl from its heights
//   Circle        radius diameter length area perimeter, z rl
//   Polyline      the segment the tip is on: bearing distance dx dy dz grade
//                 segment; the whole: length vertices chainage z rl; a
//                 closed one's area and perimeter
//   Dimension     measurement (a length) or angle (an angular one's)
//
// `length` is the WHOLE entity's - a pipe's length, not the segment the tip
// landed on, which is `distance`. `chainage` is the length along the entity
// from its start to the tip.
[[nodiscard]] std::vector<std::string_view> leaderValueNames();

// What `name` is, nullopt when a leader has no such value (prop.NAME
// included: a property's quantity is its own). A KIND.NAME of the context
// is known whatever NAME is: whether the drawing has it is the context's to
// say, when the leader is drawn.
[[nodiscard]] std::optional<LabelQuantity> leaderValueQuantity(std::string_view name);

// Checks a leader's template: braces balanced, every field one of
// leaderValueNames(), a KIND.NAME of the context or a prop.NAME, every step
// applicable. Refused with the first problem, named, as a label style's
// template is.
[[nodiscard]] katana::core::Status checkLeaderTemplate(std::string_view templateText);

// The values `target` offers at the place `ref` names on it (ref.entity is
// not checked against target.id). `tip` is where the tip is now: the place
// when `ref` names none the target still has (a vertex since removed), in
// which case the nearest place on the target stands for it. `codeProperties`
// are where a survey code is looked for, as for a label (label_values.hpp).
// With a context, also every KIND.NAME of each surface and alignment it
// lists, read at that place.
[[nodiscard]] LabelValues anchorValues(const Entity& target, const AnchorRef& ref,
                                       const katana::geometry::Point2& tip,
                                       std::span<const std::string> codeProperties = {},
                                       const LeaderContext& context = {});

// The context values `names` name (others are passed over), read at `at`
// for a target whose own values are `values` - its level is values' "z" -
// added to `values`. What anchorValues does for every name a context lists,
// and leaderNote for only those its template names.
void addContextValues(LabelValues& values, const katana::geometry::Point2& at,
                      const LeaderContext& context, std::span<const std::string> names);

// Why the context value `name` is not in `values` read at `at`, in words -
// "the tip is off surface ground", "the drawing has no alignment MC01" -
// or empty when it is there or is no context value.
[[nodiscard]] std::string whyAbsent(std::string_view name, const LabelValues& values,
                                    const katana::geometry::Point2& at,
                                    const LeaderContext& context);

// Whether a leader's note is read off its target.
[[nodiscard]] inline bool isSmart(const LeaderGeometry& leader)
{
    return leader.fields || !leader.labelStyle.empty();
}

// The template a smart leader's note is made from: its own text, or its
// label style's; nullopt for a plain leader and for one whose label style is
// not in the model.
[[nodiscard]] std::optional<std::string> leaderTemplate(const Model& model,
                                                        const LeaderGeometry& leader);

// The values of a leader's target at its tip, with every value of the
// context (anchorValues); nullopt when the tip names no entity or that
// entity is gone.
[[nodiscard]] std::optional<LabelValues>
leaderValues(const Model& model, const LeaderGeometry& leader,
             std::span<const std::string> codeProperties = {}, const LeaderContext& context = {});

// The note as it is drawn: a plain leader's text, verbatim; a smart leader's
// template filled from its target's values and the context values it names,
// a line naming a value that is absent dropped - empty when every line was.
[[nodiscard]] std::string leaderNote(const Model& model, const LeaderGeometry& leader,
                                     std::span<const std::string> codeProperties = {},
                                     const LeaderContext& context = {});

// The value names `templateText` uses that `values` lacks, in the order the
// template names them: what a refusal to make a leader that would say
// nothing lists.
[[nodiscard]] std::vector<std::string> missingValues(std::string_view templateText,
                                                     const LabelValues& values);

// Refuses a smart leader that could not say anything: one whose tip names no
// entity, or names one that is gone, or whose note would be empty - with the
// values its template names that the target lacks, since a property the
// target does not carry is far more often a typo than an intent, and for a
// value of the context why it is absent (whyAbsent). As LABEL refuses a
// label that would say nothing. A plain leader always passes. What LEADER
// and the Leader tools all ask before making or changing one.
[[nodiscard]] katana::core::Status
checkLeaderSaysSomething(const Model& model, const LeaderGeometry& leader,
                         std::span<const std::string> codeProperties = {},
                         const LeaderContext& context = {});

} // namespace katana::entity
