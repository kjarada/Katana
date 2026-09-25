#include <gtest/gtest.h>

#include <algorithm>
#include <chrono>
#include <filesystem>
#include <fstream>
#include <map>
#include <string>

#include "katana/entity/model.hpp"
#include "katana/ifc/front_end.hpp"
#include "katana/ifc/import.hpp"
#include "katana/terrain/tin_surface.hpp"

namespace ifc = katana::ifc;
using katana::core::ErrorCode;
using katana::entity::Entity;
using katana::geometry::Point2;
using katana::geometry::Polyline2;

namespace {

std::string refusal(const std::optional<katana::core::Result<ifc::ExportArguments>>& parsed)
{
    if (!parsed) {
        return "<not an ifc line>";
    }
    return *parsed ? std::string("ok") : parsed->error().describe();
}

std::string temporaryFile(const std::string& name, const std::string& text)
{
    const auto path = std::filesystem::temp_directory_path() / name;
    std::ofstream(path, std::ios::binary) << text;
    return path.string();
}

} // namespace

// ---- the grammar ------------------------------------------------------------------

TEST(IfcFrontEnd, AnExportLineIsItsPathAndItsOptionsInAnyCase)
{
    const auto parsed =
        ifc::parseExportArguments("site plan.ifc utilities \"a b.csv\" Schema s.csv RULES r.csv "
                                  "spacing 12.5 NoDrawing");
    ASSERT_TRUE(parsed && *parsed) << refusal(parsed);
    const ifc::ExportArguments& arguments = **parsed;
    EXPECT_EQ(arguments.path, "site plan.ifc"); // up to the first .ifc ending a word
    EXPECT_EQ(arguments.schedule, "a b.csv");
    EXPECT_EQ(arguments.schema, "s.csv");
    EXPECT_EQ(arguments.rules, "r.csv");
    EXPECT_EQ(arguments.spacing, 12.5);
    EXPECT_FALSE(arguments.drawing);

    const auto quoted = ifc::parseExportArguments("  \"C:/jobs/x y.IFC\"  ");
    ASSERT_TRUE(quoted && *quoted);
    EXPECT_EQ((*quoted)->path, "C:/jobs/x y.IFC");
    EXPECT_TRUE((*quoted)->drawing);
    EXPECT_FALSE((*quoted)->schedule);
}

// Not an .ifc: the line is another exporter's, and is left to it.
TEST(IfcFrontEnd, ALineThatNamesNoIfcIsLeftToTheOtherFormats)
{
    EXPECT_FALSE(ifc::parseExportArguments("site.dxf"));
    EXPECT_FALSE(ifc::parseExportArguments("site.ifcxml"));
    EXPECT_FALSE(ifc::parseExportArguments("\"site.dxf\" UTILITIES x.csv"));
    EXPECT_FALSE(ifc::parseImportArguments("survey.12da"));
    EXPECT_FALSE(ifc::parseImportArguments(""));
}

// The refusals, word for word what katana_cli has always said, since both
// front ends now say it from here.
TEST(IfcFrontEnd, ARefusedExportLineSaysWhyAsTheCommandLineAlwaysHas)
{
    EXPECT_EQ(refusal(ifc::parseExportArguments("x.ifc UTILITIS s.csv")),
              "InvalidArgument: \"UTILITIS\" is not an option of EXPORT <file.ifc>: UTILITIES "
              "<schedule.csv>, SCHEMA <schema.csv>, RULES <rules.csv>, SPACING <m>, NODRAWING");
    EXPECT_EQ(refusal(ifc::parseExportArguments("x.ifc SCHEMA s.csv")),
              "InvalidArgument: SCHEMA describes a schedule: give it with UTILITIES");
    EXPECT_EQ(refusal(ifc::parseExportArguments("x.ifc SPACING 0")),
              "InvalidArgument: SPACING takes a positive number of metres");
    EXPECT_EQ(refusal(ifc::parseExportArguments("x.ifc SPACING ten")),
              "InvalidArgument: SPACING takes a positive number of metres");
    EXPECT_EQ(refusal(ifc::parseExportArguments("x.ifc UTILITIES \"never closed")),
              "InvalidArgument: a quoted path is never closed");
    // A keyword with no value is not taken for one.
    EXPECT_NE(refusal(ifc::parseExportArguments("x.ifc UTILITIES")).find("\"UTILITIES\" is not"),
              std::string::npos);

    const auto local = ifc::parseImportArguments("x.ifc local");
    ASSERT_TRUE(local && *local);
    EXPECT_TRUE((*local)->local);
    const auto wrong = ifc::parseImportArguments("x.ifc LOCALLY");
    ASSERT_TRUE(wrong && !*wrong);
    EXPECT_EQ(wrong->error().describe(),
              "InvalidArgument: \"LOCALLY\" is not an option of IMPORT <file.ifc>: LOCAL");
}

