// Dimension label formatting (PLAN.MD Phase 09).
//
// Every expected string here is worked out by hand from the stated rules, not
// captured from a run. This number ends up on a drawing somebody signs.

#include <gtest/gtest.h>

#include <cfenv>
#include <clocale>
#include <cmath>
#include <limits>
#include <string>

#include "katana/entity/dimension_text.hpp"
#include "katana/entity/tables.hpp"

using katana::core::ErrorCode;
using katana::entity::ArrowHead;
using katana::entity::DimensionStyle;
using katana::entity::DimensionStyleDatabase;
using katana::entity::dimensionLabel;
using katana::entity::formatMeasurement;

namespace {

DimensionStyle metres(int decimals = 3)
{
    DimensionStyle style;
    style.decimals = decimals;
    return style;
}

} // namespace

TEST(DimensionText, FixesTheNumberOfDecimalsAndPadsOnTheLeft)
{
    const DimensionStyle style = metres(3);
    EXPECT_EQ(formatMeasurement(12.3456, style), "12.346");
    EXPECT_EQ(formatMeasurement(12.0, style), "12.000");
    EXPECT_EQ(formatMeasurement(0.05, style), "0.050") << "padded on the LEFT, not '0.5'";
    EXPECT_EQ(formatMeasurement(0.0, style), "0.000");
    EXPECT_EQ(formatMeasurement(0.0005, style), "0.001");

    EXPECT_EQ(formatMeasurement(12.3456, metres(0)), "12");
    EXPECT_EQ(formatMeasurement(12.6, metres(0)), "13");
    EXPECT_EQ(formatMeasurement(1.0 / 3.0, metres(6)), "0.333333");
}

TEST(DimensionText, TiesGoAwayFromZeroNotToEven)
{
    // THE rule that rules out printf, std::to_chars(fixed) and
    // QString::number, all of which round ties to EVEN. A surveyor reading 2.5
    // at whole metres expects 3; round-half-even gives 2, and 0.125 at two
    // decimals would give 0.12 instead of 0.13.
    //
    // std::llround is specified (C17 7.12.9.6-7) to round halfway cases away
    // from zero regardless of the current rounding mode, which is why the
    // implementation goes through it.
    EXPECT_EQ(formatMeasurement(2.5, metres(0)), "3") << "round-half-even would give 2";
    EXPECT_EQ(formatMeasurement(3.5, metres(0)), "4");
    EXPECT_EQ(formatMeasurement(0.5, metres(0)), "1") << "round-half-even would give 0";
    EXPECT_EQ(formatMeasurement(1.5, metres(0)), "2");

    // 0.125 and 0.375 are EXACTLY representable, so these really are ties.
    EXPECT_EQ(formatMeasurement(0.125, metres(2)), "0.13") << "round-half-even would give 0.12";
    EXPECT_EQ(formatMeasurement(0.375, metres(2)), "0.38");
}

TEST(DimensionText, TheTieDirectionSurvivesAChangeOfRoundingMode)
{
    // llround is specified to ignore the current rounding mode. An
    // implementation built on rint or nearbyint would silently follow it, and
    // the drawing would change meaning because some other library set the mode.
    const int original = std::fegetround();
    ASSERT_EQ(std::fesetround(FE_TOWARDZERO), 0);

    EXPECT_EQ(formatMeasurement(2.5, metres(0)), "3");
    EXPECT_EQ(formatMeasurement(0.125, metres(2)), "0.13");

    ASSERT_EQ(std::fesetround(FE_DOWNWARD), 0);
    EXPECT_EQ(formatMeasurement(2.5, metres(0)), "3");

    std::fesetround(original);
}

