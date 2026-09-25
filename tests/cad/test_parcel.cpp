// Parcels (include/katana/cad/parcel.hpp).
//
// The assertions here are definitions rather than worked results: due north
// is azimuth 0 and due east is pi/2, a 100 by 50 rectangle has 5000 of area
// and 300 of perimeter, a 3-4-5 triangle closes over 500. They are exactly
// what would catch the one easy mistake in this code, which is swapping
// easting and northing on the way into the survey layer - a rectangle would
// still report 5000, and every bearing would be wrong.

#include <gtest/gtest.h>

#include <cmath>
#include <limits>
#include <string>
#include <vector>

#include "katana/cad/parcel.hpp"
#include "katana/math/numerics.hpp"

using namespace katana::cad;
using katana::core::ErrorCode;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::math::kPi;

namespace {

Polyline2 closed(std::vector<Point2> vertices)
{
    Polyline2 boundary;
    boundary.vertices = std::move(vertices);
    boundary.closed = true;
    return boundary;
}

// (0,0) -> (100,0) -> (100,50) -> (0,50): east, north, west, south, drawn
// counter-clockwise.
Polyline2 rectangle()
{
    return closed({Point2(0, 0), Point2(100, 0), Point2(100, 50), Point2(0, 50)});
}

} // namespace

TEST(Parcel, ARectangleReportsItsCoursesAreaAndCentroid)
{
    const auto report = parcelReport(rectangle());
    ASSERT_TRUE(report.ok()) << report.error().describe();
    ASSERT_EQ(report->courses.size(), 4u);
    const auto& courses = report->courses;

    EXPECT_NEAR(courses[0].azimuth, kPi / 2.0, 1e-12) << "east";
    EXPECT_NEAR(courses[0].distance, 100.0, 1e-12);
    EXPECT_NEAR(courses[1].azimuth, 0.0, 1e-12) << "north";
    EXPECT_NEAR(courses[1].distance, 50.0, 1e-12);
    EXPECT_NEAR(courses[2].azimuth, 3.0 * kPi / 2.0, 1e-12) << "west";
    EXPECT_NEAR(courses[3].azimuth, kPi, 1e-12) << "south";
    EXPECT_EQ(courses[3].to, Point2(0, 0)) << "the last course closes on the first vertex";

    EXPECT_NEAR(report->area, 5000.0, 1e-9);
    EXPECT_NEAR(report->perimeter, 300.0, 1e-9);
    EXPECT_NEAR(report->centroid.x, 50.0, 1e-9);
    EXPECT_NEAR(report->centroid.y, 25.0, 1e-9);
    EXPECT_FALSE(report->clockwise);

    // Bearings as a surveyor writes them: the survey layer puts pi/2 in the
    // north-east quadrant as N 90 E, pi as S 0 E, 3 pi / 2 as S 90 W.
    EXPECT_NE(courses[0].bearing.find("N 90"), std::string::npos) << courses[0].bearing;
    EXPECT_NE(courses[0].bearing.find('E'), std::string::npos);
    EXPECT_NE(courses[1].bearing.find("N 0"), std::string::npos) << courses[1].bearing;
    EXPECT_NE(courses[2].bearing.find("S 90"), std::string::npos) << courses[2].bearing;
    EXPECT_NE(courses[2].bearing.find('W'), std::string::npos);
    EXPECT_NE(courses[3].bearing.find("S 0"), std::string::npos) << courses[3].bearing;
}

TEST(Parcel, TheSameBoundaryDrawnClockwiseIsTheSameParcel)
{
    const auto report =
        parcelReport(closed({Point2(0, 0), Point2(0, 50), Point2(100, 50), Point2(100, 0)}));
    ASSERT_TRUE(report.ok()) << report.error().describe();
    EXPECT_TRUE(report->clockwise);
    EXPECT_NEAR(report->area, 5000.0, 1e-9) << "area is never negative";
    EXPECT_NEAR(report->perimeter, 300.0, 1e-9);
    EXPECT_NEAR(report->courses[0].azimuth, 0.0, 1e-12) << "the first course now runs north";
    EXPECT_NEAR(report->centroid.x, 50.0, 1e-9);
}

