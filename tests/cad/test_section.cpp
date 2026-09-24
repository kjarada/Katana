// Profiles and cross sections (PLAN.MD Phases 14 and 21).
//
// The surfaces here are built by hand with elevations that are exact in binary,
// so an expected profile elevation can be written down rather than read off the
// program. That is what makes an interpolation error visible instead of
// self-confirming.

#include <gtest/gtest.h>

#include "katana/geometry/profile.hpp"

#include <algorithm>

#include <cmath>

#include "katana/cad/section.hpp"
#include "katana/cad/spatial_query.hpp"
#include "katana/geometry/spatial_index.hpp"

using katana::cad::extractSection;
using katana::cad::SampleReason;
using katana::cad::Section;
using katana::cad::SectionOptions;
using katana::cad::SectionSurfaceInput;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::terrain::TinSurface;

namespace {

Polyline2 lineFrom(std::initializer_list<Point2> points)
{
    Polyline2 line;
    line.vertices = points;
    return line;
}

// A 100 x 100 square sloping 1:10 to the east: z = x / 10 exactly, which is
// exact in binary at every tenth so the expected values below are exact too.
TinSurface ramp()
{
    std::vector<katana::geometry::Point3> vertices = {
        {0.0, 0.0, 0.0}, {100.0, 0.0, 10.0}, {100.0, 100.0, 10.0}, {0.0, 100.0, 0.0}};
    std::vector<katana::terrain::TinTriangle> triangles = {{0, 1, 2}, {0, 2, 3}};
    auto surface = TinSurface::create(std::move(vertices), std::move(triangles));
    EXPECT_TRUE(surface.ok()) << (surface.ok() ? "" : surface.error().describe());
    return surface.ok() ? std::move(*surface) : TinSurface{};
}

// A roof: flat at z = 0 along the west edge, rising to a ridge at x = 50 and
// falling again. The ridge is a slope break that interval sampling alone will
// cut the top off unless the crossing is found.
TinSurface roof()
{
    std::vector<katana::geometry::Point3> vertices = {
        {0.0, 0.0, 0.0},   {50.0, 0.0, 10.0},   {100.0, 0.0, 0.0},
        {0.0, 100.0, 0.0}, {50.0, 100.0, 10.0}, {100.0, 100.0, 0.0}};
    std::vector<katana::terrain::TinTriangle> triangles = {
        {0, 1, 4}, {0, 4, 3}, {1, 2, 5}, {1, 5, 4}};
    auto surface = TinSurface::create(std::move(vertices), std::move(triangles));
    EXPECT_TRUE(surface.ok()) << (surface.ok() ? "" : surface.error().describe());
    return surface.ok() ? std::move(*surface) : TinSurface{};
}

} // namespace

TEST(CadSection, SamplesTheSurfaceAtEveryStation)
{
    const TinSurface surface = ramp();
    const std::vector<SectionSurfaceInput> inputs = {{"Existing", &surface}};

    SectionOptions options;
    options.interval = 10.0;
    options.includeCrossings = false;

    const auto section =
        extractSection(lineFrom({Point2(0.0, 50.0), Point2(100.0, 50.0)}), inputs, nullptr, options);
    ASSERT_TRUE(section.ok()) << section.error().describe();
    ASSERT_EQ(section->surfaces.size(), 1u);
    EXPECT_EQ(section->surfaces[0].name, "Existing");
    EXPECT_NEAR(section->length, 100.0, 1e-12);

    const auto& samples = section->surfaces[0].samples;
    ASSERT_GE(samples.size(), 11u);
    EXPECT_NEAR(samples.front().station, 0.0, 1e-12);
    EXPECT_NEAR(samples.back().station, 100.0, 1e-12);

    // z = x / 10, and the alignment runs along y = 50, so elevation = station/10.
    for (const auto& sample : samples) {
        ASSERT_TRUE(sample.elevation.has_value()) << "station " << sample.station;
        EXPECT_NEAR(*sample.elevation, sample.station / 10.0, 1e-9)
            << "station " << sample.station;
    }
    ASSERT_TRUE(section->surfaces[0].minElevation.has_value());
    EXPECT_NEAR(*section->surfaces[0].minElevation, 0.0, 1e-9);
    EXPECT_NEAR(*section->surfaces[0].maxElevation, 10.0, 1e-9);
}

