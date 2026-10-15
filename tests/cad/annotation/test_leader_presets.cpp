// Leaders that read the drawing's surfaces and alignments, and the presets
// (docs/annotation.md, "What a leader can say" and "Presets";
// cad/annotation/leader_presets.hpp), driven as an agent drives them - the
// LEADER verbs on a Document holding a real TIN - one undo step each.
//
// The surface is the plane z = 10 + 0.1 x + 0.2 y over the square 0..100 x
// 0..100, two triangles; a TIN interpolates a plane exactly, so its level at
// (30, 40) is 10 + 3 + 8 = 21 and at (60, 20) 10 + 6 + 4 = 20. Every other
// expected value is worked out by hand beside it.

#include <gtest/gtest.h>

#include <algorithm>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/annotation/export_annotation.hpp"
#include "katana/cad/annotation/leader_edit.hpp"
#include "katana/cad/annotation/leader_presets.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/leader_values.hpp"
#include "katana/terrain/surface_store.hpp"
#include "katana/terrain/tin_surface.hpp"

using namespace katana::cad;
using namespace katana::entity;
using katana::geometry::Point2;
using katana::geometry::Point3;
namespace ann = katana::cad::annotation;
namespace cmd = katana::commands;
namespace terrain = katana::terrain;

namespace {

// The plane z = z0 + ax x + ay y over the square 0..100 x 0..100.
std::shared_ptr<const terrain::TinSurface> plane(double z0, double ax, double ay)
{
    const auto z = [&](double x, double y) { return z0 + ax * x + ay * y; };
    auto made = terrain::TinSurface::create(
        {Point3(0, 0, z(0, 0)), Point3(100, 0, z(100, 0)), Point3(100, 100, z(100, 100)),
         Point3(0, 100, z(0, 100))},
        {terrain::TinTriangle{0, 1, 2}, terrain::TinTriangle{0, 2, 3}});
    EXPECT_TRUE(made.ok()) << made.error().describe();
    return std::make_shared<const terrain::TinSurface>(std::move(*made));
}

terrain::NamedSurface named(std::string name, std::shared_ptr<const terrain::TinSurface> surface,
                            bool visible = true)
{
    terrain::NamedSurface entry;
    entry.name = std::move(name);
    entry.surface = std::move(surface);
    entry.source = "test plane";
    entry.visible = visible;
    return entry;
}

struct Session {
    Document document;
    CommandInterpreter interpreter{document};

    std::string run(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << "\n  -> " << reply.error().describe();
        return reply.ok() ? *reply : std::string();
    }
    std::string refusal(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " was not refused: " << *reply;
        return reply.ok() ? std::string() : reply.error().describe();
    }
    EntityId point(Point2 at, std::optional<double> z)
    {
        Entity entity;
        entity.geometry = PointGeometry{at};
        if (z) {
            entity.properties["elevation"] = *z;
        }
        EXPECT_TRUE(document.execute(cmd::createEntities({std::move(entity)})).ok());
        return document.lastCreatedEntities().front();
    }
    // The drawing's surfaces: "ground", the plane above, and any others.
    void surfaces(std::vector<terrain::NamedSurface> entries)
    {
        terrain::SurfaceStore store;
        for (terrain::NamedSurface& entry : entries) {
            EXPECT_TRUE(store.add(std::move(entry)).ok());
        }
        EXPECT_TRUE(document.setSurfaces(std::move(store), "TEST_SURFACES").ok());
    }
    void ground() { surfaces({named("ground", plane(10.0, 0.1, 0.2))}); }
    [[nodiscard]] EntityId last() const { return document.lastCreatedEntities().front(); }
    [[nodiscard]] const LeaderGeometry& leader(EntityId id) const
    {
        return std::get<LeaderGeometry>(document.model().entities.find(id)->geometry);
    }
    // What leader `id` says, read as every front end reads it.
    [[nodiscard]] std::string note(EntityId id) const
    {
        return ann::leaderSays(document.model(), leader(id), ann::leaderContext(document));
    }
    [[nodiscard]] std::size_t steps() const { return document.history().undoCount(); }
};

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

std::string id(EntityId value)
{
    return std::to_string(value);
}

} // namespace

