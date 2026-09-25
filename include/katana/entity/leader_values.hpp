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
// included: a property's quantity is its own).
[[nodiscard]] std::optional<LabelQuantity> leaderValueQuantity(std::string_view name);

// Checks a leader's template: braces balanced, every field one of
// leaderValueNames() or a prop.NAME, every step applicable. Refused with the
// first problem, named, as a label style's template is.
[[nodiscard]] katana::core::Status checkLeaderTemplate(std::string_view templateText);

// The values `target` offers at the place `ref` names on it (ref.entity is
// not checked against target.id). `tip` is where the tip is now: the place
// when `ref` names none the target still has (a vertex since removed), in
// which case the nearest place on the target stands for it. `codeProperties`
// are where a survey code is looked for, as for a label (label_values.hpp).
[[nodiscard]] LabelValues anchorValues(const Entity& target, const AnchorRef& ref,
                                       const katana::geometry::Point2& tip,
                                       std::span<const std::string> codeProperties = {});

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

// The values of a leader's target at its tip; nullopt when the tip names no
// entity or that entity is gone.
[[nodiscard]] std::optional<LabelValues>
leaderValues(const Model& model, const LeaderGeometry& leader,
             std::span<const std::string> codeProperties = {});

// The note as it is drawn: a plain leader's text, verbatim; a smart leader's
// template filled from its target's values, a line naming a value the target
// lacks dropped - empty when every line was.
[[nodiscard]] std::string leaderNote(const Model& model, const LeaderGeometry& leader,
                                     std::span<const std::string> codeProperties = {});

// The value names `templateText` uses that `values` lacks, in the order the
// template names them: what a refusal to make a leader that would say
// nothing lists.
[[nodiscard]] std::vector<std::string> missingValues(std::string_view templateText,
                                                     const LabelValues& values);

// Refuses a smart leader that could not say anything: one whose tip names no
// entity, or names one that is gone, or whose note would be empty - with the
// values its template names that the target lacks, since a property the
// target does not carry is far more often a typo than an intent. As LABEL
// refuses a label that would say nothing. A plain leader always passes.
// What LEADER and the Leader tool both ask before making or changing one.
[[nodiscard]] katana::core::Status
checkLeaderSaysSomething(const Model& model, const LeaderGeometry& leader,
                         std::span<const std::string> codeProperties = {});

} // namespace katana::entity