TEST(DimensionText, ScalesBeforeItRounds)
{
    // DIMLFAC then DIMRND, and the order changes the answer. 1.24 m scaled to
    // millimetres is 1240; rounding THAT to a multiple of 0.05 does nothing, so
    // the label is 1240.00 mm. Rounding first would give 1.25 m = 1250 mm - a
    // centimetre of difference from getting the order wrong.
    DimensionStyle style = metres(2);
    style.unitScale = 1000.0;
    style.roundTo = 0.05;
    style.suffix = " mm";
    EXPECT_EQ(formatMeasurement(1.24, style), "1240.00 mm");

    // And with the scale at 1 the same rounding does bite.
    style.unitScale = 1.0;
    style.suffix = " m";
    EXPECT_EQ(formatMeasurement(1.24, style), "1.25 m");
    EXPECT_EQ(formatMeasurement(1.22, style), "1.20 m");
    EXPECT_EQ(formatMeasurement(1.26, style), "1.25 m");
}

TEST(DimensionText, RoundingToAMultipleTiesAwayFromZeroToo)
{
    DimensionStyle style = metres(3);
    style.roundTo = 0.5;
    // 1.25 is exactly halfway between 1.0 and 1.5 in units of 0.5.
    EXPECT_EQ(formatMeasurement(1.25, style), "1.500") << "away from zero, not to even";
    EXPECT_EQ(formatMeasurement(0.75, style), "1.000");
    EXPECT_EQ(formatMeasurement(1.74, style), "1.500");
    EXPECT_EQ(formatMeasurement(1.76, style), "2.000");
}

TEST(DimensionText, CarriesIntoTheWholePartRatherThanGrowingAThirdDigit)
{
    // 0.9999 at two decimals rounds to 100 hundredths, which is a carry - not
    // "0.100" and not "0.1000".
    EXPECT_EQ(formatMeasurement(0.9999, metres(2)), "1.00");
    EXPECT_EQ(formatMeasurement(9.999, metres(2)), "10.00");
    EXPECT_EQ(formatMeasurement(99.9999, metres(3)), "100.000");
    EXPECT_EQ(formatMeasurement(0.99999, metres(4)), "1.0000");
}

TEST(DimensionText, TrailingZeroSuppressionNeverLeavesABarePointOrNothing)
{
    DimensionStyle style = metres(3);
    style.suppressTrailingZeros = true;

    EXPECT_EQ(formatMeasurement(12.0, style), "12") << "not '12.'";
    EXPECT_EQ(formatMeasurement(0.0, style), "0") << "not '' and not '.'";
    EXPECT_EQ(formatMeasurement(12.5, style), "12.5");
    EXPECT_EQ(formatMeasurement(12.500, style), "12.5");
    EXPECT_EQ(formatMeasurement(0.050, style), "0.05");
    EXPECT_EQ(formatMeasurement(100.0, style), "100") << "the zeros BEFORE the point stay";

    // Off by default, because a dimension usually wants a fixed number of
    // decimals so a column of them lines up.
    EXPECT_EQ(formatMeasurement(12.0, metres(3)), "12.000");
}

TEST(DimensionText, NegativeZeroNeverAppearsOnTheDrawing)
{
    DimensionStyle style = metres(3);
    EXPECT_EQ(formatMeasurement(-0.0, style), "0.000");

    style.suppressTrailingZeros = true;
    EXPECT_EQ(formatMeasurement(-0.0, style), "0");
    EXPECT_EQ(formatMeasurement(-0.0001, style), "0") << "rounds to zero, so no sign";

    // A genuine negative still carries its sign. A measured distance is never
    // negative, but the formatter is not the place to decide that.
    EXPECT_EQ(formatMeasurement(-1.5, style), "-1.5");
}

TEST(DimensionText, PrefixAndSuffixWrapTheNumber)
{
    DimensionStyle style = metres(2);
    style.prefix = "R";
    style.suffix = " m";
    EXPECT_EQ(formatMeasurement(2.5, style), "R2.50 m");

    style.prefix.clear();
    style.unitScale = 1000.0;
    style.decimals = 0;
    style.suffix = " mm";
    EXPECT_EQ(formatMeasurement(2.5, style), "2500 mm");
}

