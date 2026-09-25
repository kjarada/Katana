// The setting-out table and the profile report (include/katana/cad/
// alignment_report.hpp), and the ALIGN verbs that print them and edit PIs.
//
// The numbers are worked by hand from the circle, not taken from a run:
//   * the corner (0,0) (100,0) (100,100) rounded with R = 50 turns 90 degrees,
//     so its tangent length is R tan 45 = 50 - the TC is at chainage 50,
//     (50,0) - and its arc is 50 pi / 2 = 78.540 long, so the CT is at
//     128.540, (100,50), and the end at 178.540;
//   * 25 m into that arc the chord angle is 25 / 50 = 0.5 rad about the
//     centre (50,50): the point is (50 + 50 sin 0.5, 50 - 50 cos 0.5) =
//     (73.971, 6.121), heading 0.5 rad left of east, azimuth 61.352 degrees;
//   * a 30 degree corner with R = 300 and 90 m spirals keeps an arc of
//     R (delta - 2 theta_s) with theta_s = Ls / 2R = 0.15 rad: 67.080 m;
//   * the textbook sag, -3% into +2% over a 100 m curve, has K = 100 / 5 = 20.

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <limits>
#include <string>
#include <vector>

#include "katana/cad/alignment_report.hpp"
#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/geometry/alignment.hpp"
#include "katana/geometry/profile.hpp"
#include "katana/math/numerics.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::SettingOutStation;
using katana::core::ErrorCode;
using katana::geometry::AlignmentPI;
using katana::geometry::HorizontalAlignment;
using katana::geometry::Point2;
using katana::geometry::SolvedAlignment;
using katana::math::kDegToRad;
using katana::math::kPi;

namespace {

SolvedAlignment solved(std::vector<AlignmentPI> pis)
{
    HorizontalAlignment definition;
    definition.pis = std::move(pis);
    auto result = katana::geometry::solveAlignment(definition);
    EXPECT_TRUE(result.ok()) << (result.ok() ? "" : result.error().describe());
    return *result;
}

// (0,0) (100,0) (100,100), the corner rounded with R = 50: a left turn.
SolvedAlignment roundedCorner()
{
    return solved({{Point2(0, 0)}, {Point2(100, 0), 50.0}, {Point2(100, 100)}});
}

std::vector<std::string> keysOf(const std::vector<SettingOutStation>& rows)
{
    std::vector<std::string> keys;
    for (const SettingOutStation& row : rows) {
        if (!row.key.empty()) {
            keys.push_back(row.key);
        }
    }
    return keys;
}

struct Session {
    Document document;
    CommandInterpreter interpreter{document};

    std::string ok(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << " -> " << (reply.ok() ? "" : reply.error().describe());
        return reply.ok() ? *reply : std::string{};
    }
    ErrorCode fails(const std::string& line)
    {
        const auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " unexpectedly succeeded: " << (reply.ok() ? *reply : "");
        return reply.ok() ? ErrorCode::Internal : reply.error().code;
    }
    const std::vector<AlignmentPI>& pis(const std::string& name)
    {
        static const std::vector<AlignmentPI> none;
        const auto* alignment = document.model().alignments.find(name);
        EXPECT_NE(alignment, nullptr) << "no alignment " << name;
        return alignment != nullptr ? alignment->horizontal.pis : none;
    }
};

} // namespace

TEST(SettingOut, EveryIntervalStationAndEveryKeyStationIsARowAndAKeyOnTheIntervalIsOneRow)
{
    const auto rows = katana::cad::settingOutStations(roundedCorner(), 25.0);
    ASSERT_TRUE(rows.ok()) << rows.error().describe();
    // 0 25 50 75 100 125 150 175 on the interval, the CT at 128.540 and the
    // end at 178.540 off it; the TC at 50 is on it and is ONE row.
    const std::vector<double> expected = {0, 25, 50, 75, 100, 125, 50 + 25 * kPi, 150, 175,
                                          100 + 25 * kPi};
    ASSERT_EQ(rows->size(), expected.size());
    for (std::size_t i = 0; i < expected.size(); ++i) {
        EXPECT_NEAR((*rows)[i].station, expected[i], 1e-9) << "row " << i;
    }
    EXPECT_EQ((*rows)[0].key, "start");
    EXPECT_EQ((*rows)[2].key, "TC");
    EXPECT_EQ((*rows)[6].key, "CT");
    EXPECT_EQ((*rows)[9].key, "end");
    EXPECT_TRUE((*rows)[1].key.empty());
}