TEST(LeaderPresets, PresetLevelSaysThePointsLevelAndIsReadLive)
{
    Session s;
    const EntityId pit = s.point(Point2(30, 40), 25.0);
    const std::string made = s.run("LEADER #" + id(pit) + " 40,50 preset=level");
    const EntityId leader = s.last();
    EXPECT_TRUE(contains(made, "text=\"RL 25.000\"")) << made;
    EXPECT_TRUE(contains(made, " preset=level")) << made;
    EXPECT_FALSE(contains(made, "chosen=")) << "a level reads nothing to choose: " << made;
    EXPECT_TRUE(s.leader(leader).fields);
    EXPECT_EQ(s.leader(leader).text, "RL {rl:.3f}");
    s.run("SELECT " + id(pit));
    s.run("PROP SET elevation 26.5");
    EXPECT_EQ(s.note(leader), "RL 26.500");
    EXPECT_TRUE(contains(s.run("LEADER LIST " + id(leader)), "preset=level"));
}

TEST(LeaderPresets, ASurfaceNamedNoneIsTheFirstVisibleAndTheReplySaysItWasChosen)
{
    Session s;
    // A hidden surface first: the first VISIBLE one is ground.
    s.surfaces({named("old", plane(0.0, 0.0, 0.0), false), named("ground", plane(10.0, 0.1, 0.2))});
    const EntityId pit = s.point(Point2(30, 40), 25.0);
    // 25 above ground 21: +4.
    const std::string chosen = s.run("LEADER #" + id(pit) + " 40,50 preset=surface");
    EXPECT_TRUE(contains(chosen, "text=+4.000")) << chosen;
    EXPECT_TRUE(contains(chosen, "preset=surface surface=ground")) << chosen;
    EXPECT_TRUE(contains(chosen, "chosen=\"first visible surface\"")) << chosen;
    EXPECT_EQ(s.leader(s.last()).text, "{dsurface.ground:+.3f}");
    // Named - in any case, as a surface is found - it is the drawing's
    // spelling that goes into the template, and nothing was chosen.
    const std::string asked = s.run("LEADER #" + id(pit) + " 40,50 preset=surface surface=GROUND");
    EXPECT_FALSE(contains(asked, "chosen=")) << asked;
    EXPECT_EQ(s.leader(s.last()).text, "{dsurface.ground:+.3f}");
    // The hidden one, named, is read all the same: hiding is the picture's.
    s.run("LEADER #" + id(pit) + " 40,50 preset=surface surface=old");
    EXPECT_EQ(s.note(s.last()), "+25.000");
}

TEST(LeaderPresets, TheNoteFollowsTheSurfaceThroughAnEditAndItsUndo)
{
    Session s;
    s.ground();
    const EntityId pit = s.point(Point2(30, 40), 25.0);
    s.run("LEADER #" + id(pit) + " 40,50 preset=surface surface=ground");
    const EntityId leader = s.last();
    EXPECT_EQ(s.note(leader), "+4.000");
    // The ground raised a metre: 25 above 22.
    terrain::SurfaceStore raised = s.document.surfaces();
    ASSERT_TRUE(raised.replace("ground", plane(11.0, 0.1, 0.2)).ok());
    ASSERT_TRUE(s.document.setSurfaces(std::move(raised), "RAISE").ok());
    EXPECT_EQ(s.note(leader), "+3.000");
    ASSERT_TRUE(s.document.undo().ok());
    EXPECT_EQ(s.note(leader), "+4.000");
}

TEST(LeaderPresets, CutFillSaysTheCutBelowAndTheFillAboveOneStepForMany)
{
    Session s;
    s.ground();
    // 18 under ground 21 at (30, 40): a cut of 3. 30 over ground 20 at
    // (60, 20): a fill of 10.
    const EntityId low = s.point(Point2(30, 40), 18.0);
    const EntityId high = s.point(Point2(60, 20), 30.0);
    const std::size_t before = s.steps();
    const std::string made =
        s.run("LEADER FOR " + id(low) + " " + id(high) + " preset=cutfill surface=ground");
    EXPECT_EQ(s.steps(), before + 1) << "one undo step";
    EXPECT_TRUE(contains(made, "created leaders=2")) << made;
    EXPECT_TRUE(contains(made, "preset=cutfill surface=ground")) << made;
    const auto ids = s.document.lastCreatedEntities();
    ASSERT_EQ(ids.size(), 2U);
    EXPECT_EQ(s.note(ids[0]), "CUT 3.000");
    EXPECT_EQ(s.note(ids[1]), "FILL 10.000");
}