TEST(CadSection, StationsAreStrictlyAscendingWithNoDuplicates)
{
    const TinSurface surface = roof();
    const std::vector<SectionSurfaceInput> inputs = {{"Design", &surface}};
    SectionOptions options;
    options.interval = 7.0; // does not divide the length, so the end is ragged
    options.includeCrossings = false;

    const auto section = extractSection(
        lineFrom({Point2(0.0, 20.0), Point2(60.0, 20.0), Point2(100.0, 80.0)}), inputs, nullptr,
        options);
    ASSERT_TRUE(section.ok()) << section.error().describe();

    const auto& samples = section->surfaces[0].samples;
    ASSERT_GE(samples.size(), 2u);
    ASSERT_EQ(samples.size(), section->surfaces[0].reasons.size());
    for (std::size_t i = 1; i < samples.size(); ++i) {
        ASSERT_GT(samples[i].station, samples[i - 1].station) << "at index " << i;
    }
    EXPECT_EQ(samples.front().station, 0.0);
    EXPECT_NEAR(samples.back().station, section->length, 1e-9);
}

TEST(CadSection, TheRidgeIsSampledExactlyRatherThanCutOff)
{
    // THE point of finding surface breaks. The ridge is at x = 50, z = 10; an
    // interval of 30 puts samples at 0, 30, 60, 90, 100 and would report a peak
    // of 8 instead of 10 - a 20% error in the highest point on the profile.
    const TinSurface surface = roof();
    const std::vector<SectionSurfaceInput> inputs = {{"Design", &surface}};

    SectionOptions options;
    options.interval = 30.0;
    options.includeCrossings = false;
    options.includeSurfaceBreaks = true;

    const auto section =
        extractSection(lineFrom({Point2(0.0, 50.0), Point2(100.0, 50.0)}), inputs, nullptr, options);
    ASSERT_TRUE(section.ok()) << section.error().describe();

    // Along y = 50 the alignment crosses three triangle edges: the two mesh
    // diagonals at x = 25 and x = 75, and the ridge itself at x = 50. All three
    // are genuine triangle boundaries and all three are expected - only the
    // middle one is a change of SLOPE, but the sampler does not and should not
    // try to tell them apart, because a diagonal is where interpolation hands
    // over from one plane to another whether or not the planes agree.
    double peak = 0.0;
    std::vector<double> breaks;
    for (std::size_t i = 0; i < section->surfaces[0].samples.size(); ++i) {
        const auto& sample = section->surfaces[0].samples[i];
        if (sample.elevation.has_value()) {
            peak = std::max(peak, *sample.elevation);
        }
        if (section->surfaces[0].reasons[i] == SampleReason::SurfaceBreak) {
            breaks.push_back(sample.station);
        }
    }
    ASSERT_EQ(breaks.size(), 3u);
    EXPECT_NEAR(breaks[0], 25.0, 1e-3);
    EXPECT_NEAR(breaks[1], 50.0, 1e-3) << "the ridge must be found exactly";
    EXPECT_NEAR(breaks[2], 75.0, 1e-3);
    // Bisection returns the station just PAST the change, so every break lands
    // in the new triangle rather than ambiguously on the shared edge.
    for (double station : breaks) {
        EXPECT_GE(station, std::round(station / 25.0) * 25.0);
    }
    EXPECT_NEAR(peak, 10.0, 1e-3) << "the ridge must reach its true height";

    // And with break detection off it genuinely does cut the corner, which is
    // what makes the assertion above meaningful rather than vacuous.
    options.includeSurfaceBreaks = false;
    const auto coarse =
        extractSection(lineFrom({Point2(0.0, 50.0), Point2(100.0, 50.0)}), inputs, nullptr, options);
    ASSERT_TRUE(coarse.ok());
    double coarsePeak = 0.0;
    for (const auto& sample : coarse->surfaces[0].samples) {
        if (sample.elevation.has_value()) {
            coarsePeak = std::max(coarsePeak, *sample.elevation);
        }
    }
    EXPECT_LT(coarsePeak, 9.0) << "without break detection the ridge is missed";
}

TEST(CadSection, StationsOffTheSurfaceReportNoElevationRatherThanZero)
{
    const TinSurface surface = ramp(); // covers x in [0, 100]
    const std::vector<SectionSurfaceInput> inputs = {{"Existing", &surface}};
    SectionOptions options;
    options.interval = 10.0;
    options.includeCrossings = false;

    // Runs well past the eastern edge of the surface.
    const auto section = extractSection(lineFrom({Point2(50.0, 50.0), Point2(250.0, 50.0)}),
                                        inputs, nullptr, options);
    ASSERT_TRUE(section.ok()) << section.error().describe();

    bool sawCovered = false;
    bool sawGap = false;
    for (const auto& sample : section->surfaces[0].samples) {
        if (sample.elevation.has_value()) {
            sawCovered = true;
            EXPECT_LE(sample.station, 51.0);
        } else {
            sawGap = true;
        }
    }
    EXPECT_TRUE(sawCovered);
    EXPECT_TRUE(sawGap) << "past the rim the profile must be a gap, not a zero";
}

