#pragma once

// Editing leaders (docs/annotation.md, "Smart leaders"): what the LEADER and
// BALLOON verbs and the window's Leaders manager both do. Each operation
// checks its edit against the model and returns ONE command - or refuses,
// saying why in the words the command line replies with - so a verb typed, a
// button pressed and an agent's call make the same change, and one undo
// takes it back. Nothing here executes: the caller runs the command through
// Document::execute, whose associative update then keeps every leader on
// what it is on.

#include <cstddef>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/annotation/dimension_build.hpp"
#include "katana/commands/command.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/label_text.hpp"
#include "katana/entity/model.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/math/numerics.hpp"

namespace katana::cad::annotation {

// What a leader's note is: literal text, a template read off the entity its
// tip is on, or a label style's template (LeaderGeometry::fields and
// ::labelStyle).
struct LeaderNote {
    enum class Kind { Text, Template, LabelStyle };
    Kind kind = Kind::Text;
    // The literal note, or the template; for LabelStyle the style's name.
    std::string text{};

    friend bool operator==(const LeaderNote&, const LeaderNote&) = default;
};

// The note `leader` has now.
[[nodiscard]] LeaderNote noteOf(const katana::entity::LeaderGeometry& leader);

// A change to leaders: each member that is set is applied, each unset one
// left as it is.
struct LeaderChange {
    std::optional<LeaderNote> note{};
    std::optional<katana::entity::ArrowHead> arrow{};
    std::optional<katana::entity::CalloutShape> callout{};
    // The text style; empty for the default.
    std::optional<std::string> textStyle{};
    // Millimetres on paper.
    std::optional<double> paperHeight{};
    std::optional<double> arrowSize{};
    std::optional<double> landing{};
    // Moves the tip, and says what it is on (nothing, for a plain point).
    std::optional<AnchoredPoint> tip{};
    // Moves the last vertex: where the note hangs.
    std::optional<katana::geometry::Point2> hang{};