TEST(Parcel, AClosingCourseAcrossTwoQuadrantsReadsAsAQuadrantBearing)
{
    // (0,0) -> (300,0) -> (300,400): the closing course runs 300 west and
    // 400 south, azimuth atan2(-300, -400) + 2 pi = 216.8699 degrees, which
    // surveyors write S 36°52'12" W (36.8699 degrees is 36°52'11.6", to the
    // nearest second 12). The 3-4-5 triangle closes over exactly 500.
    const auto report = parcelReport(closed({Point2(0, 0), Point2(300, 0), Point2(300, 400)}));
    ASSERT_TRUE(report.ok()) << report.error().describe();
    ASSERT_EQ(report->courses.size(), 3u);
    const ParcelCourse& closing = report->courses[2];
    EXPECT_NEAR(closing.azimuth, std::atan2(-300.0, -400.0) + 2.0 * kPi, 1e-12);
    EXPECT_NEAR(closing.distance, 500.0, 1e-12);
    EXPECT_NE(closing.bearing.find("S 36"), std::string::npos) << closing.bearing;
    EXPECT_NE(closing.bearing.find("52'"), std::string::npos) << closing.bearing;
    EXPECT_NE(closing.bearing.find("12\""), std::string::npos) << closing.bearing;
    EXPECT_NE(closing.bearing.find('W'), std::string::npos) << closing.bearing;
    EXPECT_NEAR(report->area, 60000.0, 1e-9);
    EXPECT_NEAR(report->perimeter, 1200.0, 1e-9);
}

TEST(Parcel, TheLegalDescriptionListsEveryCourseAndTheArea)
{
    const auto report = parcelReport(rectangle());
    ASSERT_TRUE(report.ok());
    const std::string description = legalDescription(*report, "Lot 1");
    EXPECT_NE(description.find("Lot 1"), std::string::npos) << description;
    std::size_t thence = 0;
    for (std::size_t at = description.find("thence"); at != std::string::npos;
         at = description.find("thence", at + 1)) {
        ++thence;
    }
    EXPECT_EQ(thence, 4u) << description;
    EXPECT_NE(description.find("N 90"), std::string::npos) << description;
    EXPECT_NE(description.find("100.000"), std::string::npos) << description;
    EXPECT_NE(description.find("5000.000"), std::string::npos) << description;
    EXPECT_NE(description.find("0.5000 ha"), std::string::npos) << description;
    EXPECT_NE(description.find("point of beginning"), std::string::npos) << description;
}

TEST(Parcel, LabelsReadAlongEachCourseAndNoneIsUpsideDown)
{
    const auto report = parcelReport(rectangle());
    ASSERT_TRUE(report.ok());
    const auto labels = parcelLabels(*report, 2.0);
    ASSERT_TRUE(labels.ok()) << labels.error().describe();
    ASSERT_EQ(labels->size(), 5u) << "four courses and the area";
    for (std::size_t i = 0; i < 4; ++i) {
        const katana::entity::TextGeometry& label = (*labels)[i];
        EXPECT_EQ(label.height, 2.0);
        // Readable: rotated to lie along the course but never past a quarter
        // turn either way, so the west and south courses are flipped.
        EXPECT_GT(label.rotation, -kPi / 2.0 - 1e-12) << "course " << i;
        EXPECT_LE(label.rotation, kPi / 2.0 + 1e-12) << "course " << i;
        EXPECT_NE(label.text.find(report->courses[i].bearing), std::string::npos) << label.text;
    }
    EXPECT_NE((*labels)[0].text.find("100.000"), std::string::npos);
    EXPECT_NEAR((*labels)[0].rotation, 0.0, 1e-12) << "east reads left to right";
    EXPECT_NEAR((*labels)[1].rotation, kPi / 2.0, 1e-12) << "north reads upward";
    EXPECT_NEAR((*labels)[2].rotation, 0.0, 1e-12) << "west is flipped to read left to right";
    EXPECT_NEAR((*labels)[3].rotation, kPi / 2.0, 1e-12) << "south is flipped to read upward";
    // Course labels sit inside the boundary: the east course runs along y = 0,
    // so its label is above it.
    EXPECT_GT((*labels)[0].position.y, 0.0);
    EXPECT_LT((*labels)[0].position.y, 25.0);
    const katana::entity::TextGeometry& area = labels->back();
    EXPECT_NE(area.text.find("5000.000"), std::string::npos) << area.text;
    EXPECT_NEAR(area.position.y, 25.0, 2.0 + 1e-9); // at the centroid, give or take the text
    EXPECT_EQ(parcelLabels(*report, 0.0).error().code, ErrorCode::InvalidArgument);
}

