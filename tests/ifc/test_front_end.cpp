#include <gtest/gtest.h>

#include <algorithm>
#include <chrono>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <map>
#include <string>

#include "katana/core/text.hpp"
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
    // NODRAWING: the investigation alone - no entities, alignments or surfaces.
    EXPECT_FALSE(arguments.entities || arguments.alignments || arguments.surfaces);

    const auto quoted = ifc::parseExportArguments("  \"C:/jobs/x y.IFC\"  ");
    ASSERT_TRUE(quoted && *quoted);
    EXPECT_EQ((*quoted)->path, "C:/jobs/x y.IFC");
    EXPECT_TRUE((*quoted)->entities && (*quoted)->alignments && (*quoted)->surfaces);
    EXPECT_FALSE((*quoted)->selected || (*quoted)->preview);
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

// The refusals, said once here for both front ends: an unknown word names
// every option the verb takes.
TEST(IfcFrontEnd, ARefusedExportLineSaysWhyAsTheCommandLineAlwaysHas)
{
    EXPECT_EQ(refusal(ifc::parseExportArguments("x.ifc UTILITIS s.csv")),
              "InvalidArgument: \"UTILITIS\" is not an option of EXPORT <file.ifc>: UTILITIES "
              "<schedule.csv>, SCHEMA <schema.csv>, RULES <rules.csv>, SPACING <m>, NODRAWING, "
              "NOENTITIES, SELECTED, NOALIGNMENTS, NOSURFACES, PREVIEW");
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
              "InvalidArgument: \"LOCALLY\" is not an option of IMPORT <file.ifc>: LOCAL, "
              "NOALIGNMENTS, NOELEMENTS, NOSURFACES, TOLERANCE <m>, TAKECRS, KEEPCRS");
}

// Every choice the IFC dialogs offer is a word of the line, so that a typed
// line, a script and an agent can ask for anything a dialog can.
TEST(IfcFrontEnd, EveryChoiceOfTheDialogsIsAWordOfTheLine)
{
    const auto exported =
        ifc::parseExportArguments("x.ifc noalignments NOSURFACES selected Preview");
    ASSERT_TRUE(exported && *exported) << refusal(exported);
    EXPECT_TRUE((*exported)->entities);
    EXPECT_FALSE((*exported)->alignments);
    EXPECT_FALSE((*exported)->surfaces);
    EXPECT_TRUE((*exported)->selected);
    EXPECT_TRUE((*exported)->preview);
    const auto noEntities = ifc::parseExportArguments("x.ifc NOENTITIES");
    ASSERT_TRUE(noEntities && *noEntities);
    EXPECT_FALSE((*noEntities)->entities);
    EXPECT_TRUE((*noEntities)->alignments && (*noEntities)->surfaces);

    const auto imported =
        ifc::parseImportArguments("x.ifc NOALIGNMENTS nosurfaces TOLERANCE 0.005 KeepCrs");
    ASSERT_TRUE(imported && *imported);
    EXPECT_FALSE((*imported)->alignments);
    EXPECT_TRUE((*imported)->elements);
    EXPECT_FALSE((*imported)->surfaces);
    EXPECT_EQ((*imported)->tolerance, 0.005);
    EXPECT_EQ((*imported)->takeCoordinateSystem, false);
    const auto take = ifc::parseImportArguments("x.ifc TAKECRS NOELEMENTS");
    ASSERT_TRUE(take && *take);
    EXPECT_EQ((*take)->takeCoordinateSystem, true);
    EXPECT_FALSE((*take)->elements);
    // Neither: the front end's own way (the window asks, a typed line says how).
    const auto plain = ifc::parseImportArguments("x.ifc");
    ASSERT_TRUE(plain && *plain);
    EXPECT_FALSE((*plain)->takeCoordinateSystem);
}

