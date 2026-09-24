// Storing sheets and editing them undoably (sheet_json.hpp, sheet_commands.hpp,
// Document::sheetSet / setSheetSet).
//
// A sheet set lives in the project's metadata under "sheets" as versioned
// JSON, and its logo in the project's assets/ directory - no schema change.
// Every edit is one undoable step.

#include <gtest/gtest.h>

#include <cmath>
#include <filesystem>
#include <fstream>
#include <limits>
#include <string>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/generators.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/plotting/sheet_json.hpp"

namespace fs = std::filesystem;
using katana::cad::Document;
using katana::cad::PaperSize;
using namespace katana::cad::plotting;
using katana::core::ErrorCode;
using katana::core::Status;

namespace {

// A set with something other than the default in every member, and doubles
// that only survive a round trip written in full: a third, 0.1 + 0.2, a
// value near the bottom of the range, a negative angle.
SheetSet everything()
{
    SheetSet set;
    set.defaults.organisation = "Example Surveys";
    set.defaults.projectLines = {"Main Road Upgrade", "Stage 2", "Lot 7 \xE2\x80\x94 North",
                                 "Detail"};
    set.defaults.client = "Client Pty Ltd";
    set.defaults.locator = {"L. OCATOR", "01/01/26"};
    set.defaults.surveyor = {"J. CITIZEN", "02/01/26"};
    set.defaults.compiler = {"C. OMPILER", ""};
    set.defaults.reviewer = {"R. EVIEWER", "04/01/26"};
    set.defaults.approver = {"A. PPROVER", "05/01/26"};
    set.defaults.notes = "Services are approximate.\nConfirm before digging.";
    set.defaults.heightDatum = "LOCAL";
    set.defaults.coordinateSystem = "GRID 56";
    set.defaults.modelName = "GROUND";
    set.defaults.setNumber = "DS-0001";
    set.defaults.logoAsset = "logo.png";
    set.numbering = "{set}-{n:02}";
    set.revisions = {{"A", "01/02/26", "First issue", "JC"}, {"B", "03/02/26", "Kerbs", "RE"}};

    Sheet sheet;
    sheet.id = "s7";
    sheet.name = "PLAN \xE5\x8C\x97"; // a non-ASCII name
    sheet.paper = PaperSize::A1;
    sheet.landscape = true;
    sheet.frameLegend = false;
    sheet.fields["scale"] = "1:250 @ A1";
    Viewport plan;
    plan.id = "vp9";
    plan.kind = ViewportKind::Plan;
    plan.rect = Box2(Point2(46.0, 70.0), Point2(820.0 / 3.0, 574.0));
    plan.scale = 250.0;
    plan.autoScale = true;
    plan.centre = Point2(0.1 + 0.2, 1e-300);
    plan.autoCentre = true;
    plan.rotation = -0.7853981633974483;
    plan.verticalExaggeration = 2.5;
    plan.tiltDegrees = 45.0;
    plan.source = {"ROAD", 12.5, 250.25, 20.0, {10.0, 20.0}, 15.0};
    plan.hiddenLayers.hide("FRAMES");
    plan.hiddenLayers.hide("TREES");
    plan.title = "PLAN AT THE CORNER";
    plan.northArrow = true;
    plan.scaleBar = true;
    plan.locked = true;
    plan.text = "note";
    plan.marks.push_back({WorldMark::Kind::MatchLine,
                          {Point2(1.0, 2.0), Point2(3.0, 4.0)},
                          "MATCH LINE CH 250.000",
                          "s8"});
    plan.marks.push_back({WorldMark::Kind::SheetOutline, {Point2(5.0, 6.0)}, "", "s1"});
    sheet.viewports.push_back(plan);
    Viewport legend;
    legend.id = "vp10";
    legend.kind = ViewportKind::Legend;
    sheet.viewports.push_back(legend);
    set.sheets.push_back(sheet);
    Sheet portrait;
    portrait.id = "s8";
    portrait.paper = PaperSize::A4;
    portrait.landscape = false;
    portrait.frame.clear();
    set.sheets.push_back(portrait);
    return set;
}

Sheet namedSheet(std::string name)
{
    Sheet sheet;
    sheet.name = std::move(name);
    Viewport plan;
    plan.kind = ViewportKind::Plan;
    sheet.viewports.push_back(plan);
    return sheet;
}

std::vector<std::string> names(const Document& document)
{
    std::vector<std::string> out;
    for (const Sheet& sheet : document.sheetSet().sheets) {
        out.push_back(sheet.name);
    }
    return out;
}

bool hasSheetsKey(const Document& document)
{
    return document.metadata().unknownKeys.contains("sheets");
}

// A directory of the test's own, removed by name: ctest runs cases at the
// same time, and a shared one would be removed from under another.
struct ScratchDirectory {
    fs::path path;
    explicit ScratchDirectory(const std::string& name)
        : path(fs::temp_directory_path() / ("katana-cad-tests-" + name))
    {
        fs::remove_all(path);
        fs::create_directories(path);
    }
    ~ScratchDirectory() { fs::remove_all(path); }
};

void writeFile(const fs::path& path, const std::string& bytes)
{
    std::ofstream out(path, std::ios::binary);
    out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
}

// The eight-byte PNG signature and a little after it: enough for a logo to
// be recognised by its content (nothing here decodes the image).
const std::string kPngBytes = std::string("\x89PNG\r\n\x1a\n", 8) + "not really pixels";

} // namespace

