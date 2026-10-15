// The scope and filter of the ROADOBJECT verbs (include/katana/cad/road/
// road_object_scope.hpp, docs/road_objects.md, "Scope and filter"): LIST,
// QUANTITIES and DELETE take the shared words - the selection, a view, an AREA,
// named layers, the whole drawing, then WHERE - and the answer is what each
// means for a road object, which is a definition on a layer with a plan extent
// and not an entity. Driven through CommandInterpreter::run on a Document, as
// the window's command line, katana_cli and katana_mcp drive it.
//
// The site: alignment Main, straight from (0,0) to (100,0) with a flat design
// profile at level 10, so station s, offset o is the point (s, o). On it, in
// this order (the ids are the kind and the set's next number):
//
//   barrier-1   road/objects/barrier   W-beam, stations 0 to 20, offset 4
//   sign-2      road/objects/sign      at station 50, offset 4
//   light-3     lighting/poles        poles at stations 0, 30 and 60, offset 6
//   barrier-4   road/objects/barrier   thrie-beam, stations 70 to 90, offset -4
//
// A barrier's rail runs its two stations and its end posts are 100 mm wide, so
// barrier-1 lies within x from -0.05 to 20.05 (and barrier-4 from 69.95 to 90.05),
// and a pole, an arm of 1.5 m across the road and a lantern are all within a
// couple of metres of their station: the boxes below leave metres to spare on
// either side, which is why they are the numbers they are.

#include <gtest/gtest.h>

#include <memory>
#include <optional>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/road/road_object_scope.hpp"
#include "katana/cad/road/road_object_verbs.hpp"
#include "katana/cad/road/road_objects.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/core/text.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::ScopeView;
using katana::core::ErrorCode;
using katana::geometry::Box2;
using katana::geometry::Point2;

namespace {

struct ScopedSite {
    ScopedSite()
    {
        katana::entity::Alignment main;
        main.name = "Main";
        main.horizontal.pis = {katana::geometry::AlignmentPI{Point2(0, 0)},
                               katana::geometry::AlignmentPI{Point2(100, 0)}};
        main.vertical = katana::geometry::VerticalAlignment{
            {katana::geometry::ProfilePVI{0.0, 10.0}, katana::geometry::ProfilePVI{100.0, 10.0}}};
        const auto status = document.execute(katana::commands::createAlignment(main));
        EXPECT_TRUE(status.ok()) << (status.ok() ? "" : status.error().describe());
        ok("ROADOBJECT ADD barrier ALIGNMENT Main station=0 to=20 offset=4");
        ok("ROADOBJECT ADD sign ALIGNMENT Main station=50 offset=4");
        ok("ROADOBJECT ADD light ALIGNMENT Main station=0 to=60 spacing=30 offset=6 "
           "layer=lighting/poles");
        ok("ROADOBJECT ADD barrier ALIGNMENT Main station=70 to=90 offset=-4 type=thriebeam");
    }

    std::string ok(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << ": " << (reply.ok() ? "" : reply.error().describe());
        return reply.ok() ? *reply : std::string{};
    }

    katana::core::Error fails(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " was not refused: " << (reply.ok() ? *reply : "");
        return reply.ok() ? katana::core::Error{} : reply.error();
    }

    // The window's view: id 3, hiding the sign layer of its own, seeing x from -5
    // to 65 and y from -10 to 10.
    void showView()
    {
        interpreter.setScopeContext(
            [](std::optional<std::uint32_t>) -> katana::core::Result<ScopeView> {
                ScopeView view;
                view.id = 3;
                view.layers.hide("road/objects/sign");
                view.area = Box2(Point2(-5, -10), Point2(65, 10));
                return view;
            });
    }

    [[nodiscard]] std::size_t objects() const { return document.roadObjects().objects.size(); }