    [[nodiscard]] bool empty() const
    {
        return !note && !arrow && !callout && !textStyle && !paperHeight && !arrowSize &&
               !landing && !tip && !hang;
    }
};

// What of `change` can be refused without a leader: a template that does
// not check (entity::checkLeaderTemplate), a text or label style not in the
// model, a size below 0 mm (or not a number).
[[nodiscard]] katana::core::Status checkLeaderChange(const katana::entity::Model& model,
                                                     const LeaderChange& change);

// A balloon (`balloon`) numbered for want of a note in `change` is refused
// with a callout other than a circle: only a circle's number is counted, by
// nextBalloonNumber and BALLOON RENUMBER, so another would be given again.
[[nodiscard]] katana::core::Status checkNumberedBalloon(const LeaderChange& change, bool balloon);

// `change` applied to `leader` (the leader with id `id`, or 0 for one not
// made yet). A label-style note with no text style or paper height in the
// change lends the leader the style's, so the leader looks as the style's
// labels do. Refused: what checkLeaderChange refuses, a tip put on the
// leader itself, and a smart leader that would say nothing
// (entity::checkLeaderSaysSomething). The geometry's own validity is the
// command's to check.
[[nodiscard]] katana::core::Status applyLeaderChange(const katana::entity::Model& model,
                                                     const LeaderChange& change,
                                                     katana::entity::LeaderGeometry& leader,
                                                     katana::entity::EntityId id = 0);

// Whether every one of `ids` is a leader in the model; the first that is not
// is named.
[[nodiscard]] katana::core::Status requireLeaders(const katana::entity::Model& model,
                                                  const std::vector<katana::entity::EntityId>& ids);

// A new leader through `points` (the tip first): `change` applied to a plain
// one, a tip Inside an outline ending in a dot unless the change gives an
// arrow (ISO 128-22, leader lines), and a balloon a circle callout numbered
// on from the highest when the change gives it no note - refused with any
// other callout, since only a circle's number is counted.
[[nodiscard]] katana::core::Result<katana::entity::LeaderGeometry>
newLeader(const katana::entity::Model& model, const std::vector<AnchoredPoint>& points,
          const LeaderChange& change, bool balloon);

// ONE command changing each of `ids` by `change` ("SET_LEADER"); nullptr
// when nothing would change. `hang` is refused for more than one leader,
// since it would put every note on one place.
[[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
changeLeaders(const katana::entity::Model& model, std::vector<katana::entity::EntityId> ids,
              const LeaderChange& change);

// The tip of leader `id` put on `place`, which must name an entity
// ("ATTACH_LEADER"); nullptr when the tip is on that place already.
[[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
attachLeader(const katana::entity::Model& model, katana::entity::EntityId id,
             const AnchoredPoint& place);

// FREEZE (detach false): a smart leader's note becomes the words it says
// now, its tip still following. DETACH (detach true): the tip lets go, a
// smart note frozen first, since a note read off nothing would say nothing.
// The command is null when nothing changes.
struct LeaderRelease {
    katana::commands::CommandPtr command{};
    std::size_t changed = 0;
    std::size_t frozen = 0;
};
[[nodiscard]] katana::core::Result<LeaderRelease>
releaseLeaders(const katana::entity::Model& model, std::vector<katana::entity::EntityId> ids,
               bool detach);

// The attribute `key` of the entity leader `id`'s tip is on: set to `value`,
// or removed when there is none. The same commands PROP SET and PROP DELETE
// run.
[[nodiscard]] katana::core::Result<katana::commands::CommandPtr>
setLeaderTargetProperty(const katana::entity::Model& model, katana::entity::EntityId id,
                        const std::string& key,
                        const std::optional<katana::entity::PropertyValue>& value);

// A leader to each of `targets` as ONE command ("CREATE_LEADERS" or
// "CREATE_BALLOONS"): the tip at a point's or a text's position, the middle
// of a line or an arc, halfway along an open polyline, inside a closed one,
// on a circle on the side the note goes; the note `length` paper
// millimetres from the tip at `angle` (radians). An entity that offers no
// place, or that the note would say nothing about, is skipped and counted;
// refused when none is made, with the first reason, and - as newLeader - a
// balloon numbered for want of a note with a callout that is not a circle.
struct LeadersForOptions {
    LeaderChange change{};
    double angle = 0.25 * katana::math::kPi; // 45 degrees: up and to the right
    double length = 10.0;
    bool balloon = false;
};
struct LeadersFor {
    katana::commands::CommandPtr command{};
    std::size_t made = 0;
    std::size_t skipped = 0;
    std::string firstSkip{};
};
[[nodiscard]] katana::core::Result<LeadersFor>
leadersFor(const katana::entity::Model& model, std::vector<katana::entity::EntityId> targets,
           const LeadersForOptions& options, double scale,
           const katana::commands::EntityAttributes& attributes);

// Where leadersFor puts a leader's tip on `entity` for a note in direction
// `angle` (radians): the place, named so the tip follows it, and the point.
// nullopt for an entity that offers no place (a dimension, a label, a
// leader). What the window previews a leader for the selection from.
[[nodiscard]] std::optional<AnchoredPoint> leaderPlaceOn(const katana::entity::Entity& entity,
                                                         double angle);

// The notes of `ids` in a column ("ALIGN_LEADERS"): top down by where each
// hangs (its last vertex), each moved to `x` (the topmost's when unset) and,
// with `spacing` (model units), stacked that far apart from the top one's
// height. The command is null when nothing moves.
struct LeaderAlignment {
    katana::commands::CommandPtr command{};
    double x = 0.0;
    std::size_t count = 0;
};
[[nodiscard]] katana::core::Result<LeaderAlignment>
alignLeaders(const katana::entity::Model& model, std::vector<katana::entity::EntityId> ids,
             std::optional<double> x, std::optional<double> spacing);

// The numbered balloons - a circled plain leader saying a whole number -
// numbered again from `start` ("RENUMBER_BALLOONS"): in the order they were
// made, across the sheet by their tips (X, left to right) or down it (Y, top
// down), ties in id order. The command is null when every one already says
// its number.
enum class BalloonOrder { Id, X, Y };
struct BalloonRenumbering {
    katana::commands::CommandPtr command{};
    std::size_t balloons = 0;
    std::size_t renumbered = 0;
};
[[nodiscard]] BalloonRenumbering renumberBalloons(const katana::entity::Model& model,
                                                  long long start, BalloonOrder order);

// One more than the highest numbered balloon.
[[nodiscard]] long long nextBalloonNumber(const katana::entity::Model& model);

// A leader's or a place's values as rows of name and text, each in its own
// format: leaderValueNames() order, then the properties in name order. What
// LEADER VALUES replies and the Leaders manager lists.
struct LeaderValueRow {
    std::string name;
    std::string text;
};
[[nodiscard]] std::vector<LeaderValueRow>
leaderValueRows(const katana::entity::LabelValues& values);

// The values of the entity leader `id`'s tip is on, at the tip, with the
// survey code looked for where survey coding looks; nullopt for a leader on
// nothing, or on an entity that is gone.
[[nodiscard]] std::optional<katana::entity::LabelValues>
leaderTargetValues(const katana::entity::Model& model,
                   const katana::entity::LeaderGeometry& leader);

// The note a leader says now, the survey code looked for where survey
// coding looks: entity::leaderNote with cad's code properties.
[[nodiscard]] std::string leaderSays(const katana::entity::Model& model,
                                     const katana::entity::LeaderGeometry& leader);

} // namespace katana::cad::annotation