TEST(LeaderPresets, PresetStationReadsTheAlignmentItsOffsetAndItsProfile)
{
    Session s;
    // MC01 runs east along the x axis, designed from 20 at station 0 to 21
    // at 100: 20.3 at station 30. (30, -4) is 4 to the right of it.
    s.run("ALIGN NEW MC01 0,0 100,0");
    s.run("ALIGN DESIGN MC01 0,20 100,21");
    const EntityId pit = s.point(Point2(30, -4), 25.0);
    const std::string made = s.run("LEADER #" + id(pit) + " 40,-10 preset=station");
    EXPECT_TRUE(contains(made, "preset=station alignment=MC01")) << made;
    EXPECT_TRUE(contains(made, "chosen=\"first alignment\"")) << made;
    EXPECT_EQ(s.note(s.last()), "CH 0+030.000\nOS -4.000");
    // 25 above the design's 20.3.
    s.run("LEADER #" + id(pit) + " 40,-10 template=\"{dprofile.MC01:+.3f}\"");
    EXPECT_EQ(s.note(s.last()), "+4.700");
}

TEST(LeaderPresets, WhatAPresetCannotMakeIsRefusedAndNothingIsMade)
{
    Session s;
    const EntityId pit = s.point(Point2(30, 40), 25.0);
    const std::string at = "LEADER #" + id(pit) + " 40,50 ";
    // No surface in the drawing yet.
    EXPECT_TRUE(contains(s.refusal(at + "preset=surface"), "the drawing has none"));
    s.surfaces({named("ground", plane(10.0, 0.1, 0.2), false)});
    EXPECT_TRUE(contains(s.refusal(at + "preset=surface"), "every surface is hidden"));
    s.ground();
    EXPECT_TRUE(contains(s.refusal(at + "preset=bogus"), "no such leader preset"));
    EXPECT_TRUE(contains(s.refusal(at + "preset=surface surface=nope"), "no surface of that name"));
    EXPECT_TRUE(contains(s.refusal(at + "preset=level surface=ground"),
                         "surface= is for preset=surface"));
    EXPECT_TRUE(contains(s.refusal(at + "template=\"{rl}\" surface=ground"),
                         "say what a preset= reads"));
    EXPECT_TRUE(contains(s.refusal(at + "preset=level text=A"), "one of text="));
    EXPECT_TRUE(contains(s.refusal(at + "preset=station"), "reads an alignment"));
    EXPECT_TRUE(contains(s.refusal(at + "preset=station alignment=MC09"),
                         "no alignment of that name"));
    // A point with no level says no level; one off the surface no height
    // above it - each refused saying why, not made blank.
    const EntityId flat = s.point(Point2(30, 40), std::nullopt);
    EXPECT_TRUE(contains(s.refusal("LEADER #" + id(flat) + " 40,50 preset=level"),
                         "would say nothing"));
    const EntityId far = s.point(Point2(150, 40), 25.0);
    EXPECT_TRUE(contains(s.refusal("LEADER #" + id(far) + " 160,50 preset=surface"),
                         "the tip is off surface ground"));
    EXPECT_EQ(s.document.model().entities.size(), 3U) << "three points and no leader";
}

TEST(LeaderPresets, LeaderPresetsListsEachWithItsTemplateAndTool)
{
    Session s;
    const std::string list = s.run("LEADER PRESETS");
    EXPECT_EQ(std::count(list.begin(), list.end(), '\n'), 6) << "seven presets: " << list;
    EXPECT_TRUE(contains(list, "preset=level title=Level reads=target template=\"RL {rl:.3f}\" "
                               "tool=annotate.leader.level"))
        << list;
    EXPECT_TRUE(contains(list, "preset=surface title=\"Surface Difference\" reads=surface "
                               "template={dsurface.NAME:+.3f} tool=annotate.leader.surface"))
        << list;
    EXPECT_TRUE(contains(list, "preset=details")) << list;
    EXPECT_TRUE(contains(list, "tool=none")) << "details has no tool of its own: " << list;
}