    Document document;
    CommandInterpreter interpreter{document};
};

bool contains(const std::string& text, std::string_view part)
{
    return text.find(part) != std::string::npos;
}

std::vector<std::string> lines(const std::string& text)
{
    std::vector<std::string> out;
    for (const std::string_view line : katana::core::splitLines(text)) {
        out.emplace_back(line);
    }
    return out;
}

// The ids of the `object id=` records of a LIST reply, in order, joined by ",".
std::string idsOf(const std::string& reply)
{
    std::string out;
    for (const std::string& line : lines(reply)) {
        if (line.starts_with("object id=")) {
            const std::size_t end = line.find(' ', 10);
            out += (out.empty() ? "" : ",") + line.substr(10, end - 10);
        }
    }
    return out;
}

} // namespace

// ---- the scope ---------------------------------------------------------------------------------

TEST(RoadObjectScope, TheLayersAScopeNamesTakeTheObjectsOnThemWithOrWithoutSublayers)
{
    ScopedSite site;
    const auto sign = lines(site.ok("ROADOBJECT LIST LAYERS road/objects/sign"));
    ASSERT_EQ(sign.size(), 3u);
    EXPECT_EQ(sign[0], "scope=layers layers=road/objects/sign sublayers=yes matched=1");
    EXPECT_EQ(sign[1], "roadobjects count=1 problems=0");
    EXPECT_TRUE(sign[2].starts_with("object id=sign-2 kind=sign ")) << sign[2];

    // "road" and everything beneath it: the barriers and the sign, not the poles.
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST LAYERS road")), "barrier-1,sign-2,barrier-4");
    // ONLY the layer itself, which holds none: reported, not refused.
    EXPECT_EQ(site.ok("ROADOBJECT LIST LAYERS road ONLY"),
              "scope=layers layers=road sublayers=no matched=0\nroadobjects count=0 problems=0");
    // Two layers, taken in the order the drawing keeps the objects.
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST LAYERS lighting,road/objects/sign")),
              "sign-2,light-3");
    // The layer pattern of WHERE is the object's layer path, '*' and '?' wild
    // (and '*' runs over a '/'): "light*" is "lighting/poles".
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST DRAWING WHERE LAYER=road/objects/s*")), "sign-2");
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST DRAWING WHERE LAYER=road/objects/s*,light*")),
              "sign-2,light-3");
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST DRAWING WHERE LAYER=lighting/?oles")), "light-3");
    EXPECT_EQ(site.fails("ROADOBJECT LIST LAYERS nosuch").code, ErrorCode::NotFound);
}

TEST(RoadObjectScope, AnAreaTakesTheObjectsWhosePlanExtentMeetsIt)
{
    ScopedSite site;
    // x from 10 to 25 meets the 20 m barrier (-0.05 to 20.05) and the row of poles
    // (light-3 is ONE object, its extent from the pole at 0 to the pole at 60, so
    // 10 to 25 lies along it though no pole stands there), not the sign (50) nor
    // the other barrier (70 to 90).
    const auto barrier = lines(site.ok("ROADOBJECT LIST AREA 10,-10,25,10"));
    ASSERT_GE(barrier.size(), 3u);
    EXPECT_EQ(barrier[0], "scope=area area=10,-10,25,10 matched=2");
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST AREA 10,-10,25,10")), "barrier-1,light-3");
    // The corners in either order are the same box, and the record says it sorted.
    EXPECT_EQ(lines(site.ok("ROADOBJECT LIST AREA 25,10,10,-10"))[0],
              "scope=area area=10,-10,25,10 matched=2");
    // 45 to 55 meets the sign at 50 and, again, the row of poles along 0 to 60.
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST AREA 45,0,55,10")), "sign-2,light-3");
    // The poles at 0, 30 and 60 are all in x from -5 to 65.
    const std::string poles = site.ok("ROADOBJECT LIST AREA -5,-10,65,10 WHERE PROP=kind:light");
    EXPECT_EQ(lines(poles)[0], "scope=area area=-5,-10,65,10 where=\"PROP=kind:light\" matched=1");
    EXPECT_EQ(idsOf(poles), "light-3");
    // Nothing there: reported.
    EXPECT_EQ(site.ok("ROADOBJECT LIST AREA 200,200,300,300"),
              "scope=area area=200,200,300,300 matched=0\nroadobjects count=0 problems=0");
    // Not four numbers is the parser's refusal.
    EXPECT_FALSE(site.interpreter.run("ROADOBJECT LIST AREA 1,2,3").ok());
}

