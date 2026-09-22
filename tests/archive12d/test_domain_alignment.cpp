#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <numbers>
#include <sstream>
#include <string>

#include "katana/archive12d/domain.hpp"
#include "katana/archive12d/reader.hpp"
#include "katana/archive12d/writer.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"

namespace a12 = katana::archive12d;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

constexpr double kPi = std::numbers::pi;

// These fixtures are hand-written, and a hand-written 12da inherits the
// format's CURRENT BREAKLINE TYPE, whose default is `point` (commands,
// 1.4.4). A fixture that means a line has to say so, exactly as every string
// in a real archive does - 12d writes the flag on all 25,659 strings of a
// production file and leaves nothing to the default. Rather than repeat it in
// every fixture, the helper states it once at file level, which is the
// format's own way of saying it; a fixture that wants POINTS says
// `breakline point` inside the string and overrides this.
constexpr const char* kBreaklineLine = "breakline line\n";

a12::DomainImport import(const std::string& text, const a12::ImportOptions& options = {})
{
    auto archive = a12::readArchive(kBreaklineLine + text);
    EXPECT_TRUE(archive.ok()) << (archive.ok() ? "" : archive.error().describe());
    auto domain = a12::toDomain(archive.ok() ? *archive : a12::Archive{}, options);
    EXPECT_TRUE(domain.ok());
    return domain.ok() ? std::move(*domain) : a12::DomainImport{};
}

std::string allWarnings(const a12::DomainImport& domain)
{
    std::ostringstream out;
    for (const std::string& warning : domain.warnings) {
        out << "\n  " << warning;
    }
    return out.str();
}

bool anyWarningContains(const a12::DomainImport& domain, const std::string& needle)
{
    return std::any_of(domain.warnings.begin(), domain.warnings.end(),
                       [&](const std::string& w) { return w.find(needle) != std::string::npos; });
}

// A right-angle corner rounded at radius 40, worked by hand: PIs (0,0),
// (100,0), (100,100); the tangent length of a 90 degree curve is R, so the
// curve runs from (60,0) to (100,40) and turns LEFT - a negative radius in 12d.
constexpr const char* kCornerElements = R"(
  horizontal_data { name "C" closed 0
    data_2d { 0 0  60 0  100 40  100 100 }
    geometry_data { straight { } arc { radius RADIUS major 0 } straight { } } })";

std::string corner(const std::string& radius)
{
    std::string text = kCornerElements;
    text.replace(text.find("RADIUS"), 6, radius);
    return text;
}

} // namespace

TEST(DomainImportAlignment, TheIpMethodGivesItsPisDirectly)
{
    const auto domain = import("model \"Road\" string super_alignment { name \"MC01\" chainage 1000\n"
                               " spiral_type \"clothoid\" horizontal_parts {\n"
                               "  ip { id 100 x 0 y 0 } spiral { id 200 r 200 l1 30 l2 40 x 500 y 0 }\n"
                               "  arc { id 300 r 150 x 500 y 600 } ip { id 400 x 1000 y 600 } } }");
    ASSERT_EQ(domain.alignments.size(), 1u) << allWarnings(domain);
    const auto& alignment = domain.alignments[0];
    EXPECT_EQ(alignment.name, "MC01");
    EXPECT_EQ(alignment.horizontal.startStation, 1000.0);
    ASSERT_EQ(alignment.horizontal.pis.size(), 4u);
    EXPECT_EQ(alignment.horizontal.pis[1],
              (katana::geometry::AlignmentPI{Point2(500.0, 0.0), 200.0, 30.0, 40.0}));
    EXPECT_EQ(alignment.horizontal.pis[2].radius, 150.0);
    EXPECT_EQ(alignment.horizontal.pis[2].spiralIn, 0.0);
    EXPECT_FALSE(alignment.vertical.has_value());
    // ... and the centreline is on the drawing as well.
    ASSERT_EQ(domain.entities.size(), 1u);
    EXPECT_TRUE(std::holds_alternative<Polyline2>(domain.entities[0].geometry));
    EXPECT_EQ(domain.entities[0].layer, "Road");
}

