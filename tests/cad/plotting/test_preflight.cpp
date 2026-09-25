// Preflight checks (include/katana/cad/plotting/preflight.hpp): one test per
// check, each with the smallest set that trips it and the nearest set that
// does not, and the order the findings come in.
//
// Every test starts from a CLEAN set - one A3 sheet with a filled title
// block, a logo, and a plan at 1:500 over a line - that the checker passes
// without a word, and changes one thing. The A3 numbers are the frame's:
// drawing area 23..410 x 35..287 mm, title block 21.825..411.309 x
// 8.945..34.625 mm, tiling area 24..409 x 36..286 mm (docs/plotting.md).

#include <gtest/gtest.h>

#include <algorithm>
#include <cmath>
#include <numbers>
#include <set>
#include <string>
#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/preflight.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/geometry/alignment.hpp"

using katana::cad::PaperSize;
using namespace katana::cad::plotting;
using katana::entity::Entity;
using katana::entity::Model;
using katana::geometry::AlignmentPI;
using katana::geometry::Segment2;

namespace {

const Box2 kTiling(Point2(24.0, 36.0), Point2(409.0, 286.0));

void add(Model& model, katana::entity::Geometry geometry, std::string layer = "0")
{
    Entity entity;
    entity.geometry = std::move(geometry);
    entity.layer = std::move(layer);
    ASSERT_TRUE(model.entities.add(std::move(entity)).ok());
}

// A line 100 m east from the origin, and a straight 100 m alignment MC01
// along y = 50, CH 0 to 100.
Model drawing()
{
    Model model;
    add(model, Segment2{Point2(0.0, 0.0), Point2(100.0, 0.0)});
    katana::entity::Alignment road;
    road.name = "MC01";
    road.horizontal.pis = {AlignmentPI{Point2(0.0, 50.0)}, AlignmentPI{Point2(100.0, 50.0)}};
    EXPECT_TRUE(model.alignments.add(road).ok());
    return model;
}

Viewport planAt(std::string id, Box2 rect, double scale, Point2 centre)
{
    Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = ViewportKind::Plan;
    viewport.rect = rect;
    viewport.scale = scale;
    viewport.centre = centre;
    return viewport;
}

Viewport panel(std::string id, ViewportKind kind, Box2 rect, std::string text = "TEXT")
{
    Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = kind;
    viewport.rect = rect;
    viewport.text = std::move(text);
    return viewport;
}

Sheet sheetOf(std::string id, std::string name, std::vector<Viewport> viewports)
{
    Sheet sheet;
    sheet.id = std::move(id);
    sheet.name = std::move(name);
    sheet.viewports = std::move(viewports);
    return sheet;
}

// Every value the frame prints, filled.
SheetDefaults filledTitleBlock()
{
    SheetDefaults d;
    d.organisation = "ACME SURVEYS";
    d.projectLines = {"MAIN ROAD", "DETAIL SURVEY"};
    d.setNumber = "DS-1";
    d.heightDatum = "AHD";
    d.modelName = "DESIGN";
    d.locator = {"A. LOCATOR", ""};
    d.surveyor = {"B. SURVEYOR", ""};
    d.compiler = {"C. COMPILER", ""};
    d.reviewer = {"D. REVIEWER", ""};
    d.approver = {"E. APPROVER", ""};
    d.logoAsset = "logo.png";
    return d;
}

FieldContext context()
{
    FieldContext c;
    c.projectName = "Main Road";
    c.projectDescription = "Detail survey";
    c.coordinateSystem = "EPSG:7856";
    c.fileName = "main_road";
    c.plotDate = "25/09/26";
    return c;
}

// One sheet, its plan filling the tiling area at 1:500 over the line: the
// window is 192.5 x 125 m about (50, 0).
SheetSet cleanSet()
{
    SheetSet set;
    set.defaults = filledTitleBlock();
    set.revisions.push_back(Revision{"A", "25/09/26", "FIRST ISSUE", "BS"});
    set.sheets.push_back(sheetOf("s1", "PLAN", {planAt("vp1", kTiling, 500.0, Point2(50.0, 0.0))}));
    return set;
}

PreflightOptions knownLogo()
{
    PreflightOptions options;
    options.logoReadable = true;
    return options;
}

std::vector<Finding> check(const SheetSet& set, const Model& model,
                           PreflightOptions options = knownLogo())
{
    return checkSheets(set, model, context(), options);
}

std::vector<Finding> withCode(const std::vector<Finding>& findings, std::string_view code)
{
    std::vector<Finding> out;
    std::copy_if(findings.begin(), findings.end(), std::back_inserter(out),
                 [code](const Finding& f) { return f.code == code; });
    return out;
}

std::string codesOf(const std::vector<Finding>& findings)
{
    std::string out;
    for (const Finding& f : findings) {
        out += (out.empty() ? "" : " ") + f.code;
    }
    return out;
}

} // namespace

TEST(SheetPreflight, ACleanSetPassesWithoutAWord)
{
    const Model model = drawing();
    const auto findings = check(cleanSet(), model);
    EXPECT_TRUE(findings.empty()) << codesOf(findings);
}

// ---- viewport.outside ---------------------------------------------------------------------

TEST(SheetPreflight, AViewUnderTheTitleBlockIsAnError)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    // Down to 20 mm: 14.625 mm into the title block, whose top is 34.625.
    set.sheets[0].viewports[0].rect = Box2(Point2(100.0, 20.0), Point2(300.0, 200.0));
    const auto found = withCode(check(set, model), "viewport.outside");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Error);
    EXPECT_EQ(found[0].sheetIndex, 0u);
    EXPECT_EQ(found[0].sheetId, "s1");
    EXPECT_EQ(found[0].viewportId, "vp1");
    EXPECT_NE(found[0].message.find("14.6 mm under the title block"), std::string::npos) << found[0].message;
    EXPECT_FALSE(found[0].fix.empty());
}