TEST(DimensionText, AnOverrideWinsVerbatim)
{
    // DXF does not apply DIMPOST to overridden text, and someone who typed a
    // value means that value - not that value scaled, rounded and wrapped in a
    // prefix they did not ask for.
    DimensionStyle style = metres(2);
    style.prefix = "R";
    style.suffix = " m";
    style.unitScale = 1000.0;
    style.roundTo = 10.0;

    EXPECT_EQ(dimensionLabel(2.5, "TYP", style), "TYP");
    EXPECT_EQ(dimensionLabel(2.5, "2.5 m EXACT", style), "2.5 m EXACT");
    EXPECT_EQ(dimensionLabel(2.5, "", style), "R2500.00 m") << "no override: the style applies";
}

TEST(DimensionText, ARoundingStepCanLegitimatelyEraseASmallMeasurement)
{
    // This is the chosen precision doing its job, not a fault - but it IS a
    // wrong number on a signed drawing if the precision was chosen carelessly,
    // so the behaviour is pinned rather than left to be discovered.
    DimensionStyle style = metres(3);
    style.roundTo = 0.05;
    EXPECT_EQ(formatMeasurement(0.0004, style), "0.000")
        << "a 0.4 mm gap rounded to 50 mm steps is zero";

    EXPECT_EQ(formatMeasurement(0.0004, metres(0)), "0")
        << "and so is a 0.4 mm gap shown to whole metres";
}

TEST(DimensionText, LargeAndExtremeValuesDoNotProduceNonsense)
{
    const DimensionStyle style = metres(3);
    EXPECT_EQ(formatMeasurement(1234567.891, style), "1234567.891");
    // Past 2^53 a double has no fraction left, so the value is emitted whole
    // rather than with invented decimals.
    const std::string huge = formatMeasurement(1.0e17, style);
    EXPECT_EQ(huge.find('.'), std::string::npos) << huge;
    EXPECT_EQ(huge, "100000000000000000");

    // Non-finite cannot reach here from a validated entity, but printing "inf"
    // or "nan" as though it were a length would be worse than saying so.
    DimensionStyle marked = style;
    marked.suffix = " m";
    EXPECT_EQ(formatMeasurement(std::numeric_limits<double>::infinity(), marked), "? m");
    EXPECT_EQ(formatMeasurement(std::numeric_limits<double>::quiet_NaN(), marked), "? m");
}

TEST(DimensionText, TheOutputIsLocaleIndependent)
{
    // A decimal comma would make a drawing read as a different number. The
    // implementation uses std::to_chars and hand-emitted digits precisely so
    // that no global locale can reach it; this asserts the result.
    const char* const previous = std::setlocale(LC_ALL, nullptr);
    const std::string saved = previous != nullptr ? previous : "C";

    // German uses a comma as the decimal separator. If it is not installed the
    // call returns null and the test still checks the C locale, which is worth
    // doing anyway.
    if (std::setlocale(LC_ALL, "de_DE.UTF-8") != nullptr ||
        std::setlocale(LC_ALL, "German_Germany.1252") != nullptr) {
        EXPECT_EQ(formatMeasurement(1.5, metres(1)), "1.5") << "a comma here would be a defect";
        EXPECT_EQ(formatMeasurement(1234.5, metres(1)), "1234.5")
            << "and no thousands separator either";
    }
    std::setlocale(LC_ALL, saved.c_str());
}

// ---- the style table ----------------------------------------------------------------