TEST(LeaderPresets, ListJsonSaysWhatEachLeaderSaysNowAndWhatItReads)
{
    Session s;
    s.ground();
    const EntityId pit = s.point(Point2(30, 40), 25.0);
    s.run("LEADER #" + id(pit) + " 40,50 preset=surface");
    const std::string json = s.run("LEADER LIST JSON");
    EXPECT_TRUE(contains(json, "\"preset\": \"surface\"")) << json;
    EXPECT_TRUE(contains(json, "\"reads\": \"ground\"")) << json;
    EXPECT_TRUE(contains(json, "\"text\": \"+4.000\"")) << json;
    EXPECT_TRUE(contains(json, "\"target\": " + id(pit))) << json;
    EXPECT_TRUE(contains(json, "\"surfaces\": [\n    \"ground\"\n  ]")) << json;
    EXPECT_TRUE(contains(json, "\"name\": \"cutfill\"")) << json;
}

TEST(LeaderPresets, ValuesListEachSurfacesValuesThatHoldAtThePlace)
{
    Session s;
    s.ground();
    const EntityId pit = s.point(Point2(30, 40), 25.0);
    const std::string values = s.run("LEADER VALUES #" + id(pit));
    EXPECT_TRUE(contains(values, "value=surface.ground text=21.000")) << values;
    EXPECT_TRUE(contains(values, "value=dsurface.ground text=4.000")) << values;
    EXPECT_TRUE(contains(values, "value=fill.ground text=4.000")) << values;
    EXPECT_FALSE(contains(values, "value=cut.ground")) << "above the ground, no cut: " << values;
}

TEST(LeaderPresets, EachPresetsTemplateIsRecognisedWithTheNameItReads)
{
    for (const ann::LeaderPresetInfo& info : ann::leaderPresets()) {
        const auto made = ann::presetTemplate(info.preset, "Existing Ground");
        ASSERT_TRUE(made.ok()) << info.word;
        const auto recognised = ann::recognisePreset(*made);
        ASSERT_TRUE(recognised.has_value()) << *made;
        EXPECT_EQ(recognised->preset, info.preset) << *made;
        EXPECT_EQ(recognised->reads,
                  info.reads == ann::PresetReads::Target ? "" : "Existing Ground")
            << *made;
        EXPECT_TRUE(checkLeaderTemplate(*made).ok()) << *made;
    }
    EXPECT_FALSE(ann::recognisePreset("RL {rl:.2f}").has_value()) << "not the preset's format";
    EXPECT_FALSE(ann::recognisePreset("CUT {cut.a:.3f}\nFILL {fill.b:.3f}").has_value())
        << "cut and fill of two surfaces";
    // A name the template language cannot write is refused; a preset that
    // reads only its target ignores any.
    EXPECT_FALSE(ann::presetTemplate(ann::LeaderPreset::Surface, "a:b").ok());
    EXPECT_FALSE(ann::presetTemplate(ann::LeaderPreset::Station, "").ok());
    EXPECT_EQ(*ann::presetTemplate(ann::LeaderPreset::Level, "a:b"), "RL {rl:.3f}");
    EXPECT_EQ(ann::leaderPresetNamed("CutFill").value(), ann::LeaderPreset::CutFill);
}

TEST(LeaderPresets, AFrozenSurfaceLeaderKeepsWhatItSaidWhenFrozen)
{
    Session s;
    s.ground();
    const EntityId pit = s.point(Point2(30, 40), 25.0);
    s.run("LEADER #" + id(pit) + " 40,50 preset=surface");
    const EntityId leader = s.last();
    s.run("LEADER FREEZE " + id(leader));
    EXPECT_FALSE(s.leader(leader).fields);
    EXPECT_EQ(s.leader(leader).text, "+4.000") << "frozen as the surface made it read";
    terrain::SurfaceStore raised = s.document.surfaces();
    ASSERT_TRUE(raised.replace("ground", plane(11.0, 0.1, 0.2)).ok());
    ASSERT_TRUE(s.document.setSurfaces(std::move(raised), "RAISE").ok());
    EXPECT_EQ(s.note(leader), "+4.000");
}

