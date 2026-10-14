// The preset leaders by hand (docs/annotation.md, "Presets"; docs/tools.md):
// Level Leader, Surface Leader, Cut/Fill Leader, Point Name Leader, Code
// Leader and Station Leader, one click each on the Annotate menu and
// toolbar - the owner's request of 2026-10-15 for "a leader to display a
// point's height, a leader to get a point's difference to a TIN, a leader
// to display names".
//
//   Pick what the note reads [Surface]           a click on a point, a line
//                                                 or what else the tip goes
//                                                 on - or a snap to its end,
//                                                 middle, vertex
//   Specify where the note goes <up and right>   a click; Enter puts it
//                                                 where LEADER FOR does
//   Enter the surface name <ground>              after S (A for a station's
//                                                 alignment)
//
// Each makes exactly the leader "LEADER #id@x,y x,y preset=level" makes -
// through the same door, annotation::resolveLeaderPreset and newLeader - as
// ONE command, and starts again for the next point, so a row of spot levels
// is a click and a click each. The note is a smart leader's: read live off
// the point and the surface, however either changes. A point that has
// nothing to say - no level, off the surface - is refused at the first
// click with why, before a note's place is asked for.
//
// The surface (or the alignment) offered is the one the drawing's newest
// leader of the same preset reads - remembered by the drawing, not the
// program, as Label Objects offers the newest label's style - else none,
// which reads the first visible surface (the first alignment) and says so.

#include <cmath>
#include <optional>
#include <set>
#include <string>
#include <utility>
#include <vector>

#include "annotate_common.hpp"
#include "katana/cad/annotation/leader_edit.hpp"
#include "katana/cad/annotation/leader_presets.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/text.hpp"
#include "katana/entity/anchor.hpp"
#include "katana/entity/annotation.hpp"
#include "katana/entity/leader_values.hpp"

namespace katana::cad::tools::annotate {

namespace {

namespace ann = katana::cad::annotation;
using katana::entity::AnchorRef;
using katana::entity::Entity;
using katana::entity::LeaderGeometry;
using katana::geometry::Point2;
using katana::geometry::Vec2;

// What a tip can be put on, as the Leader tool's (annotate_leader.cpp):
// what offers a place along it or a position (entity::nearestAnchor).
const std::set<katana::entity::EntityType> kTipTargets = {
    katana::entity::EntityType::Point,         katana::entity::EntityType::Line,
    katana::entity::EntityType::Arc,           katana::entity::EntityType::Polyline,
    katana::entity::EntityType::Circle,        katana::entity::EntityType::Text,
    katana::entity::EntityType::CurvePolyline, katana::entity::EntityType::Ellipse};

// The surface or alignment the newest leader made with `preset` reads, if
// the drawing still has it; empty for none.
std::string newestReads(const Document& document, ann::LeaderPreset preset)
{
    std::string reads;
    document.model().entities.forEach([&](const Entity& entity) {
        const auto* leader = std::get_if<LeaderGeometry>(&entity.geometry);
        if (leader == nullptr || !leader->fields) {
            return;
        }
        if (const auto recognised = ann::recognisePreset(leader->text);
            recognised && recognised->preset == preset) {
            reads = recognised->reads;
        }
    });
    const bool surface = ann::presetInfo(preset).reads == ann::PresetReads::Surface;
    const bool there = surface ? document.surfaces().find(reads) != nullptr
                               : document.model().alignments.contains(reads);
    return there ? reads : std::string();
}

class PresetLeaderTool final : public InteractiveTool {
  public:
    PresetLeaderTool(const ToolContext& context, ann::LeaderPreset preset)
        : context_(context), preset_(preset)
    {
        if (context.document != nullptr &&
            ann::presetInfo(preset).reads != ann::PresetReads::Target) {
            reads_ = newestReads(*context.document, preset);
        }
    }

    [[nodiscard]] std::string prompt() const override
    {
        switch (step_) {
        case Step::Tip:
            return "Pick what the note reads" + option() + reading();
        case Step::Note:
            return "Specify where the note goes, or press Enter for up and to the right [Undo]";
        case Step::Name:
            return std::string("Enter the ") + noun() + " name <" +
                   (reads_.empty() ? "the first " + std::string(firstWhat()) : reads_) + ">";
        }
        return {};
    }

    [[nodiscard]] ToolInput expects() const override
    {
        return step_ == Step::Name ? ToolInput::Value : ToolInput::Point;
    }