TEST(IfcFrontEnd, TheGlobalIdNamespaceIsTheProjectAndTheTimestampIsUtc)
{
    EXPECT_EQ(ifc::guidNamespaceFor("Road 7", "2026-09-01T10:00:00Z"),
              "Road 7/2026-09-01T10:00:00Z");
    // 2026-09-25T16:01:09.750Z, truncated to the second.
    const auto when = std::chrono::sys_days(std::chrono::year(2026) / 9 / 25) +
                      std::chrono::hours(16) + std::chrono::minutes(1) +
                      std::chrono::milliseconds(9750);
    EXPECT_EQ(ifc::headerTimestamp(when), "2026-09-25T16:01:09");
}

// ---- what an export names ------------------------------------------------------------

TEST(IfcFrontEnd, TheFilesAnExportNamesAreReadTheSchemaOwnedAndTheRulesBeforeTheDefaults)
{
    ifc::ExportArguments arguments;
    arguments.path = "x.ifc";
    arguments.schedule = std::string(KATANA_SAMPLES) + "/utilities/schedule_tfnsw.csv";
    arguments.schema = std::string(KATANA_IFC_TEST_DATA) + "/delivery_schema.csv";
    arguments.spacing = 20.0;
    arguments.rules = temporaryFile("katana_ifc_rules_test.csv",
                                    "rule,words,class\nlight pole,POLE*,IfcColumn\n");
    auto files = ifc::readExportFiles(arguments);
    ASSERT_TRUE(files.ok()) << files.error().describe();
    ASSERT_TRUE(files->utilities);
    EXPECT_EQ(files->utilities->lines.size(), 3u);
    EXPECT_EQ(files->utilities->sourceName, "schedule_tfnsw.csv");
    EXPECT_EQ(files->utilities->grading.maximumDetectedSpacing, 20.0);
    ASSERT_NE(files->schema, nullptr);
    // Moved, the schema stays where the input points.
    ifc::ExportFiles moved = std::move(*files);
    EXPECT_EQ(moved.utilities->schema, moved.schema.get());
    ASSERT_EQ(moved.rules.size(), 1 + ifc::defaultClassificationRules().size());
    EXPECT_EQ(moved.rules.front().name, "light pole");
    EXPECT_EQ(moved.rules[1].name, ifc::defaultClassificationRules().front().name);

    arguments.schedule = "/no/such/schedule.csv";
    const auto missing = ifc::readExportFiles(arguments);
    ASSERT_FALSE(missing.ok());
    EXPECT_EQ(missing.error().code, ErrorCode::NotFound);
    EXPECT_NE(missing.error().describe().find("/no/such/schedule.csv"), std::string::npos);

    // A schedule that does not parse is named with the reader's error.
    arguments.schedule = temporaryFile("katana_ifc_bad_schedule.csv", "line,point\nW,1\n");
    arguments.schema.reset();
    const auto bad = ifc::readExportFiles(arguments);
    ASSERT_FALSE(bad.ok());
    EXPECT_NE(bad.error().context.find("katana_ifc_bad_schedule.csv"), std::string::npos)
        << bad.error().describe();
}

// ---- a project's rules ------------------------------------------------------------