TEST(DomainImportAlignment, PisAreReconstructedFromSolvedElementsAndTheTurnIsReadTheRightWay)
{
    const auto domain = import("string super_alignment { name \"C\"" + corner("-40") + " }");
    ASSERT_EQ(domain.alignments.size(), 1u) << allWarnings(domain);
    const auto& pis = domain.alignments[0].horizontal.pis;
    ASSERT_EQ(pis.size(), 3u);
    EXPECT_NEAR(pis[1].point.x, 100.0, 1e-6);
    EXPECT_NEAR(pis[1].point.y, 0.0, 1e-6);
    EXPECT_EQ(pis[1].radius, 40.0);

    // What Katana solves from them is the curve that was written: a quarter
    // circle of radius 40 between two 60 m tangents.
    const auto solved = katana::geometry::solveAlignment(domain.alignments[0].horizontal);
    ASSERT_TRUE(solved.ok());
    EXPECT_NEAR(solved->length(), 60.0 + 40.0 * kPi / 2.0 + 60.0, 1e-9);
}

TEST(DomainImportAlignment, AReconstructionThatDoesNotMatchTwelveDsGeometryIsRejected)
{
    // The same corner with the radius's sign flipped: the arc now bulges the
    // other way, its tangents meet nowhere near (100,0), and the alignment
    // solved from that does not pass through 12d's vertices. It must arrive as
    // a polyline only - with its shape, and a reason - not as a wrong Alignment.
    const auto domain = import("string super_alignment { name \"C\"" + corner("40") + " }");
    EXPECT_TRUE(domain.alignments.empty());
    ASSERT_EQ(domain.entities.size(), 1u);
    EXPECT_TRUE(anyWarningContains(domain, "imported as a polyline only")) << allWarnings(domain);
}

TEST(DomainImportAlignment, TransitionsKatanaDoesNotHaveKeepTheAlignmentAPolyline)
{
    const auto domain = import(R"(string super_alignment { name "Rail" horizontal_data { closed 0
  data_2d { 1262.0 1760.41  1342.85432099 1789.30813759  1419.06779208 1813.40212949 }
  geometry_data { straight { }
    spiral { type "cubic parabola" leading 1 l1 0 r1 0 a1 20.20620846 l2 80 r2 294.56299992 a2 12.2642972 } } } })");
    EXPECT_TRUE(domain.alignments.empty());
    ASSERT_EQ(domain.entities.size(), 1u);
    EXPECT_GT(std::get<Polyline2>(domain.entities[0].geometry).vertices.size(), 10u)
        << "the transition is still drawn as the curve it is";
    EXPECT_TRUE(anyWarningContains(domain, "cubic parabola")) << allWarnings(domain);
}

TEST(DomainImportAlignment, CompoundCurvesAndAlignmentsEndingOnACurveAreNotPiDefinable)
{
    const auto compound = import(R"(string super_alignment { name "Compound" horizontal_data { closed 0
  data_2d { 0 0  60 0  100 40  100 100 }
  geometry_data { straight { } arc { radius -40 major 0 } arc { radius -5000 major 0 } } } })");
    EXPECT_TRUE(compound.alignments.empty());
    EXPECT_EQ(compound.entities.size(), 1u);
    EXPECT_TRUE(anyWarningContains(compound, "polyline only")) << allWarnings(compound);

    const auto endsOnCurve = import(R"(string super_alignment { name "Stub" horizontal_data { closed 0
  data_2d { 0 0  60 0  100 40 } geometry_data { straight { } arc { radius -40 major 0 } } } })");
    EXPECT_TRUE(endsOnCurve.alignments.empty());
    EXPECT_TRUE(anyWarningContains(endsOnCurve, "ends on a curve")) << allWarnings(endsOnCurve);
}