TEST(DimensionStyleTable, RejectsEachBadFieldWithItsOwnReason)
{
    DimensionStyle style;
    style.name = "site";
    EXPECT_TRUE(katana::entity::validate(style).ok());

    const auto refuses = [](DimensionStyle bad, const char* expectInMessage) {
        bad.name = "site";
        const auto status = katana::entity::validate(bad);
        ASSERT_FALSE(status.ok()) << expectInMessage;
        EXPECT_EQ(status.error().code, ErrorCode::InvalidArgument);
        EXPECT_NE(status.error().describe().find(expectInMessage), std::string::npos)
            << status.error().describe();
    };

    DimensionStyle bad = style;
    bad.textHeight = 0.0;
    refuses(bad, "text height");

    bad = style;
    bad.arrowSize = -1.0;
    refuses(bad, "arrow size");

    bad = style;
    bad.unitScale = 0.0;
    refuses(bad, "unit scale");

    bad = style;
    bad.textGap = -0.1;
    refuses(bad, "text gap");

    bad = style;
    bad.decimals = 13;
    refuses(bad, "decimals");

    bad = style;
    bad.roundTo = -1.0;
    refuses(bad, "rounding step");

    // No new epsilon: rounding finer than the geometric tolerance is rounding
    // to noise, and a tiny step overflows value/step before it rounds.
    bad = style;
    bad.roundTo = 1.0e-12;
    refuses(bad, "geometric tolerance");

    bad = style;
    bad.suffix = std::string("m\xE9"); // CP1252, not UTF-8
    refuses(bad, "UTF-8");
}

TEST(DimensionStyleTable, AlwaysHoldsStandardAndWillNotGiveItUp)
{
    DimensionStyleDatabase styles;
    EXPECT_EQ(styles.size(), 1u);
    ASSERT_NE(styles.find("Standard"), nullptr);

    EXPECT_FALSE(styles.remove("Standard").ok());
    EXPECT_TRUE(styles.contains("Standard"));

    // Standard can be RE-STYLED, which is how a job sets its own defaults.
    DimensionStyle restyled = *styles.find("Standard");
    restyled.decimals = 2;
    restyled.suffix = " m";
    ASSERT_TRUE(styles.update(restyled).ok());
    EXPECT_EQ(styles.find("Standard")->decimals, 2);

    // And an invalid update is refused without damaging the stored one.
    DimensionStyle broken = restyled;
    broken.textHeight = -1.0;
    EXPECT_FALSE(styles.update(broken).ok());
    EXPECT_EQ(styles.find("Standard")->textHeight, restyled.textHeight);
}

TEST(DimensionStyleTable, AddUpdateAndRemoveBehaveLikeTheOtherNamedTables)
{
    DimensionStyleDatabase styles;

    DimensionStyle site;
    site.name = "site";
    site.decimals = 2;
    ASSERT_TRUE(styles.add(site).ok());
    EXPECT_EQ(styles.add(site).error().code, ErrorCode::AlreadyExists);
    EXPECT_EQ(styles.update(DimensionStyle{"missing"}).error().code, ErrorCode::NotFound);
    EXPECT_EQ(styles.remove("missing").error().code, ErrorCode::NotFound);

    // Ascending by name, like every other table, so iteration is deterministic.
    const auto names = styles.names();
    ASSERT_EQ(names.size(), 2u);
    EXPECT_EQ(names[0], "Standard");
    EXPECT_EQ(names[1], "site");

    const auto removed = styles.remove("site");
    ASSERT_TRUE(removed.ok());
    EXPECT_EQ(removed->decimals, 2);
    EXPECT_EQ(styles.size(), 1u);

    styles.reset();
    EXPECT_EQ(styles.size(), 1u);
    EXPECT_TRUE(styles.contains("Standard"));
}

TEST(DimensionStyleTable, ArrowHeadNamesRoundTrip)
{
    for (const ArrowHead head : {ArrowHead::None, ArrowHead::Tick, ArrowHead::ClosedFilled,
                                 ArrowHead::Open, ArrowHead::Dot}) {
        const auto name = katana::entity::toString(head);
        EXPECT_NE(name, "Unknown");
        const auto parsed = katana::entity::arrowHeadFromString(name);
        ASSERT_TRUE(parsed.ok()) << name;
        EXPECT_EQ(*parsed, head);
    }
    // Case insensitive, because an exchange format spells these in capitals.
    EXPECT_TRUE(katana::entity::arrowHeadFromString("CLOSEDFILLED").ok());
    EXPECT_FALSE(katana::entity::arrowHeadFromString("Diamond").ok());
}