    ToolStep point(const Point2& at) override
    {
        switch (step_) {
        case Step::Tip: {
            if (context_.document == nullptr) {
                return ToolStep::rejected("there is no drawing to read");
            }
            const auto picked = pickUnder(context_, at, kTipTargets);
            const Entity* entity =
                picked ? context_.document->model().entities.find(*picked) : nullptr;
            const auto ref =
                entity != nullptr ? katana::entity::nearestAnchor(*entity, at) : std::nullopt;
            return takeTip(ref ? *ref : AnchorRef{});
        }
        case Step::Note:
            return place(at);
        case Step::Name:
            break;
        }
        return InteractiveTool::point(at);
    }

    // A snap to a point of an entity - its end, its middle, a vertex - is
    // that point, as #id.end on the command line is.
    ToolStep anchoredPoint(const Point2& at, const AnchorRef& anchor) override
    {
        if (step_ == Step::Tip && anchor.associated()) {
            return takeTip(anchor);
        }
        return point(at);
    }

    ToolStep value(std::string_view text) override
    {
        switch (step_) {
        case Step::Tip:
            if (reads() && isOption(text, optionWord())) {
                step_ = Step::Name;
                return ToolStep::next();
            }
            return ToolStep::rejected("click the point (or the line) the note reads" +
                                      (reads() ? ", or type " + std::string(1, optionWord()[0]) +
                                                     " to choose the " + noun()
                                               : std::string()));
        case Step::Note:
            if (isOption(text, "Undo")) {
                return undo();
            }
            return ToolStep::rejected("click where the note goes, or press Enter");
        case Step::Name: {
            const std::string name(katana::core::trimmed(text));
            if (name.empty()) {
                return enter();
            }
            if (context_.document == nullptr) {
                return ToolStep::rejected("there is no drawing to read");
            }
            std::optional<std::string> surface;
            std::optional<std::string> alignment;
            (surfaceReader() ? surface : alignment) = name;
            auto resolved = ann::resolveLeaderPreset(*context_.document, preset_, surface, alignment);
            if (!resolved) {
                return ToolStep::rejected(resolved.error().message + ": " +
                                          resolved.error().context);
            }
            reads_ = resolved->reads;
            step_ = Step::Tip;
            return ToolStep::next();
        }
        }
        return InteractiveTool::value(text);
    }

    ToolStep enter() override
    {
        switch (step_) {
        case Step::Tip:
            return ToolStep::done(nullptr);
        case Step::Note: {
            // LEADER FOR's place for a note: its own direction and distance
            // on paper, from their one source, at the drawing's scale.
            const ann::LeadersForOptions defaults;
            const double reach =
                katana::entity::annotationModelSize(defaults.length, scale());
            return place(tip_.point +
                         Vec2(std::cos(defaults.angle), std::sin(defaults.angle)) * reach);
        }
        case Step::Name:
            step_ = Step::Tip;
            return ToolStep::next();
        }
        return InteractiveTool::enter();
    }

    ToolStep undo() override
    {
        if (step_ == Step::Note || step_ == Step::Name) {
            step_ = Step::Tip;
            return ToolStep::next();
        }
        return ToolStep::rejected("nothing to undo: no point has been picked");
    }

    [[nodiscard]] ToolFeedback preview(const Point2& cursor) const override
    {
        ToolFeedback feedback;
        if (step_ != Step::Note) {
            return feedback;
        }
        // The leader as it will be, its note already read off the point.
        if (!coincident(tip_.point, cursor)) {
            if (auto leader = make(cursor)) {
                feedback.shapes.emplace_back(std::move(*leader));
            }
        }
        feedback.markers.push_back(tip_.point);
        return feedback;
    }

    [[nodiscard]] std::optional<Point2> lastPoint() const override
    {
        if (step_ == Step::Note) {
            return tip_.point;
        }
        return std::nullopt;
    }

  private:
    enum class Step { Tip, Note, Name };

    [[nodiscard]] bool reads() const
    {
        return ann::presetInfo(preset_).reads != ann::PresetReads::Target;
    }
    [[nodiscard]] bool surfaceReader() const
    {
        return ann::presetInfo(preset_).reads == ann::PresetReads::Surface;
    }
    [[nodiscard]] const char* noun() const { return surfaceReader() ? "surface" : "alignment"; }
    [[nodiscard]] const char* firstWhat() const
    {
        return surfaceReader() ? "visible surface" : "alignment";
    }
    [[nodiscard]] std::string_view optionWord() const
    {
        return surfaceReader() ? "Surface" : "Alignment";
    }
    [[nodiscard]] std::string option() const
    {
        return reads() ? " [" + std::string(optionWord()) + "]" : std::string();
    }
    // What is read, after the prompt: nothing else on screen says.
    [[nodiscard]] std::string reading() const
    {
        if (!reads()) {
            return {};
        }
        return std::string(" (") + noun() + ": " +
               (reads_.empty() ? "the first " + std::string(firstWhat()) : reads_) + ")";
    }