TEST(Parcel, RefusesWhatIsNotAParcelAndDropsRepeatedCorners)
{
    Polyline2 open = rectangle();
    open.closed = false;
    EXPECT_EQ(parcelReport(open).error().code, ErrorCode::InvalidGeometry);
    EXPECT_EQ(parcelReport(closed({Point2(0, 0), Point2(1, 1)})).error().code,
              ErrorCode::InvalidGeometry);
    EXPECT_EQ(parcelReport(closed({Point2(0, 0), Point2(1, 0), Point2(2, 0)})).error().code,
              ErrorCode::InvalidGeometry) << "collinear: no area";
    EXPECT_EQ(parcelReport(closed({Point2(0, 0), Point2(std::numeric_limits<double>::quiet_NaN(), 0),
                                   Point2(1, 1)}))
                  .error()
                  .code,
              ErrorCode::InvalidArgument);
    // A corner clicked twice is one corner, not a zero-length course.
    const auto report = parcelReport(
        closed({Point2(0, 0), Point2(100, 0), Point2(100, 0), Point2(100, 50), Point2(0, 50)}));
    ASSERT_TRUE(report.ok()) << report.error().describe();
    EXPECT_EQ(report->courses.size(), 4u);
    EXPECT_NEAR(report->area, 5000.0, 1e-9);
}

// ---- the report's text: what PARCEL id prints and the dialog shows ------------------------

TEST(Parcel, TheReportIsARowPerCourseThenTheSummary)
{
    const auto report = parcelReport(rectangle());
    ASSERT_TRUE(report.ok());
    // 5000 m2 is 0.5 ha; the perimeter 300; the centroid the middle.
    EXPECT_EQ(formatParcelSummary(*report),
              "area 5000.000 m2 (0.500 ha), perimeter 300.000 m, centroid 50.000,25.000, "
              "drawn counter-clockwise");
    const std::string text = formatParcelReport(*report);
    std::vector<std::string> lines;
    for (std::size_t start = 0;;) {
        const std::size_t end = text.find('\n', start);
        lines.push_back(text.substr(start, end == std::string::npos ? end : end - start));
        if (end == std::string::npos) {
            break;
        }
        start = end + 1;
    }
    ASSERT_EQ(lines.size(), 6u) << text; // a header, four courses, the summary
    EXPECT_EQ(lines.back(), "  " + formatParcelSummary(*report));
    EXPECT_NE(lines[1].find("   1 "), std::string::npos) << lines[1];
    EXPECT_NE(lines[1].find(report->courses[0].bearing), std::string::npos) << lines[1];
    EXPECT_NE(lines[1].find("100.000"), std::string::npos) << lines[1];
    EXPECT_NE(lines[4].find(report->courses[3].bearing), std::string::npos) << lines[4];
}

TEST(Parcel, TheCoursesCsvQuotesTheBearingAndKeepsANegativeCoordinatesSign)
{
    // The rectangle moved to straddle the origin: its first corner is at
    // (-10, -20).
    const auto report = parcelReport(
        closed({Point2(-10, -20), Point2(90, -20), Point2(90, 30), Point2(-10, 30)}));
    ASSERT_TRUE(report.ok());
    const std::string csv = parcelCoursesCsv(*report);
    EXPECT_EQ(csv.substr(0, csv.find("\r\n")),
              "Course,From easting,From northing,To easting,To northing,Azimuth (deg),Bearing,"
              "Distance");
    // Due east is azimuth 90, N 90 E; the bearing holds a double quote, so
    // the field is quoted and the quote doubled (RFC 4180).
    ASSERT_EQ(report->courses[0].bearing, "N 90\u00b000'00\" E");
    const std::string first =
        "1,-10.000,-20.000,90.000,-20.000,90.000000,\"N 90\u00b000'00\"\" E\",100.000\r\n";
    EXPECT_NE(csv.find(first), std::string::npos) << csv;
    std::size_t records = 0;
    for (std::size_t at = csv.find("\r\n"); at != std::string::npos; at = csv.find("\r\n", at + 2)) {
        ++records;
    }
    EXPECT_EQ(records, 5u); // the header and four courses
}