TEST(SheetPreflight, AViewOffThePaperIsAnErrorAndOneInTheMarginAWarning)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    // 40 mm past A3's right edge at 420.
    set.sheets[0].viewports[0].rect = Box2(Point2(300.0, 100.0), Point2(460.0, 200.0));
    auto found = withCode(check(set, model), "viewport.outside");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Error);
    EXPECT_NE(found[0].message.find("40.0 mm off the paper"), std::string::npos) << found[0].message;

    set.sheets[0].viewports[0].rect = Box2(Point2(500.0, 500.0), Point2(600.0, 600.0));
    found = withCode(check(set, model), "viewport.outside");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_NE(found[0].message.find("is off the paper"), std::string::npos) << found[0].message;

    // From 15 mm: 8 mm left of the drawing area at 23, on the paper and
    // clear of the title block.
    set.sheets[0].viewports[0].rect = Box2(Point2(15.0, 100.0), Point2(200.0, 200.0));
    found = withCode(check(set, model), "viewport.outside");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Warning);
    EXPECT_NE(found[0].message.find("8.0 mm outside the drawing area"), std::string::npos)
        << found[0].message;
}

TEST(SheetPreflight, AViewFillingTheDrawingAreaExactlyIsInside)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets[0].viewports[0].rect = drawingArea(set.sheets[0]);
    EXPECT_TRUE(withCode(check(set, model), "viewport.outside").empty());
    // A frameless sheet's drawing area is the paper less 10 mm.
    set.sheets[0].frame.clear();
    set.sheets[0].viewports[0].rect = Box2(Point2(10.0, 10.0), Point2(410.0, 287.0));
    EXPECT_TRUE(withCode(check(set, model), "viewport.outside").empty());
    set.sheets[0].viewports[0].rect = Box2(Point2(5.0, 10.0), Point2(410.0, 287.0));
    EXPECT_EQ(withCode(check(set, model), "viewport.outside").size(), 1u);
}

// ---- viewport.overlap ---------------------------------------------------------------------

TEST(SheetPreflight, OverlappingViewsAreReportedOnTheOneInFront)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets[0].viewports = {planAt("vp1", Box2(Point2(30.0, 40.0), Point2(220.0, 280.0)), 500.0, Point2(50.0, 0.0)),
                               planAt("vp2", Box2(Point2(200.0, 40.0), Point2(400.0, 280.0)), 500.0, Point2(50.0, 0.0))};
    const auto found = withCode(check(set, model), "viewport.overlap");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Warning);
    EXPECT_EQ(found[0].viewportId, "vp2");
    EXPECT_EQ(found[0].subject, "vp1");
    EXPECT_NE(found[0].message.find("20.0 x 240.0 mm"), std::string::npos) << found[0].message;
}

TEST(SheetPreflight, ViewsSnappedEdgeToEdgeDoNotOverlap)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets[0].viewports = {planAt("vp1", Box2(Point2(30.0, 40.0), Point2(215.0, 280.0)), 500.0, Point2(50.0, 0.0)),
                               planAt("vp2", Box2(Point2(215.0, 40.0), Point2(400.0, 280.0)), 500.0, Point2(50.0, 0.0))};
    EXPECT_TRUE(withCode(check(set, model), "viewport.overlap").empty());
}

TEST(SheetPreflight, ALegendSetInsideAPlanIsAnInsetNotAMistake)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets[0].viewports.push_back(
        panel("vp2", ViewportKind::Legend, Box2(Point2(340.0, 40.0), Point2(405.0, 100.0))));
    const auto found = withCode(check(set, model), "viewport.overlap");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Info);
    EXPECT_EQ(found[0].viewportId, "vp2");
}

// ---- viewport.unplaced ----------------------------------------------------------------------

TEST(SheetPreflight, AnUnplacedViewIsReportedAndCheckedNoFurther)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    Viewport loose = planAt("vp2", Box2{}, 437.0, Point2(1e6, 1e6));
    set.sheets[0].viewports.push_back(loose);
    const auto findings = check(set, model);
    ASSERT_EQ(findings.size(), 1u) << codesOf(findings);
    EXPECT_EQ(findings[0].code, "viewport.unplaced");
    EXPECT_EQ(findings[0].severity, Severity::Warning);
    EXPECT_EQ(findings[0].viewportId, "vp2");
}

// ---- plan.empty -----------------------------------------------------------------------------

TEST(SheetPreflight, APlanOverNothingIsEmpty)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets[0].viewports[0].centre = Point2(5000.0, 5000.0);
    const auto found = withCode(check(set, model), "plan.empty");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Warning);
    EXPECT_NE(found[0].message.find("192.5 x 125.0 m window at 1:500"), std::string::npos) << found[0].message;
}

TEST(SheetPreflight, ThePlanWindowTurnsWithTheViewport)
{
    // A point at (30, 30) and a window 100 x 10 m (100 x 10 mm at 1:1000)
    // about the origin: level, it reaches y = 5 and misses the point; turned
    // 45 degrees it runs along y = x, and the point is 42.4 m along it.
    Model model;
    add(model, katana::entity::PointGeometry{Point2(30.0, 30.0)});
    SheetSet set = cleanSet();
    set.sheets[0].viewports = {planAt("vp1", Box2(Point2(100.0, 100.0), Point2(200.0, 110.0)), 1000.0, Point2(0.0, 0.0))};
    EXPECT_EQ(withCode(check(set, model), "plan.empty").size(), 1u);
    set.sheets[0].viewports[0].rotation = std::numbers::pi / 4.0;
    EXPECT_TRUE(withCode(check(set, model), "plan.empty").empty());
}