TEST(CadSection, TwoSurfacesAreCutOnTheSameStations)
{
    // Existing against design is the whole reason a profile has more than one
    // surface: the stations must line up or the comparison is meaningless.
    const TinSurface existing = ramp();
    const TinSurface design = roof();
    const std::vector<SectionSurfaceInput> inputs = {{"Existing", &existing}, {"Design", &design}};

    SectionOptions options;
    options.interval = 20.0;
    options.includeCrossings = false;
    options.includeSurfaceBreaks = false; // breaks are per surface by design

    const auto section =
        extractSection(lineFrom({Point2(0.0, 50.0), Point2(100.0, 50.0)}), inputs, nullptr, options);
    ASSERT_TRUE(section.ok()) << section.error().describe();
    ASSERT_EQ(section->surfaces.size(), 2u);
    ASSERT_EQ(section->surfaces[0].samples.size(), section->surfaces[1].samples.size());
    for (std::size_t i = 0; i < section->surfaces[0].samples.size(); ++i) {
        EXPECT_EQ(section->surfaces[0].samples[i].station,
                  section->surfaces[1].samples[i].station);
    }
}

TEST(CadSection, RejectsAnAlignmentThatIsNotOne)
{
    const TinSurface surface = ramp();
    const std::vector<SectionSurfaceInput> inputs = {{"Existing", &surface}};

    EXPECT_FALSE(extractSection(Polyline2{}, inputs).ok());
    EXPECT_FALSE(extractSection(lineFrom({Point2(1.0, 1.0)}), inputs).ok());
    // Two coincident vertices collapse to one distinct vertex.
    EXPECT_FALSE(extractSection(lineFrom({Point2(1.0, 1.0), Point2(1.0, 1.0)}), inputs).ok());
}

TEST(CadSection, RejectsAnIntervalThatWouldExhaustMemory)
{
    const TinSurface surface = ramp();
    const std::vector<SectionSurfaceInput> inputs = {{"Existing", &surface}};

    SectionOptions options;
    options.interval = 1e-9; // 100 m at a nanometre: 1e11 samples
    const auto section =
        extractSection(lineFrom({Point2(0.0, 50.0), Point2(100.0, 50.0)}), inputs, nullptr, options);
    ASSERT_FALSE(section.ok()) << "a typo must fail cleanly, not swap the machine out";
    EXPECT_EQ(section.error().code, katana::core::ErrorCode::InvalidArgument);

    options.interval = 0.0;
    EXPECT_FALSE(
        extractSection(lineFrom({Point2(0.0, 50.0), Point2(100.0, 50.0)}), inputs, nullptr, options)
            .ok());
    options.interval = std::nan("");
    EXPECT_FALSE(
        extractSection(lineFrom({Point2(0.0, 50.0), Point2(100.0, 50.0)}), inputs, nullptr, options)
            .ok());
}

TEST(CadSection, RejectsANullSurfaceInsteadOfDereferencingIt)
{
    const std::vector<SectionSurfaceInput> inputs = {{"Broken", nullptr}};
    const auto section = extractSection(lineFrom({Point2(0.0, 0.0), Point2(10.0, 0.0)}), inputs);
    ASSERT_FALSE(section.ok());
    EXPECT_EQ(section.error().code, katana::core::ErrorCode::InvalidArgument);
}