// ---- JSON ----------------------------------------------------------------------------

TEST(SheetStorage, EverySheetSetMemberSurvivesAJsonRoundTripExactly)
{
    const SheetSet set = everything();
    const auto json = sheetSetToJson(set);
    ASSERT_TRUE(json.ok()) << json.error().describe();
    EXPECT_NE(json->find("\"format\":\"katana-sheets\""), std::string::npos);
    EXPECT_NE(json->find("\"version\":1"), std::string::npos);
    const auto back = sheetSetFromJson(*json);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    // Bit for bit: operator== compares every double exactly.
    EXPECT_TRUE(*back == set);
    EXPECT_EQ(back->sheets[0].viewports[0].centre.x, 0.1 + 0.2);
    EXPECT_EQ(back->sheets[0].viewports[0].centre.y, 1e-300);
    // And the text is stable: writing what was read gives the same text.
    EXPECT_EQ(*sheetSetToJson(*back), *json);
}

TEST(SheetStorage, MembersLeftOutReadAsTheirDefaults)
{
    const auto set = sheetSetFromJson(
        R"({"format":"katana-sheets","version":1,"sheets":[{"viewports":[{"kind":"legend"}]}]})");
    ASSERT_TRUE(set.ok()) << set.error().describe();
    ASSERT_EQ(set->sheets.size(), 1u);
    const Sheet& sheet = set->sheets.front();
    EXPECT_EQ(sheet.paper, PaperSize::A3);
    EXPECT_TRUE(sheet.landscape);
    EXPECT_EQ(sheet.frame, kBuiltInFrameId);
    ASSERT_EQ(sheet.viewports.size(), 1u);
    EXPECT_EQ(sheet.viewports[0].kind, ViewportKind::Legend);
    EXPECT_EQ(sheet.viewports[0].scale, 500.0);
    EXPECT_EQ(sheet.viewports[0].verticalExaggeration, 1.0);
    EXPECT_EQ(set->numbering, "{n}");
}

