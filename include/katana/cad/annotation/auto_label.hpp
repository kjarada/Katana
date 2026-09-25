#pragma once

// Labels made by hand and by rule (docs/annotation.md, "Labels" and
// "Auto-labelling").
//
// Every function here returns a COMMAND, never an edited model: the caller
// executes it through Document::execute, so each is one validated,
// undoable step (Rule 2), and the command line, the window and an agent get
// the same result for the same request.

#include <cstddef>
#include <map>
#include <optional>
#include <string>
#include <vector>

#include "katana/commands/command.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::cad::annotation {

// A label to make by hand.
struct LabelRequest {
    katana::entity::EntityId target = 0; // or
    std::string alignment{};             // a chainage label's alignment
    std::string style{};
    std::int32_t part = -1;
    // The layer it goes on; empty puts it on its target's (an alignment's
    // label on "0").
    std::string layer{};
    std::optional<katana::geometry::Point2> position{};
    std::string textOverride{};
};

// One label as a command ("CREATE_LABEL"). Refused when the style or the
// target is missing, the style's kind cannot label the target (a Segment
// style on a point), the part does not exist, or the label would say nothing
// (every line of its template needs a value the target lacks) - so a label
// that could never draw is not made.
[[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
createLabel(const katana::entity::Model& model, const LabelRequest& request);

// Every request as ONE command ("CREATE_LABEL"), so one undo takes back the
// lot: what the LABEL verb and the Label Objects tool both make, so the two
// cannot differ. The first request refused refuses the whole, its context
// naming the target ("id=12 ..." or "alignment=MC01 ..."); InvalidArgument
// for no requests.
[[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
createLabels(const katana::entity::Model& model, const std::vector<LabelRequest>& requests);

// What AUTOLABEL RUN did or would do.
struct AutoLabelReport {
    std::size_t created = 0;
    std::size_t kept = 0;    // already there, left alone (a dragged label keeps its place)
    std::size_t removed = 0; // made by a rule that no longer matches its target
    // By rule name: the labels each rule has after the run.
    std::map<std::string, std::size_t> perRule{};
    // Targets a rule matched but could not label: their label layer is
    // locked, or the label would say nothing.
    std::size_t skipped = 0;
};

// Applies the rules named (every ENABLED rule when `rules` is empty) as ONE
// command ("AUTOLABEL"): each matching entity (or alignment, for a Chainage
// style) gets the rule's label unless it already has it; a label a rule made
// earlier that the rule no longer asks for is removed; labels placed by hand
// and labels of other rules are not touched. Re-running with nothing changed
// is nullptr with no error: there is nothing to undo. Refused for a rule
// that does not exist or names a label style that does not.
//
// A rule's label layer that does not exist yet is created in the same step.
// Matching is in entity id order, so two runs on one model make the same
// labels in the same order (Rule 7).
[[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
autoLabel(const katana::entity::Model& model, const std::vector<std::string>& rules,
          AutoLabelReport* report = nullptr);

// Removes the labels the named rules made (every rule's when empty) as one
// command ("AUTOLABEL_CLEAR"); nullptr with no error when there are none.
[[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
clearAutoLabels(const katana::entity::Model& model, const std::vector<std::string>& rules,
                std::size_t* removed = nullptr);

// Whether `rule` asks to label `entity` (its filters, and its style's kind).
[[nodiscard]] bool ruleMatches(const katana::entity::LabelRule& rule,
                               const katana::entity::LabelStyle& style,
                               const katana::entity::Entity& entity);

// The standard label styles LABELSTYLE DEFAULTS adds: point number, spot
// level, bearing and distance, arc data, lot area, chainage. Every size in
// paper millimetres, the templates in the formats survey plans use.
[[nodiscard]] std::vector<katana::entity::LabelStyle> defaultLabelStyles();

} // namespace katana::cad::annotation