TEST(CadSection, EntitiesCrossingTheAlignmentAreReportedAtTheirStation)
{
    const TinSurface surface = ramp();
    katana::entity::Model model;
    // The layers the entities name must exist: a section shows what the plan
    // shows, and an entity on a layer that is not in the table is not drawn
    // (LayerDatabase::resolve). This fixture put them on layers it never
    // created, which the model's own commands would have refused, and passed
    // only while sections ignored layers altogether (audit REN-04).
    ASSERT_TRUE(model.layers.ensure("FENCE").ok());
    ASSERT_TRUE(model.layers.ensure("DRAINAGE").ok());

    // A line crossing the alignment at x = 30, and a circle centred on the
    // alignment crossing it twice.
    katana::entity::Entity fence;
    fence.geometry = katana::geometry::Segment2{Point2(30.0, 20.0), Point2(30.0, 80.0)};
    fence.layer = "FENCE";
    ASSERT_TRUE(model.entities.add(fence).ok());

    katana::entity::Entity manhole;
    manhole.geometry = katana::geometry::Circle2{Point2(70.0, 50.0), 5.0};
    manhole.layer = "DRAINAGE";
    ASSERT_TRUE(model.entities.add(manhole).ok());

    SectionOptions options;
    options.interval = 25.0;
    options.includeCrossings = true;

    const auto section = extractSection(lineFrom({Point2(0.0, 50.0), Point2(100.0, 50.0)}),
                                        {{"Existing", &surface}}, &model, options);
    ASSERT_TRUE(section.ok()) << section.error().describe();
    ASSERT_EQ(section->crossings.size(), 3u) << "one fence crossing and two circle crossings";

    // Sorted by station: 30 (fence), 65 and 75 (the circle's two sides).
    EXPECT_NEAR(section->crossings[0].station, 30.0, 1e-9);
    EXPECT_EQ(section->crossings[0].layer, "FENCE");
    EXPECT_NEAR(section->crossings[1].station, 65.0, 1e-9);
    EXPECT_NEAR(section->crossings[2].station, 75.0, 1e-9);
    // The elevation comes from the surface at the crossing: z = x / 10.
    ASSERT_TRUE(section->crossings[0].elevation.has_value());
    EXPECT_NEAR(*section->crossings[0].elevation, 3.0, 1e-9);
}

TEST(CadSection, HiddenEntitiesDoNotAppearOnTheSection)
{
    const TinSurface surface = ramp();
    katana::entity::Model model;
    katana::entity::Entity hidden;
    hidden.geometry = katana::geometry::Segment2{Point2(30.0, 20.0), Point2(30.0, 80.0)};
    hidden.visible = false;
    ASSERT_TRUE(model.entities.add(hidden).ok());

    const auto section = extractSection(lineFrom({Point2(0.0, 50.0), Point2(100.0, 50.0)}),
                                        {{"Existing", &surface}}, &model);
    ASSERT_TRUE(section.ok()) << section.error().describe();
    EXPECT_TRUE(section->crossings.empty());
}

// ---- cross section lines --------------------------------------------------------

TEST(CadCrossSection, TheLineIsPerpendicularAndCentredOnTheStation)
{
    const Polyline2 alignment = lineFrom({Point2(0.0, 0.0), Point2(100.0, 0.0)});

    const auto line = katana::cad::crossSectionLine(alignment, 40.0, 15.0);
    ASSERT_TRUE(line.ok()) << line.error().describe();
    ASSERT_EQ(line->vertices.size(), 2u);

    // Alignment runs east; the section runs north-south through x = 40.
    EXPECT_NEAR(line->vertices[0].x, 40.0, 1e-9);
    EXPECT_NEAR(line->vertices[1].x, 40.0, 1e-9);
    EXPECT_NEAR(line->vertices[0].y, 15.0, 1e-9) << "left of travel comes first";
    EXPECT_NEAR(line->vertices[1].y, -15.0, 1e-9);
    EXPECT_NEAR(line->length(), 30.0, 1e-9);
}

TEST(CadCrossSection, AStationOnAVertexUsesTheBisectorRatherThanOneSide)
{
    // A right-angle bend. At the corner the "segment direction" is ambiguous;
    // rounding to either side would swing the section by 45 degrees.
    const Polyline2 alignment =
        lineFrom({Point2(0.0, 0.0), Point2(100.0, 0.0), Point2(100.0, 100.0)});

    const auto line = katana::cad::crossSectionLine(alignment, 100.0, 10.0);
    ASSERT_TRUE(line.ok()) << line.error().describe();
    const auto direction = line->vertices[1] - line->vertices[0];
    // The bisector of east-then-north points north-east, so its perpendicular
    // points south-east: equal and opposite components.
    EXPECT_NEAR(std::abs(direction.x), std::abs(direction.y), 1e-6);
}