TEST(RoadObjectScope, AViewTakesTheObjectsOnLayersItShowsInsideItsVisibleArea)
{
    ScopedSite site;
    site.showView();
    // Hiding the sign layer of its own, seeing x from -5 to 65: barrier-1 and
    // the three poles (0, 30, 60); not the sign (hidden), not barrier-4 (from 70).
    const std::string seen = site.ok("ROADOBJECT LIST VIEW");
    EXPECT_EQ(lines(seen)[0], "scope=view view=3 area=-5,-10,65,10 matched=2");
    EXPECT_EQ(idsOf(seen), "barrier-1,light-3");
    // EXTENTS drops the area and keeps the layers: all but the sign, and the
    // record says what it was.
    const std::string everywhere = site.ok("ROADOBJECT LIST VIEW 3 EXTENTS");
    EXPECT_EQ(lines(everywhere)[0], "scope=view view=3 extents=yes matched=3");
    EXPECT_EQ(idsOf(everywhere), "barrier-1,light-3,barrier-4");
    // A layer the DRAWING hides is hidden in every view.
    site.ok("LAYER HIDE road/objects/barrier");
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST VIEW EXTENTS")), "light-3");
    // The quantities of that view are the poles' alone.
    const std::string quantities = site.ok("ROADOBJECT QUANTITIES VIEW EXTENTS");
    EXPECT_TRUE(contains(quantities, "total kind=light objects=1")) << quantities;
    EXPECT_FALSE(contains(quantities, "kind=barrier")) << quantities;
}

TEST(RoadObjectScope, AViewWithNoWindowIsRefusedNamingTheAreaThatSaysTheSameWithoutOne)
{
    ScopedSite site;
    const auto error = site.fails("ROADOBJECT LIST VIEW");
    EXPECT_EQ(error.code, ErrorCode::InvalidState);
    EXPECT_TRUE(contains(error.message, "AREA")) << error.describe();
    const std::size_t steps = site.document.history().undoCount();
    EXPECT_EQ(site.fails("ROADOBJECT DELETE VIEW").code, ErrorCode::InvalidState);
    EXPECT_EQ(site.objects(), 4u);
    EXPECT_EQ(site.document.history().undoCount(), steps);
}

TEST(RoadObjectScope, TheSelectionTakesTheObjectsPlacedAlongASelectedLineAndNoOthers)
{
    ScopedSite site;
    site.ok("LINE 0,20 10,20");
    site.ok("LINE 0,30 10,30");
    site.ok("SELECT 1");
    site.ok("PROP SET elevation 2.5");
    site.ok("ROADOBJECT ADD fence ALONG #1"); // fence-5, source=#1
    site.ok("SELECT NONE");
    EXPECT_EQ(site.ok("ROADOBJECT LIST SELECTION"),
              "scope=selection matched=0\nroadobjects count=0 problems=0");
    site.ok("SELECT 1");
    const std::string along = site.ok("ROADOBJECT LIST SELECTION");
    EXPECT_EQ(lines(along)[0], "scope=selection matched=1");
    EXPECT_EQ(idsOf(along), "fence-5");
    // Another line selected: no object was placed along it.
    site.ok("SELECT 2");
    EXPECT_EQ(lines(site.ok("ROADOBJECT LIST SELECTION"))[0], "scope=selection matched=0");
    // Both: the objects along an alignment or at points have no entity to
    // select, so the four on Main are never taken by a selection.
    site.ok("SELECT ALL");
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST SELECTION")), "fence-5");
    // WHERE with no scope word is the selection, as it is everywhere.
    site.ok("SELECT 1");
    const std::string filtered = site.ok("ROADOBJECT LIST WHERE PROP=kind:fence");
    EXPECT_EQ(lines(filtered)[0], "scope=selection where=\"PROP=kind:fence\" matched=1");
    EXPECT_EQ(idsOf(filtered), "fence-5");
    EXPECT_EQ(lines(site.ok("ROADOBJECT LIST WHERE PROP=kind:barrier"))[0],
              "scope=selection where=\"PROP=kind:barrier\" matched=0");
    // The line is the fence's source in its record.
    EXPECT_TRUE(contains(site.ok("ROADOBJECT LIST SELECTION"), "source=#1")) << "the fence";
}