    [[nodiscard]] double scale() const
    {
        return context_.document != nullptr ? context_.document->annotationScale()
                                            : katana::entity::kDefaultAnnotationScale;
    }

    // The preset for the drawing as it is: the surface or alignment chosen,
    // or the preset's own choice.
    [[nodiscard]] katana::core::Result<ann::ResolvedPreset> resolve() const
    {
        std::optional<std::string> surface;
        std::optional<std::string> alignment;
        if (!reads_.empty()) {
            (surfaceReader() ? surface : alignment) = reads_;
        }
        return ann::resolveLeaderPreset(*context_.document, preset_, surface, alignment);
    }

    // The leader from the tip to `note`, as LEADER makes it: newLeader with
    // the preset's template. Refused as LEADER refuses it.
    [[nodiscard]] katana::core::Result<LeaderGeometry> make(const Point2& note) const
    {
        auto resolved = resolve();
        if (!resolved) {
            return resolved.error();
        }
        ann::LeaderChange change;
        change.note = ann::LeaderNote{ann::LeaderNote::Kind::Template, resolved->templateText};
        return ann::newLeader(context_.document->model(), {tip_, ann::AnchoredPoint{note, {}}},
                              change, false, ann::leaderContext(*context_.document));
    }

    // The tip put on what `ref` names, checked for having something to say
    // there before the note's place is asked.
    ToolStep takeTip(const AnchorRef& ref)
    {
        if (!ref.associated()) {
            return ToolStep::rejected(
                "pick the point (or the line) the note reads: nothing is under the cursor");
        }
        const auto anchored = ann::anchoredPoint(context_.document->model(), ref);
        if (!anchored) {
            return ToolStep::rejected(anchored.error().message);
        }
        tip_ = *anchored;
        // Any second point will do to ask: what the note says depends on the
        // tip alone.
        const auto trial = make(tip_.point + Vec2(1.0, 1.0));
        if (!trial) {
            return ToolStep::rejected(trial.error().message +
                                      (trial.error().context.empty()
                                           ? std::string()
                                           : " [" + trial.error().context + "]"));
        }
        step_ = Step::Note;
        const Entity* target = context_.document->model().entities.find(ref.entity);
        return ToolStep::next(
            std::string(ann::presetInfo(preset_).title) + " leader on " +
            (target != nullptr ? std::string(katana::entity::toString(target->type())) + " " +
                                     std::to_string(target->id)
                               : std::string("it")) +
            ": " + oneLine(ann::leaderSays(context_.document->model(), *trial,
                                           ann::leaderContext(*context_.document))));
    }

    ToolStep place(const Point2& at)
    {
        if (coincident(tip_.point, at)) {
            return ToolStep::rejected("that is the tip itself; the note goes somewhere off it");
        }
        auto leader = make(at);
        if (!leader) {
            return ToolStep::rejected(leader.error().message);
        }
        const std::string says = oneLine(ann::leaderSays(
            context_.document->model(), *leader, ann::leaderContext(*context_.document)));
        auto resolved = resolve();
        std::string message = "leader " + std::string(ann::presetInfo(preset_).word) + ": " + says;
        if (resolved && !resolved->chosen.empty()) {
            message += " (reads " + resolved->reads + ", the " + resolved->chosen + ")";
        }
        std::vector<Entity> entities;
        entities.push_back(newEntity(std::move(*leader), context_.attributes));
        return ToolStep::done(createAll("CREATE_LEADER", std::move(entities)), std::move(message),
                              /*restart=*/true);
    }

    // A note's lines on one line of the command log.
    static std::string oneLine(std::string text)
    {
        for (char& c : text) {
            if (c == '\n') {
                c = ' ';
            }
        }
        return text;
    }

    ToolContext context_;
    ann::LeaderPreset preset_;
    Step step_ = Step::Tip;
    // The surface or alignment chosen; empty for the preset's own choice.
    std::string reads_;
    ann::AnchoredPoint tip_{};
};

} // namespace

std::unique_ptr<InteractiveTool> makePresetLeaderTool(const ToolContext& context,
                                                      katana::cad::annotation::LeaderPreset preset)
{
    return std::make_unique<PresetLeaderTool>(context, preset);
}

} // namespace katana::cad::tools::annotate