TEST(CadCrossSection, RejectsAStationOutsideTheAlignment)
{
    const Polyline2 alignment = lineFrom({Point2(0.0, 0.0), Point2(100.0, 0.0)});
    EXPECT_FALSE(katana::cad::crossSectionLine(alignment, -5.0, 10.0).ok());
    EXPECT_FALSE(katana::cad::crossSectionLine(alignment, 150.0, 10.0).ok());
    EXPECT_FALSE(katana::cad::crossSectionLine(alignment, 50.0, 0.0).ok());
    EXPECT_FALSE(katana::cad::crossSectionLine(alignment, 50.0, -1.0).ok());
    // The ends themselves are inside.
    EXPECT_TRUE(katana::cad::crossSectionLine(alignment, 0.0, 10.0).ok());
    EXPECT_TRUE(katana::cad::crossSectionLine(alignment, 100.0, 10.0).ok());
}

TEST(CadCrossSection, StationsAlwaysIncludeBothEndsAndNeverDrift)
{
    const Polyline2 alignment = lineFrom({Point2(0.0, 0.0), Point2(1000.0, 0.0)});
    const auto stations = katana::cad::sectionStations(alignment, 0.1);
    ASSERT_TRUE(stations.ok()) << stations.error().describe();

    EXPECT_EQ(stations->front(), 0.0);
    EXPECT_NEAR(stations->back(), 1000.0, 1e-9);
    // Accumulating 0.1 ten thousand times drifts by about 1e-12; multiplying
    // does not. The last interior station must still be where it belongs.
    EXPECT_NEAR((*stations)[5000], 500.0, 1e-12);
    for (std::size_t i = 1; i < stations->size(); ++i) {
        ASSERT_GT((*stations)[i], (*stations)[i - 1]);
    }
}

TEST(CadCrossSection, StationsRejectNonsenseIntervals)
{
    const Polyline2 alignment = lineFrom({Point2(0.0, 0.0), Point2(100.0, 0.0)});
    EXPECT_FALSE(katana::cad::sectionStations(alignment, 0.0).ok());
    EXPECT_FALSE(katana::cad::sectionStations(alignment, -1.0).ok());
    EXPECT_FALSE(katana::cad::sectionStations(alignment, 1e-9).ok());
    EXPECT_FALSE(katana::cad::sectionStations(Polyline2{}, 1.0).ok());
}

TEST(CadSection, ACrossSectionOfARampIsFlatAcrossTheFallLine)
{
    // End to end: cut the ramp perpendicular to its fall and every elevation on
    // the section must be the same, because the ramp only falls east.
    const TinSurface surface = ramp();
    const Polyline2 alignment = lineFrom({Point2(10.0, 50.0), Point2(90.0, 50.0)});

    const auto line = katana::cad::crossSectionLine(alignment, 40.0, 20.0);
    ASSERT_TRUE(line.ok()) << line.error().describe();

    SectionOptions options;
    options.interval = 5.0;
    options.includeCrossings = false;
    const auto section = extractSection(*line, {{"Existing", &surface}}, nullptr, options);
    ASSERT_TRUE(section.ok()) << section.error().describe();

    // Station 40 along an alignment starting at x = 10 is x = 50, so z = 5.
    for (const auto& sample : section->surfaces[0].samples) {
        ASSERT_TRUE(sample.elevation.has_value());
        EXPECT_NEAR(*sample.elevation, 5.0, 1e-6) << "station " << sample.station;
    }
}

// ---- the design profile as a series of the section --------------------------------

namespace {

// A straight 300 m section along +x with one ground series sampled every
// 50 m at a constant 10.0, built by hand so the test depends on nothing but
// the Section struct.
katana::cad::Section straightSection()
{
    katana::cad::Section section;
    section.alignment.vertices = {katana::geometry::Point2(0, 0), katana::geometry::Point2(300, 0)};
    section.length = 300.0;
    katana::cad::SectionSurface ground;
    ground.name = "ground";
    for (double s = 0.0; s <= 300.0; s += 50.0) {
        katana::cad::SectionSample sample;
        sample.station = s;
        sample.plan = katana::geometry::Point2(s, 0);
        sample.elevation = 10.0;
        ground.samples.push_back(sample);
        ground.reasons.push_back(katana::cad::SampleReason::Interval);
    }
    ground.minElevation = 10.0;
    ground.maxElevation = 10.0;
    section.surfaces.push_back(ground);
    return section;
}

// The textbook sag from the profile tests, shifted to start at 0: PVC 100,
// PVT 300... no - kept inside the 300 m section: PVIs (0, 16), (100, 13,
// L = 100), (300, 17): -3% into +2%, PVC 50 @ 14.5, PVT 150 @ 14.0, low point
// at x = 0.03 * 100 / 0.05 = 60 -> station 110 @ 14.5 - 1.8 + 0.9 = 13.6.
katana::geometry::SolvedProfile sagProfile()
{
    katana::geometry::VerticalAlignment definition;
    definition.pvis = {katana::geometry::ProfilePVI{0.0, 16.0},
                       katana::geometry::ProfilePVI{100.0, 13.0, 100.0},
                       katana::geometry::ProfilePVI{300.0, 17.0}};
    auto solved = katana::geometry::solveProfile(definition);
    EXPECT_TRUE(solved.ok()) << solved.error().describe();
    return *solved;
}

} // namespace