// ---- the filter --------------------------------------------------------------------------------

TEST(RoadObjectScope, WhereReadsTheKeysSetWritesKindTypeIdAndPlacement)
{
    ScopedSite site;
    const auto idsFor = [&site](const std::string& where) {
        return idsOf(site.ok("ROADOBJECT LIST DRAWING WHERE " + where));
    };
    EXPECT_EQ(idsFor("PROP=kind:barrier"), "barrier-1,barrier-4");
    // The key in any case; a value is a pattern.
    EXPECT_EQ(idsFor("PROP=Kind:SIGN"), "sign-2");
    EXPECT_EQ(idsFor("PROP=id:barrier-*"), "barrier-1,barrier-4");
    EXPECT_EQ(idsFor("PROP=id:???ht-3"), "light-3");
    // The barrier's type: barrier-4 was given thriebeam, barrier-1 takes the
    // kind's default, which is a W-beam.
    EXPECT_EQ(idsFor("PROP=type:thriebeam"), "barrier-4");
    EXPECT_EQ(idsFor("PROP=type:wbeam"), "barrier-1");
    // A number as the reply prints it: offset 4 twice, -4, 6.
    EXPECT_EQ(idsFor("PROP=offset:4"), "barrier-1,sign-2");
    EXPECT_EQ(idsFor("PROP=offset:-4"), "barrier-4");
    EXPECT_EQ(idsFor("PROP=offset:6"), "light-3");
    EXPECT_EQ(idsFor("PROP=spacing:30"), "light-3");
    EXPECT_EQ(idsFor("PROP=spacing"), "light-3"); // the key alone: any value
    EXPECT_EQ(idsFor("PROP=heights:profile"), "barrier-1,sign-2,light-3,barrier-4");
    EXPECT_EQ(idsFor("PROP=placement:points"), "");
    // A value of any property: only barrier-4 says "thriebeam" anywhere.
    EXPECT_EQ(idsFor("PROP=:thriebeam"), "barrier-4");
    // Both a layer and a property.
    EXPECT_EQ(idsFor("LAYER=road/objects/barrier PROP=offset:4"), "barrier-1");
    // The record says the filter as it was typed.
    EXPECT_EQ(lines(site.ok("ROADOBJECT LIST DRAWING WHERE PROP=Kind:SIGN"))[0],
              "scope=drawing where=\"PROP=Kind:SIGN\" matched=1");
}

TEST(RoadObjectScope, AColourFilterReadsTheColourAnObjectIsDrawnInAndByLayerMeansNoneOfItsOwn)
{
    ScopedSite site;
    site.ok("ROADOBJECT SET sign-2 colour=#FF0000");
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST DRAWING WHERE COLOUR=#FF0000")), "sign-2");
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST DRAWING WHERE COLOUR=#00FF01")), "");
    // The other three were never given a colour of their own.
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST DRAWING WHERE COLOUR=ByLayer")),
              "barrier-1,light-3,barrier-4");
    // The colour a kind is drawn in is a colour too.
    const auto colour = katana::cad::road::colourOf(*site.document.roadObjects().find("barrier-1"));
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST DRAWING WHERE COLOUR=" + colour.toHex() +
                            " PROP=kind:barrier")),
              "barrier-1,barrier-4");
}

TEST(RoadObjectScope, DrawnLeavesOutObjectsOnALayerTheDrawingHides)
{
    ScopedSite site;
    site.ok("LAYER HIDE road/objects/sign");
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST DRAWING")), "barrier-1,sign-2,light-3,barrier-4");
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST DRAWING WHERE DRAWN")),
              "barrier-1,light-3,barrier-4");
    // A parent hidden hides what is beneath it.
    site.ok("LAYER HIDE road");
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST DRAWING WHERE DRAWN")), "light-3");
}