TEST(DomainImportAlignment, TheGradeLineComesFromTheSolvedVerticalGeometry)
{
    // VIPs (0,30), (100,33) with a 40 m parabola, (220,31). The curve runs from
    // chainage 80 to 120: 33 - 0.03*20 = 32.4 in, 33 - (2/120)*20 = 32.6667 out.
    const auto domain = import("string super_alignment { name \"C\"" + corner("-40") + R"(
  vertical_data { closed 0
    data_2d { 0 30  80 32.4  120 32.666666666666664  220 31 }
    geometry_data { straight { } parabola { chainage 100 height 33 } straight { } } } })");
    ASSERT_EQ(domain.alignments.size(), 1u) << allWarnings(domain);
    ASSERT_TRUE(domain.alignments[0].vertical.has_value()) << allWarnings(domain);
    const auto& pvis = domain.alignments[0].vertical->pvis;
    ASSERT_EQ(pvis.size(), 3u);
    EXPECT_EQ(pvis[1], (katana::geometry::ProfilePVI{100.0, 33.0, 40.0}));
    EXPECT_EQ(pvis[2], (katana::geometry::ProfilePVI{220.0, 31.0, 0.0}));
}

TEST(DomainImportAlignment, AGradeLineThatDoesNotMatchIsLeftOutAndTheAlignmentKept)
{
    // The parabola's VIP is recorded at height 40, which the vertices either
    // side of it contradict.
    const auto domain = import("string super_alignment { name \"C\"" + corner("-40") + R"(
  vertical_data { closed 0
    data_2d { 0 30  80 32.4  120 32.666666666666664  220 31 }
    geometry_data { straight { } parabola { chainage 100 height 40 } straight { } } } })");
    ASSERT_EQ(domain.alignments.size(), 1u);
    EXPECT_FALSE(domain.alignments[0].vertical.has_value());
    EXPECT_TRUE(anyWarningContains(domain, "vertical geometry was not imported"))
        << allWarnings(domain);
}

TEST(DomainImportAlignment, AnOldAlignmentStringsVipsBecomeTheGradeLine)
{
    // K = L / |change of grade in percent|. Grades +3% then -1.6667%: a change
    // of 4.6667%, so K = 10 asks for a curve of 46.667 m. A circular curve of
    // radius 1000 over the same change is 1000 * 0.046667 = 46.667 m too.
    const auto domain = import(R"(string alignment { name "AL"
  hipdata { 0 0 0  100 0 40  100 100 0 }
  vipdata { 0 30 0  100 33 40 parabola  220 31 0 } }
string super_alignment { name "K" horizontal_parts { ip { id 100 x 0 y 0 } ip { id 200 x 300 y 0 } }
  vertical_parts { ip { id 300 x 0 y 30 } kvalue { id 400 k 10 x 100 y 33 } ip { id 500 x 220 y 31 } } }
string super_alignment { name "R" horizontal_parts { ip { id 100 x 0 y 0 } ip { id 200 x 300 y 0 } }
  vertical_parts { ip { id 300 x 0 y 30 } radius { id 400 r 1000 x 100 y 33 } ip { id 500 x 220 y 31 } } })");
    ASSERT_EQ(domain.alignments.size(), 3u) << allWarnings(domain);
    ASSERT_TRUE(domain.alignments[0].vertical.has_value());
    EXPECT_EQ(domain.alignments[0].vertical->pvis[1].curveLength, 40.0);
    EXPECT_EQ(domain.alignments[0].horizontal.pis[1].radius, 40.0);
    const double change = (3.0 / 100.0 + 2.0 / 120.0) * 100.0;
    ASSERT_TRUE(domain.alignments[1].vertical.has_value());
    EXPECT_NEAR(domain.alignments[1].vertical->pvis[1].curveLength, 10.0 * change, 1e-9);
    ASSERT_TRUE(domain.alignments[2].vertical.has_value());
    EXPECT_NEAR(domain.alignments[2].vertical->pvis[1].curveLength, 1000.0 * change / 100.0, 1e-9);
}

TEST(DomainImportAlignment, NamesAreMadeUniqueAndABlankOneIsGivenOne)
{
    const std::string two = "horizontal_parts { ip { id 100 x 0 y 0 } ip { id 200 x 10 y 0 } }";
    const auto domain = import("string super_alignment { name \"A\" " + two + " }\n"
                               "string super_alignment { name \"a\" " + two + " }\n"
                               "string super_alignment { name \"\" " + two + " }");
    ASSERT_EQ(domain.alignments.size(), 3u);
    EXPECT_EQ(domain.alignments[0].name, "A");
    EXPECT_EQ(domain.alignments[1].name, "a (2)") << "names differing only in case would collide";
    EXPECT_EQ(domain.alignments[2].name, "Alignment");
}