TEST(SheetPreflight, ALineIsTestedByItselfNotByItsBox)
{
    // The line x + y = 100 from (0, 100) to (100, 0): its box holds the whole
    // 10 x 10 m window about (10, 10), but the line passes 49.5 m from the
    // window's nearest corner.
    Model model;
    add(model, Segment2{Point2(0.0, 100.0), Point2(100.0, 0.0)});
    SheetSet set = cleanSet();
    set.sheets[0].viewports = {planAt("vp1", Box2(Point2(100.0, 100.0), Point2(120.0, 120.0)), 500.0, Point2(10.0, 10.0))};
    EXPECT_EQ(withCode(check(set, model), "plan.empty").size(), 1u);
    // A line straight through it, both ends far outside, is seen.
    add(model, Segment2{Point2(-1000.0, 10.0), Point2(1000.0, 10.0)});
    EXPECT_TRUE(withCode(check(set, model), "plan.empty").empty());
}

TEST(SheetPreflight, APlanShowingOnlyHiddenLayersIsEmpty)
{
    Model model;
    katana::entity::Layer survey;
    survey.name = "survey";
    ASSERT_TRUE(model.layers.add(survey).ok());
    add(model, Segment2{Point2(0.0, 0.0), Point2(100.0, 0.0)}, "survey");
    SheetSet set = cleanSet();
    EXPECT_TRUE(withCode(check(set, model), "plan.empty").empty());
    (void)set.sheets[0].viewports[0].hiddenLayers.hide("survey");
    EXPECT_EQ(withCode(check(set, model), "plan.empty").size(), 1u);
}

TEST(SheetPreflight, AlignmentsAndImageryCountAsShown)
{
    Model model;
    SheetSet set = cleanSet();
    EXPECT_EQ(withCode(check(set, model), "plan.empty").size(), 1u);
    // Imagery under the window, from the caller.
    PreflightOptions options = knownLogo();
    options.otherContent = {Box2(Point2(0.0, 0.0), Point2(10.0, 10.0))};
    EXPECT_TRUE(withCode(check(set, model, options), "plan.empty").empty());
    // An alignment through it.
    katana::entity::Alignment road;
    road.name = "MC01";
    road.horizontal.pis = {AlignmentPI{Point2(-500.0, 20.0)}, AlignmentPI{Point2(500.0, 20.0)}};
    ASSERT_TRUE(model.alignments.add(road).ok());
    EXPECT_TRUE(withCode(check(set, model), "plan.empty").empty());
}

// ---- text.too-small -------------------------------------------------------------------------

TEST(SheetPreflight, TextSmallerThanTheMinimumAtThePlansScaleIsCounted)
{
    Model model = drawing();
    // At 1:500 a 0.5 m text prints 1.0 mm and a 0.8 m one 1.6 mm: both under
    // 1.8 mm. A 1.0 m text prints 2.0 mm. One far outside the window does
    // not count, nor does blank text.
    katana::entity::TextGeometry small{Point2(10.0, 10.0), "A", 0.5, 0.0};
    katana::entity::TextGeometry medium{Point2(20.0, 10.0), "B", 0.8, 0.0};
    katana::entity::TextGeometry large{Point2(30.0, 10.0), "C", 1.0, 0.0};
    katana::entity::TextGeometry away{Point2(9000.0, 10.0), "D", 0.1, 0.0};
    katana::entity::TextGeometry blank{Point2(40.0, 10.0), "  ", 0.1, 0.0};
    for (const auto& text : {small, medium, large, away, blank}) {
        add(model, text);
    }
    const auto found = withCode(check(cleanSet(), model), "text.too-small");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Warning);
    EXPECT_EQ(found[0].viewportId, "vp1");
    EXPECT_NE(found[0].message.find("2 texts printing smaller than 1.8 mm at 1:500, the smallest 1.00 mm"),
              std::string::npos)
        << found[0].message;
    // 0.5 m reads at 1.8 mm at 1:277.8, so 1:250 is the largest standard
    // scale that holds it; at 1:500 text must be 0.9 m high.
    EXPECT_NE(found[0].fix.find("1:250"), std::string::npos) << found[0].fix;
    EXPECT_NE(found[0].fix.find("0.9 m"), std::string::npos) << found[0].fix;
}

TEST(SheetPreflight, TextThatReadsIsNotCounted)
{
    Model model = drawing();
    add(model, katana::entity::TextGeometry{Point2(10.0, 10.0), "A", 0.9, 0.0}); // 1.8 mm exactly
    EXPECT_TRUE(withCode(check(cleanSet(), model), "text.too-small").empty());
    // A smaller minimum lets smaller text through.
    add(model, katana::entity::TextGeometry{Point2(10.0, 20.0), "B", 0.6, 0.0}); // 1.2 mm
    PreflightOptions options = knownLogo();
    EXPECT_EQ(withCode(check(cleanSet(), model, options), "text.too-small").size(), 1u);
    options.minimumTextMm = 1.0;
    EXPECT_TRUE(withCode(check(cleanSet(), model, options), "text.too-small").empty());
}

TEST(SheetPreflight, DimensionTextIsCountedAtItsStylesHeight)
{
    // The default dimension style's text is 2.5 m high: 1.25 mm at 1:2000.
    Model model = drawing();
    add(model, katana::entity::DimensionGeometry{Point2(0.0, 0.0), Point2(50.0, 0.0), 5.0, {}});
    SheetSet set = cleanSet();
    EXPECT_TRUE(withCode(check(set, model), "text.too-small").empty());
    set.sheets[0].viewports[0].scale = 2000.0;
    EXPECT_EQ(withCode(check(set, model), "text.too-small").size(), 1u);
}

// ---- sections -------------------------------------------------------------------------------