TEST(CadSection, ADesignProfileIsSampledWhereTheGroundIsAndAtItsOwnKeyStations)
{
    katana::cad::Section section = straightSection();
    const auto profile = sagProfile();
    ASSERT_TRUE(katana::cad::appendDesignProfile(section, profile, "design").ok());
    ASSERT_EQ(section.surfaces.size(), 2u);
    const katana::cad::SectionSurface& design = section.surfaces[1];
    EXPECT_EQ(design.name, "design");

    // Every ground station is present, so the two can be read against each
    // other; the PVC (50), PVT (150) and low point (110) are present too and
    // marked as the design's own.
    std::vector<double> stations;
    std::size_t vertices = 0;
    for (std::size_t i = 0; i < design.samples.size(); ++i) {
        stations.push_back(design.samples[i].station);
        if (design.reasons[i] == katana::cad::SampleReason::ProfileVertex) {
            ++vertices;
        }
    }
    for (double ground : {0.0, 50.0, 100.0, 150.0, 200.0, 250.0, 300.0}) {
        EXPECT_NE(std::find(stations.begin(), stations.end(), ground), stations.end()) << ground;
    }
    EXPECT_NE(std::find(stations.begin(), stations.end(), 110.0), stations.end()) << "low point";
    EXPECT_GE(vertices, 4u) << "start, PVC, low point, PVT, end at least";
    EXPECT_TRUE(std::is_sorted(stations.begin(), stations.end()));
    EXPECT_EQ(std::adjacent_find(stations.begin(), stations.end()), stations.end())
        << "a station appeared twice";

    // Elevations are the profile's, exactly, and the plan positions are on
    // the alignment: station s of a straight along +x is (s, 0).
    for (const katana::cad::SectionSample& sample : design.samples) {
        ASSERT_TRUE(sample.elevation.has_value()) << sample.station;
        EXPECT_NEAR(*sample.elevation, *profile.elevationAt(sample.station), 1e-12);
        EXPECT_NEAR(sample.plan.x, sample.station, 1e-9);
        EXPECT_NEAR(sample.plan.y, 0.0, 1e-9);
    }
    EXPECT_NEAR(*design.minElevation, 13.6, 1e-12) << "the low point";
    EXPECT_NEAR(*design.maxElevation, 17.0, 1e-12);

    // The section's extent now spans ground and design together.
    const auto extent = section.extent();
    EXPECT_NEAR(extent.min.y, 10.0, 1e-12);
    EXPECT_NEAR(extent.max.y, 17.0, 1e-12);
}

TEST(CadSection, ADesignShorterThanTheSectionLeavesAGapRatherThanInventingGrades)
{
    katana::cad::Section section = straightSection();
    katana::geometry::VerticalAlignment definition;
    definition.pvis = {katana::geometry::ProfilePVI{0.0, 12.0},
                       katana::geometry::ProfilePVI{200.0, 14.0}};
    const auto profile = katana::geometry::solveProfile(definition);
    ASSERT_TRUE(profile.ok());
    ASSERT_TRUE(katana::cad::appendDesignProfile(section, *profile, "design").ok());
    const katana::cad::SectionSurface& design = section.surfaces.back();
    std::size_t gaps = 0;
    for (const katana::cad::SectionSample& sample : design.samples) {
        if (sample.station > 200.0) {
            EXPECT_FALSE(sample.elevation.has_value()) << sample.station;
            ++gaps;
        } else {
            EXPECT_TRUE(sample.elevation.has_value()) << sample.station;
        }
    }
    EXPECT_GE(gaps, 2u) << "250 and 300 lie beyond the design";
}