TEST(SheetStorage, ANewerVersionIsRefusedAndNonsenseIsAParseFailure)
{
    EXPECT_EQ(sheetSetFromJson(R"({"format":"katana-sheets","version":2})").error().code,
              ErrorCode::Unsupported);
    EXPECT_EQ(sheetSetFromJson("not json").error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(sheetSetFromJson(R"({"version":1})").error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(sheetSetFromJson(
                  R"({"format":"katana-sheets","version":1,"sheets":[{"viewports":[{"kind":"globe"}]}]})")
                  .error()
                  .code,
              ErrorCode::ParseFailure);
    EXPECT_EQ(sheetSetFromJson(R"({"format":"katana-sheets","version":1,"sheets":[{"paper":"B7"}]})")
                  .error()
                  .code,
              ErrorCode::ParseFailure);
}

TEST(SheetStorage, AnUnplacedViewportRoundTripsWithItsEmptyRectangle)
{
    // A viewport not placed yet has Box2's empty value, made of infinities
    // JSON cannot write; it is the default, so it is left out, and read back
    // as the same. A null written by hand reads as it too.
    SheetSet set;
    set.sheets.push_back(namedSheet("UNPLACED"));
    ASSERT_TRUE(set.sheets[0].viewports[0].rect.empty());
    const auto json = sheetSetToJson(set);
    ASSERT_TRUE(json.ok()) << json.error().describe();
    EXPECT_EQ(json->find("rect"), std::string::npos);
    const auto back = sheetSetFromJson(*json);
    ASSERT_TRUE(back.ok()) << back.error().describe();
    EXPECT_TRUE(*back == set);
    const auto null = sheetSetFromJson(
        R"({"format":"katana-sheets","version":1,"sheets":[{"viewports":[{"rect":null}]}]})");
    ASSERT_TRUE(null.ok()) << null.error().describe();
    EXPECT_TRUE(null->sheets[0].viewports[0].rect == Box2{});
}

TEST(SheetStorage, OnlyWhatDiffersFromTheDefaultsIsWritten)
{
    // One sheet, one viewport, every member at its default but the ids: the
    // format, the version, the sheet's id and the viewport's id and kind -
    // keys in the order the JSON writer sorts them.
    SheetSet set;
    set.sheets.resize(1);
    set.sheets[0].id = "s1";
    set.sheets[0].viewports.resize(1);
    set.sheets[0].viewports[0].id = "vp1";
    EXPECT_EQ(*sheetSetToJson(set), R"({"format":"katana-sheets","sheets":[{"id":"s1",)"
                                    R"("viewports":[{"id":"vp1","kind":"plan"}]}],"version":1})");
    // A value equal to a default in every bit but the sign is not the
    // default: -0.0 is written, and reads back as -0.0.
    set.sheets[0].viewports[0].rotation = -0.0;
    const auto back = sheetSetFromJson(*sheetSetToJson(set));
    ASSERT_TRUE(back.ok());
    EXPECT_TRUE(std::signbit(back->sheets[0].viewports[0].rotation));
    // Nothing at all: the format and the version.
    EXPECT_EQ(*sheetSetToJson(SheetSet{}), R"({"format":"katana-sheets","version":1})");
}

TEST(SheetStorage, TextThatIsNotUtf8OrANumberThatIsNotFiniteCannotBeStored)
{
    SheetSet set;
    set.sheets.push_back(namedSheet("\xff\xfe broken"));
    EXPECT_EQ(sheetSetToJson(set).error().code, ErrorCode::InvalidArgument);
    set.sheets[0].name = "FINE";
    set.sheets[0].viewports[0].centre.x = std::numeric_limits<double>::quiet_NaN();
    EXPECT_EQ(sheetSetToJson(set).error().code, ErrorCode::InvalidArgument);
    set.sheets[0].viewports[0].centre.x = 0.0;
    set.sheets[0].viewports[0].scale = std::numeric_limits<double>::infinity();
    EXPECT_EQ(sheetSetToJson(set).error().code, ErrorCode::InvalidArgument);
}

// ---- Document ------------------------------------------------------------------------

TEST(SheetStorage, SettingTheSheetSetIsOneUndoableStepThatLeavesNoKeyWhenUndone)
{
    Document document;
    EXPECT_TRUE(document.sheetSet().sheets.empty());
    EXPECT_FALSE(hasSheetsKey(document));
    const SheetSet set = everything();
    ASSERT_TRUE(document.setSheetSet(set).ok());
    EXPECT_TRUE(document.sheetSet() == set);
    EXPECT_TRUE(hasSheetsKey(document));
    EXPECT_TRUE(document.isModified());
    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.sheetSet().sheets.empty());
    // The metadata is exactly as it was: no "sheets" key at all.
    EXPECT_FALSE(hasSheetsKey(document));
    EXPECT_FALSE(document.isModified());
    ASSERT_TRUE(document.redo().ok());
    EXPECT_TRUE(document.sheetSet() == set);
}

TEST(SheetStorage, SettingTheSameSetAgainRecordsNoStep)
{
    Document document;
    ASSERT_TRUE(document.setSheetSet(everything()).ok());
    ASSERT_TRUE(document.setSheetSet(everything()).ok());
    ASSERT_TRUE(document.undo().ok());
    EXPECT_FALSE(document.history().canUndo()); // one step, not two
}

TEST(SheetStorage, AddRemoveMoveAndDuplicateAreEachOneStep)
{
    Document document;
    ASSERT_TRUE(addSheet(document, namedSheet("A")).ok());
    ASSERT_TRUE(addSheet(document, namedSheet("C")).ok());
    ASSERT_TRUE(addSheet(document, namedSheet("B"), 1).ok());
    EXPECT_EQ(names(document), (std::vector<std::string>{"A", "B", "C"}));
    // Each sheet and viewport got an id of its own.
    EXPECT_EQ(document.sheetSet().sheets[0].id, "s1");
    EXPECT_EQ(document.sheetSet().sheets[1].id, "s3");
    EXPECT_EQ(document.sheetSet().sheets[2].id, "s2");
    EXPECT_EQ(document.sheetSet().sheets[1].viewports[0].id, "vp3");

    ASSERT_TRUE(moveSheet(document, 0, 2).ok());
    EXPECT_EQ(names(document), (std::vector<std::string>{"B", "C", "A"}));
    ASSERT_TRUE(duplicateSheet(document, 1).ok());
    EXPECT_EQ(names(document), (std::vector<std::string>{"B", "C", "C (copy)", "A"}));
    const Sheet& copy = document.sheetSet().sheets[2];
    EXPECT_EQ(copy.id, "s4");
    EXPECT_EQ(copy.viewports[0].id, "vp4");
    ASSERT_TRUE(removeSheet(document, 0).ok());
    EXPECT_EQ(names(document), (std::vector<std::string>{"C", "C (copy)", "A"}));

    // Undo walks back one edit at a time.
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(names(document), (std::vector<std::string>{"B", "C", "C (copy)", "A"}));
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(names(document), (std::vector<std::string>{"B", "C", "A"}));
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(names(document), (std::vector<std::string>{"A", "B", "C"}));
}

TEST(SheetStorage, TheLastSheetCanBeRemovedAndAnEmptySetIsValid)
{
    Document document;
    ASSERT_TRUE(addSheet(document, namedSheet("ONLY")).ok());
    ASSERT_TRUE(removeSheet(document, 0).ok());
    EXPECT_TRUE(document.sheetSet().sheets.empty());
    EXPECT_TRUE(document.sheetSetStatus().ok());
    EXPECT_FALSE(hasSheetsKey(document)); // nothing left to store
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(names(document), (std::vector<std::string>{"ONLY"}));
}

TEST(SheetStorage, AnEditOutOfRangeOrRefusedChangesNothing)
{
    Document document;
    ASSERT_TRUE(addSheet(document, namedSheet("A")).ok());
    const SheetSet before = document.sheetSet();
    EXPECT_EQ(removeSheet(document, 1).error().code, ErrorCode::NotFound);
    EXPECT_EQ(moveSheet(document, 0, 1).error().code, ErrorCode::NotFound);
    EXPECT_EQ(duplicateSheet(document, 3).error().code, ErrorCode::NotFound);
    EXPECT_EQ(addSheet(document, namedSheet("B"), 5).error().code, ErrorCode::NotFound);
    EXPECT_EQ(editViewport(document, "vp99", [](Viewport&) -> Status { return {}; })
                  .error()
                  .code,
              ErrorCode::NotFound);
    const Status refused = editViewport(document, "vp1", [](Viewport& viewport) -> Status {
        viewport.scale = 1.0; // changed, then refused: the change must not stick
        return katana::core::makeError(ErrorCode::InvalidArgument, "no");
    });
    EXPECT_EQ(refused.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(document.sheetSet() == before);
    // Only the one add is on the history.
    ASSERT_TRUE(document.undo().ok());
    EXPECT_FALSE(document.history().canUndo());
}

TEST(SheetStorage, EditingAViewportFindsItByIdOnAnySheet)
{
    Document document;
    ASSERT_TRUE(addSheet(document, namedSheet("A")).ok());
    ASSERT_TRUE(addSheet(document, namedSheet("B")).ok());
    ASSERT_TRUE(editViewport(document, "vp2", [](Viewport& viewport) -> Status {
                    viewport.scale = 250.0;
                    viewport.hiddenLayers.hide("TREES");
                    return {};
                }).ok());
    EXPECT_EQ(document.sheetSet().sheets[1].viewports[0].scale, 250.0);
    EXPECT_TRUE(document.sheetSet().sheets[1].viewports[0].hiddenLayers.hides("TREES"));
    EXPECT_EQ(document.sheetSet().sheets[0].viewports[0].scale, 500.0);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.sheetSet().sheets[1].viewports[0].scale, 500.0);
}

TEST(SheetStorage, GeneratedSheetsJoinASetInOneStepWithTheirReferencesIntact)
{
    Document document;
    ASSERT_TRUE(addSheet(document, namedSheet("COVER")).ok());
    GridRequest grid;
    grid.area = Box2(Point2(0.0, 0.0), Point2(400.0, 200.0));
    grid.scale = 500.0;
    auto tiles = gridSheets(grid);
    ASSERT_TRUE(tiles.ok());
    ASSERT_TRUE(addSheets(document, std::move(*tiles)).ok());
    const SheetSet& set = document.sheetSet();
    ASSERT_EQ(set.sheets.size(), 8u); // the cover, a key plan, six tiles
    // The key plan's first outline is the first tile, now sheet 3.
    EXPECT_EQ(markLabel(set, set.sheets[1].viewports[0].marks[0]), "3");
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.sheetSet().sheets.size(), 1u);
}