Viewport sectionOf(std::string id, ViewportKind kind, std::string alignment)
{
    Viewport viewport;
    viewport.id = std::move(id);
    viewport.kind = kind;
    viewport.rect = kTiling;
    viewport.scale = 500.0;
    viewport.autoCentre = true;
    viewport.source.alignment = std::move(alignment);
    viewport.source.stations = {50.0};
    return viewport;
}

TEST(SheetPreflight, ASectionOfAMissingAlignmentIsAnError)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets[0].viewports = {sectionOf("vp1", ViewportKind::LongSection, "NOPE")};
    auto found = withCode(check(set, model), "section.alignment-missing");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Error);
    EXPECT_EQ(found[0].subject, "NOPE");

    set.sheets[0].viewports = {sectionOf("vp1", ViewportKind::CrossSections, "")};
    found = withCode(check(set, model), "section.alignment-missing");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_NE(found[0].message.find("has no alignment"), std::string::npos);

    set.sheets[0].viewports = {sectionOf("vp1", ViewportKind::LongSection, "MC01")};
    EXPECT_TRUE(check(set, model).empty()) << codesOf(check(set, model));
}

TEST(SheetPreflight, ACrossSectionOffTheAlignmentIsAnError)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    Viewport sections = sectionOf("vp1", ViewportKind::CrossSections, "MC01");
    sections.source.stations = {0.0, 50.0, 100.0, 150.0, -5.0};
    set.sheets[0].viewports = {sections};
    const auto found = withCode(check(set, model), "section.station-outside");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Error);
    EXPECT_EQ(found[0].subject, "MC01");
    EXPECT_NE(found[0].message.find("2 of its 5 sections"), std::string::npos) << found[0].message;
    EXPECT_NE(found[0].message.find("CH 150.000, CH -5.000"), std::string::npos) << found[0].message;
    EXPECT_NE(found[0].message.find("MC01 runs CH 0.000 to CH 100.000"), std::string::npos)
        << found[0].message;

    // Both ends are on it.
    set.sheets[0].viewports[0].source.stations = {0.0, 100.0};
    EXPECT_TRUE(withCode(check(set, model), "section.station-outside").empty());
}

TEST(SheetPreflight, ALongSectionRunningPastTheEndIsAWarningAndOneWhollyOffAnError)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    Viewport profile = sectionOf("vp1", ViewportKind::LongSection, "MC01");
    profile.source.chainageFrom = 50.0;
    profile.source.chainageTo = 150.0;
    set.sheets[0].viewports = {profile};
    auto found = withCode(check(set, model), "section.station-outside");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Warning);

    set.sheets[0].viewports[0].source.chainageFrom = 200.0;
    set.sheets[0].viewports[0].source.chainageTo = 300.0;
    found = withCode(check(set, model), "section.station-outside");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Error);

    set.sheets[0].viewports[0].source.chainageFrom = 0.0;
    set.sheets[0].viewports[0].source.chainageTo = 100.0;
    EXPECT_TRUE(withCode(check(set, model), "section.station-outside").empty());
}

TEST(SheetPreflight, CrossSectionsWithNoChainageAreAnError)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    Viewport sections = sectionOf("vp1", ViewportKind::CrossSections, "MC01");
    sections.source.stations.clear();
    set.sheets[0].viewports = {sections};
    EXPECT_EQ(withCode(check(set, model), "section.no-stations").size(), 1u);
    // An interval over a range lists them, as the painter does.
    set.sheets[0].viewports[0].source.sectionInterval = 20.0;
    set.sheets[0].viewports[0].source.chainageTo = 100.0;
    EXPECT_TRUE(withCode(check(set, model), "section.no-stations").empty());
}

TEST(SheetPreflight, ASectionWithNoSurfaceToCutIsAnErrorWhenTheSurfacesAreKnown)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets[0].viewports = {sectionOf("vp1", ViewportKind::CrossSections, "MC01")};
    // Not known: not checked.
    EXPECT_TRUE(withCode(check(set, model), "section.no-surface").empty());
    PreflightOptions options = knownLogo();
    options.sectionSurfaces = 0;
    EXPECT_EQ(withCode(check(set, model, options), "section.no-surface").size(), 1u);
    options.sectionSurfaces = 1;
    EXPECT_TRUE(withCode(check(set, model, options), "section.no-surface").empty());
}

// ---- scales ---------------------------------------------------------------------------------

TEST(SheetPreflight, AScaleOffTheLadderIsANote)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets[0].viewports[0].scale = 437.0;
    const auto found = withCode(check(set, model), "scale.non-standard");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Info);
    EXPECT_NE(found[0].message.find("1:437"), std::string::npos) << found[0].message;
    EXPECT_NE(found[0].fix.find("1:500"), std::string::npos) << found[0].fix;
    EXPECT_NE(found[0].fix.find("1:250"), std::string::npos) << found[0].fix;
    // An automatic scale is the painter's to choose.
    set.sheets[0].viewports[0].autoScale = true;
    EXPECT_TRUE(withCode(check(set, model), "scale.non-standard").empty());
    // A standard one says nothing.
    set.sheets[0].viewports[0].autoScale = false;
    set.sheets[0].viewports[0].scale = 1250.0;
    EXPECT_TRUE(withCode(check(set, model), "scale.non-standard").empty());
}

TEST(SheetPreflight, AScaleThatIsNotAPositiveNumberIsAnError)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets[0].viewports[0].scale = 0.0;
    EXPECT_EQ(withCode(check(set, model), "scale.invalid").size(), 1u);
    // Not also an empty plan: nothing can be said about a window with no size.
    EXPECT_TRUE(withCode(check(set, model), "plan.empty").empty());
}

// ---- the title block ------------------------------------------------------------------------