TEST(CadSection, ADesignProfileRejectsAnEmptyNameAndAnEmptySection)
{
    katana::cad::Section section = straightSection();
    const auto profile = sagProfile();
    EXPECT_EQ(katana::cad::appendDesignProfile(section, profile, "").error().code,
              katana::core::ErrorCode::InvalidArgument);
    katana::cad::Section empty;
    EXPECT_EQ(katana::cad::appendDesignProfile(empty, profile, "design").error().code,
              katana::core::ErrorCode::InvalidArgument);
    EXPECT_EQ(section.surfaces.size(), 1u) << "a refused append changes nothing";
}


// ---- the crossing search: box rejection and the document's index -----------------------

namespace {

// Every entity's broad-phase box, as Document::rebuildSpatialIndex stores it.
katana::geometry::SpatialIndex indexOf(const katana::entity::Model& model)
{
    std::vector<katana::geometry::SpatialEntry> entries;
    model.entities.forEach([&](const katana::entity::Entity& entity) {
        entries.push_back({static_cast<katana::geometry::SpatialId>(entity.id),
                           katana::cad::detail::queryExtents(model, entity)});
    });
    katana::geometry::SpatialIndex index;
    index.rebuild(entries);
    return index;
}

katana::entity::EntityId addOn(katana::entity::Model& model, katana::entity::Geometry geometry)
{
    katana::entity::Entity entity;
    entity.geometry = std::move(geometry);
    entity.layer = "0";
    const auto added = model.entities.add(entity);
    EXPECT_TRUE(added.ok());
    return added.ok() ? added.value() : katana::entity::kInvalidEntityId;
}

} // namespace

TEST(CadSection, CrossingsAreTheSameWithTheDocumentsIndexAsWithoutIt)
{
    // An L-shaped alignment: (0, 50) east to (100, 50), then north to
    // (100, 100). Station s on the first leg is at x = s; on the second leg
    // station 100 + d is at y = 50 + d.
    //
    // Expected crossings, worked by hand:
    //   fence   x = 30 from y 20 to 80        -> station 30
    //   post    x = 40 from y 50 (touching) to 90 -> station 40
    //   string  polyline (60,0) (60,40) (60,60) -> its second edge, station 60
    //   manhole circle centre (70,50) r 5      -> stations 65 and 75
    //   kerb    y = 80 from x 90 to 110        -> second leg at y 80: 100 + 30 = 130
    // plus 2000 short strings scattered over y in [200, 900], none of which
    // reaches the alignment (it never goes above y = 100).
    katana::entity::Model model;
    ASSERT_TRUE(model.layers.ensure("0").ok());
    const auto fence = addOn(model, katana::geometry::Segment2{Point2(30, 20), Point2(30, 80)});
    const auto post = addOn(model, katana::geometry::Segment2{Point2(40, 50), Point2(40, 90)});
    const auto string = addOn(model, lineFrom({Point2(60, 0), Point2(60, 40), Point2(60, 60)}));
    const auto manhole = addOn(model, katana::geometry::Circle2{Point2(70, 50), 5.0});
    const auto kerb = addOn(model, katana::geometry::Segment2{Point2(90, 80), Point2(110, 80)});
    for (int i = 0; i < 2000; ++i) {
        // A fixed, spread-out pattern rather than random numbers: the answer
        // must not depend on a generator.
        const double x = static_cast<double>((i * 37) % 1000);
        const double y = 200.0 + static_cast<double>((i * 53) % 700);
        addOn(model, lineFrom({Point2(x, y), Point2(x + 3.0, y + 2.0), Point2(x + 5.0, y)}));
    }
    const katana::geometry::SpatialIndex index = indexOf(model);

    const Polyline2 alignment = lineFrom({Point2(0, 50), Point2(100, 50), Point2(100, 100)});
    SectionOptions options;
    options.interval = 10.0;
    const auto scanned = extractSection(alignment, {}, &model, options);
    options.spatialIndex = &index;
    const auto indexed = extractSection(alignment, {}, &model, options);
    ASSERT_TRUE(scanned.ok()) << scanned.error().describe();
    ASSERT_TRUE(indexed.ok()) << indexed.error().describe();

    const std::vector<std::pair<double, katana::entity::EntityId>> expected = {
        {30.0, fence}, {40.0, post}, {60.0, string}, {65.0, manhole}, {75.0, manhole},
        {130.0, kerb}};
    for (const auto* section : {&scanned.value(), &indexed.value()}) {
        ASSERT_EQ(section->crossings.size(), expected.size());
        for (std::size_t i = 0; i < expected.size(); ++i) {
            EXPECT_NEAR(section->crossings[i].station, expected[i].first, 1e-9) << i;
            EXPECT_EQ(section->crossings[i].entity, expected[i].second) << i;
        }
    }
    // Not merely close: the same crossings, bit for bit, in the same order.
    for (std::size_t i = 0; i < expected.size(); ++i) {
        EXPECT_EQ(scanned->crossings[i].station, indexed->crossings[i].station) << i;
        EXPECT_EQ(scanned->crossings[i].plan, indexed->crossings[i].plan) << i;
    }
}

