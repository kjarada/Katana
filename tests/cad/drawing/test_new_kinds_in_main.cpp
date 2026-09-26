// The drawing system's kinds (docs/drawing.md) in the code main gained while
// the drawing system was on its own branch: smart leaders' anchors and
// values, the labels, the snaps that make annotation associative, AREA and
// PARCEL, the one scope grammar's TYPE=, the utilities' design line, the
// label keep-out, and the verb lines that replace the drawing. Each of these
// was written before CurvePolyline2, Ellipse2 and Spline2 existed and took
// them as "nothing": a note with no value, a label that never appeared, a
// filter that refused the name.
//
// The figure most of them use is a lot with one curved side, worked by hand:
// (0,0) -> (10,0) straight, (10,0) -> (10,10) a semicircle of radius 5
// bulging east through (15,5) (bulge 1, counter-clockwise), then (0,10) and
// back. Area 10 x 10 + half of pi 5^2 = 100 + 12.5 pi; perimeter three sides
// of 10 and half of 2 pi 5 = 30 + 5 pi.

#include <gtest/gtest.h>

#include <cmath>
#include <string>
#include <vector>

#include "katana/cad/annotation/drawing.hpp"
#include "katana/cad/annotation/label_layout.hpp"
#include "katana/cad/annotation/leader_edit.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/cad/snapping.hpp"
#include "katana/cad/survey_tools.hpp"
#include "katana/cad/utilities/utility_data.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/entity/anchor.hpp"
#include "katana/entity/label_values.hpp"
#include "katana/entity/leader_values.hpp"
#include "katana/math/numerics.hpp"

using namespace katana::cad;
using katana::core::ErrorCode;
using katana::entity::AnchorPoint;
using katana::entity::AnchorRef;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::EntityType;
using katana::entity::LabelKind;
using katana::entity::LabelStyle;
using katana::geometry::CurvePolyline2;
using katana::geometry::CurveVertex;
using katana::geometry::Ellipse2;
using katana::geometry::Point2;
using katana::geometry::Vec2;
using katana::math::kPi;

namespace {

constexpr double kLotArea = 100.0 + 12.5 * kPi;
constexpr double kLotPerimeter = 30.0 + 5.0 * kPi;

CurvePolyline2 lot()
{
    return CurvePolyline2{{CurveVertex{Point2(0, 0), 0.0, std::nullopt},
                           CurveVertex{Point2(10, 0), 1.0, std::nullopt},
                           CurveVertex{Point2(10, 10), 0.0, std::nullopt},
                           CurveVertex{Point2(0, 10), 0.0, std::nullopt}},
                          true};
}

struct Drawing {
    Document document;
    CommandInterpreter interpreter{document};

    EntityId add(katana::entity::Geometry geometry)
    {
        EXPECT_TRUE(document
                        .execute(katana::commands::createEntities(
                            {Entity{.geometry = std::move(geometry)}}))
                        .ok());
        return document.lastCreatedEntities().front();
    }
    const Entity& entity(EntityId id) const { return *document.model().entities.find(id); }
};

double number(const katana::entity::LabelValues& values, const std::string& name)
{
    const auto found = values.find(name);
    EXPECT_NE(found, values.end()) << "no value '" << name << "'";
    return found != values.end() ? found->second.number : std::nan("");
}

} // namespace

// ---- smart leaders: anchors and the values a note reads ------------------------------------

TEST(NewKindsInMain, ACurvePolylineOffersAPlaceAlongItsArcAndInsideIt)
{
    Drawing d;
    const EntityId id = d.add(lot());
    const auto near = katana::entity::nearestAnchor(d.entity(id), Point2(16, 5));
    ASSERT_TRUE(near.has_value()) << "a curve polyline offered no place to attach to";
    EXPECT_EQ(near->point, AnchorPoint::Along);
    EXPECT_EQ(near->index, 1u) << "the arc is segment 1";
    EXPECT_NEAR(near->parameter, 0.5, 1e-12) << "due east of the centre is half its sweep";
    const auto at = katana::entity::resolveAnchor(d.entity(id), *near);
    ASSERT_TRUE(at.has_value());
    EXPECT_NEAR(at->x, 15.0, 1e-9) << "ON the arc, not on its chord at x = 10";
    EXPECT_NEAR(at->y, 5.0, 1e-9);

    const auto inside = katana::entity::resolveAnchor(d.entity(id), AnchorRef{id, AnchorPoint::Inside});
    ASSERT_TRUE(inside.has_value());
    EXPECT_GT(inside->x, 0.0);
    EXPECT_LT(inside->x, 15.0);
    EXPECT_GT(inside->y, 0.0);
    EXPECT_LT(inside->y, 10.0);
}