TEST(RoadObjectScope, TypeStyleAndTextAreAboutEntitiesAndAreRefusedForARoadObjectByName)
{
    ScopedSite site;
    for (const char* word : {"TYPE=point", "STYLE=thin", "TEXT=STOP"}) {
        const auto error = site.fails(std::string("ROADOBJECT LIST DRAWING WHERE ") + word);
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << word;
        EXPECT_EQ(error.context, std::string(word).substr(0, std::string(word).find('=')))
            << error.describe();
        EXPECT_FALSE(error.message.empty());
    }
    // Refused by every verb that takes a scope, with nothing changed.
    EXPECT_FALSE(site.interpreter.run("ROADOBJECT DELETE DRAWING WHERE TYPE=point").ok());
    EXPECT_FALSE(site.interpreter.run("ROADOBJECT QUANTITIES DRAWING WHERE TEXT=a").ok());
    EXPECT_EQ(site.objects(), 4u);
}

// ---- what the verbs do with what the scope took -----------------------------------------------------

TEST(RoadObjectScope, AScopeThatTakesNothingIsReportedByListQuantitiesAndDeleteAlike)
{
    ScopedSite site;
    site.ok("LAYER NEW empty");
    const std::size_t steps = site.document.history().undoCount();
    const auto revision = site.document.modelRevision();
    EXPECT_EQ(site.ok("ROADOBJECT LIST LAYERS empty"),
              "scope=layers layers=empty sublayers=yes matched=0\nroadobjects count=0 problems=0");
    EXPECT_EQ(site.ok("ROADOBJECT QUANTITIES LAYERS empty"),
              "scope=layers layers=empty sublayers=yes matched=0\nroadobjects count=0");
    EXPECT_EQ(site.ok("ROADOBJECT DELETE LAYERS empty"),
              "scope=layers layers=empty sublayers=yes matched=0\ndeleted count=0");
    // Nothing deleted is no undo step, and no change to the drawing.
    EXPECT_EQ(site.document.history().undoCount(), steps);
    EXPECT_EQ(site.document.modelRevision(), revision);
    EXPECT_EQ(site.objects(), 4u);
}

TEST(RoadObjectScope, ADeleteByScopeIsOneUndoStepAndAPreviewChangesNothing)
{
    ScopedSite site;
    const std::size_t steps = site.document.history().undoCount();
    const auto revision = site.document.modelRevision();
    EXPECT_EQ(site.ok("ROADOBJECT DELETE LAYERS road/objects/barrier PREVIEW"),
              "scope=layers layers=road/objects/barrier sublayers=yes matched=2\n"
              "preview delete count=2 ids=barrier-1,barrier-4");
    EXPECT_EQ(site.document.modelRevision(), revision);
    EXPECT_EQ(site.objects(), 4u);

    EXPECT_EQ(site.ok("ROADOBJECT DELETE LAYERS road/objects/barrier"),
              "scope=layers layers=road/objects/barrier sublayers=yes matched=2\n"
              "deleted count=2 ids=barrier-1,barrier-4");
    EXPECT_EQ(site.objects(), 2u);
    EXPECT_EQ(site.document.history().undoCount(), steps + 1);
    // One undo gives both back.
    site.ok("UNDO");
    EXPECT_EQ(site.objects(), 4u);
    EXPECT_NE(site.document.roadObjects().find("barrier-1"), nullptr);
    EXPECT_NE(site.document.roadObjects().find("barrier-4"), nullptr);

    // By a filter on the whole drawing; ALL then WHERE is the drawing's scope.
    EXPECT_EQ(site.ok("ROADOBJECT DELETE ALL WHERE PROP=offset:4"),
              "scope=drawing where=\"PROP=offset:4\" matched=2\n"
              "deleted count=2 ids=barrier-1,sign-2");
    EXPECT_EQ(site.objects(), 2u);
    // ALL on its own is every object, with the plain reply it always had.
    EXPECT_EQ(site.ok("ROADOBJECT DELETE ALL"), "deleted count=2 ids=light-3,barrier-4");
    EXPECT_EQ(site.objects(), 0u);
}