TEST(SettingOut, APointOnTheArcIsWhereTheCircleSaysWithItsAzimuthAndCurvature)
{
    const auto rows = katana::cad::settingOutStations(roundedCorner(), 25.0);
    ASSERT_TRUE(rows.ok());
    const SettingOutStation& onArc = (*rows)[3]; // chainage 75
    EXPECT_NEAR(onArc.point.x, 50.0 + 50.0 * std::sin(0.5), 1e-6);
    EXPECT_NEAR(onArc.point.y, 50.0 - 50.0 * std::cos(0.5), 1e-6);
    EXPECT_NEAR(onArc.direction, 0.5, 1e-9);
    EXPECT_NEAR(onArc.azimuth, kPi / 2.0 - 0.5, 1e-9);
    EXPECT_NEAR(onArc.curvature, 1.0 / 50.0, 1e-12); // left
    // The start runs due east, the end due north: azimuths 90 and 0.
    EXPECT_NEAR(rows->front().azimuth, kPi / 2.0, 1e-12);
    EXPECT_NEAR(rows->back().azimuth, 0.0, 1e-9);
    EXPECT_NEAR(rows->back().point.x, 100.0, 1e-9);
    EXPECT_NEAR(rows->back().point.y, 100.0, 1e-9);
    EXPECT_EQ(rows->front().curvature, 0.0);
}

TEST(SettingOut, ASpiralledCurveIsKeyedTSSCCSSTWithTheLengthsBetweenThem)
{
    const auto alignment =
        solved({{Point2(0, 0)}, {Point2(400, 0), 300.0, 90.0, 90.0}, {Point2(659.808, 150)}});
    const auto rows = katana::cad::settingOutStations(alignment, 1000.0);
    ASSERT_TRUE(rows.ok()) << rows.error().describe();
    // An interval longer than the road leaves only the key stations.
    EXPECT_EQ(keysOf(*rows), (std::vector<std::string>{"start", "TS", "SC", "CS", "ST", "end"}));
    ASSERT_EQ(rows->size(), 6u);
    EXPECT_NEAR((*rows)[2].station - (*rows)[1].station, 90.0, 1e-6);
    // 659.808,150 is 30 degrees to 3 decimals, so the arc is too.
    EXPECT_NEAR((*rows)[3].station - (*rows)[2].station, 300.0 * (30.0 * kDegToRad - 0.3), 1e-3);
    EXPECT_NEAR((*rows)[4].station - (*rows)[3].station, 90.0, 1e-6);
}

TEST(SettingOut, ACornerWithNoCurveIsKeyedPI)
{
    const auto rows = katana::cad::settingOutStations(
        solved({{Point2(0, 0)}, {Point2(100, 0)}, {Point2(100, 100)}}), 150.0);
    ASSERT_TRUE(rows.ok());
    EXPECT_EQ(keysOf(*rows), (std::vector<std::string>{"start", "PI", "end"}));
    // 0 and 150 on the interval, the PI at 100 and the end at 200.
    ASSERT_EQ(rows->size(), 4u);
    EXPECT_NEAR((*rows)[1].station, 100.0, 1e-9);
    EXPECT_NEAR((*rows)[2].station, 150.0, 1e-9);
}

TEST(SettingOut, AnIntervalThatIsNotPositiveOrGivesTooManyStationsIsRefused)
{
    const SolvedAlignment alignment = roundedCorner();
    for (const double interval : {0.0, -5.0, std::numeric_limits<double>::quiet_NaN(),
                                  std::numeric_limits<double>::infinity()}) {
        const auto rows = katana::cad::settingOutStations(alignment, interval);
        ASSERT_FALSE(rows.ok()) << interval;
        EXPECT_EQ(rows.error().code, ErrorCode::InvalidArgument);
    }
    // 100 + 25 pi = 178.5398 m every 1 mm is the stations 0 to 178.539 m:
    // 178 540 of them, over the limit.
    const auto tooMany = katana::cad::settingOutStations(alignment, 0.001);
    ASSERT_FALSE(tooMany.ok());
    EXPECT_EQ(tooMany.error().code, ErrorCode::InvalidArgument);
    EXPECT_NE(tooMany.error().context.find("178540 stations"), std::string::npos)
        << tooMany.error().describe();
}