TEST(SheetStorage, SheetsWrittenByANewerVersionAreKeptAndNotOverwritten)
{
    Document document;
    auto metadata = document.metadata();
    const std::string newer = R"({"format":"katana-sheets","version":99})";
    metadata.unknownKeys["sheets"] = newer;
    document.setMetadata(metadata);
    EXPECT_TRUE(document.sheetSet().sheets.empty());
    EXPECT_EQ(document.sheetSetStatus().error().code, ErrorCode::Unsupported);
    EXPECT_EQ(addSheet(document, namedSheet("A")).error().code, ErrorCode::CommandRejected);
    EXPECT_EQ(document.metadata().unknownKeys.at("sheets"), newer);
}

TEST(SheetStorage, TheSheetSetIsReadAgainWhenTheMetadataIsReplaced)
{
    Document document;
    ASSERT_TRUE(addSheet(document, namedSheet("A")).ok());
    ASSERT_EQ(document.sheetSet().sheets.size(), 1u);
    auto metadata = document.metadata();
    metadata.unknownKeys.erase("sheets");
    document.setMetadata(metadata);
    EXPECT_TRUE(document.sheetSet().sheets.empty());
}

TEST(SheetStorage, SheetsSurviveASaveAndReopenWithNoSchemaChange)
{
    const ScratchDirectory scratch("sheets-save");
    const fs::path project = scratch.path / "sheets.katana";
    const SheetSet set = everything();
    {
        Document document;
        ASSERT_TRUE(document.setSheetSet(set).ok());
        const Status saved = document.saveAs(project);
        ASSERT_TRUE(saved.ok()) << saved.error().describe();
        EXPECT_FALSE(document.isModified());
    }
    Document reopened;
    const Status opened = reopened.open(project);
    ASSERT_TRUE(opened.ok()) << opened.error().describe();
    EXPECT_TRUE(reopened.sheetSetStatus().ok());
    EXPECT_TRUE(reopened.sheetSet() == set);
    EXPECT_FALSE(reopened.isModified());
}