TEST(LeaderPresets, LeaderSetMakesALeadersNoteAPreset)
{
    Session s;
    const EntityId pit = s.point(Point2(30, 40), 25.0);
    s.run("LEADER #" + id(pit) + " 40,50 text=PIT");
    const EntityId leader = s.last();
    const std::size_t before = s.steps();
    const std::string set = s.run("LEADER SET " + id(leader) + " preset=level");
    EXPECT_EQ(s.steps(), before + 1);
    EXPECT_TRUE(contains(set, "updated leaders=1 preset=level")) << set;
    EXPECT_EQ(s.note(leader), "RL 25.000");
}

TEST(LeaderPresets, TheContextIsTheDrawingAsItWasWhenItWasMade)
{
    Session s;
    s.ground();
    s.run("ALIGN NEW MC01 0,0 100,0");
    const katana::entity::LeaderContext before = ann::leaderContext(s.document);
    EXPECT_EQ(before.surfaces, (std::vector<std::string>{"ground"}));
    EXPECT_EQ(before.alignments, (std::vector<std::string>{"MC01"}));
    ASSERT_TRUE(before.surfaceLevel("ground", Point2(30, 40)).has_value());
    EXPECT_NEAR(*before.surfaceLevel("GROUND", Point2(30, 40)), 21.0, 1e-9)
        << "a surface is found ignoring case";
    EXPECT_FALSE(before.surfaceLevel("ground", Point2(150, 40)).has_value()) << "off it";
    EXPECT_FALSE(before.alignmentPlace("MC01", Point2(150, 0)).has_value()) << "beyond its end";
    const auto place = before.alignmentPlace("MC01", Point2(30, -4));
    ASSERT_TRUE(place.has_value());
    EXPECT_NEAR(place->station, 30.0, 1e-9);
    EXPECT_NEAR(place->offset, -4.0, 1e-9);
    EXPECT_FALSE(place->profile.has_value()) << "MC01 has no design profile";
    // The ground raised: the context made before reads what it was made
    // from; one made now, the raised ground.
    terrain::SurfaceStore raised = s.document.surfaces();
    ASSERT_TRUE(raised.replace("ground", plane(11.0, 0.1, 0.2)).ok());
    ASSERT_TRUE(s.document.setSurfaces(std::move(raised), "RAISE").ok());
    EXPECT_NEAR(*before.surfaceLevel("ground", Point2(30, 40)), 21.0, 1e-9);
    EXPECT_NEAR(*ann::leaderContext(s.document).surfaceLevel("ground", Point2(30, 40)), 22.0,
                1e-9);
}

TEST(LeaderPresets, ListAndTheExportSayTheNoteTheSurfaceGivesIt)
{
    Session s;
    s.ground();
    const EntityId pit = s.point(Point2(30, 40), 25.0);
    s.run("LEADER #" + id(pit) + " 40,50 preset=surface");
    const EntityId leader = s.last();
    EXPECT_TRUE(contains(s.run("LIST"), "\"+4.000\""));
    EXPECT_TRUE(contains(s.run("INFO " + id(leader)), "\"+4.000\""));
    // A file is handed the note as the plan view draws it, read off the
    // surface through the context; with none, the line is left out and the
    // leader is drawn bare.
    const auto texts = [&](const katana::entity::LeaderContext& context) {
        std::vector<std::string> found;
        const auto drawn = ann::drawAnnotationForExport(
            s.document.model(), s.document.annotationScale(), ann::estimatedMeasure(), context);
        for (const Geometry& shape : drawn.at(leader)) {
            if (const auto* text = std::get_if<TextGeometry>(&shape)) {
                found.push_back(text->text);
            }
        }
        return found;
    };
    EXPECT_EQ(texts(ann::leaderContext(s.document)), (std::vector<std::string>{"+4.000"}));
    EXPECT_TRUE(texts({}).empty());
}