TEST(SettingOut, TheRecordsGiveEveryFieldAsKeyEqualsValue)
{
    const auto rows = katana::cad::settingOutStations(roundedCorner(), 25.0);
    ASSERT_TRUE(rows.ok());
    const std::string text = katana::cad::formatSettingOut(*rows);
    EXPECT_EQ(text.substr(0, text.find('\n')),
              "station=0.000 x=0.000 y=0.000 direction=0.000 azimuth=90°00'00\" "
              "radius=straight key=start");
    EXPECT_NE(text.find("station=75.000 x=73.971 y=6.121 direction=28.648 "
                        "azimuth=61°21'08\" radius=50.000 turn=left\n"),
              std::string::npos)
        << text;
    // A station with no key has no key field: absent, not blank.
    EXPECT_NE(text.find("station=25.000 x=25.000 y=0.000 direction=0.000 "
                        "azimuth=90°00'00\" radius=straight\n"),
              std::string::npos)
        << text;
    EXPECT_EQ(std::count(text.begin(), text.end(), '\n') + 1, 10);
}

TEST(SettingOut, ARightHandCurveSaysSoAndItsCsvRadiusIsNegative)
{
    const auto rows = katana::cad::settingOutStations(
        solved({{Point2(0, 0)}, {Point2(100, 0), 50.0}, {Point2(100, -100)}}), 25.0);
    ASSERT_TRUE(rows.ok());
    EXPECT_NE(katana::cad::formatSettingOut(*rows).find("radius=50.000 turn=right"),
              std::string::npos);
    const std::string csv = katana::cad::settingOutCsv(*rows);
    EXPECT_EQ(csv.substr(0, csv.find("\r\n")),
              "Chainage,Easting,Northing,Azimuth (deg),Azimuth (DMS),Radius,Key");
    // The DMS field holds a double quote, so it is quoted and the quote doubled.
    EXPECT_NE(csv.find("0.000,0.000,0.000,90.000000,\"90°00'00\"\"\",,start\r\n"),
              std::string::npos)
        << csv;
    EXPECT_NE(csv.find(",-50.000,"), std::string::npos) << csv;
    // A header and one line per row, each ended CRLF.
    std::size_t lines = 0;
    for (std::size_t at = csv.find("\r\n"); at != std::string::npos; at = csv.find("\r\n", at + 2)) {
        ++lines;
    }
    EXPECT_EQ(lines, rows->size() + 1);
}

TEST(ProfileReport, AVerticalCurvesKIsItsLengthPerPercentOfGradeChange)
{
    katana::geometry::VerticalAlignment sag;
    sag.pvis = {{0, 16}, {100, 13, 100}, {300, 17}};
    const auto profile = katana::geometry::solveProfile(sag);
    ASSERT_TRUE(profile.ok());
    std::size_t curves = 0;
    for (const auto& element : profile->elements()) {
        const auto k = katana::cad::curveK(element);
        if (element.kind == katana::geometry::ProfileElementKind::Curve) {
            ++curves;
            ASSERT_TRUE(k.has_value());
            EXPECT_NEAR(*k, 20.0, 1e-9);
        } else {
            EXPECT_FALSE(k.has_value());
        }
    }
    EXPECT_EQ(curves, 1u);
    const std::string report = katana::cad::formatProfileReport(sag, *profile);
    EXPECT_NE(report.find("K=20.000"), std::string::npos) << report;
    EXPECT_NE(report.find("low point  at 110.000 @ 13.600"), std::string::npos) << report;
}

TEST(ProfileReport, ACurveWithNoChangeOfGradeHasNoK)
{
    katana::geometry::ProfileElement flat;
    flat.kind = katana::geometry::ProfileElementKind::Curve;
    flat.length = 50.0;
    flat.startGrade = 0.02;
    flat.endGrade = 0.02;
    EXPECT_FALSE(katana::cad::curveK(flat).has_value());
}