// ---- The logo ------------------------------------------------------------------------

TEST(SheetStorage, ALogoIsCopiedIntoTheProjectsAssetsAndItsNameIsOneUndoableStep)
{
    const ScratchDirectory scratch("sheets-logo");
    const fs::path project = scratch.path / "logo.katana";
    const fs::path source = scratch.path / "Our Logo!.png";
    writeFile(source, kPngBytes);
    Document document;
    // Nowhere to keep it until the drawing is a project.
    EXPECT_EQ(importLogo(document, source).error().code, ErrorCode::InvalidState);
    ASSERT_TRUE(document.saveAs(project).ok());

    const auto name = importLogo(document, source);
    ASSERT_TRUE(name.ok()) << name.error().describe();
    // The stem made safe for any file system; the extension from the content.
    EXPECT_EQ(*name, "Our_Logo_.png");
    EXPECT_EQ(document.sheetSet().defaults.logoAsset, "Our_Logo_.png");
    ASSERT_TRUE(logoPath(document).has_value());
    EXPECT_EQ(*logoPath(document), project / "assets" / "Our_Logo_.png");
    EXPECT_EQ(fs::file_size(*logoPath(document)), kPngBytes.size());

    // The same image again is found, not copied twice; a different image of
    // the same name gets a name of its own rather than replacing it.
    EXPECT_EQ(*importLogo(document, source), "Our_Logo_.png");
    writeFile(source, kPngBytes + "changed");
    EXPECT_EQ(*importLogo(document, source), "Our_Logo_-2.png");
    EXPECT_TRUE(fs::exists(project / "assets" / "Our_Logo_.png"));

    // Undo takes the name back; the file stays for a redo.
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.sheetSet().defaults.logoAsset, "Our_Logo_.png");
    ASSERT_TRUE(document.undo().ok());
    EXPECT_TRUE(document.sheetSet().defaults.logoAsset.empty());
    EXPECT_FALSE(logoPath(document).has_value());
    EXPECT_TRUE(fs::exists(project / "assets" / "Our_Logo_-2.png"));
}