TEST(SheetPreflight, ABlankTitleBlockValueIsListedOnceForTheSet)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets.push_back(sheetOf("s2", "PLAN 2", {planAt("vp2", kTiling, 500.0, Point2(50.0, 0.0))}));
    set.sheets.push_back(sheetOf("s3", "PLAN 3", {planAt("vp3", kTiling, 500.0, Point2(50.0, 0.0))}));
    set.defaults.organisation.clear();
    set.defaults.heightDatum = "   ";
    const auto found = withCode(check(set, model), "field.empty");
    ASSERT_EQ(found.size(), 2u);
    // In the Title Block's order: the organisation, then the datum.
    EXPECT_EQ(found[0].subject, "organisation");
    EXPECT_EQ(found[1].subject, "height_datum");
    for (const Finding& f : found) {
        EXPECT_EQ(f.severity, Severity::Warning);
        EXPECT_FALSE(f.sheetIndex.has_value());
        EXPECT_TRUE(f.sheetId.empty());
        EXPECT_NE(f.message.find("every sheet"), std::string::npos) << f.message;
    }
}

TEST(SheetPreflight, ASheetsOwnValueFillsItsBlankAndTheCountSaysSo)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets.push_back(sheetOf("s2", "PLAN 2", {planAt("vp2", kTiling, 500.0, Point2(50.0, 0.0))}));
    set.defaults.surveyor.name.clear();
    set.sheets[0].fields["surveyor_name"] = "F. FIELD";
    auto found = withCode(check(set, model), "field.empty");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].subject, "surveyor_name");
    EXPECT_NE(found[0].message.find("Surveyed by is blank in the title block of 1 of 2 sheets"),
              std::string::npos)
        << found[0].message;
    // A value a sheet sets to nothing is blank on purpose.
    set.sheets[1].fields["surveyor_name"] = "";
    EXPECT_TRUE(withCode(check(set, model), "field.empty").empty());
}

TEST(SheetPreflight, TheProjectFillsWhatTheSetLeavesAndOptionalLinesMayStayBlank)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    // Project lines 1 and 2 and the coordinate system fall back to the
    // project; lines 3 and 4, the client and the notes may be blank.
    set.defaults.projectLines.clear();
    set.defaults.coordinateSystem.clear();
    set.defaults.client.clear();
    set.defaults.notes.clear();
    EXPECT_TRUE(withCode(check(set, model), "field.empty").empty());
    FieldContext bare = context();
    bare.projectName.clear();
    bare.coordinateSystem.clear();
    bare.fileName.clear();
    const auto found = withCode(checkSheets(set, model, bare, knownLogo()), "field.empty");
    std::vector<std::string> subjects;
    for (const Finding& f : found) {
        subjects.push_back(f.subject);
    }
    EXPECT_EQ(subjects, (std::vector<std::string>{"project_line_1", "coordinate_system", "file_name"}));
}

TEST(SheetPreflight, AFramelessSheetHasNoTitleBlockToLeaveBlank)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.defaults = {};
    set.sheets[0].frame.clear();
    const auto findings = check(set, model);
    EXPECT_TRUE(withCode(findings, "field.empty").empty());
    EXPECT_TRUE(withCode(findings, "logo.missing").empty());
}

TEST(SheetPreflight, NoLogoIsANoteAndAnUnreadableOneAWarning)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.defaults.logoAsset.clear();
    auto found = withCode(check(set, model), "logo.missing");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Info);
    EXPECT_FALSE(found[0].sheetIndex.has_value());

    set.defaults.logoAsset = "logo.png";
    PreflightOptions options;
    options.logoReadable = false;
    found = withCode(check(set, model, options), "logo.unreadable");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Warning);
    EXPECT_EQ(found[0].subject, "logo.png");
    // Not known: not reported.
    options.logoReadable.reset();
    EXPECT_TRUE(check(set, model, options).empty());
}

// ---- sheets ---------------------------------------------------------------------------------

TEST(SheetPreflight, TwoSheetsOfOneNameAreReportedOnTheLater)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets.push_back(sheetOf("s2", " plan ", {planAt("vp2", kTiling, 500.0, Point2(50.0, 0.0))}));
    auto found = withCode(check(set, model), "sheet.duplicate-name");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Warning);
    EXPECT_EQ(found[0].sheetIndex, 1u);
    EXPECT_EQ(found[0].sheetId, "s2");
    EXPECT_EQ(found[0].subject, "s1");
    EXPECT_TRUE(found[0].viewportId.empty());

    set.sheets[1].name = "PLAN 2";
    EXPECT_TRUE(withCode(check(set, model), "sheet.duplicate-name").empty());
}

TEST(SheetPreflight, AMarkLeadingToARemovedSheetDangles)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets.push_back(sheetOf("s2", "PLAN 2", {planAt("vp2", kTiling, 500.0, Point2(50.0, 0.0))}));
    WorldMark toTwo{WorldMark::Kind::MatchLine, {Point2(60.0, -10.0), Point2(60.0, 10.0)}, "MATCH LINE", "s2"};
    set.sheets[0].viewports[0].marks = {toTwo};
    EXPECT_TRUE(withCode(check(set, model), "matchline.dangling").empty());

    WorldMark toGone = toTwo;
    toGone.sheet = "s9";
    WorldMark outline{WorldMark::Kind::SheetOutline, {Point2(0.0, 0.0), Point2(10.0, 0.0)}, {}, "s9"};
    set.sheets[0].viewports[0].marks = {toTwo, toGone, toGone, outline};
    const auto found = withCode(check(set, model), "matchline.dangling");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Warning);
    EXPECT_EQ(found[0].viewportId, "vp1");
    EXPECT_EQ(found[0].subject, "s9");
    EXPECT_NE(found[0].message.find("2 match lines and 1 key-plan outline"), std::string::npos)
        << found[0].message;
}