TEST(AlignVerb, StationsPrintsTheSettingOutFunctionsTable)
{
    Session session;
    session.ok("ALIGN NEW road 0,0 100,0 100,100");
    session.ok("ALIGN SET road 1 50");
    const auto rows = katana::cad::settingOutStations(roundedCorner(), 25.0);
    ASSERT_TRUE(rows.ok());
    EXPECT_EQ(session.ok("ALIGN STATIONS road 25"), katana::cad::formatSettingOut(*rows));
    EXPECT_EQ(session.fails("ALIGN STATIONS road 0.0001"), ErrorCode::InvalidArgument);
}

TEST(AlignVerb, PisAlonePrintsEveryPIExactlyAsARecord)
{
    Session session;
    session.ok("ALIGN NEW road 0,0 100.125,0.1 100,100");
    session.ok("ALIGN SET road 1 50 10 12.5");
    EXPECT_EQ(session.ok("ALIGN PIS road"),
              "pi index=0 x=0 y=0 radius=0 spiral_in=0 spiral_out=0\n"
              "pi index=1 x=100.125 y=0.1 radius=50 spiral_in=10 spiral_out=12.5\n"
              "pi index=2 x=100 y=100 radius=0 spiral_in=0 spiral_out=0");
    EXPECT_EQ(session.fails("ALIGN PIS nosuch"), ErrorCode::NotFound);
}

TEST(AlignVerb, PisReplacesEveryPIAsOneUndoStepAndKeepsTheRest)
{
    Session session;
    session.ok("ALIGN NEW road 0,0 100,0 100,100");
    session.ok("ALIGN START road 1000");
    session.ok("ALIGN DESIGN road 1000,16 1150,17");
    const std::size_t before = session.document.history().undoCount();
    EXPECT_EQ(session.ok("ALIGN PIS road 0,0 100,0,50 100,200,0,0,0 250,200"),
              "alignment road now has 4 PIs");
    EXPECT_EQ(session.document.history().undoCount(), before + 1);
    const auto& pis = session.pis("road");
    ASSERT_EQ(pis.size(), 4u);
    EXPECT_EQ(pis[1].point, Point2(100, 0));
    EXPECT_EQ(pis[1].radius, 50.0);
    EXPECT_EQ(pis[2].point, Point2(100, 200));
    EXPECT_EQ(pis[3].point, Point2(250, 200));
    // The chainage origin and the design profile are not the PIs'.
    const auto* road = session.document.model().alignments.find("road");
    EXPECT_EQ(road->horizontal.startStation, 1000.0);
    EXPECT_TRUE(road->vertical.has_value());

    session.ok("UNDO");
    ASSERT_EQ(session.pis("road").size(), 3u);
    EXPECT_EQ(session.pis("road")[2].point, Point2(100, 100));
}

TEST(AlignVerb, PisRefusesWhatCannotBeAnAlignmentAndKeepsTheLastGoodOne)
{
    Session session;
    session.ok("ALIGN NEW road 0,0 100,0 100,100");
    EXPECT_EQ(session.fails("ALIGN PIS road 0,0"), ErrorCode::InvalidArgument); // one PI
    EXPECT_EQ(session.fails("ALIGN PIS road 0,0 5"), ErrorCode::InvalidArgument);
    EXPECT_EQ(session.fails("ALIGN PIS road 0,0 1,2,3,4,5,6"), ErrorCode::InvalidArgument);
    EXPECT_EQ(session.fails("ALIGN PIS road 0,0 1,x"), ErrorCode::InvalidArgument);
    EXPECT_EQ(session.fails("ALIGN PIS road 0,0 1,1,"), ErrorCode::InvalidArgument);
    // Coincident PIs: the solver refuses them, naming them, and the model
    // keeps the definition it had.
    EXPECT_EQ(session.fails("ALIGN PIS road 0,0 0,0 5,5"), ErrorCode::InvalidGeometry);
    EXPECT_EQ(session.pis("road").size(), 3u);
    EXPECT_EQ(session.pis("road")[1].point, Point2(100, 0));
}
