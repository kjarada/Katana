// Rings that are holes (entity/ring_tags.hpp): how an entity says it is one,
// and which exterior a hole joins. Both the feature-table conversion and the
// volume verbs read holes through these, so the cases here are the rule.

#include <gtest/gtest.h>

#include <cstddef>
#include <cstdint>
#include <optional>
#include <string>
#include <utility>
#include <vector>

#include "katana/entity/entity.hpp"
#include "katana/entity/ring_tags.hpp"

using katana::entity::Entity;
using katana::entity::ExteriorCandidate;
using katana::entity::HoleJoiner;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

// The square [x, x + side] x [y, y + side] as a closed ring that does not
// repeat its first vertex.
Polyline2 square(double x, double y, double side)
{
    Polyline2 ring;
    ring.closed = true;
    ring.vertices = {Point2(x, y), Point2(x + side, y), Point2(x + side, y + side),
                     Point2(x, y + side)};
    return ring;
}

ExteriorCandidate exterior(katana::entity::EntityId id, double x, double y, double side,
                           std::optional<std::string> part = std::nullopt)
{
    return ExteriorCandidate{id, std::move(part), square(x, y, side)};
}

} // namespace

TEST(RingTags, AnEntityIsAHoleOnlyWhenARingTagSaysHoleInAnyCaseInItsPropertiesOrMetadata)
{
    Entity untagged;
    EXPECT_FALSE(katana::entity::isTaggedHole(untagged));

    Entity imported; // IMPORT writes metadata
    imported.metadata["source.ring"] = std::string("hole");
    EXPECT_TRUE(katana::entity::isTaggedHole(imported));
    imported.metadata["source.ring"] = std::string("HOLE");
    EXPECT_TRUE(katana::entity::isTaggedHole(imported));

    Entity written; // a result writes properties
    written.properties["gis.ring"] = std::string("Hole");
    EXPECT_TRUE(katana::entity::isTaggedHole(written));

    // An exterior says so, and a word that merely contains the word is not it.
    Entity outer;
    outer.metadata["source.ring"] = std::string("exterior");
    EXPECT_FALSE(katana::entity::isTaggedHole(outer));
    outer.metadata["source.ring"] = std::string("not a hole");
    EXPECT_FALSE(katana::entity::isTaggedHole(outer));
    // One tag saying hole is enough, though the other says exterior.
    outer.properties["gis.ring"] = std::string("hole");
    EXPECT_TRUE(katana::entity::isTaggedHole(outer));
}

TEST(RingTags, ThePartIsTheResultsTagBeforeTheImportsAndNoneWhenNeitherIsThere)
{
    Entity none;
    EXPECT_FALSE(katana::entity::ringPart(none).has_value());

    Entity imported;
    imported.metadata["source.part"] = std::string("feature 3");
    EXPECT_EQ(katana::entity::ringPart(imported), std::optional<std::string>("feature 3"));

    // A result writes the exterior's id as an integer; it reads as its text.
    Entity written;
    written.properties["gis.part"] = std::int64_t{7};
    written.metadata["source.part"] = std::string("feature 3");
    EXPECT_EQ(katana::entity::ringPart(written), std::optional<std::string>("7"));
}

TEST(HoleJoiner, AHoleJoinsTheRingThatHoldsAllOfItAndNoneWhenOneVertexIsOutside)
{
    const HoleJoiner joiner({exterior(1, 0, 0, 100), exterior(2, 200, 0, 100)});
    // [40, 60]^2 lies in the first.
    const std::vector<Point2> inside{Point2(40, 40), Point2(60, 40), Point2(60, 60), Point2(40, 60)};
    EXPECT_EQ(joiner.exteriorOf(inside, 9, std::nullopt), std::optional<std::size_t>(0));
    // In the second, by its position, not its order.
    const std::vector<Point2> second{Point2(240, 40), Point2(260, 40), Point2(260, 60),
                                     Point2(240, 60)};
    EXPECT_EQ(joiner.exteriorOf(second, 9, std::nullopt), std::optional<std::size_t>(1));
    // One vertex out of the first (x = 101): held by no ring.
    const std::vector<Point2> straddling{Point2(40, 40), Point2(101, 40), Point2(60, 60)};
    EXPECT_FALSE(joiner.exteriorOf(straddling, 9, std::nullopt).has_value());
    // Between the two rings.
    const std::vector<Point2> between{Point2(120, 40), Point2(180, 40), Point2(150, 60)};
    EXPECT_FALSE(joiner.exteriorOf(between, 9, std::nullopt).has_value());
    // A vertex on an edge is held, as Polyline2::contains says: a hole that
    // shares an edge with its exterior is still its hole.
    const std::vector<Point2> onTheEdge{Point2(0, 40), Point2(20, 40), Point2(20, 60), Point2(0, 60)};
    EXPECT_EQ(joiner.exteriorOf(onTheEdge, 9, std::nullopt), std::optional<std::size_t>(0));
    // No exterior to join.
    EXPECT_FALSE(HoleJoiner(std::vector<ExteriorCandidate>{}).exteriorOf(inside, 9, std::nullopt).has_value());
}