TEST(RoadObjectScope, QuantitiesOfAScopeAreTheSolidsOfTheObjectsItTookAndTheirTotals)
{
    ScopedSite site;
    // The two barriers are 20 m each: 20 m of W-beam and 20 m of thrie-beam.
    const auto quantities = lines(site.ok("ROADOBJECT QUANTITIES LAYERS road/objects/barrier"));
    ASSERT_GE(quantities.size(), 4u);
    EXPECT_EQ(quantities[0], "scope=layers layers=road/objects/barrier sublayers=yes matched=2");
    EXPECT_TRUE(quantities[1].starts_with("quantity object=barrier-1 ")) << quantities[1];
    const std::string all = site.ok("ROADOBJECT QUANTITIES LAYERS road/objects/barrier");
    EXPECT_TRUE(contains(all, "total kind=barrier objects=2 length=40")) << all;
    EXPECT_FALSE(contains(all, "kind=sign")) << all;
    EXPECT_FALSE(contains(all, "kind=light")) << all;
    // One kind by its key, over the drawing: only the lights.
    const std::string lights = site.ok("ROADOBJECT QUANTITIES DRAWING WHERE PROP=kind:light");
    EXPECT_TRUE(contains(lights, "scope=drawing where=\"PROP=kind:light\" matched=1\n")) << lights;
    EXPECT_TRUE(contains(lights, "total kind=light objects=1 count=3")) << lights;
}

TEST(RoadObjectScope, ObjectsThatAreNotMadeHaveNoExtentAndAreCountedNotTaken)
{
    ScopedSite site;
    site.ok("LINE 0,20 10,20");
    site.ok("SELECT 1");
    site.ok("PROP SET elevation 2.5");
    site.ok("ROADOBJECT ADD fence ALONG #1"); // fence-5: its line carries its heights
    site.ok("ALIGN DELETE Main");              // the four on it can no longer be made
    // The whole drawing still has all five; four of them are problems.
    EXPECT_EQ(lines(site.ok("ROADOBJECT LIST DRAWING"))[1], "roadobjects count=5 problems=4");
    // A box round everything takes the one that is made, and counts the four
    // that would be taken if they were: they draw nothing, so they are nowhere.
    const std::string area = site.ok("ROADOBJECT LIST AREA -1000,-1000,1000,1000");
    EXPECT_EQ(lines(area)[0], "scope=area area=-1000,-1000,1000,1000 matched=1 unmade=4");
    EXPECT_EQ(idsOf(area), "fence-5");
    // The layers do not ask for an extent: all five are on road/objects/... or
    // lighting/poles still.
    EXPECT_EQ(idsOf(site.ok("ROADOBJECT LIST LAYERS road,lighting")),
              "barrier-1,sign-2,light-3,barrier-4,fence-5");
    // DRAWN is about being drawn, so it leaves them out and counts them.
    const std::string drawn = site.ok("ROADOBJECT LIST DRAWING WHERE DRAWN");
    EXPECT_EQ(lines(drawn)[0], "scope=drawing where=DRAWN matched=1 unmade=4");
}

// ---- what is refused -------------------------------------------------------------------------------