TEST(SheetStorage, ALogoMustBeASmallImageOfAKnownKind)
{
    const ScratchDirectory scratch("sheets-logo-kinds");
    const fs::path project = scratch.path / "kinds.katana";
    Document document;
    ASSERT_TRUE(document.saveAs(project).ok());
    // Recognised by content whatever the extension says.
    writeFile(scratch.path / "a.dat", "\xff\xd8\xff\xe0 jpeg");
    EXPECT_EQ(*importLogo(document, scratch.path / "a.dat"), "a.jpg");
    writeFile(scratch.path / "b.img", "GIF89a....");
    EXPECT_EQ(*importLogo(document, scratch.path / "b.img"), "b.gif");
    writeFile(scratch.path / "c.img", "BM......");
    EXPECT_EQ(*importLogo(document, scratch.path / "c.img"), "c.bmp");
    // Not an image.
    writeFile(scratch.path / "d.png", "<svg/>");
    EXPECT_EQ(importLogo(document, scratch.path / "d.png").error().code, ErrorCode::Unsupported);
    // Over the cap: 4 MiB and one byte.
    const fs::path large = scratch.path / "large.png";
    writeFile(large, kPngBytes);
    fs::resize_file(large, kMaximumLogoBytes + 1);
    EXPECT_EQ(importLogo(document, large).error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(importLogo(document, scratch.path / "missing.png").error().code,
              ErrorCode::NotFound);
    EXPECT_EQ(document.sheetSet().defaults.logoAsset, "c.bmp"); // the failures changed nothing
}

// ---- Field context -------------------------------------------------------------------

TEST(SheetStorage, TheProjectSuppliesTheFieldContextAndDatesPrintDayMonthYear)
{
    const ScratchDirectory scratch("sheets-context");
    Document document;
    auto metadata = document.metadata();
    metadata.description = "Detail survey";
    metadata.coordinateSystem = "EPSG:7856";
    document.setMetadata(metadata);
    ASSERT_TRUE(document.saveAs(scratch.path / "main_road.katana").ok());
    const FieldContext context = fieldContextFor(document, "24/09/26");
    EXPECT_EQ(context.projectName, "main_road"); // a project is named after its directory
    EXPECT_EQ(context.projectDescription, "Detail survey");
    EXPECT_EQ(context.coordinateSystem, "EPSG:7856");
    EXPECT_EQ(context.fileName, "main_road.katana");
    EXPECT_EQ(context.plotDate, "24/09/26");
    using namespace std::chrono;
    EXPECT_EQ(frameDate(year{2026} / September / day{24}), "24/09/26");
    EXPECT_EQ(frameDate(year{2031} / January / day{5}), "05/01/31");
}