TEST(HoleJoiner, OfSeveralRingsThatHoldAHoleItJoinsTheSmallestUnlessItSharesAPartWithAnother)
{
    // A lot of 100 x 100 (10 000 m2) with a smaller one of 60 x 60 (3600 m2)
    // inside it, and a hole at [40, 50]^2 inside both.
    const std::vector<Point2> hole{Point2(40, 40), Point2(50, 40), Point2(50, 50), Point2(40, 50)};
    {
        const HoleJoiner joiner({exterior(1, 0, 0, 100), exterior(2, 20, 20, 60)});
        EXPECT_EQ(joiner.exteriorOf(hole, 9, std::nullopt), std::optional<std::size_t>(1))
            << "the smaller of the two";
    }
    {
        // The hole's own feature is the big one's: that is the one it belongs to,
        // though the small is smaller.
        const HoleJoiner joiner(
            {exterior(1, 0, 0, 100, std::string("feature 1")),
             exterior(2, 20, 20, 60, std::string("feature 2"))});
        EXPECT_EQ(joiner.exteriorOf(hole, 9, std::string("feature 1")),
                  std::optional<std::size_t>(0));
        EXPECT_EQ(joiner.exteriorOf(hole, 9, std::string("feature 2")),
                  std::optional<std::size_t>(1));
        // A part that matches no ring changes nothing: the smallest.
        EXPECT_EQ(joiner.exteriorOf(hole, 9, std::string("feature 7")),
                  std::optional<std::size_t>(1));
    }
    {
        // A part shared with a ring that does not hold the hole does not count.
        const HoleJoiner joiner({exterior(1, 0, 0, 100, std::string("feature 1")),
                                 exterior(2, 300, 0, 60, std::string("feature 2"))});
        EXPECT_EQ(joiner.exteriorOf(hole, 9, std::string("feature 2")),
                  std::optional<std::size_t>(0));
    }
}

TEST(HoleJoiner, OfEqualRingsTheHoleJoinsTheOneMadeNearestBeforeIt)
{
    // The same file imported twice: two exteriors equal in everything but their
    // ids (1 and 5), each hole made just after its own (6, and 2).
    const std::vector<Point2> hole{Point2(40, 40), Point2(50, 40), Point2(50, 50), Point2(40, 50)};
    const HoleJoiner joiner({exterior(1, 0, 0, 100), exterior(5, 0, 0, 100)});
    // Hole 6: exterior 5 is one before it, exterior 1 five.
    EXPECT_EQ(joiner.exteriorOf(hole, 6, std::nullopt), std::optional<std::size_t>(1));
    // Hole 2: exterior 1 is one before it, and 5 is after it, which is the
    // farthest there is.
    EXPECT_EQ(joiner.exteriorOf(hole, 2, std::nullopt), std::optional<std::size_t>(0));
    // The order the rings are given in does not matter.
    const HoleJoiner reversed({exterior(5, 0, 0, 100), exterior(1, 0, 0, 100)});
    EXPECT_EQ(reversed.exteriorOf(hole, 6, std::nullopt), std::optional<std::size_t>(0));
    EXPECT_EQ(reversed.exteriorOf(hole, 2, std::nullopt), std::optional<std::size_t>(1));
}