TEST(NewKindsInMain, AnEllipseOffersAPlaceAlongItAndASplineRefusesOne)
{
    Drawing d;
    // Semi-axes 4 east and 2 north; the point due north is a quarter turn
    // of eccentric anomaly, (0, 2).
    const EntityId ellipse = d.add(Ellipse2{Point2(0, 0), Vec2(4, 0), 0.5, 0.0, 2.0 * kPi});
    const auto near = katana::entity::nearestAnchor(d.entity(ellipse), Point2(0, 3));
    ASSERT_TRUE(near.has_value());
    EXPECT_NEAR(near->parameter, 0.25, 1e-9);
    const auto at = katana::entity::resolveAnchor(d.entity(ellipse), *near);
    ASSERT_TRUE(at.has_value());
    EXPECT_NEAR(at->x, 0.0, 1e-9);
    EXPECT_NEAR(at->y, 2.0, 1e-9);

    const EntityId spline =
        d.add(*katana::geometry::Spline2::throughPoints({Point2(0, 0), Point2(5, 3), Point2(10, 0)}, 3));
    EXPECT_FALSE(katana::entity::nearestAnchor(d.entity(spline), Point2(5, 3)).has_value())
        << "a spline's parameter is not its length: no place along it";
    const auto reply = d.interpreter.run("LEADER #" + std::to_string(spline) + "@5,3 20,20 text=note");
    ASSERT_FALSE(reply.ok());
    EXPECT_NE(reply.error().message.find("spline"), std::string::npos)
        << "the refusal says why: " << reply.error().describe();
}

TEST(NewKindsInMain, ALeaderOnACurvePolylineReadsItsArcLengthAndArea)
{
    Drawing d;
    const EntityId id = d.add(lot());
    const auto values = katana::entity::anchorValues(d.entity(id), AnchorRef{id, AnchorPoint::Along, 1, 0.5},
                                                     Point2(15, 5));
    EXPECT_NEAR(number(values, "radius"), 5.0, 1e-9) << "the tip is on the arc: an arc's values";
    EXPECT_NEAR(number(values, "area"), kLotArea, 1e-9);
    EXPECT_NEAR(number(values, "perimeter"), kLotPerimeter, 1e-9);
    EXPECT_NEAR(number(values, "length"), kLotPerimeter, 1e-9) << "the whole line's length";
    EXPECT_EQ(number(values, "vertices"), 4.0);
    // Ten metres of the first side, then half the arc's 5 pi.
    EXPECT_NEAR(number(values, "chainage"), 10.0 + 2.5 * kPi, 1e-9);
}

TEST(NewKindsInMain, LeadersForAndAttachPutTheTipInsideAClosedCurvePolyline)
{
    Drawing d;
    const EntityId id = d.add(lot());
    const auto placed = annotation::leaderPlaceOn(d.entity(id), 0.0);
    ASSERT_TRUE(placed.has_value()) << "LEADER FOR had nowhere to put the tip";
    EXPECT_EQ(placed->ref.point, AnchorPoint::Inside);
    const auto attached = annotation::tipPlaceOn(d.entity(id), Point2(12, 5));
    ASSERT_TRUE(attached.has_value());
    EXPECT_EQ(attached->ref.point, AnchorPoint::Inside) << "(12,5) is inside the curved side";

    // A whole ellipse, semi-axes 4 east and 2: (3,1) is inside (9/16 + 1/4
    // < 1), (3,1.5) outside (9/16 + 9/16 > 1).
    const EntityId ellipse = d.add(Ellipse2{Point2(0, 0), Vec2(4, 0), 0.5, 0.0, 2.0 * kPi});
    const auto in = annotation::tipPlaceOn(d.entity(ellipse), Point2(3, 1));
    ASSERT_TRUE(in.has_value());
    EXPECT_EQ(in->ref.point, AnchorPoint::Inside);
    const auto out = annotation::tipPlaceOn(d.entity(ellipse), Point2(3, 1.5));
    ASSERT_TRUE(out.has_value());
    EXPECT_EQ(out->ref.point, AnchorPoint::Along);

    const EntityId spline =
        d.add(*katana::geometry::Spline2::throughPoints({Point2(0, 0), Point2(5, 3), Point2(10, 0)}, 3));
    const auto start = annotation::tipPlaceOn(d.entity(spline), Point2(5, 3));
    ASSERT_TRUE(start.has_value()) << "Attach refused a spline";
    EXPECT_EQ(start->ref.point, AnchorPoint::Start);
}