TEST(SheetPreflight, APortraitSheetAskingForAFrameIsWarned)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets[0].landscape = false;
    set.sheets[0].viewports[0].rect = Box2(Point2(20.0, 20.0), Point2(280.0, 400.0));
    const auto found = withCode(check(set, model), "portrait.no-frame");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Warning);
    // Portrait with no frame asked for is what was meant.
    set.sheets[0].frame.clear();
    EXPECT_TRUE(withCode(check(set, model), "portrait.no-frame").empty());
}

TEST(SheetPreflight, ASheetWithNoViewsIsEmpty)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets.push_back(sheetOf("s2", "BLANK", {}));
    const auto found = withCode(check(set, model), "sheet.empty");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].sheetId, "s2");
    EXPECT_NE(found[0].message.find("empty frame"), std::string::npos);
}

TEST(SheetPreflight, AnUnknownFrameIsAnError)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets[0].frame = "a1_plan";
    const auto found = withCode(check(set, model), "frame.unknown");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Error);
    EXPECT_EQ(found[0].subject, "a1_plan");
}

TEST(SheetPreflight, RepeatedIdsAreErrors)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets.push_back(sheetOf("s1", "PLAN 2", {planAt("vp1", kTiling, 500.0, Point2(50.0, 0.0))}));
    const auto findings = check(set, model);
    const auto sheets = withCode(findings, "sheet.duplicate-id");
    ASSERT_EQ(sheets.size(), 1u);
    EXPECT_EQ(sheets[0].sheetIndex, 1u);
    const auto views = withCode(findings, "viewport.duplicate-id");
    ASSERT_EQ(views.size(), 1u);
    EXPECT_EQ(views[0].sheetIndex, 1u);
    EXPECT_EQ(views[0].subject, "vp1");
}

// ---- panels ---------------------------------------------------------------------------------

TEST(SheetPreflight, AnImageNamingNoFileOrAMissingOneIsAnError)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets[0].viewports = {panel("vp1", ViewportKind::Image, kTiling, "")};
    EXPECT_EQ(withCode(check(set, model), "image.missing").size(), 1u);
    set.sheets[0].viewports[0].text = "site.png";
    // Where the assets are is not known: the name is taken on trust.
    EXPECT_TRUE(withCode(check(set, model), "image.missing").empty());
    PreflightOptions options = knownLogo();
    options.assets = std::filesystem::path(testing::TempDir()) / "no-such-assets-folder";
    EXPECT_EQ(withCode(check(set, model, options), "image.missing").size(), 1u);
}

TEST(SheetPreflight, NotesWithNoTextAreANote)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets[0].viewports = {panel("vp1", ViewportKind::Notes, kTiling, " \n")};
    const auto found = withCode(check(set, model), "notes.empty");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Info);
    set.sheets[0].viewports[0].text = "ALL LEVELS IN METRES";
    EXPECT_TRUE(check(set, model).empty());
}

TEST(SheetPreflight, AViewSmallerThanItsKindReadsAtIsANote)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    // A plan reads at 35 x 30 mm at least (layout.hpp).
    set.sheets[0].viewports[0].rect = Box2(Point2(100.0, 100.0), Point2(130.0, 200.0));
    const auto found = withCode(check(set, model), "viewport.too-small");
    ASSERT_EQ(found.size(), 1u);
    EXPECT_EQ(found[0].severity, Severity::Info);
    set.sheets[0].viewports[0].rect = Box2(Point2(100.0, 100.0), Point2(135.0, 130.0));
    EXPECT_TRUE(withCode(check(set, model), "viewport.too-small").empty());
}

TEST(SheetPreflight, APlanFollowingAMissingAlignmentIsWarned)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.sheets[0].viewports[0].source.alignment = "GONE";
    EXPECT_EQ(withCode(check(set, model), "plan.alignment-missing").size(), 1u);
    set.sheets[0].viewports[0].source.alignment = "MC01";
    EXPECT_TRUE(withCode(check(set, model), "plan.alignment-missing").empty());
}

// ---- automatic plans --------------------------------------------------------------------------

TEST(SheetPreflight, AnAutomaticPlanIsCheckedWhereThePainterWouldDrawIt)
{
    // The line and MC01 span 0..100 x 0..50: on auto, centred on (50, 25),
    // and 100 m with 4% to spare in 385 mm needs 1:270, so 1:500.
    const Model model = drawing();
    Viewport plan = planAt("vp1", kTiling, 437.0, Point2(1e6, 1e6));
    plan.autoScale = true;
    plan.autoCentre = true;
    const PlanWindow at = planWindow(plan, model);
    EXPECT_EQ(at.scale, 500.0);
    EXPECT_NEAR(at.centre.x, 50.0, 1e-9);
    EXPECT_NEAR(at.centre.y, 25.0, 1e-9);
    SheetSet set = cleanSet();
    set.sheets[0].viewports = {plan};
    EXPECT_TRUE(check(set, model).empty()) << codesOf(check(set, model));
    // The caller's rule wins where it has one (the painter's, in the editor).
    PreflightOptions options = knownLogo();
    options.resolvePlan = [](const Viewport&) { return PlanWindow{500.0, Point2(-9000.0, 0.0)}; };
    EXPECT_EQ(withCode(check(set, model, options), "plan.empty").size(), 1u);
}

TEST(SheetPreflight, TheWindowsCornersTurnAboutItsCentre)
{
    // 100 x 50 mm at 1:1000 is 100 x 50 m; turned 90 degrees, the paper's
    // left-to-right runs north.
    Viewport plan = planAt("vp1", Box2(Point2(0.0, 0.0), Point2(100.0, 50.0)), 1000.0, Point2(10.0, 20.0));
    plan.rotation = std::numbers::pi / 2.0;
    const auto corners = planWindowCorners(plan, PlanWindow{1000.0, Point2(10.0, 20.0)});
    EXPECT_NEAR(corners[0].x, 35.0, 1e-9); // bottom-left on the paper
    EXPECT_NEAR(corners[0].y, -30.0, 1e-9);
    EXPECT_NEAR(corners[2].x, -15.0, 1e-9); // top-right
    EXPECT_NEAR(corners[2].y, 70.0, 1e-9);
}