TEST(DomainImportAlignment, AnAlignmentKatanaCannotSolveIsStillDrawnThroughItsIps)
{
    // One IP: nothing to solve, and no solved geometry. The IP itself is
    // still where the designer put it, and is drawn - as a point here, as the
    // tangent polygon when there are more - with a warning that says that is
    // all it is. Silently drawing nothing while saying "polyline only" was a
    // defect found by review.
    const auto one = import("string super_alignment { name \"One\" horizontal_parts { ip { id 100 x 5 y 6 } } }");
    EXPECT_TRUE(one.alignments.empty());
    ASSERT_EQ(one.entities.size(), 1u);
    EXPECT_EQ(std::get<katana::entity::PointGeometry>(one.entities[0].geometry).position,
              Point2(5.0, 6.0));
    EXPECT_TRUE(anyWarningContains(one, "\"One\"")) << allWarnings(one);
    EXPECT_TRUE(anyWarningContains(one, "polygon through its IPs only")) << allWarnings(one);

    // A reversal - the alignment doubles back through 180 degrees - cannot be
    // rounded by any curve (alignment.hpp), so the three IPs are all there is.
    const auto reversal = import("string alignment { name \"Back\" hipdata { 0 0 0  100 0 40  0 0 0 } }");
    EXPECT_TRUE(reversal.alignments.empty());
    ASSERT_EQ(reversal.entities.size(), 1u);
    const auto& polygon = std::get<Polyline2>(reversal.entities[0].geometry);
    ASSERT_EQ(polygon.vertices.size(), 3u);
    EXPECT_EQ(polygon.vertices[1], Point2(100.0, 0.0));
    EXPECT_TRUE(anyWarningContains(reversal, "polygon through its IPs only")) << allWarnings(reversal);
}

TEST(DomainImportAlignment, AnIpDefinitionWithAnotherTransitionTypeIsApproximatedAndSaidToBe)
{
    // Manual 1.5.16: an alignment string's spiral_type may be cubic parabola,
    // westrail-cubic or cubic spiral as well as clothoid. With no solved
    // geometry to check against, the IPs are taken with clothoids of the same
    // lengths - which is what the warning must say.
    const auto domain = import(R"(string alignment { name "rail" spiral_type "cubic parabola"
  hipdata { 0 0 0   500 0 200 spil1 40 spil2 40   900 400 0 }
  vipdata { 0 10 0  450 16 100  900 13 0 } })");
    ASSERT_EQ(domain.alignments.size(), 1u) << allWarnings(domain);
    EXPECT_EQ(domain.alignments[0].horizontal.pis.at(1).spiralIn, 40.0);
    ASSERT_TRUE(domain.alignments[0].vertical.has_value());
    EXPECT_EQ(domain.entities.size(), 1u);
    EXPECT_TRUE(anyWarningContains(domain, "represented by clothoids of the same length"))
        << allWarnings(domain);
}

TEST(DomainImportAlignment, AVerticalArcsMajorFlagIsIgnoredAsTheManualSays)
{
    // Manual 1.5.9.2.3(b): "major ... is ignored since only minor arcs are
    // used". Identical files but for the flag must give identical profiles.
    const std::string head = "string super_alignment { name \"V\" horizontal_data { closed 0"
                             " data_2d { 0 0  1000 0 } geometry_data { straight { } } }"
                             " vertical_data { closed 0 data_2d { 0 10  400 18  600 20  1000 20 }"
                             " geometry_data { straight { } arc { radius 10000.5 major ";
    const auto minor = import(head + "0 } straight { } } } }");
    const auto major = import(head + "1 } straight { } } } }");
    ASSERT_EQ(minor.alignments.size(), 1u) << allWarnings(minor);
    ASSERT_EQ(major.alignments.size(), 1u) << allWarnings(major);
    ASSERT_TRUE(minor.alignments[0].vertical.has_value()) << allWarnings(minor);
    ASSERT_TRUE(major.alignments[0].vertical.has_value()) << allWarnings(major);
    EXPECT_EQ(major.alignments[0].vertical->pvis, minor.alignments[0].vertical->pvis);
}