TEST(CadSection, ACrossingWithinToleranceOfTheAlignmentSurvivesTheBoxTest)
{
    // A fence that stops 5e-8 short of the alignment y = 50. intersect()
    // accepts a crossing within kGeometric (1e-7) of both segments, so this
    // one is reported at x = 20 - but the fence's exact box ends below the
    // alignment's, and a box test without a margin would have thrown it away
    // before intersect() was ever asked.
    katana::entity::Model model;
    ASSERT_TRUE(model.layers.ensure("0").ok());
    const auto fence =
        addOn(model, katana::geometry::Segment2{Point2(20, 10), Point2(20, 50.0 - 5e-8)});
    const katana::geometry::SpatialIndex index = indexOf(model);

    for (const katana::geometry::SpatialIndex* withIndex :
         {static_cast<const katana::geometry::SpatialIndex*>(nullptr), &index}) {
        SectionOptions options;
        options.interval = 10.0;
        options.spatialIndex = withIndex;
        const auto section =
            extractSection(lineFrom({Point2(0, 50), Point2(100, 50)}), {}, &model, options);
        ASSERT_TRUE(section.ok()) << section.error().describe();
        ASSERT_EQ(section->crossings.size(), 1u);
        EXPECT_EQ(section->crossings[0].entity, fence);
        EXPECT_NEAR(section->crossings[0].station, 20.0, 1e-9);
    }
}

TEST(CadSection, APolylineEdgeWithinToleranceOfTheAlignmentSurvivesTheSideTest)
{
    // The polyline form of the test above, which is the path with the second
    // reject: both ends of the edge (20, 10) - (20, 50 - 5e-8) lie below the
    // alignment y = 50, one of them by only 5e-8, inside kGeometric. The side
    // test must keep it, and intersect() then reports the crossing at x = 20.
    // A diagonal alignment is added so the side test runs on a line that is
    // not axis-aligned too: (0, 0) - (100, 100) against an edge ending 5e-8
    // (perpendicular) short of it at (70, 70).
    katana::entity::Model model;
    ASSERT_TRUE(model.layers.ensure("0").ok());
    const auto fence = addOn(model, lineFrom({Point2(20, 10), Point2(20, 50.0 - 5e-8)}));
    const double offset = 5e-8 / std::sqrt(2.0);
    const auto spur =
        addOn(model, lineFrom({Point2(90, 50), Point2(70.0 + offset, 70.0 - offset)}));

    SectionOptions options;
    options.interval = 10.0;
    const auto across =
        extractSection(lineFrom({Point2(0, 50), Point2(100, 50)}), {}, &model, options);
    ASSERT_TRUE(across.ok()) << across.error().describe();
    // The spur, from (90, 50) on the line itself, crosses it there too:
    // stations 20 (the fence) and 90 (the spur's start).
    ASSERT_EQ(across->crossings.size(), 2u);
    EXPECT_EQ(across->crossings[0].entity, fence);
    EXPECT_NEAR(across->crossings[0].station, 20.0, 1e-9);
    EXPECT_EQ(across->crossings[1].entity, spur);
    EXPECT_NEAR(across->crossings[1].station, 90.0, 1e-9);

    // Along the diagonal y = x: the fence (x = 20, y 10 to 50) crosses it
    // at (20, 20), station 20 * sqrt(2); the spur's end at (70, 70), 5e-8
    // off it, is at station 70 * sqrt(2).
    const auto diagonal =
        extractSection(lineFrom({Point2(0, 0), Point2(100, 100)}), {}, &model, options);
    ASSERT_TRUE(diagonal.ok()) << diagonal.error().describe();
    ASSERT_EQ(diagonal->crossings.size(), 2u);
    EXPECT_EQ(diagonal->crossings[0].entity, fence);
    EXPECT_NEAR(diagonal->crossings[0].station, 20.0 * std::sqrt(2.0), 1e-9);
    EXPECT_EQ(diagonal->crossings[1].entity, spur);
    EXPECT_NEAR(diagonal->crossings[1].station, 70.0 * std::sqrt(2.0), 1e-6);
}