// ---- labels -----------------------------------------------------------------------------------

TEST(NewKindsInMain, ACurvePolylineIsLabelledByAreaSegmentAndArcStyles)
{
    Drawing d;
    const EntityId id = d.add(lot());
    const auto& model = d.document.model();

    LabelStyle area{.name = "Lot", .kind = LabelKind::Area, .text = "{area}"};
    const auto areas = katana::entity::labelPiecesFor(model, d.entity(id), -1, area);
    ASSERT_EQ(areas.size(), 1u) << "a lot with a curved side got no area label";
    EXPECT_NEAR(number(areas.front().values, "area"), kLotArea, 1e-9)
        << "exact, the arc's segment of area included";

    LabelStyle segment{.name = "Side", .kind = LabelKind::Segment, .text = "{length}"};
    EXPECT_EQ(katana::entity::labelPiecesFor(model, d.entity(id), -1, segment).size(), 4u)
        << "three straight sides and the arc";
    LabelStyle arc{.name = "Curve", .kind = LabelKind::Arc, .text = "{radius}"};
    const auto arcs = katana::entity::labelPiecesFor(model, d.entity(id), -1, arc);
    ASSERT_EQ(arcs.size(), 1u);
    EXPECT_NEAR(number(arcs.front().values, "radius"), 5.0, 1e-9);
    EXPECT_NEAR(number(arcs.front().values, "length"), 5.0 * kPi, 1e-9);
}

TEST(NewKindsInMain, LabelsKeepOffTheNewKinds)
{
    Drawing d;
    d.add(lot());
    // Three straight sides, and the semicircle chorded as an arc is: sixteen
    // chords a turn, so eight.
    EXPECT_EQ(annotation::labelKeepOut(d.document.model(), 1000.0, annotation::estimatedMeasure(), 1000)
                  .size(),
              11u);
    Drawing e;
    e.add(Ellipse2{Point2(0, 0), Vec2(4, 0), 0.5, 0.0, 2.0 * kPi});
    EXPECT_FALSE(annotation::labelKeepOut(e.document.model(), 1000.0, annotation::estimatedMeasure(), 1000)
                     .empty())
        << "a label could stand on an ellipse";
}

// ---- snaps that make a dimension or leader follow ---------------------------------------------

TEST(NewKindsInMain, ASnapToACurvePolylinesArcMiddleNamesItsSegment)
{
    Drawing d;
    const EntityId id = d.add(lot());
    SnapResult snap;
    snap.point = Point2(15, 5);
    snap.mode = SnapMode::Midpoint;
    snap.entity = id;
    const auto anchor = snapAnchor(d.document.model(), snap);
    ASSERT_TRUE(anchor.has_value()) << "the snap named nothing: the note would not follow";
    EXPECT_EQ(anchor->point, AnchorPoint::SegmentMid);
    EXPECT_EQ(anchor->index, 1u);

    const EntityId ellipse = d.add(Ellipse2{Point2(50, 50), Vec2(4, 0), 0.5, 0.0, 2.0 * kPi});
    snap.point = Point2(50, 50);
    snap.mode = SnapMode::Center;
    snap.entity = ellipse;
    const auto centre = snapAnchor(d.document.model(), snap);
    ASSERT_TRUE(centre.has_value());
    EXPECT_EQ(centre->point, AnchorPoint::Centre);
}

// ---- AREA and PARCEL ----------------------------------------------------------------------------

TEST(NewKindsInMain, AreaMeasuresAClosedCurvePolylineAndAnEllipseExactly)
{
    Drawing d;
    const EntityId curved = d.add(lot());
    // pi a b, with a = 4 and b = 2.
    const EntityId ellipse = d.add(Ellipse2{Point2(50, 50), Vec2(4, 0), 0.5, 0.0, 2.0 * kPi});
    const auto result = computeArea(d.document, {curved, ellipse});
    ASSERT_TRUE(result.ok()) << result.error().describe();
    ASSERT_EQ(result->items.size(), 2u) << "skipped as having no area";
    EXPECT_NEAR(result->items[0].area, kLotArea, 1e-9);
    EXPECT_NEAR(result->items[0].perimeter, kLotPerimeter, 1e-9);
    EXPECT_NEAR(result->items[1].area, 8.0 * kPi, 1e-9);
}