TEST(LeaderPresets, EveryPointOnTheSurfaceInsideATriangleIsAFillOfNothingNeverACut)
{
    // The plane z = 10 + 0.1 x + 0.2 y, and points whose RL is that to the
    // last digit the arithmetic gives. The TIN interpolates by barycentric
    // weights, which is not the same rounding, so about one such point in six
    // came out a few bits BELOW the surface and read as a cut of 4e-15: of a
    // 19 x 19 grid, 61 "CUT 0.000" beside 281 "FILL 0.000". Hand-worked: at
    // (5.4, 5) the plane is 10 + 0.54 + 1.0 = 11.54.
    Session s;
    s.ground();
    const EntityId first = s.point(Point2(5.4, 5.0), 11.54);
    EXPECT_TRUE(contains(s.run("LEADER #" + id(first) + " 20,20 preset=cutfill"), "text=\"FILL 0.000\""));
    // A leader that says only the fill is accepted on a point on grade.
    EXPECT_TRUE(contains(s.run("LEADER #" + id(first) + " 25,25 template=\"FILL {fill.ground:.3f}\""),
                         "created leader"));
    const LeaderContext context = ann::leaderContext(s.document);
    int onGrade = 0;
    for (int i = 0; i < 20; ++i) {
        for (int j = 0; j < 20; ++j) {
            const double x = 1.7 * i + 0.3;
            const double y = 2.3 * j + 0.7;
            const EntityId point = s.point(Point2(x, y), 10.0 + 0.1 * x + 0.2 * y);
            const Entity* entity = s.document.model().entities.find(point);
            ASSERT_NE(entity, nullptr);
            const LabelValues values =
                anchorValues(*entity, AnchorRef{point, AnchorPoint::Position}, Point2(x, y), {}, context);
            EXPECT_FALSE(values.contains("cut.ground")) << "(" << x << ", " << y << ") read as a cut";
            EXPECT_TRUE(values.contains("fill.ground")) << "(" << x << ", " << y << ")";
            ++onGrade;
        }
    }
    EXPECT_EQ(onGrade, 400);
    // A point a micrometre under the surface is under it, by more than the
    // tolerance: a cut of a micrometre, not a fill.
    const EntityId below = s.point(Point2(5.4, 5.0), 11.54 - 1.0e-6);
    const Entity* entity = s.document.model().entities.find(below);
    ASSERT_NE(entity, nullptr);
    const LabelValues cut =
        anchorValues(*entity, AnchorRef{below, AnchorPoint::Position}, Point2(5.4, 5.0), {}, context);
    EXPECT_TRUE(cut.contains("cut.ground"));
    EXPECT_FALSE(cut.contains("fill.ground"));
}

TEST(LeaderPresets, TheUsageOfLeaderSetNamesThePresetOptionsAndHelpReadsAsSentences)
{
    // LEADER SET accepts preset= surface= and alignment=; its usage line, said
    // when no option is typed, listed only the older ones, and HELP ran the
    // new paragraph on from "...ends in a dot" with a lower-case "also".
    Session s;
    const EntityId pit = s.point(Point2(10, 10), 5.0);
    s.run("LEADER #" + id(pit) + " 20,20 text=A");
    const std::string usage = s.refusal("LEADER SET " + id(s.last()));
    EXPECT_TRUE(contains(usage, "usage: LEADER SET")) << usage;
    EXPECT_TRUE(contains(usage, "preset=")) << usage;
    EXPECT_TRUE(contains(usage, "surface=name")) << usage;
    EXPECT_TRUE(contains(usage, "alignment=name")) << usage;
    const std::string help = CommandInterpreter::helpText();
    EXPECT_TRUE(contains(help, "(#id.inside) ends in a dot.")) << "the sentence ends";
    EXPECT_FALSE(contains(help, "ends in a dot\n")) << "and does not run on";
    EXPECT_FALSE(contains(help, "ends in a dot\r"));
    EXPECT_TRUE(contains(help, "A note may also read the drawing's surfaces and alignments at the tip"));
}