TEST(IfcRulesFile, ARulesFileIsReadByItsHeaderAndEachRuleChecked)
{
    const std::string text =
        "\xEF\xBB\xBF# a project's own layers\n"
        "Class,Rule,Words,Kinds,Predefined_Type,System\n"
        "IfcColumn,light pole,\"POLE*; LP\",point,column,\n"
        "ifcpipesegment,drainage line,DRN,Polyline;Line,rigidsegment,stormwater\n"
        "\n";
    const auto rules = ifc::parseClassificationRules(text);
    ASSERT_TRUE(rules.ok()) << rules.error().describe();
    ASSERT_EQ(rules->size(), 2u);
    const auto& pole = (*rules)[0];
    EXPECT_EQ(pole.name, "light pole");
    EXPECT_EQ(pole.words, (std::vector<std::string>{"POLE*", "LP"}));
    EXPECT_EQ(pole.kinds,
              std::vector<katana::entity::EntityType>{katana::entity::EntityType::Point});
    EXPECT_EQ(pole.target, (ifc::IfcClass{"IfcColumn", "COLUMN", {}}));
    const auto& drain = (*rules)[1];
    EXPECT_EQ(drain.target.entity, "IfcPipeSegment"); // the schema's spelling
    EXPECT_EQ(drain.system, "STORMWATER");
    EXPECT_EQ(drain.kinds.size(), 2u);
}

TEST(IfcRulesFile, WhatWouldWriteAnInvalidFileIsRefusedWithItsLine)
{
    const auto why = [](const std::string& rows) {
        const auto rules = ifc::parseClassificationRules(
            "rule,words,kinds,class,predefined_type,object_type\n" + rows);
        return rules.ok() ? std::string("ok") : rules.error().describe();
    };
    EXPECT_EQ(why("pole,POLE,,IfcColumn,COLUMN,\n"), "ok");
    EXPECT_NE(
        why("pole,POLE,,IfcColumnn,,\n").find("\"IfcColumnn\" is not a class this export writes"),
        std::string::npos);
    EXPECT_NE(why("pole,POLE,,IfcColumn,PILLAR,\n")
                  .find("\"PILLAR\" is not a predefined type of IfcColumn"),
              std::string::npos);
    EXPECT_NE(why("bore,BH,,IfcBorehole,NOTDEFINED,\n").find("IfcBorehole has no predefined type"),
              std::string::npos);
    EXPECT_NE(why("pole,POLE,Tree,IfcColumn,,\n").find("\"Tree\" is not a kind"),
              std::string::npos);
    EXPECT_NE(why("pole,,,IfcColumn,,\n").find("has no words"), std::string::npos);
    EXPECT_NE(why("pole,POLE,,IfcColumn,USERDEFINED,\n").find("says nothing of what it is"),
              std::string::npos);
    EXPECT_EQ(why("pole,POLE,,IfcColumn,USERDEFINED,LIGHT POLE\n"), "ok");
    EXPECT_NE(why("pole,POLE,,IfcColumn\n").find("4 fields where the header has 6"),
              std::string::npos);
    EXPECT_NE(why("pole,\"POLE,,IfcColumn,,\n").find("never closed"), std::string::npos);
    // The line is the file's own: the header is line 1.
    EXPECT_NE(why("ok,A,,IfcColumn,,\nbad,B,,IfcNothing,,\n").find("[line 3]"), std::string::npos);

    const auto noClass = ifc::parseClassificationRules("rule,words\npole,POLE\n");
    ASSERT_FALSE(noClass.ok());
    EXPECT_NE(noClass.error().describe().find("\"class\" is missing"), std::string::npos);
    const auto unknown = ifc::parseClassificationRules("rule,words,class,colour\n");
    ASSERT_FALSE(unknown.ok());
    EXPECT_NE(unknown.error().describe().find("unknown column \"colour\""), std::string::npos);
}