// ---- order, options, catalogue, JSON ----------------------------------------------------------

TEST(SheetPreflight, FindingsComeInOneOrderEveryTime)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.defaults.organisation.clear();
    set.defaults.logoAsset.clear();
    set.sheets[0].viewports.push_back(planAt("vp2", Box2(Point2(100.0, 20.0), Point2(200.0, 100.0)), 437.0,
                                             Point2(50.0, 0.0)));
    set.sheets.push_back(sheetOf("s2", "PLAN", {}));
    const auto first = check(set, model);
    // The set's, then sheet 1's viewports back to front, each viewport's in
    // the catalogue's order, then sheet 2's own.
    EXPECT_EQ(codesOf(first), "field.empty logo.missing viewport.outside viewport.overlap scale.non-standard "
                              "sheet.duplicate-name sheet.empty");
    for (int run = 0; run < 3; ++run) {
        EXPECT_EQ(check(set, model), first);
    }
}

TEST(SheetPreflight, OnlyTheSheetsAskedForAreChecked)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.defaults.organisation.clear();
    set.sheets.push_back(sheetOf("s2", "EMPTY", {}));
    set.sheets.push_back(sheetOf("s3", "ALSO EMPTY", {}));
    PreflightOptions options = knownLogo();
    options.sheets = {2, 2, 7};
    const auto findings = check(set, model, options);
    ASSERT_EQ(findings.size(), 2u) << codesOf(findings);
    EXPECT_EQ(findings[0].code, "field.empty");
    EXPECT_NE(findings[0].message.find("of the sheet"), std::string::npos) << findings[0].message;
    EXPECT_EQ(findings[1].code, "sheet.empty");
    EXPECT_EQ(findings[1].sheetId, "s3");
}

TEST(SheetPreflight, ASkippedCodeIsNotReported)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.defaults.logoAsset.clear();
    set.sheets[0].viewports[0].scale = 437.0;
    PreflightOptions options = knownLogo();
    options.skip = {"logo.missing"};
    EXPECT_EQ(codesOf(check(set, model, options)), "scale.non-standard");
}

TEST(SheetPreflight, TheCatalogueNamesEveryCodeOnceAndTheirSeverities)
{
    std::set<std::string_view> codes;
    for (const CheckDescription& description : preflightChecks()) {
        EXPECT_TRUE(codes.insert(description.code).second) << description.code;
        EXPECT_FALSE(description.looksFor.empty()) << description.code;
    }
    for (const std::string_view code :
         {"viewport.outside", "viewport.overlap", "plan.empty", "section.alignment-missing",
          "section.station-outside", "scale.non-standard", "field.empty", "logo.missing",
          "sheet.duplicate-name", "matchline.dangling", "text.too-small", "portrait.no-frame",
          "viewport.unplaced", "sheet.empty"}) {
        EXPECT_TRUE(codes.contains(code)) << code;
    }
    EXPECT_EQ(toString(Severity::Error), "error");
    EXPECT_EQ(severityFrom("info"), Severity::Info);
    EXPECT_FALSE(severityFrom("fatal").has_value());
}

TEST(SheetPreflight, TheSummaryCountsEachSeverity)
{
    const std::vector<Finding> findings{
        Finding{Severity::Error, "a", {}, {}, {}, {}, {}, {}},
        Finding{Severity::Error, "b", {}, {}, {}, {}, {}, {}},
        Finding{Severity::Warning, "c", {}, {}, {}, {}, {}, {}},
        Finding{Severity::Info, "d", {}, {}, {}, {}, {}, {}},
    };
    EXPECT_EQ(summarize(findings), (PreflightSummary{2, 1, 1}));
    EXPECT_EQ(summaryText(summarize(findings)), "2 errors, 1 warning, 1 note");
    EXPECT_EQ(summaryText({}), "no problems found");
}

TEST(SheetPreflight, AFindingReadsAsOneLine)
{
    const Finding finding{Severity::Error, "viewport.outside", 1u, "s2", "vp3", {},
                          "PLAN 1:500 (vp3) runs 14.6 mm under the title block", "Drag it back"};
    EXPECT_EQ(findingLine(finding),
              "ERROR viewport.outside [s2/vp3] PLAN 1:500 (vp3) runs 14.6 mm under the title block. "
              "Fix: Drag it back.");
    const Finding setWide{Severity::Info, "logo.missing", {}, {}, {}, {}, "No logo", {}};
    EXPECT_EQ(findingLine(setWide), "INFO logo.missing No logo.");
}