TEST(RoadObjectScope, IdsAndAScopeAreNotMixedAndAWordAfterAScopeIsNotIgnored)
{
    ScopedSite site;
    const std::size_t steps = site.document.history().undoCount();
    for (const char* line : {"ROADOBJECT DELETE sign-2 DRAWING", "ROADOBJECT DELETE sign-2 WHERE LAYER=x",
                             "ROADOBJECT QUANTITIES sign-2 LAYERS road"}) {
        const auto error = site.fails(line);
        EXPECT_EQ(error.code, ErrorCode::InvalidArgument) << line;
        EXPECT_TRUE(contains(error.message, "not both")) << line << ": " << error.describe();
    }
    // A word after a scope that is not the verb's own: refused, naming it.
    const auto afterList = site.fails("ROADOBJECT LIST DRAWING bogus");
    EXPECT_EQ(afterList.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(afterList.context, "bogus");
    const auto afterQuantities = site.fails("ROADOBJECT QUANTITIES DRAWING sign-2");
    EXPECT_EQ(afterQuantities.code, ErrorCode::ParseFailure);
    EXPECT_EQ(afterQuantities.context, "sign-2");
    const auto afterDelete = site.fails("ROADOBJECT DELETE DRAWING PREVIEW now");
    EXPECT_EQ(afterDelete.code, ErrorCode::ParseFailure);
    EXPECT_EQ(afterDelete.context, "now");
    // PREVIEW before a scope would be read as a layer or an id: refused too.
    EXPECT_EQ(site.fails("ROADOBJECT DELETE PREVIEW DRAWING").code, ErrorCode::InvalidArgument);
    // Plain ids after LIST are not ids: LIST takes none.
    EXPECT_EQ(site.fails("ROADOBJECT LIST sign-2").code, ErrorCode::InvalidArgument);
    // Two scopes: the parser's refusal.
    EXPECT_FALSE(site.interpreter.run("ROADOBJECT LIST DRAWING SELECTION").ok());
    EXPECT_EQ(site.objects(), 4u);
    EXPECT_EQ(site.document.history().undoCount(), steps);
}

TEST(RoadObjectScope, ListJsonIsTheWholeStoredSetAndTakesNoScope)
{
    ScopedSite site;
    const auto error = site.fails("ROADOBJECT LIST JSON LAYERS road/objects/sign");
    EXPECT_EQ(error.code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(error.message, "whole stored set")) << error.describe();
    // Plain, it is the JSON it was.
    const auto read = katana::cad::road::roadObjectsFromJson(site.ok("ROADOBJECT LIST JSON"));
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_EQ(*read, site.document.roadObjects());
}

TEST(RoadObjectScope, ARecordTheDrawingCannotReadIsSaidSoAndNotReportedAsEmpty)
{
    ScopedSite site;
    auto metadata = site.document.metadata();
    metadata.unknownKeys["road_objects"] =
        R"({"format": "katana-road-objects", "version": 2, "next": 2, "objects": []})";
    site.document.setMetadata(metadata);
    EXPECT_EQ(site.fails("ROADOBJECT LIST DRAWING").code, ErrorCode::Unsupported);
    EXPECT_EQ(site.fails("ROADOBJECT QUANTITIES LAYERS road").code, ErrorCode::Unsupported);
    EXPECT_EQ(site.fails("ROADOBJECT DELETE DRAWING").code, ErrorCode::CommandRejected);
}

// ---- the pieces -------------------------------------------------------------------------------------

TEST(RoadObjectScope, ThePlanExtentIsTheBoxRoundTheSolidsAndNothingForAnObjectThatIsNotMade)
{
    ScopedSite site;
    const auto context = katana::cad::road::placementContext(site.document);
    // The 20 m barrier at offset 4: its length, and a metre of width at most.
    const auto barrier = katana::cad::road::planExtent(context, *site.document.roadObjects().find("barrier-1"));
    ASSERT_TRUE(barrier.has_value());
    EXPECT_GT(barrier->width(), 19.9);
    EXPECT_LT(barrier->width(), 20.5);
    EXPECT_LT(barrier->height(), 1.0);
    EXPECT_GT(barrier->min.y, 3.0);
    EXPECT_LT(barrier->max.y, 5.0);
    // The sign: within a metre and a half of (50, 4).
    const auto sign = katana::cad::road::planExtent(context, *site.document.roadObjects().find("sign-2"));
    ASSERT_TRUE(sign.has_value());
    EXPECT_TRUE(sign->inflated(1.5).contains(Point2(50, 4)));
    EXPECT_LT(sign->width(), 3.0);
    EXPECT_LT(sign->height(), 3.0);
    // An object whose alignment is gone is not made, and has no extent.
    site.ok("ALIGN DELETE Main");
    const auto context2 = katana::cad::road::placementContext(site.document);
    EXPECT_FALSE(katana::cad::road::planExtent(context2, *site.document.roadObjects().find("barrier-1")).has_value());
}

TEST(RoadObjectScope, TheFilterKeysAreTheKeysSetWritesWithTheValuesTheObjectIsMadeWith)
{
    using katana::cad::road::filterProperties;
    using katana::entity::toString;
    ScopedSite site;
    const auto& set = site.document.roadObjects();

    const auto barrier = filterProperties(*set.find("barrier-4"));
    const auto text = [](const katana::entity::PropertyMap& map, const char* key) {
        const auto found = map.find(key);
        return found == map.end() ? std::string("<absent>") : toString(found->second);
    };
    EXPECT_EQ(text(barrier, "id"), "barrier-4");
    EXPECT_EQ(text(barrier, "kind"), "barrier");
    EXPECT_EQ(text(barrier, "layer"), "road/objects/barrier");
    EXPECT_EQ(text(barrier, "placement"), "alignment");
    EXPECT_EQ(text(barrier, "alignment"), "Main");
    EXPECT_EQ(text(barrier, "station"), "70");
    EXPECT_EQ(text(barrier, "to"), "90");
    EXPECT_EQ(text(barrier, "offset"), "-4");
    EXPECT_EQ(text(barrier, "heights"), "profile");
    EXPECT_EQ(text(barrier, "crossfall"), "0");
    EXPECT_EQ(text(barrier, "dz"), "0");
    EXPECT_EQ(text(barrier, "type"), "thriebeam");
    // Not given, so absent: absent is not zero.
    EXPECT_EQ(text(barrier, "spacing"), "<absent>");
    EXPECT_EQ(text(barrier, "surface"), "<absent>");
    EXPECT_EQ(text(barrier, "name"), "<absent>");
    EXPECT_EQ(text(barrier, "facility"), "<absent>");
    EXPECT_EQ(text(barrier, "source"), "<absent>");
    // The default a barrier is made with, which no one typed.
    EXPECT_EQ(text(filterProperties(*set.find("barrier-1")), "type"), "wbeam");

    const auto lights = filterProperties(*set.find("light-3"));
    EXPECT_EQ(text(lights, "spacing"), "30");
    EXPECT_EQ(text(lights, "layer"), "lighting/poles");

    // An object placed along a line has its points and its source line.
    site.ok("LINE 0,20 10,20");
    site.ok("SELECT 1");
    site.ok("PROP SET elevation 2.5");
    site.ok("ROADOBJECT ADD fence ALONG #1 name=\"North fence\" facility=Main");
    const auto fence = filterProperties(*site.document.roadObjects().find("fence-5"));
    EXPECT_EQ(text(fence, "placement"), "line");
    EXPECT_EQ(text(fence, "points"), "2");
    EXPECT_EQ(text(fence, "source"), "1");
    EXPECT_EQ(text(fence, "name"), "North fence");
    EXPECT_EQ(text(fence, "facility"), "Main");
    EXPECT_EQ(text(fence, "heights"), "line");
    EXPECT_EQ(text(fence, "alignment"), "<absent>");
}

TEST(RoadObjectScope, AnUnreachableFilterIsCheckedBeforeAnythingIsLookedUp)
{
    katana::cad::ModifyFilter filter;
    EXPECT_TRUE(katana::cad::road::checkFilterFits(filter).ok());
    filter.layers = {"road/*"};
    filter.property = "kind";
    filter.drawnOnly = true;
    EXPECT_TRUE(katana::cad::road::checkFilterFits(filter).ok());
    filter.types.insert(katana::entity::EntityType::Point);
    const auto types = katana::cad::road::checkFilterFits(filter);
    ASSERT_FALSE(types.ok());
    EXPECT_EQ(types.error().context, "TYPE");
    filter.types.clear();
    filter.style = "";
    EXPECT_EQ(katana::cad::road::checkFilterFits(filter).error().context, "STYLE");
    filter.style.reset();
    filter.text = "";
    EXPECT_EQ(katana::cad::road::checkFilterFits(filter).error().context, "TEXT");
}