// The defaults written out read back as themselves: the window offers them
// as the starting point of a project's file.
TEST(IfcRulesFile, TheDefaultsWrittenOutReadBackAsThemselves)
{
    const auto& defaults = ifc::defaultClassificationRules();
    const std::string text = ifc::formatClassificationRules(defaults);
    const auto back = ifc::parseClassificationRules(text);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    ASSERT_EQ(back->size(), defaults.size());
    for (std::size_t i = 0; i < defaults.size(); ++i) {
        EXPECT_EQ((*back)[i].name, defaults[i].name);
        EXPECT_EQ((*back)[i].words, defaults[i].words);
        EXPECT_EQ((*back)[i].kinds, defaults[i].kinds);
        EXPECT_EQ((*back)[i].target, defaults[i].target);
        EXPECT_EQ((*back)[i].system, defaults[i].system);
    }
}

// Every class the export can choose on its own names a predefined type the
// schema has: the check a rules file gets, applied to the defaults and to
// the service classes (the enumerations are IfcOpenShell's, generated).
TEST(IfcRulesFile, EveryClassTheExportChoosesIsOneTheSchemaHas)
{
    // The defaults pass the check a file's rules get.
    EXPECT_TRUE(ifc::parseClassificationRules(
                    ifc::formatClassificationRules(ifc::defaultClassificationRules()))
                    .ok());
    namespace sub = katana::survey::subsurface;
    std::string rows = "rule,words,class,predefined_type,object_type\n";
    for (int type = 0; type <= static_cast<int>(sub::UtilityType::Other); ++type) {
        for (const char* feature :
             {"",           "Culvert",   "Conduit",   "Trough", "Optic fibre", "Cable", "Pipe",
              "Manhole",    "Valve pit", "Meter pit", "Sump",   "Chamber",     "Pit",   "Hydrant",
              "Stop valve", "Air valve", "Scour",     "Valve",  "Marker",      "Pillar"}) {
            sub::UtilityAttributes service;
            service.type = static_cast<sub::UtilityType>(type);
            service.fields["AssetFeature"] = feature;
            const auto run = ifc::classifyUtilityRun(service).element;
            rows += "run,X," + run.entity + "," + run.predefinedType + "," + run.objectType + "\n";
            if (const auto point = ifc::classifyUtilityPoint(service)) {
                rows += "point,X," + point->element.entity + "," + point->element.predefinedType +
                        "," + point->element.objectType + "\n";
            }
        }
    }
    const auto checked = ifc::parseClassificationRules(rows);
    EXPECT_TRUE(checked.ok()) << checked.error().describe();
}

// ---- what an export accounts for ---------------------------------------------------

TEST(IfcExportTally, TheReportAccountsForEachSourceByTheClassItBecameAndWhy)
{
    katana::entity::Model model;
    Entity kerb;
    kerb.geometry = Polyline2{{Point2{0.0, 0.0}, Point2{10.0, 0.0}}, false};
    kerb.layer = "Survey/Kerb";
    ASSERT_TRUE(model.entities.add(kerb).ok());
    ASSERT_TRUE(model.entities.add(kerb).ok());
    Entity label;
    katana::entity::LabelGeometry labelShape;
    labelShape.style = "Chainage";
    labelShape.alignment = "MC01";
    label.geometry = labelShape;
    label.layer = "Notes";
    ASSERT_TRUE(model.entities.add(label).ok());
    Entity point;
    point.geometry = katana::entity::PointGeometry{Point2{5.0, 5.0}};
    point.layer = "Notes";
    ASSERT_TRUE(model.entities.add(point).ok());

    const auto out = ifc::writeIfc({&model, {}, {}});
    ASSERT_TRUE(out.ok()) << out.error().describe();
    const std::vector<ifc::ClassTally> expected{
        {"layer Notes", "", "", "", "", "a label: labels are not exported", 1},
        {"layer Notes", "IfcAnnotation", "SURVEY", "", "", "no rule: a point", 1},
        {"layer Survey/Kerb", "IfcKerb", "NOTDEFINED", "", "", "rule kerb", 2},
    };
    EXPECT_EQ(out->tally, expected);
}

// ---- the extent of an import ------------------------------------------------------