TEST(SheetPreflight, FindingsRoundTripThroughJson)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.defaults.organisation.clear();
    set.defaults.logoAsset.clear();
    set.sheets[0].viewports.push_back(planAt("vp2", Box2(Point2(100.0, 20.0), Point2(200.0, 100.0)), 437.0,
                                             Point2(50.0, 0.0)));
    const auto findings = check(set, model);
    ASSERT_FALSE(findings.empty());
    const std::string json = findingsToJson(findings);
    EXPECT_NE(json.find("\"format\":\"katana-sheet-checks\""), std::string::npos);
    EXPECT_NE(json.find("\"summary\":{\"errors\":1,\"info\":2,\"warnings\":2}"), std::string::npos) << json;
    // A set-wide finding has no sheet: the member is left out, not null.
    EXPECT_EQ(json.find("null"), std::string::npos) << json;
    const auto back = findingsFromJson(json);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_EQ(*back, findings);

    EXPECT_FALSE(findingsFromJson("{\"format\":\"katana-sheet-checks\",\"version\":2,\"findings\":[]}"));
    EXPECT_FALSE(findingsFromJson("[1, 2]"));
    EXPECT_FALSE(findingsFromJson(
        "{\"format\":\"katana-sheet-checks\",\"version\":1,\"findings\":[{\"severity\":\"fatal\",\"code\":\"x\"}]}"));
    EXPECT_FALSE(findingsFromJson("{\"format\":\"katana-sheet-checks\",\"version\":1,\"findings\":{}}"));
    // A position is a count: -1 is not read as the largest one there is.
    EXPECT_FALSE(findingsFromJson("{\"format\":\"katana-sheet-checks\",\"version\":1,\"findings\":"
                                  "[{\"severity\":\"error\",\"code\":\"x\",\"sheet_index\":-1}]}"));
    EXPECT_FALSE(findingsFromJson("{\"format\":\"katana-sheet-checks\",\"version\":1,\"findings\":"
                                  "[{\"severity\":\"error\",\"code\":\"x\",\"sheet_index\":1.5}]}"));
    const auto empty = findingsFromJson(findingsToJson({}));
    ASSERT_TRUE(empty.ok());
    EXPECT_TRUE(empty->empty());
}

TEST(SheetPreflight, APlotOfSomeSheetsIsWarnedAboutThemAndTheSet)
{
    const Model model = drawing();
    SheetSet set = cleanSet();
    set.defaults.logoAsset.clear();
    set.sheets.push_back(sheetOf("s2", "EMPTY", {}));
    set.sheets.push_back(sheetOf("s3", "ALSO EMPTY", {}));
    const auto findings = check(set, model);
    ASSERT_EQ(codesOf(findings), "logo.missing sheet.empty sheet.empty");

    const std::vector<std::size_t> third{2};
    const auto onThird = findingsOnSheets(findings, third);
    ASSERT_EQ(onThird.size(), 2u);
    EXPECT_EQ(onThird[0].code, "logo.missing"); // the set's: on every sheet
    EXPECT_EQ(onThird[1].sheetId, "s3");
    const std::vector<std::size_t> first{0};
    EXPECT_EQ(codesOf(findingsOnSheets(findings, first)), "logo.missing");
    EXPECT_EQ(findingsOnSheets(findings, {}), findings);
}

TEST(SheetPreflight, TheSpatialIndexFindsWhatAScanOfTheDrawingFinds)
{
    // A 1 km square of lines and small texts, and plans over parts of it:
    // level and turned, over lines, over nothing, at scales where the text
    // is too small and where it reads. A plan's window is a few percent of
    // the drawing, so the index answers - and must answer as the scan does.
    katana::cad::Document document;
    std::vector<Entity> entities;
    for (int i = 0; i < 40; ++i) {
        Entity line;
        line.geometry = Segment2{Point2(i * 25.0, 0.0), Point2(i * 25.0 + 10.0, 1000.0)};
        line.layer = "0";
        entities.push_back(line);
        Entity text;
        text.geometry = katana::entity::TextGeometry{Point2(i * 25.0 + 3.0, i * 25.0), "T", 0.2 + 0.05 * i, 0.3};
        text.layer = "0";
        entities.push_back(text);
    }
    ASSERT_TRUE(document.execute(katana::commands::createEntities(std::move(entities))).ok());
    SheetSet set = cleanSet();
    set.sheets[0].viewports = {
        planAt("vp1", Box2(Point2(30.0, 40.0), Point2(215.0, 280.0)), 500.0, Point2(300.0, 300.0)),
        planAt("vp2", Box2(Point2(218.0, 160.0), Point2(405.0, 280.0)), 200.0, Point2(612.0, 612.0)),
        planAt("vp3", Box2(Point2(218.0, 40.0), Point2(405.0, 157.0)), 1000.0, Point2(5000.0, 5000.0)),
    };
    set.sheets[0].viewports[1].rotation = 0.7;
    const Model& model = document.model();
    PreflightOptions scan = knownLogo();
    PreflightOptions indexed = knownLogo();
    indexed.index = &document.spatialIndex();
    const auto byIndex = check(set, model, indexed);
    EXPECT_EQ(byIndex, check(set, model, scan));
    // And there is something to agree about: the far plan is empty; the
    // first shows 0.75 to 0.85 m texts, under the 0.9 m that reads at 1:500;
    // the turned one shows a 1.4 m text, 7 mm at 1:200.
    const auto empty = withCode(byIndex, "plan.empty");
    ASSERT_EQ(empty.size(), 1u);
    EXPECT_EQ(empty[0].viewportId, "vp3");
    const auto small = withCode(byIndex, "text.too-small");
    ASSERT_EQ(small.size(), 1u);
    EXPECT_EQ(small[0].viewportId, "vp1");
    EXPECT_NE(small[0].message.find("3 texts"), std::string::npos) << small[0].message;
}

TEST(SheetPreflight, ADocumentsSheetsAreCheckedWithItsOwnDrawingAndFields)
{
    katana::cad::Document document;
    SheetSet set = cleanSet();
    ASSERT_TRUE(addSheets(document, set.sheets).ok());
    ASSERT_TRUE(editSheetSet(document, [](SheetSet& s) {
                    s.defaults = filledTitleBlock();
                    s.defaults.logoAsset.clear();
                    s.revisions.push_back(Revision{"A", "", "", ""});
                    return katana::core::Status{};
                }).ok());
    const auto findings = checkDocumentSheets(document);
    // A new drawing: no coordinate system to fall back on, no logo, and a
    // plan over nothing.
    std::string codes;
    for (const Finding& f : findings) {
        codes += f.code + (f.subject.empty() ? "" : ":" + f.subject) + " ";
    }
    EXPECT_EQ(codes, "field.empty:coordinate_system logo.missing plan.empty ");
}