// What would be contradictory, repeated, or would import nothing is refused
// by name rather than read one way or the other.
TEST(IfcFrontEnd, ContradictoryOrRepeatedWordsAreRefused)
{
    EXPECT_EQ(refusal(ifc::parseExportArguments("x.ifc PREVIEW preview")),
              "InvalidArgument: PREVIEW is given twice");
    EXPECT_EQ(refusal(ifc::parseExportArguments("x.ifc NOENTITIES SELECTED")),
              "InvalidArgument: SELECTED chooses among the entities, and NOENTITIES leaves them "
              "out");
    EXPECT_EQ(refusal(ifc::parseExportArguments("x.ifc SELECTED NODRAWING UTILITIES s.csv")),
              "InvalidArgument: SELECTED chooses among the entities, and NODRAWING leaves them "
              "out");
    const auto importRefusal = [](std::string_view line) {
        const auto parsed = ifc::parseImportArguments(line);
        return !parsed ? std::string("<not an ifc line>")
                       : (*parsed ? std::string("ok") : parsed->error().describe());
    };
    EXPECT_EQ(importRefusal("x.ifc LOCAL local"), "InvalidArgument: LOCAL is given twice");
    EXPECT_EQ(importRefusal("x.ifc NOALIGNMENTS NOELEMENTS NOSURFACES"),
              "InvalidArgument: NOALIGNMENTS, NOELEMENTS and NOSURFACES together leave nothing "
              "to import");
    EXPECT_EQ(importRefusal("x.ifc TAKECRS KEEPCRS"),
              "InvalidArgument: TAKECRS and KEEPCRS say opposite things: give one");
    EXPECT_EQ(importRefusal("x.ifc LOCAL TAKECRS"),
              "InvalidArgument: LOCAL moves the data out of the file's coordinate system, which "
              "TAKECRS would give the project");
    EXPECT_EQ(importRefusal("x.ifc TOLERANCE -1"),
              "InvalidArgument: TOLERANCE takes a positive number of metres");
    EXPECT_NE(importRefusal("x.ifc TOLERANCE").find("\"TOLERANCE\" is not an option"),
              std::string::npos);
    // LOCAL with KEEPCRS says the same thing twice, and is taken.
    EXPECT_EQ(importRefusal("x.ifc LOCAL KEEPCRS"), "ok");
}