TEST(NewKindsInMain, ParcelRefusesArcCoursesSayingSoRatherThanChordingThem)
{
    Drawing d;
    const EntityId id = d.add(lot());
    const auto reply = d.interpreter.run("PARCEL " + std::to_string(id));
    ASSERT_FALSE(reply.ok());
    EXPECT_EQ(reply.error().code, ErrorCode::Unsupported);
    EXPECT_NE(reply.error().message.find("arc"), std::string::npos) << reply.error().describe();
}

// ---- the one scope grammar ------------------------------------------------------------------

TEST(NewKindsInMain, TypeFiltersNameTheNewKindsInAnyCase)
{
    for (const char* word : {"TYPE=curvepolyline", "TYPE=CurvePolyline", "TYPE=CURVEPOLYLINE"}) {
        ModifyFilter filter;
        ASSERT_TRUE(parseWhereCondition(word, filter).ok()) << word;
        EXPECT_EQ(filter.types, (std::set<EntityType>{EntityType::CurvePolyline})) << word;
    }
    ModifyFilter filter;
    ASSERT_TRUE(parseWhereCondition("TYPE=ellipse,spline", filter).ok());
    EXPECT_EQ(filter.types, (std::set<EntityType>{EntityType::Ellipse, EntityType::Spline}));

    // And what the words are written as reads back as the same filter.
    ScopeWords words;
    words.filter.types = {EntityType::CurvePolyline};
    const auto written = formatScopeWords(words);
    ASSERT_TRUE(written.ok());
    const auto tokens = CommandInterpreter::tokenize(*written);
    ASSERT_TRUE(tokens.ok());
    std::size_t at = 0;
    const auto read = parseScopeWords(*tokens, at);
    ASSERT_TRUE(read.ok()) << *written << ": " << read.error().describe();
    EXPECT_EQ(read->filter.types, words.filter.types) << *written;

    Drawing d;
    d.add(lot());
    d.add(Ellipse2{Point2(50, 50), Vec2(4, 0), 0.5, 0.0, 2.0 * kPi});
    ASSERT_TRUE(d.interpreter.run("SELECT TYPE CURVEPOLYLINE").ok());
    EXPECT_EQ(d.document.selection().ids().size(), 1u);
}

// ---- the utilities' design line -----------------------------------------------------------------

TEST(NewKindsInMain, AUtilityDesignLineMayHaveArcs)
{
    Drawing d;
    // A quarter circle of radius 10 from (0,0) to (10,10), centre (0,10),
    // at 50 then 51: bulge tan(90/4).
    const EntityId id = d.add(CurvePolyline2{
        {CurveVertex{Point2(0, 0), std::tan(kPi / 8.0), 50.0}, CurveVertex{Point2(10, 10), 0.0, 51.0}},
        false});
    const auto design = utilities::designFromEntity(d.entity(id), std::nullopt);
    ASSERT_TRUE(design.ok()) << design.error().describe();
    ASSERT_GT(design->vertices.size(), 2u) << "the arc was taken as its chord";
    for (const auto& vertex : design->vertices) {
        // Every chord's end is ON the arc: 10 from the centre. The design
        // vertices are northing, easting.
        const double east = vertex.position.easting;
        const double north = vertex.position.northing;
        EXPECT_NEAR(std::hypot(east - 0.0, north - 10.0), 10.0, 1e-9);
    }
    EXPECT_EQ(design->vertices.front().level, 50.0);
    EXPECT_EQ(design->vertices.back().level, 51.0);
}

// ---- the lines that replace the drawing -----------------------------------------------------------

TEST(NewKindsInMain, OnlyAnOpenOfAProjectReplacesTheDrawing)
{
    EXPECT_TRUE(CommandInterpreter::replacesDocument("NEW"));
    EXPECT_TRUE(CommandInterpreter::replacesDocument("OPEN /projects/site"));
    EXPECT_TRUE(CommandInterpreter::replacesDocument("open \"C:/my projects/site\""));
    // OPEN of polylines (docs/drawing.md) is an edit: the window must not
    // ask to discard the drawing for it, nor katana_mcp refuse it for
    // unsaved changes.
    EXPECT_FALSE(CommandInterpreter::replacesDocument("OPEN #12"));
    EXPECT_FALSE(CommandInterpreter::replacesDocument("OPEN #12 #13"));
    EXPECT_FALSE(CommandInterpreter::replacesDocument("open selection"));
    EXPECT_FALSE(CommandInterpreter::replacesDocument("OPEN"));
    EXPECT_FALSE(CommandInterpreter::replacesDocument("SAVE /projects/site"));
    EXPECT_FALSE(CommandInterpreter::replacesDocument(""));
}