// An alignment alone has an extent, so LOCAL and the far-apart question
// treat it as they treat entities.
TEST(IfcImportBounds, TheExtentIncludesTheAlignmentsAndSurfaces)
{
    katana::entity::Model model;
    katana::entity::Alignment alignment;
    alignment.name = "A";
    alignment.horizontal.pis = {{Point2{1000.0, 2000.0}, 0.0, 0.0, 0.0},
                                {Point2{1100.0, 2050.0}, 0.0, 0.0, 0.0}};
    ASSERT_TRUE(model.alignments.add(alignment).ok());
    const auto out = ifc::writeIfc({&model, {}, {}});
    ASSERT_TRUE(out.ok());
    const auto back = ifc::readIfc(out->text);
    ASSERT_TRUE(back.ok());
    EXPECT_TRUE(back->entities.empty());
    ASSERT_FALSE(back->bounds.empty());
    EXPECT_NEAR(back->bounds.min.x, 1000.0, 1e-6);
    EXPECT_NEAR(back->bounds.max.y, 2050.0, 1e-6);
}

// ---- what the review found ----------------------------------------------------------

// A quote opened before a .ifc and never closed is refused, not handed to
// the other formats' grammar to be written under a name starting '"'.
TEST(IfcFrontEnd, AnUnclosedQuoteBeforeAnIfcIsRefused)
{
    EXPECT_EQ(refusal(ifc::parseExportArguments("\"site plan.ifc")),
              "InvalidArgument: a quoted path is never closed");
    const auto import = ifc::parseImportArguments("\"site.ifc LOCAL");
    ASSERT_TRUE(import && !*import);
    EXPECT_EQ(import->error().describe(), "InvalidArgument: a quoted path is never closed");
    // Not an .ifc: still the other formats' line.
    EXPECT_FALSE(ifc::parseExportArguments("\"site.dxf"));
}

TEST(IfcRulesFile, ACommentIsSkippedWholeAndAQuotedHashIsData)
{
    // A quote in a comment must not open a field that swallows the rules
    // after it; a quoted field that starts '#' is a rule, not a comment.
    const std::string text = "rule,words,kinds,class,predefined_type,object_type,system\n"
                             "# light poles, \"LP or POLE on the lighting layer\n"
                             "light pole,LIGHTING,Point,IfcColumn,COLUMN,,\n"
                             "  # kerbs surveyed as 12\" strings\n"
                             "\"#1 kerb\",KB,,\"IfcKerb\" ,,,\n";
    const auto rules = ifc::parseClassificationRules(text);
    ASSERT_TRUE(rules.ok()) << rules.error().describe();
    ASSERT_EQ(rules->size(), 2u);
    EXPECT_EQ((*rules)[0].name, "light pole");
    EXPECT_EQ((*rules)[1].name, "#1 kerb");
    EXPECT_EQ((*rules)[1].target.entity, "IfcKerb"); // the blank after the quote is not its
    // Anything but blanks between a closing quote and the comma is refused.
    const auto after =
        ifc::parseClassificationRules("rule,words,class\npole,POLE,\"IfcColumn\"x\n");
    ASSERT_FALSE(after.ok());
    EXPECT_NE(after.error().describe().find("text after a quoted field's closing quote"),
              std::string::npos);
    EXPECT_NE(after.error().describe().find("[line 2]"), std::string::npos);
}

// Whatever a rule is called and whatever its words, what the writer writes
// the reader reads back as it was.
TEST(IfcRulesFile, AnyRuleWrittenOutReadsBackAsItWas)
{
    std::vector<ifc::ClassificationRule> rules{
        {"#1 pole",
         {"POLE*", "LP"},
         {katana::entity::EntityType::Point},
         {"IfcColumn", "COLUMN", {}},
         {}},
        {"a, \"quoted\" name", {"WALL"}, {}, {"IfcWall", "USERDEFINED", "SOUND, WALL"}, "NOISE"},
        {" padded ", {"FENCE"}, {}, {"IfcRailing", "FENCE", {}}, {}},
    };
    const auto back = ifc::parseClassificationRules(ifc::formatClassificationRules(rules));
    ASSERT_TRUE(back.ok()) << back.error().describe();
    ASSERT_EQ(back->size(), rules.size());
    for (std::size_t i = 0; i < rules.size(); ++i) {
        EXPECT_EQ((*back)[i].name, rules[i].name);
        EXPECT_EQ((*back)[i].words, rules[i].words);
        EXPECT_EQ((*back)[i].kinds, rules[i].kinds);
        EXPECT_EQ((*back)[i].target, rules[i].target);
        EXPECT_EQ((*back)[i].system, rules[i].system);
    }
}

