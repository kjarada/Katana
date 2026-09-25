// The sheet verbs (sheet_verbs.hpp): the sheet system driven from the text
// command line, as a person, a script, katana_cli and an AI agent drive it.
//
// Every edit is ONE undoable step and a refused one changes nothing; a reply
// is one fact per line in the key=value form the options take; errors say
// what was refused. The lines go through CommandInterpreter::run, so the
// dispatch and the tokenizer are exercised too.

#include <gtest/gtest.h>

#include <cmath>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/plotting/generators.hpp"
#include "katana/cad/plotting/layout.hpp"
#include "katana/cad/plotting/sheet_commands.hpp"
#include "katana/cad/plotting/sheet_json.hpp"
#include "katana/cad/plotting/sheet_verbs.hpp"
#include "katana/cad/selection.hpp"
#include "katana/math/numerics.hpp"

namespace fs = std::filesystem;
using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::PaperSize;
using namespace katana::cad::plotting;
using katana::core::ErrorCode;

namespace {

// A document and the interpreter that types into it.
struct Session {
    Document document;
    CommandInterpreter interpreter{document};

    // The reply to `line`; the test fails, naming the line, when it is refused.
    std::string ok(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << "\n  -> "
                                << (reply.ok() ? std::string{} : reply.error().describe());
        return reply.ok() ? *reply : std::string{};
    }
    // The error `line` is refused with; the test fails when it is not.
    katana::core::Error refused(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " was accepted:\n" << (reply.ok() ? *reply : "");
        return reply.ok() ? katana::core::Error{} : reply.error();
    }
    std::size_t steps() const { return document.history().undoCount(); }
    const SheetSet& set() const { return document.sheetSet(); }
    std::string json() const { return sheetSetToJson(document.sheetSet()).value(); }
};

// A directory of the test's own, removed by name: ctest runs cases at the
// same time, and a shared one would be removed from under another.
struct ScratchDirectory {
    fs::path path;
    explicit ScratchDirectory(const std::string& name)
        : path(fs::temp_directory_path() / ("katana-sheet-verbs-" + name))
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

std::string readFile(const fs::path& path)
{
    std::ifstream in(path, std::ios::binary);
    return std::string((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
}

// The eight-byte PNG signature and a little after it: recognised by its
// content, never decoded.
const std::string kPngBytes = std::string("\x89PNG\r\n\x1a\n", 8) + "not really pixels";

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

std::vector<std::string> lines(const std::string& text)
{
    std::vector<std::string> out;
    std::size_t start = 0;
    while (start <= text.size()) {
        const std::size_t end = text.find('\n', start);
        out.push_back(text.substr(start, end == std::string::npos ? std::string::npos : end - start));
        if (end == std::string::npos) {
            break;
        }
        start = end + 1;
    }
    return out;
}

} // namespace

// ---- the command line ----------------------------------------------------------------

TEST(SheetVerbs, TheInterpreterHandsSheetVerbsOnAndListsThemInItsHelp)
{
    for (const char* verb : {"SHEETS", "sheet", "View", "VIEWPORT", "tile", "GENERATE",
                             "titleblock"}) {
        EXPECT_TRUE(isSheetVerb(verb)) << verb;
    }
    EXPECT_FALSE(isSheetVerb("LINE"));
    EXPECT_FALSE(isSheetVerb("PLOTSHEETS")); // the desktop window's own
    const std::string help = CommandInterpreter::helpText();
    EXPECT_TRUE(contains(help, "Sheets    SHEETS [LIST]"));
    EXPECT_TRUE(contains(help, "PLOTSHEETS path.pdf"));

    Session session;
    const std::string sheetHelp = session.ok("HELP SHEETS");
    EXPECT_EQ(sheetHelp, sheetVerbHelp());
    EXPECT_EQ(session.ok("help generate"), sheetVerbHelp());
    for (const char* line : {"SHEETS [LIST]", "SHEETS JSON [path]", "SHEET NEW", "SHEET REMOVE n",
                             "SHEET SET n", "SHEET FIELD n", "VIEW ADD n kind", "VIEW SET id",
                             "TILE n preset", "GENERATE kind", "TITLEBLOCK [LIST]",
                             "TITLEBLOCK REVISION ADD", "TITLEBLOCK LOGO path", "PLOTSHEETS"}) {
        EXPECT_TRUE(contains(sheetHelp, line)) << line;
    }
}

TEST(SheetVerbs, TheTokenizerIsTheInterpretersOwn)
{
    const auto words = CommandInterpreter::tokenize(R"(PLOTSHEETS "C:/my plots/set.pdf" sheets=1,3-5 "")");
    ASSERT_TRUE(words.ok());
    EXPECT_EQ(*words, (std::vector<std::string>{"PLOTSHEETS", "C:/my plots/set.pdf", "sheets=1,3-5",
                                                ""}));
    const auto open = CommandInterpreter::tokenize(R"(SHEET RENAME 1 "never closed)");
    ASSERT_FALSE(open.ok());
    EXPECT_EQ(open.error().code, ErrorCode::ParseFailure);
}

// ---- SHEET ---------------------------------------------------------------------------

TEST(SheetVerbs, ANewSheetIsOneStepTakingItsPaperOrientationAndPlace)
{
    Session s;
    EXPECT_EQ(s.ok(R"(SHEET NEW "GENERAL ARRANGEMENT" paper=A1 portrait)"),
              R"(added sheet 1 id=s1 name="GENERAL ARRANGEMENT" paper=A1 orientation=portrait )"
              R"(frame=a3_landscape legendblock=on views=0)");
    EXPECT_EQ(s.steps(), 1u);
    EXPECT_EQ(s.document.history().undoName(), "ADD_SHEET");
    // Unquoted words are the name too; no name at all numbers the sheet.
    EXPECT_EQ(s.ok("sheet new DRAINAGE PLAN frame=off legendblock=off"),
              R"(added sheet 2 id=s2 name="DRAINAGE PLAN" paper=A3 orientation=landscape )"
              R"(frame=none legendblock=off views=0)");
    EXPECT_EQ(s.ok("SHEET NEW at=1"),
              R"(added sheet 1 id=s3 name="SHEET 3" paper=A3 orientation=landscape )"
              R"(frame=a3_landscape legendblock=on views=0)");
    EXPECT_EQ(s.ok("SHEET NEW name=PORTRAIT orientation=landscape at=4"),
              R"(added sheet 4 id=s4 name="PORTRAIT" paper=A3 orientation=landscape )"
              R"(frame=a3_landscape legendblock=on views=0)");
    EXPECT_EQ(s.steps(), 4u);
    ASSERT_EQ(s.set().sheets.size(), 4u);
    EXPECT_EQ(s.set().sheets[1].paper, PaperSize::A1);
    EXPECT_FALSE(s.set().sheets[1].landscape);
    EXPECT_TRUE(s.set().sheets[2].frame.empty());

    // A refusal says why and records nothing.
    EXPECT_TRUE(contains(s.refused("SHEET NEW at=6").message, "at= is 1 to 5"));
    EXPECT_TRUE(contains(s.refused("SHEET NEW paper=B2").message, "A0, A1, A2, A3 or A4"));
    EXPECT_TRUE(contains(s.refused("SHEET NEW PLAN name=PLAN").message, "given twice"));
    EXPECT_TRUE(contains(s.refused("SHEET NEW colour=red").message, "no option colour="));
    EXPECT_EQ(s.steps(), 4u);
    ASSERT_TRUE(s.document.undo().ok());
    EXPECT_EQ(s.set().sheets.size(), 3u);
}

TEST(SheetVerbs, ASheetIsNamedByItsNumberOrItsId)
{
    Session s;
    s.ok("SHEET NEW A");
    s.ok("SHEET NEW B");
    s.ok("SHEET NEW C");
    s.ok("SHEET MOVE 3 1"); // C A B: ids s3 s1 s2
    EXPECT_EQ(sheetIndexFrom(s.set(), "1").value(), 0u);
    EXPECT_EQ(sheetIndexFrom(s.set(), "s3").value(), 0u);
    EXPECT_EQ(sheetIndexFrom(s.set(), "S2").value(), 2u);
    const auto zero = sheetIndexFrom(s.set(), "0");
    ASSERT_FALSE(zero.ok());
    EXPECT_EQ(zero.error().code, ErrorCode::NotFound);
    EXPECT_EQ(zero.error().message, "no sheet 0: the set has 3 sheets");
    EXPECT_EQ(sheetIndexFrom(s.set(), "4").error().message, "no sheet 4: the set has 3 sheets");
    EXPECT_EQ(sheetIndexFrom(s.set(), "s9").error().code, ErrorCode::NotFound);
    EXPECT_EQ(sheetIndexFrom(s.set(), "plan").error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(s.refused("SHEET REMOVE 7").message, "no sheet 7: the set has 3 sheets");
}

TEST(SheetVerbs, RemoveMoveCopyAndRenameAreEachOneStep)
{
    Session s;
    s.ok("SHEET NEW A");
    s.ok("SHEET NEW B");
    s.ok("SHEET NEW C");
    std::size_t steps = s.steps();

    EXPECT_EQ(s.ok("SHEET MOVE 1 3"), "moved sheet id=s1 from 1 to 3");
    EXPECT_EQ(s.steps(), ++steps);
    EXPECT_EQ(s.document.history().undoName(), "MOVE_SHEET");

    EXPECT_EQ(s.ok("SHEET COPY s2"),
              R"x(copied sheet 1 to sheet 2 id=s4 name="B (copy)" paper=A3 orientation=landscape )x"
              R"(frame=a3_landscape legendblock=on views=0)");
    EXPECT_EQ(s.steps(), ++steps);

    EXPECT_EQ(s.ok("SHEET RENAME 2 LONG SECTION CH 0 TO 500"),
              R"(renamed sheet 2 id=s4 name="LONG SECTION CH 0 TO 500" paper=A3 )"
              R"(orientation=landscape frame=a3_landscape legendblock=on views=0)");
    EXPECT_EQ(s.steps(), ++steps);
    EXPECT_EQ(s.document.history().undoName(), "RENAME_SHEET");

    EXPECT_EQ(s.ok("SHEET REMOVE s3"), R"(removed sheet 3 id=s3 name="C")");
    EXPECT_EQ(s.steps(), ++steps);

    EXPECT_EQ(s.refused(R"(SHEET RENAME 1 "")").message, "a sheet needs a name");
    EXPECT_EQ(s.refused("SHEET MOVE 1 9").message, "no position 9: the set has 3 sheets");
    EXPECT_TRUE(contains(s.refused("SHEET MOVE 1").message, "usage: SHEET MOVE n to"));
    EXPECT_TRUE(contains(s.refused("SHEET SPIN 1").message, "usage: SHEET NEW"));
    EXPECT_EQ(s.steps(), steps);

    // Undo walks each back exactly.
    ASSERT_TRUE(s.document.undo().ok());
    ASSERT_TRUE(s.document.undo().ok());
    ASSERT_TRUE(s.document.undo().ok());
    ASSERT_TRUE(s.document.undo().ok());
    ASSERT_EQ(s.set().sheets.size(), 3u);
    EXPECT_EQ(s.set().sheets[0].name, "A");
}

TEST(SheetVerbs, SheetSetChangesThePaperAndFrameInOneStepOrNothing)
{
    Session s;
    s.ok("SHEET NEW PLAN");
    const std::string before = s.json();
    const std::size_t steps = s.steps();
    EXPECT_EQ(s.ok("SHEET SET 1 paper=a1 portrait frame=off legendblock=off name=SITE"),
              R"(set sheet 1 id=s1 name="SITE" paper=A1 orientation=portrait frame=none )"
              R"(legendblock=off views=0)");
    EXPECT_EQ(s.steps(), steps + 1);
    EXPECT_EQ(s.document.history().undoName(), "EDIT_SHEET");
    EXPECT_EQ(s.ok("SHEET SET 1 orientation=LANDSCAPE frame=on"),
              R"(set sheet 1 id=s1 name="SITE" paper=A1 orientation=landscape )"
              R"(frame=a3_landscape legendblock=off views=0)");

    // One bad option refuses the lot: the good one before it is not kept.
    const std::string good = s.json();
    EXPECT_TRUE(contains(s.refused("SHEET SET 1 paper=A2 frame=title_block_b").message,
                         "no plot frame of that name"));
    EXPECT_TRUE(contains(s.refused("SHEET SET 1 legendblock=maybe").message, "on or off"));
    EXPECT_TRUE(contains(s.refused("SHEET SET 1 sideways").message, "option=value"));
    EXPECT_EQ(s.json(), good);
    EXPECT_EQ(s.steps(), steps + 2);

    ASSERT_TRUE(s.document.undo().ok());
    ASSERT_TRUE(s.document.undo().ok());
    EXPECT_EQ(s.json(), before);
}

TEST(SheetVerbs, ASheetsOwnFieldOverridesTheSetAndAnEmptyValueClearsIt)
{
    Session s;
    s.ok("SHEET NEW PLAN");
    EXPECT_EQ(s.ok("SHEET FIELD 1 sheet_number"), R"(sheet 1 field sheet_number="1" automatic)");
    EXPECT_EQ(s.ok(R"(SHEET FIELD 1 "Sheet Number" "C-101")"), R"(sheet 1 field sheet_number="C-101")");
    EXPECT_EQ(s.document.history().undoName(), "SET_SHEET_FIELD");
    EXPECT_EQ(s.set().sheets[0].fields.at("sheet_number"), "C-101");
    EXPECT_EQ(s.ok("SHEET FIELD 1 sheet_number"), R"(sheet 1 field sheet_number="C-101")");
    // SHEETS LIST shows the override under its sheet.
    EXPECT_TRUE(contains(s.ok("SHEETS"), "\n  field sheet_number=\"C-101\""));
    // Unquoted words are the value.
    EXPECT_EQ(s.ok("SHEET FIELD 1 scale AS SHOWN"), R"(sheet 1 field scale="AS SHOWN")");
    EXPECT_EQ(s.ok(R"(SHEET FIELD 1 sheet_number "")"), "sheet 1 field sheet_number cleared");
    EXPECT_FALSE(s.set().sheets[0].fields.contains("sheet_number"));

    const auto unknown = s.refused("SHEET FIELD 1 colour red");
    EXPECT_EQ(unknown.code, ErrorCode::NotFound);
    EXPECT_TRUE(contains(unknown.message, "the frame prints no field of that name"));
    EXPECT_TRUE(contains(unknown.message, "sheet_number"));
}

// ---- VIEW ----------------------------------------------------------------------------

TEST(SheetVerbs, AViewIsAddedAsTheEditorAddsOneThenGivenItsOptions)
{
    Session s;
    s.ok("SHEET NEW PLAN");
    // On an empty sheet a plan fills the tiling area, scaled and centred
    // automatically, with a north arrow and a scale bar.
    EXPECT_EQ(s.ok("VIEW ADD 1 plan"),
              "added view vp1 to sheet 1\n"
              "view id=vp1 kind=plan rect=24,36,409,286 scale=auto centre=auto rotation=0 "
              "north=on scalebar=on locked=off");
    EXPECT_EQ(s.document.history().undoName(), "ADD_VIEWPORT");
    // Beside it, a legend of its own size in the middle, and a key plan at
    // a place and scale given.
    EXPECT_EQ(s.ok("VIEW ADD s1 legend"),
              "added view vp2 to sheet 1\n"
              "view id=vp2 kind=legend rect=176.5,111,256.5,211 locked=off");
    EXPECT_EQ(s.ok(R"(VIEW ADD 1 keyplan rect=300,200,400,280 scale=1:5000 centre=150,50 title="KEY PLAN")"),
              "added view vp3 to sheet 1\n"
              "view id=vp3 kind=key_plan rect=300,200,400,280 scale=5000 centre=150,50 rotation=0 "
              "north=on scalebar=off locked=off title=\"KEY PLAN\"");
    EXPECT_EQ(s.ok(R"(VIEW ADD 1 notes text="1. LEVELS ARE IN METRES.\n2. DO NOT SCALE.")"),
              "added view vp4 to sheet 1\n"
              "view id=vp4 kind=notes rect=171.5,121,261.5,201 locked=off "
              "text=\"1. LEVELS ARE IN METRES.\\n2. DO NOT SCALE.\"");
    EXPECT_EQ(s.set().sheets[0].viewports[3].text, "1. LEVELS ARE IN METRES.\n2. DO NOT SCALE.");
    EXPECT_EQ(s.steps(), 5u);

    const std::size_t steps = s.steps();
    EXPECT_TRUE(contains(s.refused("VIEW ADD 1 hologram").message, "a view is plan"));
    EXPECT_EQ(s.refused("VIEW ADD 1 legend scale=500").message,
              "scale= is for plan, key_plan, long_section and cross_sections views, not legend");
    EXPECT_TRUE(contains(s.refused("VIEW ADD 1 plan rect=500,400,600,500").message,
                         "would be off the paper: A3 landscape is 420 x 297 mm"));
    EXPECT_EQ(s.refused("VIEW ADD 1 plan rect=10,10,10,50").message,
              "rect= must have a width and a height");
    EXPECT_TRUE(contains(s.refused("VIEW ADD 1 image").message, "an image view needs file=path"));
    EXPECT_TRUE(contains(s.refused("VIEW ADD 1 plan north").message, "expected option=value"));
    EXPECT_EQ(s.refused("VIEW ADD 9 plan").code, ErrorCode::NotFound);
    EXPECT_EQ(s.steps(), steps);
}

TEST(SheetVerbs, SectionViewsRunAlongTheDrawingsAlignment)
{
    Session s;
    s.ok("ALIGN NEW ROAD 0,0 1000,0");
    s.ok("ALIGN NEW CREEK 0,0 0,400");
    s.ok("SHEET NEW SECTIONS");
    // The first alignment by name, cut half way along.
    EXPECT_EQ(s.ok("VIEW ADD 1 cross_sections"),
              "added view vp1 to sheet 1\n"
              "view id=vp1 kind=cross_sections rect=24,36,409,286 scale=200 centre=auto "
              "alignment=\"CREEK\" from=0 to=0 interval=0 stations=200 halfwidth=20 ve=1 "
              "locked=off");
    // Moved to another alignment it is cut half way along that one.
    EXPECT_TRUE(contains(s.ok("VIEW ADD 1 xs alignment=ROAD rect=30,40,200,150"),
                         "alignment=\"ROAD\" from=0 to=0 interval=0 stations=500 halfwidth=20"));
    EXPECT_TRUE(contains(s.ok("VIEW ADD 1 profile alignment=ROAD from=100 to=600 ve=10 rect=30,160,400,280"),
                         "kind=long_section rect=30,160,400,280 scale=500 centre=auto "
                         "alignment=\"ROAD\" from=100 to=600 ve=10"));
    EXPECT_EQ(s.refused("VIEW ADD 1 xs alignment=RIVER").message,
              "the drawing has no alignment of that name");
    EXPECT_EQ(s.refused("VIEW ADD 1 xs halfwidth=0").message, "halfwidth= must be more than 0");
    EXPECT_EQ(s.refused("VIEW ADD 1 plan stations=10,20").message,
              "stations= is for cross_sections views, not plan");
    EXPECT_EQ(s.refused("VIEW ADD 1 xs stations=10,x").message,
              "stations= must be numbers separated by commas");
}

TEST(SheetVerbs, ViewSetChangesAViewInOneStepAndARefusalChangesNothing)
{
    Session s;
    s.ok("SHEET NEW PLAN");
    s.ok("VIEW ADD 1 plan");
    std::size_t steps = s.steps();
    EXPECT_EQ(s.ok("VIEW SET vp1 scale=1:250 centre=1000.5,2000.25 rotation=30 north=off"),
              "view id=vp1 kind=plan rect=24,36,409,286 scale=250 centre=1000.5,2000.25 "
              "rotation=30 north=off scalebar=on locked=off");
    EXPECT_EQ(s.steps(), ++steps);
    EXPECT_EQ(s.document.history().undoName(), "EDIT_VIEWPORT");
    const Viewport& plan = s.set().sheets[0].viewports[0];
    EXPECT_FALSE(plan.autoScale);
    EXPECT_FALSE(plan.autoCentre);
    EXPECT_DOUBLE_EQ(plan.scale, 250.0);
    EXPECT_DOUBLE_EQ(plan.rotation, 30.0 * katana::math::kDegToRad);

    EXPECT_TRUE(contains(s.ok("view set VP1 SCALE=auto Centre=AUTO locked=yes hide=TREES hide=FENCES"),
                         "scale=auto centre=auto rotation=30 north=off scalebar=on locked=on "
                         "hidden=\"FENCES,TREES\""));
    EXPECT_TRUE(contains(s.ok("VIEW SET vp1 show=TREES title=\"SITE PLAN\""),
                         "hidden=\"FENCES\" title=\"SITE PLAN\""));
    EXPECT_TRUE(contains(s.ok("VIEW SET vp1 hidden= rect=30,40,300,280"),
                         "rect=30,40,300,280 scale=auto"));
    EXPECT_TRUE(s.set().sheets[0].viewports[0].hiddenLayers.empty());
    steps += 3;
    EXPECT_EQ(s.steps(), steps);

    // All or nothing: the good scale before the bad tilt is not kept.
    const std::string before = s.json();
    EXPECT_EQ(s.refused("VIEW SET vp1 scale=100 tilt=45").message,
              "tilt= is for model_3d views, not plan");
    EXPECT_EQ(s.refused("VIEW SET vp1 kind=legend").message,
              "a view's kind is fixed: VIEW REMOVE it and VIEW ADD another");
    EXPECT_TRUE(contains(s.refused("VIEW SET vp1 zoom=2").message, "no view option of that name"));
    EXPECT_EQ(s.refused("VIEW SET vp1 scale=-5").message, "scale= must be more than 0");
    EXPECT_EQ(s.refused("VIEW SET vp1 centre=5").message, "centre= is x,y");
    EXPECT_EQ(s.refused("VIEW SET vp9 scale=100").message,
              "no view with that id; VIEW LIST shows them");
    EXPECT_TRUE(contains(s.refused("VIEW SET vp1").message, "usage: VIEW SET id"));
    EXPECT_EQ(s.json(), before);
    EXPECT_EQ(s.steps(), steps);

    // A 3D snapshot has a camera height and a turn, and nothing drawn to scale.
    s.ok("VIEW ADD 1 3d rect=300,40,400,120");
    EXPECT_TRUE(contains(s.ok("VIEW SET vp2 tilt=45 rotation=-90"),
                         "kind=model_3d rect=300,40,400,120 rotation=-90 tilt=45 locked=off"));
    EXPECT_EQ(s.refused("VIEW SET vp2 tilt=90").message,
              "tilt= is the camera's height angle, 1 to 89 degrees");
}

TEST(SheetVerbs, ViewListAndRemove)
{
    Session s;
    s.ok("SHEET NEW PLAN");
    s.ok("SHEET NEW NOTES");
    s.ok("VIEW ADD 1 plan");
    s.ok("VIEW ADD 2 notes");
    s.ok("VIEW ADD 2 legend rect=30,40,100,120");
    EXPECT_EQ(s.ok("VIEW LIST 2"),
              "sheet 2 id=s2 views=2\n"
              "view id=vp2 kind=notes rect=24,36,409,286 locked=off "
              "text=\"1. ALL DIMENSIONS ARE IN METRES UNLESS NOTED OTHERWISE.\"\n"
              "view id=vp3 kind=legend rect=30,40,100,120 locked=off");
    EXPECT_EQ(lines(s.ok("VIEW LIST")).size(), 5u);
    const std::size_t steps = s.steps();
    EXPECT_EQ(s.ok("VIEW REMOVE vp2"), "removed view vp2 from sheet 2");
    EXPECT_EQ(s.steps(), steps + 1);
    EXPECT_EQ(s.document.history().undoName(), "REMOVE_VIEWPORT");
    EXPECT_EQ(s.set().sheets[1].viewports.size(), 1u);
    EXPECT_EQ(s.refused("VIEW REMOVE vp2").code, ErrorCode::NotFound);
    ASSERT_TRUE(s.document.undo().ok());
    EXPECT_EQ(s.set().sheets[1].viewports.size(), 2u);
    // An id in another case is the same view, and the reply names it as stored.
    EXPECT_EQ(s.ok("view remove VP3"), "removed view vp3 from sheet 2");
    EXPECT_EQ(s.steps(), steps + 1);
    ASSERT_EQ(s.set().sheets[1].viewports.size(), 1u);
    EXPECT_EQ(s.set().sheets[1].viewports[0].id, "vp2");
    EXPECT_EQ(Session().ok("VIEW LIST"), "0 sheets");
}

TEST(SheetVerbs, AnImageViewCopiesItsFileIntoTheProject)
{
    const ScratchDirectory scratch("image");
    const fs::path image = scratch.path / "site photo.png";
    writeFile(image, kPngBytes);
    Session s;
    s.ok("SHEET NEW PLAN");
    const std::string line = "VIEW ADD 1 image file=\"" + image.generic_string() + "\"";
    // Nowhere to keep it before the drawing is a project.
    EXPECT_EQ(s.refused(line).code, ErrorCode::InvalidState);
    EXPECT_EQ(s.set().sheets[0].viewports.size(), 0u);

    ASSERT_TRUE(s.document.saveAs(scratch.path / "project").ok());
    EXPECT_TRUE(contains(s.ok(line), "kind=image rect=24,36,409,286 locked=off text=\"site_photo.png\""));
    EXPECT_EQ(readFile(scratch.path / "project" / "assets" / "site_photo.png"), kPngBytes);
    // A file that is not an image is refused by its content.
    const fs::path text = scratch.path / "notes.png";
    writeFile(text, "plain text");
    EXPECT_EQ(s.refused("VIEW SET vp1 file=\"" + text.generic_string() + "\"").message,
              "an image must be a PNG, JPEG, GIF or BMP image");
    EXPECT_EQ(s.refused("VIEW ADD 1 plan file=\"" + image.generic_string() + "\"").message,
              "file= is for image views, not plan");
}

// ---- TILE ----------------------------------------------------------------------------

TEST(SheetVerbs, TileLaysASheetOutByPresetIdOrMenuName)
{
    Session s;
    s.ok("SHEET NEW PLAN");
    s.ok("VIEW ADD 1 legend");
    s.ok("VIEW ADD 1 plan");
    Sheet expected = s.set().sheets[0];
    ASSERT_EQ(tileViewports(expected, TilingPreset::MainRight), 2u);

    const std::size_t steps = s.steps();
    const std::string reply = s.ok("TILE 1 Main and panel right");
    EXPECT_EQ(lines(reply).front(), "tiled 2 views on sheet 1 preset=sectionR");
    EXPECT_EQ(s.set().sheets[0], expected);
    EXPECT_EQ(s.steps(), steps + 1);
    EXPECT_EQ(s.document.history().undoName(), "TILE_VIEWPORTS");

    for (const char* preset : {"full", "COLS2", "rows2", "quad", "sectionb", "sectionBR",
                               "sectionMap3d", "\"Two columns\"", "maintworight"}) {
        EXPECT_TRUE(s.interpreter.run(std::string("TILE s1 ") + preset).ok()) << preset;
    }
    EXPECT_TRUE(contains(s.refused("TILE 1 hexagons").message, "a tiling preset is full, cols2"));
    EXPECT_TRUE(contains(s.refused("TILE 1").message, "usage: TILE n preset"));
}

// ---- GENERATE ------------------------------------------------------------------------

TEST(SheetVerbs, GenerateFitLaysTheDrawingOutAsTheSmartLayoutDoes)
{
    Session s;
    s.ok("LINE 0,0 300,100");
    const std::string reply = s.ok("GENERATE fit");
    LayoutRequest request;
    request.planArea = katana::cad::drawnExtent(s.document.model(), {});
    const auto expected = smartLayout(s.document.model(), request);
    ASSERT_TRUE(expected.ok());
    ASSERT_EQ(s.set().sheets.size(), expected->size());
    SheetSet built;
    std::vector<Sheet> sheets = *expected;
    prepareForAppend(built, sheets);
    EXPECT_EQ(s.set().sheets, sheets);
    EXPECT_EQ(lines(reply).front(), "generated 1 sheet: 1");
    EXPECT_TRUE(contains(reply, "\nsheet 1 id=s1 name="));
    EXPECT_TRUE(contains(reply, "\n  view id=vp1 kind=plan scale=1000 rect="));
    EXPECT_EQ(s.steps(), 2u); // the line, and the sheets
    EXPECT_EQ(s.document.history().undoName(), "GENERATE_SHEETS");

    // An area given, a 3D view and legend beside it, on A1.
    EXPECT_TRUE(contains(s.ok("GENERATE fit area=0,0,300,200 model3d=on legend=on paper=A1"),
                         "generated 1 sheet: 2"));
    EXPECT_EQ(s.set().sheets[1].paper, PaperSize::A1);
    EXPECT_EQ(s.set().sheets[1].viewports.size(), 3u);
}

TEST(SheetVerbs, GenerateUsesTheFrontEndsExtentAndOnlyWhenItNeedsOne)
{
    Document document;
    CommandInterpreter interpreter(document);
    int asked = 0;
    interpreter.setSheetContext([&asked] {
        ++asked;
        SheetVerbContext context;
        context.drawingExtent = Box2(Point2(0.0, 0.0), Point2(3000.0, 1000.0));
        return context;
    });
    ASSERT_TRUE(interpreter.run("SHEET NEW A").ok());
    ASSERT_TRUE(interpreter.run("SHEETS").ok());
    EXPECT_EQ(asked, 0);
    // An empty drawing, but the front end draws 3 km of imagery.
    const auto reply = interpreter.run("GENERATE fit");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_EQ(asked, 1);
    EXPECT_TRUE(contains(*reply, "kind=plan scale=10000"));
    // Headless, an empty drawing has nothing to fit.
    Session headless;
    EXPECT_TRUE(contains(headless.refused("GENERATE fit").message, "the drawing is empty"));
}

TEST(SheetVerbs, GenerateGridStripsProfileAndSectionsMatchTheirGenerators)
{
    Session s;
    s.ok("ALIGN NEW ROAD 0,0 1000,0");

    // Tiles: 400 x 200 m at 1:500 with 10 m of overlap is six tiles and a key plan.
    EXPECT_EQ(lines(s.ok("GENERATE grid area=0,0,400,200 scale=500 overlap=10")).front(),
              "generated 7 sheets: 1 to 7");
    GridRequest grid;
    grid.area = Box2(Point2(0.0, 0.0), Point2(400.0, 200.0));
    grid.overlapM = 10.0;
    auto tiles = gridSheets(grid).value();
    prepareForAppend(SheetSet{}, tiles);
    EXPECT_EQ(s.set().sheets, tiles);

    // Strips along the only alignment, replacing the tiles.
    const std::string strips = s.ok("GENERATE strips scale=500 overlap=20 replace=on");
    EXPECT_EQ(lines(strips).front(), "generated 6 sheets: 1 to 6 (the set was replaced)");
    StripRequest strip;
    strip.overlapM = 20.0;
    const auto road = katana::geometry::solveAlignment(
        s.document.model().alignments.find("ROAD")->horizontal);
    auto expectedStrips = stripSheets(road.value(), "ROAD", strip).value();
    prepareForAppend(SheetSet{}, expectedStrips);
    EXPECT_EQ(s.set().sheets, expectedStrips);

    // Plan and profile on auto: the whole road on one sheet.
    EXPECT_EQ(lines(s.ok("GENERATE profile alignment=ROAD")).front(), "generated 1 sheet: 7");
    EXPECT_EQ(s.set().sheets[6].viewports.size(), 2u);
    EXPECT_EQ(s.set().sheets[6].viewports[1].kind, ViewportKind::LongSection);

    // Cross sections every 100 m, 2 x 2 to a sheet: eleven sections, three sheets.
    EXPECT_EQ(lines(s.ok("GENERATE sections interval=100 rows=2 columns=2 halfwidth=15")).front(),
              "generated 3 sheets: 8 to 10");
    CrossSectionRequest sections;
    sections.interval = 100.0;
    sections.rows = 2;
    sections.columns = 2;
    sections.halfWidth = 15.0;
    const auto expectedSections = crossSectionSheets(road.value(), "ROAD", sections).value();
    ASSERT_EQ(expectedSections.size(), 3u);
    for (std::size_t i = 0; i < expectedSections.size(); ++i) {
        ASSERT_EQ(s.set().sheets[7 + i].viewports.size(), expectedSections[i].viewports.size());
        for (std::size_t v = 0; v < expectedSections[i].viewports.size(); ++v) {
            EXPECT_EQ(s.set().sheets[7 + i].viewports[v].source,
                      expectedSections[i].viewports[v].source);
            EXPECT_EQ(s.set().sheets[7 + i].viewports[v].rect,
                      expectedSections[i].viewports[v].rect);
        }
    }
    // Stations given instead.
    EXPECT_EQ(lines(s.ok("GENERATE sections stations=10,500,990 rows=3 columns=1")).front(),
              "generated 1 sheet: 11");
    EXPECT_EQ(s.set().sheets[10].viewports.size(), 3u);
}

TEST(SheetVerbs, GenerateSaysWhatItCannotDo)
{
    Session s;
    EXPECT_EQ(s.refused("GENERATE strips").message,
              "the drawing has no alignment: define one with ALIGN NEW");
    s.ok("ALIGN NEW ROAD 0,0 1000,0");
    s.ok("ALIGN NEW CREEK 0,0 0,400");
    EXPECT_EQ(s.refused("GENERATE sections").message,
              "name the alignment with alignment=: the drawing has CREEK, ROAD");
    EXPECT_EQ(s.refused("GENERATE strips alignment=RIVER").message,
              "the drawing has no alignment of that name");
    EXPECT_EQ(s.refused("GENERATE mosaic").message,
              "GENERATE makes fit, grid, strips, profile, sections or frames");
    EXPECT_EQ(s.refused("GENERATE grid interval=20").message,
              "GENERATE GRID takes no option interval=; it takes paper orientation frame area "
              "scale overlap keyplan replace");
    EXPECT_TRUE(contains(s.refused("GENERATE grid area=0,0,10,10 scale=auto").message,
                         "GENERATE GRID needs a scale"));
    EXPECT_TRUE(contains(s.refused("GENERATE strips alignment=ROAD overlap=10").message,
                         "need a fixed scale"));
    EXPECT_EQ(s.refused("GENERATE sections alignment=ROAD interval=20 stations=5").message,
              "GENERATE SECTIONS cuts every interval= or at stations=, not both");
    EXPECT_EQ(s.refused("GENERATE fit area=0,0,10,10 alignment=ROAD").message,
              "GENERATE FIT lays out area= or alignment=, not both");
    EXPECT_EQ(s.refused("GENERATE frames").code, ErrorCode::NotFound);
    EXPECT_EQ(s.refused("GENERATE grid area=0,0,400,200 overlap=-1").message,
              "overlap= cannot be negative");
    EXPECT_EQ(s.refused("GENERATE frames portrait").code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(s.set().sheets.empty());
    EXPECT_EQ(s.steps(), 2u); // the two alignments, and nothing else
}

// ---- TITLEBLOCK ----------------------------------------------------------------------

TEST(SheetVerbs, TheTitleBlockIsListedAndSetFieldByField)
{
    Session s;
    const std::vector<std::string> blank = lines(s.ok("TITLEBLOCK"));
    ASSERT_EQ(blank.size(), titleBlockFields().size() + 2);
    EXPECT_EQ(blank.front(), "organisation=\"\"");
    EXPECT_EQ(blank[7], "numbering=\"{n}\"");
    EXPECT_EQ(blank[blank.size() - 2], "logo=\"\"");
    EXPECT_EQ(blank.back(), "0 revisions");

    EXPECT_EQ(s.ok("TITLEBLOCK organisation Example Surveys"), "organisation=\"Example Surveys\"");
    EXPECT_EQ(s.steps(), 1u);
    EXPECT_EQ(s.document.history().undoName(), "EDIT_TITLE_BLOCK");
    EXPECT_EQ(s.ok("TITLEBLOCK organization"), "organisation=\"Example Surveys\"");
    EXPECT_EQ(s.ok("TITLEBLOCK project_line_2 \"Stage 2\""), "project2=\"Stage 2\"");
    EXPECT_EQ(s.ok("TITLEBLOCK SurveyorName J. CITIZEN"), "surveyorname=\"J. CITIZEN\"");
    EXPECT_EQ(s.ok("TITLEBLOCK surveyor_date 02/01/26"), "surveyordate=\"02/01/26\"");
    EXPECT_EQ(s.ok("TITLEBLOCK coordinate_system GRID 56"), "coordsys=\"GRID 56\"");
    EXPECT_EQ(s.ok("TITLEBLOCK datum LOCAL"), "datum=\"LOCAL\"");
    EXPECT_EQ(s.ok("TITLEBLOCK notes \"Services are approximate.\\nConfirm before digging.\""),
              "notes=\"Services are approximate.\\nConfirm before digging.\"");
    EXPECT_EQ(s.ok("TITLEBLOCK numbering {set}-{n:02}"), "numbering=\"{set}-{n:02}\"");
    EXPECT_EQ(s.ok("TITLEBLOCK set DS-0001"), "setnumber=\"DS-0001\"");

    const SheetDefaults& d = s.set().defaults;
    EXPECT_EQ(d.organisation, "Example Surveys");
    EXPECT_EQ(d.projectLines, (std::vector<std::string>{"", "Stage 2"}));
    EXPECT_EQ(d.surveyor.name, "J. CITIZEN");
    EXPECT_EQ(d.surveyor.date, "02/01/26");
    EXPECT_EQ(d.coordinateSystem, "GRID 56");
    EXPECT_EQ(d.heightDatum, "LOCAL");
    EXPECT_EQ(d.notes, "Services are approximate.\nConfirm before digging.");
    EXPECT_EQ(s.set().numbering, "{set}-{n:02}");
    EXPECT_EQ(d.setNumber, "DS-0001");
    EXPECT_EQ(s.steps(), 9u); // nine values set; the one read recorded nothing

    // A project line cleared leaves no empty lines trailing, so the set can
    // go back to its default.
    s.ok("TITLEBLOCK project2 \"\"");
    EXPECT_TRUE(s.set().defaults.projectLines.empty());
    // An empty numbering is the default; one without {n} numbers nothing.
    EXPECT_EQ(s.ok("TITLEBLOCK numbering \"\""), "numbering=\"{n}\"");
    EXPECT_TRUE(contains(s.refused("TITLEBLOCK numbering DRAWING").message, "needs {n}"));
    EXPECT_EQ(s.refused("TITLEBLOCK colour red").message,
              "no title-block field of that name; TITLEBLOCK LIST shows them");

    const std::vector<std::string> listed = lines(s.ok("TITLEBLOCK LIST"));
    EXPECT_EQ(listed[0], "organisation=\"Example Surveys\"");
    EXPECT_EQ(listed[14], "surveyorname=\"J. CITIZEN\"");
}

TEST(SheetVerbs, RevisionsAreAddedAndRemovedByCode)
{
    Session s;
    EXPECT_EQ(s.ok("TITLEBLOCK REVISION ADD A 01/02/26 \"First issue\" JC"),
              "added revision code=\"A\" date=\"01/02/26\" description=\"First issue\" by=\"JC\"");
    EXPECT_EQ(s.document.history().undoName(), "ADD_REVISION");
    s.ok("TITLEBLOCK REVISION ADD B 03/02/26 Kerbs");
    ASSERT_EQ(s.set().revisions.size(), 2u);
    EXPECT_EQ(s.set().revisions[1], (Revision{"B", "03/02/26", "Kerbs", ""}));
    EXPECT_TRUE(contains(s.ok("TITLEBLOCK"),
                         "\n2 revisions\nrevision code=\"A\" date=\"01/02/26\" "
                         "description=\"First issue\" by=\"JC\"\nrevision code=\"B\""));
    // The revisions alone.
    const std::string revisions = "2 revisions\n"
                                  "revision code=\"A\" date=\"01/02/26\" description=\"First issue\" "
                                  "by=\"JC\"\n"
                                  "revision code=\"B\" date=\"03/02/26\" description=\"Kerbs\" by=\"\"";
    EXPECT_EQ(s.ok("TITLEBLOCK REVISION"), revisions);
    EXPECT_EQ(s.ok("titleblock revisions list"), revisions);
    EXPECT_TRUE(contains(s.refused("TITLEBLOCK REVISION LIST A").message, "usage"));
    EXPECT_TRUE(contains(s.refused("TITLEBLOCK REVISION RENAME A B").message, "usage"));
    const auto twice = s.refused("TITLEBLOCK REVISION ADD A 04/02/26 Again");
    EXPECT_EQ(twice.code, ErrorCode::AlreadyExists);
    EXPECT_TRUE(contains(s.refused("TITLEBLOCK REVISION ADD C 04/02/26").message, "usage"));

    EXPECT_EQ(s.ok("TITLEBLOCK REVISION REMOVE A"), "removed revision code=\"A\"");
    EXPECT_EQ(s.document.history().undoName(), "REMOVE_REVISION");
    EXPECT_EQ(s.refused("TITLEBLOCK REVISION REMOVE A").code, ErrorCode::NotFound);
    ASSERT_EQ(s.set().revisions.size(), 1u);
    ASSERT_TRUE(s.document.undo().ok());
    EXPECT_EQ(s.set().revisions.size(), 2u);
}

TEST(SheetVerbs, TheLogoIsImportedIntoTheProjectAndCanBeRemoved)
{
    const ScratchDirectory scratch("logo");
    const fs::path logo = scratch.path / "our logo.png";
    writeFile(logo, kPngBytes);
    Session s;
    const std::string line = "TITLEBLOCK LOGO \"" + logo.generic_string() + "\"";
    EXPECT_EQ(s.refused(line).code, ErrorCode::InvalidState);
    ASSERT_TRUE(s.document.saveAs(scratch.path / "project").ok());
    EXPECT_EQ(s.ok(line), "logo=\"our_logo.png\"");
    EXPECT_EQ(s.document.history().undoName(), "SET_LOGO");
    EXPECT_EQ(s.set().defaults.logoAsset, "our_logo.png");
    EXPECT_EQ(readFile(scratch.path / "project" / "assets" / "our_logo.png"), kPngBytes);
    EXPECT_EQ(s.ok("TITLEBLOCK LOGO \"\""), "logo=\"\"");
    EXPECT_TRUE(s.set().defaults.logoAsset.empty());
    EXPECT_EQ(s.refused("TITLEBLOCK LOGO \"" + (scratch.path / "missing.png").generic_string() + "\"")
                  .code,
              ErrorCode::NotFound);
}

// ---- SHEETS --------------------------------------------------------------------------

TEST(SheetVerbs, SheetsListsEverySheetAndItsViews)
{
    Session s;
    EXPECT_EQ(s.ok("SHEETS"), "0 sheets");
    s.ok("SHEET NEW PLAN");
    s.ok("SHEET NEW \"KEY PLAN\" paper=A4 portrait");
    s.ok("VIEW ADD 1 plan scale=500 centre=0,0");
    s.ok("VIEW ADD 1 legend");
    s.ok("VIEW ADD 2 key_plan");
    EXPECT_EQ(s.ok("SHEETS LIST"),
              "2 sheets\n"
              "sheet 1 id=s1 name=\"PLAN\" paper=A3 orientation=landscape frame=a3_landscape "
              "legendblock=on views=2\n"
              "  view id=vp1 kind=plan scale=500 rect=24,36,409,286\n"
              "  view id=vp2 kind=legend rect=176.5,111,256.5,211\n"
              "sheet 2 id=s2 name=\"KEY PLAN\" paper=A4 orientation=portrait frame=a3_landscape "
              "legendblock=on views=1\n"
              "  view id=vp3 kind=key_plan scale=auto rect=11,11,199,286");
    // The key plan stores no outlines: it draws the other sheets' plans
    // where they are when it is drawn (key_plan.hpp), so none goes stale.
    const Viewport& key = s.set().sheets[1].viewports[0];
    EXPECT_TRUE(key.marks.empty());
    EXPECT_TRUE(key.autoScale && key.autoCentre);
}

TEST(SheetVerbs, SheetsJsonSaveAndLoadRoundTripTheWholeSet)
{
    const ScratchDirectory scratch("json");
    Session s;
    s.ok("ALIGN NEW ROAD 0,0 1000,0");
    s.ok("GENERATE strips scale=500 overlap=20 keyplan=on");
    s.ok("TITLEBLOCK organisation \"Example Surveys\"");
    s.ok("TITLEBLOCK REVISION ADD A 01/02/26 \"First issue\" JC");
    s.ok("SHEET FIELD 2 sheet_number C-101");
    s.ok("VIEW SET vp2 title=\"STRIP ONE\" hide=FRAMES");

    // Printed, it is exactly what the project stores.
    EXPECT_EQ(s.ok("SHEETS JSON"), s.json());

    const fs::path file = scratch.path / "set one.json";
    const std::string path = "\"" + file.generic_string() + "\"";
    EXPECT_EQ(s.ok("SHEETS SAVE " + path), "wrote 7 sheets to \"" + file.generic_string() + "\"");
    EXPECT_EQ(readFile(file), s.json() + "\n");
    EXPECT_EQ(s.ok("SHEETS JSON " + path), "wrote 7 sheets to \"" + file.generic_string() + "\"");
    const auto read = readSheetSetFile(file);
    ASSERT_TRUE(read.ok());
    EXPECT_EQ(*read, s.set());

    // Loaded into another drawing it is the same set, as one step.
    Session other;
    other.ok("SHEET NEW SCRAP");
    EXPECT_EQ(other.ok("SHEETS LOAD " + path), "loaded 7 sheets from \"" + file.generic_string() + "\"");
    EXPECT_EQ(other.steps(), 2u);
    EXPECT_EQ(other.document.history().undoName(), "LOAD_SHEETS");
    EXPECT_EQ(other.set(), s.set());
    EXPECT_EQ(other.json(), s.json());
    ASSERT_TRUE(other.document.undo().ok());
    ASSERT_EQ(other.set().sheets.size(), 1u);
    EXPECT_EQ(other.set().sheets[0].name, "SCRAP");

    // What cannot be loaded changes nothing and says why.
    const std::string before = other.json();
    writeFile(scratch.path / "newer.json", R"({"format": "katana-sheets", "version": 99})");
    EXPECT_EQ(other.refused("SHEETS LOAD \"" + (scratch.path / "newer.json").generic_string() + "\"")
                  .code,
              ErrorCode::Unsupported);
    writeFile(scratch.path / "junk.json", "not json");
    EXPECT_EQ(other.refused("SHEETS LOAD \"" + (scratch.path / "junk.json").generic_string() + "\"")
                  .code,
              ErrorCode::ParseFailure);
    EXPECT_EQ(other.refused("SHEETS LOAD \"" + (scratch.path / "none.json").generic_string() + "\"")
                  .code,
              ErrorCode::NotFound);
    EXPECT_EQ(other.refused("SHEETS SAVE \"" + (scratch.path / "no" / "such" / "dir.json").generic_string() +
                            "\"")
                  .code,
              ErrorCode::FileExportFailure);
    EXPECT_TRUE(contains(other.refused("SHEETS SAVE").message, "usage: SHEETS"));
    EXPECT_EQ(other.json(), before);
}

TEST(SheetVerbs, SheetsThatCannotBeReadAreReportedNotListedAsNone)
{
    Session s;
    auto metadata = s.document.metadata();
    metadata.unknownKeys.insert_or_assign("sheets", R"({"format": "katana-sheets", "version": 99})");
    s.document.setMetadata(metadata);
    const auto error = s.refused("SHEETS");
    EXPECT_EQ(error.code, ErrorCode::Unsupported);
    EXPECT_TRUE(contains(error.message, "the project's sheets cannot be read"));
    EXPECT_EQ(s.refused("SHEETS JSON").code, ErrorCode::Unsupported);
    EXPECT_EQ(s.refused("TITLEBLOCK").code, ErrorCode::Unsupported);
    // And an edit is refused rather than overwriting them.
    EXPECT_EQ(s.refused("SHEET NEW A").code, ErrorCode::Unsupported);
    EXPECT_EQ(s.document.metadata().unknownKeys.at("sheets"),
              R"({"format": "katana-sheets", "version": 99})");
}

TEST(SheetVerbs, SheetsThatAreNotValidJsonAreNeitherListedNorOverwritten)
{
    // Broken text is no set at all, not an empty one: every verb that reads
    // or edits the sheets is refused and the text is kept as it was. SHEETS
    // LOAD replaces them on purpose, as one step that UNDO takes back.
    Session s;
    const std::string broken = R"({"format": "katana-sheets", "version": 1, "sheets": [)";
    auto metadata = s.document.metadata();
    metadata.unknownKeys.insert_or_assign("sheets", broken);
    s.document.setMetadata(metadata);
    const std::size_t steps = s.steps();
    for (const char* line : {"SHEETS", "SHEET NEW A", "GENERATE grid area=0,0,100,100",
                             "TITLEBLOCK organisation X", "VIEW LIST", "TILE 1 full",
                             "TITLEBLOCK REVISION ADD A 01/01/26 FIRST"}) {
        const auto error = s.refused(line);
        EXPECT_EQ(error.code, ErrorCode::ParseFailure) << line;
        EXPECT_TRUE(contains(error.message, "the project's sheets cannot be read")) << line;
    }
    EXPECT_EQ(s.document.metadata().unknownKeys.at("sheets"), broken);
    EXPECT_EQ(s.steps(), steps);
}

TEST(SheetVerbs, ATextTypedBackIsTheTextStored)
{
    Session s;
    // "\n" is a line break and "\\" a backslash; the reply writes them so.
    EXPECT_EQ(s.ok(R"(TITLEBLOCK notes "SEE C:\\NEW\nLEVELS IN METRES")"),
              R"(notes="SEE C:\\NEW\nLEVELS IN METRES")");
    EXPECT_EQ(s.set().defaults.notes, "SEE C:\\NEW\nLEVELS IN METRES");
    // What the reply says, typed back, stores the same text.
    const std::string reply = s.ok("TITLEBLOCK notes");
    (void)s.ok("TITLEBLOCK " + reply.substr(0, reply.find('=')) + " " + reply.substr(reply.find('=') + 1));
    EXPECT_EQ(s.set().defaults.notes, "SEE C:\\NEW\nLEVELS IN METRES");
    // A backslash before anything else is itself.
    (void)s.ok(R"(SHEET NEW "A\B")");
    EXPECT_EQ(s.set().sheets.back().name, "A\\B");
    EXPECT_TRUE(contains(s.ok("SHEETS"), R"(name="A\\B")"));
}

// ---- every edit is one step, and the same lines make the same set --------------------

TEST(SheetVerbs, EveryEditIsOneUndoStepThatUndoesExactly)
{
    const ScratchDirectory scratch("steps");
    const fs::path image = scratch.path / "photo.png";
    writeFile(image, kPngBytes);
    Session s;
    s.ok("ALIGN NEW ROAD 0,0 1000,0");
    ASSERT_TRUE(s.document.saveAs(scratch.path / "project").ok());
    const std::vector<std::string> edits = {
        "SHEET NEW PLAN",
        "SHEET NEW \"LONG SECTION\" paper=A1",
        "SHEET MOVE 2 1",
        "SHEET COPY 1",
        "SHEET RENAME 3 SITE",
        "SHEET SET 3 paper=A2 legendblock=off",
        "SHEET FIELD 3 scale \"AS SHOWN\"",
        "SHEET REMOVE 2",
        "VIEW ADD 1 profile",
        "VIEW ADD 2 plan",
        "VIEW ADD 2 image file=\"" + image.generic_string() + "\"",
        "VIEW SET vp2 scale=1000 centre=500,0 rotation=15",
        "VIEW REMOVE vp1",
        "TILE 2 sectionR",
        "GENERATE sections interval=250",
        "GENERATE strips scale=1000 replace=on",
        "TITLEBLOCK client \"Client Pty Ltd\"",
        "TITLEBLOCK REVISION ADD A 01/02/26 \"First issue\"",
        "TITLEBLOCK REVISION REMOVE A",
        "TITLEBLOCK LOGO \"" + image.generic_string() + "\"",
    };
    std::vector<std::string> states{s.json()};
    for (const std::string& line : edits) {
        const std::size_t before = s.steps();
        s.ok(line);
        EXPECT_EQ(s.steps(), before + 1) << line;
        states.push_back(s.json());
    }
    // Back through every one, each undo landing exactly on the state before.
    for (std::size_t i = edits.size(); i-- > 0;) {
        ASSERT_TRUE(s.document.undo().ok()) << edits[i];
        EXPECT_EQ(s.json(), states[i]) << "undoing " << edits[i];
    }
    // And forward again.
    for (std::size_t i = 0; i < edits.size(); ++i) {
        ASSERT_TRUE(s.document.redo().ok());
        EXPECT_EQ(s.json(), states[i + 1]) << "redoing " << edits[i];
    }
}

TEST(SheetVerbs, TheSameLinesMakeTheSameSet)
{
    const auto build = [] {
        Session s;
        for (const char* line :
             {"ALIGN NEW ROAD 0,0 400,0 400,300", "LINE -50,-50 450,350",
              "GENERATE fit model3d=on legend=on", "GENERATE profile scale=500 interval=50",
              "GENERATE grid scale=1000 overlap=5", "SHEET MOVE 3 1", "TILE 2 quad",
              "VIEW ADD 1 notes text=\"GENERAL NOTES\""}) {
            s.ok(line);
        }
        return s.json() + "\n" + s.ok("SHEETS LIST");
    };
    EXPECT_EQ(build(), build());
}

// ---- the pieces ----------------------------------------------------------------------

TEST(SheetVerbs, APlotSelectionIsNumbersIdsAndRangesInTheOrderGiven)
{
    SheetSet set;
    for (const char* id : {"s1", "s2", "s3", "s4", "s5", "s6"}) {
        Sheet sheet;
        sheet.id = id;
        set.sheets.push_back(sheet);
    }
    EXPECT_EQ(parseSheetSelection(set, "1,3-5").value(), (std::vector<std::size_t>{0, 2, 3, 4}));
    EXPECT_EQ(parseSheetSelection(set, "6,s2,1-2").value(), (std::vector<std::size_t>{5, 1, 0}));
    EXPECT_EQ(parseSheetSelection(set, "s2-s4").value(), (std::vector<std::size_t>{1, 2, 3}));
    EXPECT_EQ(parseSheetSelection(set, "ALL").value().size(), 6u);
    EXPECT_EQ(parseSheetSelection(set, "5-3").error().message,
              "a range runs from the lower sheet to the higher");
    EXPECT_EQ(parseSheetSelection(set, "1,,2").error().code, ErrorCode::ParseFailure);
    EXPECT_EQ(parseSheetSelection(set, "7").error().message, "no sheet 7: the set has 6 sheets");
    EXPECT_EQ(parseSheetSelection(SheetSet{}, "all").error().message, "the set has no sheets");

    const auto request = parsePlotSheets(set, {"C:/plots/set.pdf", "sheets=2,4-6", "dpi=150"});
    ASSERT_TRUE(request.ok());
    EXPECT_EQ(request->path, fs::path("C:/plots/set.pdf"));
    EXPECT_EQ(request->sheets, (std::vector<std::size_t>{1, 3, 4, 5}));
    EXPECT_DOUBLE_EQ(request->dpi, 150.0);
    const auto every = parsePlotSheets(set, {"out.pdf"});
    ASSERT_TRUE(every.ok());
    EXPECT_TRUE(every->sheets.empty());
    EXPECT_DOUBLE_EQ(every->dpi, 300.0);
    EXPECT_TRUE(contains(parsePlotSheets(set, {}).error().message, "usage: PLOTSHEETS"));
    EXPECT_EQ(parsePlotSheets(set, {"out.pdf", "dpi=20"}).error().message, "dpi= is 72 to 1200");
    EXPECT_TRUE(contains(parsePlotSheets(set, {"out.pdf", "colour=on"}).error().message,
                         "takes no option colour="));
}

TEST(SheetVerbs, TheTitleBlockFunctionsReadAndWriteEveryField)
{
    SheetSet set;
    for (const std::string_view field : titleBlockFields()) {
        const std::string value = field == "numbering" ? "{n} of {N}" : "value of " + std::string(field);
        ASSERT_TRUE(setTitleBlockValue(set, field, value).ok()) << field;
        EXPECT_EQ(titleBlockValue(set, field).value(), value) << field;
    }
    EXPECT_EQ(set.defaults.projectLines.size(), 4u);
    EXPECT_EQ(set.defaults.approver.date, "value of approverdate");
    EXPECT_EQ(set.defaults.locator.name, "value of locatorname");
    EXPECT_EQ(set.defaults.modelName, "value of model");
    // Every field reached a member of its own: the JSON holds all 22.
    const std::string json = sheetSetToJson(set).value();
    for (const std::string_view field : titleBlockFields()) {
        if (field != "numbering") {
            EXPECT_TRUE(contains(json, "value of " + std::string(field))) << field;
        }
    }
    EXPECT_EQ(titleBlockValue(set, "height_datum").value(), "value of datum");
    EXPECT_EQ(titleBlockValue(set, "nothing").error().code, ErrorCode::NotFound);
}

TEST(SheetVerbs, AViewOptionIsCheckedBeforeItIsApplied)
{
    katana::entity::Model model;
    Viewport plan;
    plan.kind = ViewportKind::Plan;
    ASSERT_TRUE(setViewportOption(plan, "Scale", "1:2500", model).ok());
    EXPECT_DOUBLE_EQ(plan.scale, 2500.0);
    ASSERT_TRUE(setViewportOption(plan, "rect", "100,80,20,10", model).ok());
    EXPECT_EQ(plan.rect, Box2(Point2(20.0, 10.0), Point2(100.0, 80.0))); // either corner first
    const Viewport before = plan;
    EXPECT_FALSE(setViewportOption(plan, "rotation", "north", model).ok());
    EXPECT_FALSE(setViewportOption(plan, "alignment", "ROAD", model).ok());
    EXPECT_FALSE(setViewportOption(plan, "scale", "1:0", model).ok());
    EXPECT_FALSE(setViewportOption(plan, "centre", "nan,1", model).ok());
    EXPECT_EQ(plan, before);
}