// The line a dialog hands the command line reads back as what it was asked,
// whatever the paths hold - blanks, a ".ifc " inside a folder's name, a name
// that is an option's word - and a number to its last bit.
TEST(IfcFrontEnd, TheLineADialogWritesReadsBackAsItsArguments)
{
    ifc::ExportArguments everything;
    everything.path = "C:/jobs/old.ifc files/Site Plan.ifc";
    everything.schedule = "a b.csv";
    everything.schema = "NODRAWING";
    everything.rules = "r.csv";
    everything.spacing = 0.1; // not exact in binary: shortest round trip
    everything.alignments = false;
    everything.selected = true;
    everything.preview = true;
    ifc::ExportArguments alone;
    alone.path = "x.ifc";
    alone.schedule = "s.csv";
    alone.entities = alone.alignments = alone.surfaces = false;
    for (const ifc::ExportArguments& arguments : {everything, alone, ifc::ExportArguments{}}) {
        ifc::ExportArguments asked = arguments;
        if (asked.path.empty()) {
            asked.path = "plain.ifc";
        }
        const auto line = ifc::formatExportLine(asked);
        ASSERT_TRUE(line.ok());
        ASSERT_TRUE(line->starts_with("EXPORT \"")) << *line;
        const auto back = ifc::parseExportArguments(line->substr(std::string("EXPORT").size()));
        ASSERT_TRUE(back && *back) << *line;
        EXPECT_EQ(**back, asked) << *line;
    }
    EXPECT_EQ(*ifc::formatExportLine(alone), "EXPORT \"x.ifc\" UTILITIES \"s.csv\" NODRAWING");

    ifc::ImportArguments imported;
    imported.path = "a b.ifc";
    imported.local = true;
    imported.elements = false;
    imported.tolerance = 0.003;
    imported.takeCoordinateSystem = false;
    const auto line = ifc::formatImportLine(imported);
    ASSERT_TRUE(line.ok());
    EXPECT_EQ(*line, "IMPORT \"a b.ifc\" LOCAL NOELEMENTS TOLERANCE 0.003 KEEPCRS");
    const auto back = ifc::parseImportArguments(line->substr(std::string("IMPORT").size()));
    ASSERT_TRUE(back && *back);
    EXPECT_EQ(**back, imported);

    // A double quote cannot be said inside a quoted word: refused, not
    // written as a line that would read as something else.
    ifc::ExportArguments quote;
    quote.path = "say \"hi\".ifc";
    EXPECT_EQ(ifc::formatExportLine(quote).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(*ifc::formatInfoLine("a b.ifc"), "INFO \"a b.ifc\"");
    EXPECT_EQ(*ifc::formatRulesLine("my rules.csv"), "IFC RULES \"my rules.csv\"");
}

// IFC RULES <file.csv>: the defaults, written for a project to edit, read
// back as the defaults they are.
TEST(IfcFrontEnd, IfcRulesWritesTheDefaultsForAProjectToEdit)
{
    EXPECT_FALSE(ifc::parseRulesArguments("OTHER x.csv"));
    EXPECT_FALSE(ifc::parseRulesArguments(""));
    const auto path = ifc::parseRulesArguments(" rules \"a b.csv\"");
    ASSERT_TRUE(path && *path);
    EXPECT_EQ(**path, "a b.csv");
    const auto extra = ifc::parseRulesArguments("RULES a.csv b.csv");
    ASSERT_TRUE(extra && !*extra);
    EXPECT_EQ(extra->error().describe(), "InvalidArgument: usage: IFC RULES <file.csv>");

    const std::string file =
        (std::filesystem::temp_directory_path() / "katana ifc default rules.csv").string();
    const auto reply = ifc::writeDefaultRules(file);
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_EQ(*reply, "ifc rules file=\"katana ifc default rules.csv\" rules=" +
                          std::to_string(ifc::defaultClassificationRules().size()));
    std::ifstream in(file, std::ios::binary);
    const std::string text((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
    const auto back = ifc::parseClassificationRules(text);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    const auto& defaults = ifc::defaultClassificationRules();
    ASSERT_EQ(back->size(), defaults.size());
    for (std::size_t i = 0; i < defaults.size(); ++i) {
        EXPECT_EQ((*back)[i].name, defaults[i].name);
        EXPECT_EQ((*back)[i].target, defaults[i].target);
    }

    const auto nowhere = ifc::writeDefaultRules(
        (std::filesystem::temp_directory_path() / "no such folder" / "r.csv").string());
    ASSERT_FALSE(nowhere.ok());
    EXPECT_EQ(nowhere.error().code, ErrorCode::FileExportFailure);
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

// The reply is records a reader can take back: the head's counts are the
// report's, and the object records are the tally, row for row - the one
// reader File > Export IFC's table is filled from, and what an agent reads.
TEST(IfcReplies, AnExportsObjectRecordsReadBackAsItsTally)
{
    katana::entity::Model model;
    Entity kerb;
    kerb.geometry = Polyline2{{Point2{334000.0, 6250000.0}, Point2{334010.0, 6250000.0}}, false};
    kerb.layer = "Survey/Kerb \"east\"";
    ASSERT_TRUE(model.entities.add(kerb).ok());
    Entity label;
    label.geometry = katana::entity::PointGeometry{Point2{334005.0, 6250002.0}};
    label.layer = "Stormwater/Pits";
    ASSERT_TRUE(model.entities.add(label).ok());
    ifc::ExportArguments arguments;
    arguments.path = "x.ifc";
    arguments.schedule = std::string(KATANA_SAMPLES) + "/utilities/schedule.csv";
    auto files = ifc::readExportFiles(arguments);
    ASSERT_TRUE(files.ok());
    const auto out = ifc::writeIfc({&model, {}, std::move(files->utilities)});
    ASSERT_TRUE(out.ok()) << out.error().describe();

    const std::string reply = ifc::formatExportReply(*out, "site plan.ifc", false);
    const auto lines = katana::core::splitLines(reply);
    ASSERT_FALSE(lines.empty());
    for (const std::string_view line : lines) {
        EXPECT_TRUE(katana::core::readReplyRecord(line)) << line; // every line is a record
    }
    const auto head = katana::core::readReplyRecord(lines.front());
    ASSERT_TRUE(head);
    EXPECT_EQ(head->words, (std::vector<std::string>{"ifc", "exported"}));
    EXPECT_EQ(head->value("file"), "site plan.ifc");
    EXPECT_EQ(head->value("instances"), std::to_string(out->instances));

    const auto objects = ifc::readExportObjects(reply);
    ASSERT_TRUE(objects.ok()) << objects.error().describe();
    EXPECT_EQ(*objects, out->tally);

    const std::string preview = ifc::formatExportReply(*out, "x.ifc", true);
    EXPECT_TRUE(preview.starts_with("ifc previewed file=\"x.ifc\" schema=IFC4X3_ADD2 instances="));
    EXPECT_EQ(preview.find("bytes="), std::string::npos); // nothing was written

    const auto broken = ifc::readExportObjects("ifc exported file=\"x.ifc\"\nobject count=many");
    ASSERT_FALSE(broken.ok());
    EXPECT_EQ(broken.error().code, ErrorCode::ParseFailure);
}

// An import's reply and INFO's description are records too, the counts the
// read's own.
TEST(IfcReplies, AnImportAndADescriptionAreRecordsOfWhatWasRead)
{
    katana::entity::Model model;
    Entity kerb;
    kerb.geometry = Polyline2{{Point2{334000.0, 6250000.0}, Point2{334010.0, 6250000.0}}, false};
    kerb.layer = "Survey/Kerb";
    ASSERT_TRUE(model.entities.add(kerb).ok());
    katana::entity::Alignment alignment;
    alignment.name = "MC 01";
    alignment.horizontal.pis = {{Point2{334000.0, 6250000.0}, 0.0, 0.0, 0.0},
                                {Point2{334100.0, 6250050.0}, 0.0, 0.0, 0.0}};
    ASSERT_TRUE(model.alignments.add(alignment).ok());
    const auto out = ifc::writeIfc({&model, {}, {}});
    ASSERT_TRUE(out.ok());
    const auto read = ifc::readIfc(out->text);
    ASSERT_TRUE(read.ok()) << read.error().describe();

    const std::string reply = ifc::formatImportReply(*read, "x.ifc", read->entities.size());
    const auto head = katana::core::readReplyRecord(katana::core::splitLines(reply).front());
    ASSERT_TRUE(head);
    EXPECT_EQ(head->words, (std::vector<std::string>{"ifc", "imported"}));
    EXPECT_EQ(head->value("entities"), std::to_string(read->entities.size()));
    EXPECT_EQ(head->value("alignments"), "1");
    EXPECT_EQ(head->value("crs"), ""); // none: absent, and said as empty, not as zero

    const std::string described = ifc::formatDescription(*read, "x.ifc");
    bool sawAlignment = false;
    for (const std::string_view line : katana::core::splitLines(described)) {
        const auto record = katana::core::readReplyRecord(line);
        ASSERT_TRUE(record) << line;
        if (record->words == std::vector<std::string>{"alignment"}) {
            sawAlignment = true;
            EXPECT_EQ(record->value("name"), "MC 01");
            EXPECT_EQ(record->value("pis"), "2");
        }
    }
    EXPECT_TRUE(sawAlignment);
    EXPECT_TRUE(described.starts_with("ifc described file=\"x.ifc\""));
}