// A terrain alone has an extent too.
TEST(IfcImportBounds, ASurfaceAloneHasAnExtent)
{
    auto surface = katana::terrain::TinSurface::create(
        {{500.0, 700.0, 10.0}, {510.0, 700.0, 11.0}, {500.0, 720.0, 12.0}}, {{0, 1, 2}});
    ASSERT_TRUE(surface.ok());
    const auto out = ifc::writeIfc({nullptr, {{"Ground", &*surface}}, {}});
    ASSERT_TRUE(out.ok());
    const auto back = ifc::readIfc(out->text);
    ASSERT_TRUE(back.ok());
    ASSERT_EQ(back->surfaces.size(), 1u);
    ASSERT_FALSE(back->bounds.empty());
    EXPECT_NEAR(back->bounds.min.x, 500.0, 1e-9);
    EXPECT_NEAR(back->bounds.min.y, 700.0, 1e-9);
    EXPECT_NEAR(back->bounds.max.x, 510.0, 1e-9);
    EXPECT_NEAR(back->bounds.max.y, 720.0, 1e-9);
}

// Every product the report counts is in the account, once, as the class
// written - the preview is only as true as this. (IfcDistributionSystem is
// a group, not a product: it is in the classes map and not in the tally.)
TEST(IfcExportTally, EveryProductTheReportCountsIsAccountedForOnce)
{
    katana::entity::Model model;
    Entity kerb;
    kerb.geometry = Polyline2{{Point2{334000.0, 6250000.0}, Point2{334010.0, 6250000.0}}, false};
    kerb.layer = "Survey/Kerb";
    ASSERT_TRUE(model.entities.add(kerb).ok());
    Entity pit;
    pit.geometry = katana::entity::PointGeometry{Point2{334005.0, 6250002.0}};
    pit.layer = "Stormwater/Pits";
    ASSERT_TRUE(model.entities.add(pit).ok());
    katana::entity::Alignment alignment;
    alignment.name = "A";
    alignment.horizontal.pis = {{Point2{334000.0, 6250000.0}, 0.0, 0.0, 0.0},
                                {Point2{334100.0, 6250050.0}, 0.0, 0.0, 0.0}};
    ASSERT_TRUE(model.alignments.add(alignment).ok());
    auto surface = katana::terrain::TinSurface::create(
        {{334000.0, 6250000.0, 10.0}, {334010.0, 6250000.0, 11.0}, {334000.0, 6250010.0, 12.0}},
        {{0, 1, 2}});
    ASSERT_TRUE(surface.ok());
    ifc::ExportArguments arguments;
    arguments.path = "x.ifc";
    arguments.schedule = std::string(KATANA_SAMPLES) + "/utilities/schedule.csv";
    auto files = ifc::readExportFiles(arguments);
    ASSERT_TRUE(files.ok());

    const auto out = ifc::writeIfc({&model, {{"Ground", &*surface}}, std::move(files->utilities)});
    ASSERT_TRUE(out.ok()) << out.error().describe();
    std::map<std::string, std::size_t> tallied;
    for (const auto& row : out->tally) {
        if (!row.entity.empty()) {
            tallied[row.entity] += row.count;
        }
    }
    auto counted = out->classes;
    counted.erase("IfcDistributionSystem");
    EXPECT_EQ(tallied, counted);
    // A located point is in its service's system, as the file groups it.
    const auto point = std::find_if(out->tally.begin(), out->tally.end(), [](const auto& row) {
        return row.source == "service W1" && row.entity == "IfcAnnotation";
    });
    ASSERT_NE(point, out->tally.end());
    EXPECT_EQ(point->system, "WATERSUPPLY");
    EXPECT_EQ(point->count, 6u);
}
