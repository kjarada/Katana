// The sheet verbs an agent drives every sheet feature with
// (src/katana_cad/plotting/sheet_verbs.cpp, docs/plotting.md "Sheets on the
// command line"): the drawing register and revision table as views, a
// legend's scope, a plan's grid, key plans, GENERATE register, the page
// setup, SHEETS CHECK, ARRANGE, VIEW FIT and BESTROTATE, SHEET SUGGESTPAPER
// and a section's automatic scale. Each edit is one undo step and a refused
// one is none; each reply is in the words the options take.

#include <gtest/gtest.h>

#include <cmath>
#include <span>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/plotting/preflight.hpp"
#include "katana/cad/plotting/sheet_json.hpp"
#include "katana/cad/plotting/sheet_verbs.hpp"
#include "katana/cad/plotting/tables.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::PaperSize;
using namespace katana::cad::plotting;
using katana::core::ErrorCode;
using katana::geometry::Point2;
using katana::geometry::Box2;

namespace {

struct Session {
    Document document;
    CommandInterpreter interpreter{document};

    std::string ok(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << "\n  -> "
                                << (reply.ok() ? std::string{} : reply.error().describe());
        return reply.ok() ? *reply : std::string{};
    }
    katana::core::Error refused(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_FALSE(reply.ok()) << line << " was accepted:\n" << (reply.ok() ? *reply : "");
        return reply.ok() ? katana::core::Error{} : reply.error();
    }
    std::size_t steps() const { return document.history().undoCount(); }
    const SheetSet& set() const { return document.sheetSet(); }
    const Viewport& view(std::size_t sheet, std::size_t index) const
    {
        return document.sheetSet().sheets.at(sheet).viewports.at(index);
    }
};

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

std::vector<std::string> linesOf(const std::string& text)
{
    std::vector<std::string> lines;
    std::size_t start = 0;
    while (start <= text.size()) {
        const std::size_t end = text.find('\n', start);
        lines.push_back(text.substr(start, end == std::string::npos ? std::string::npos : end - start));
        if (end == std::string::npos) {
            break;
        }
        start = end + 1;
    }
    return lines;
}

// A drawing 400 x 100 m: a line along its long side and one across, so a
// plan of it has something to fit and to turn.
void drawRoad(Session& s)
{
    s.ok("LINE 0,0 400,0");
    s.ok("LINE 0,0 0,100");
    s.ok("LINE 400,0 400,100");
}

} // namespace

// ---- the drawing register and the revision table -----------------------------------------

TEST(SheetAgentVerbs, TheRegisterAndRevisionTableAreViewsLikeAnyOther)
{
    Session s;
    s.ok("SHEET NEW COVER");
    const std::size_t before = s.steps();
    const std::string added = s.ok("VIEW ADD 1 register");
    EXPECT_EQ(s.steps(), before + 1);
    EXPECT_TRUE(contains(added, "kind=sheet_index")) << added;
    EXPECT_EQ(s.view(0, 0).kind, ViewportKind::SheetIndex);
    EXPECT_FALSE(s.view(0, 0).rect.empty()) << "placed on the sheet";
    const std::string table = s.ok("VIEW ADD 1 revision_table");
    EXPECT_TRUE(contains(table, "kind=revisions") && contains(table, "revisions=all")) << table;
    EXPECT_TRUE(contains(s.ok("VIEW ADD 1 sheet_index"), "kind=sheet_index"));

    // The table shows the newest so many: revisions=n, all for every one.
    const std::string id = s.view(0, 1).id;
    EXPECT_TRUE(contains(s.ok("VIEW SET " + id + " revisions=3"), "revisions=3"));
    EXPECT_EQ(s.view(0, 1).revisionLimit, 3u);
    EXPECT_TRUE(contains(s.ok("VIEW SET " + id + " revisions=all"), "revisions=all"));
    EXPECT_EQ(s.view(0, 1).revisionLimit, 0u);
    const std::size_t steps = s.steps();
    EXPECT_TRUE(contains(s.refused("VIEW SET " + s.view(0, 0).id + " revisions=2").message,
                         "revisions"));
    EXPECT_EQ(s.steps(), steps) << "a refusal is no step";
}

TEST(SheetAgentVerbs, GenerateRegisterPutsACoverFirstInOneStep)
{
    Session s;
    s.ok("SHEET NEW PLAN ONE");
    s.ok("SHEET NEW PLAN TWO");
    const std::size_t before = s.steps();
    const std::string reply = s.ok("GENERATE register paper=A3");
    EXPECT_EQ(s.steps(), before + 1);
    EXPECT_TRUE(contains(reply, "generated 1 sheet: 1 (the drawing register)")) << reply;
    ASSERT_EQ(s.set().sheets.size(), 3u);
    EXPECT_EQ(s.set().sheets[0].name, kRegisterSheetName);
    EXPECT_EQ(s.set().sheets[1].name, "PLAN ONE");
    EXPECT_EQ(drawingRegister(s.set()).size(), 3u);
    // One register is enough; and it replaces nothing.
    EXPECT_EQ(s.refused("GENERATE register").code, ErrorCode::AlreadyExists);
    EXPECT_TRUE(contains(s.refused("GENERATE register replace=on").message, "replaces no sheets"));
    ASSERT_TRUE(s.document.undo().ok());
    EXPECT_EQ(s.set().sheets.size(), 2u);
}

// ---- the legend, the grid, the key plan ----------------------------------------------------

TEST(SheetAgentVerbs, ALegendsScopeAndAPlansGridAreViewOptions)
{
    Session s;
    s.ok("SHEET NEW PLAN");
    s.ok("VIEW ADD 1 plan scale=500 centre=0,0");
    s.ok("VIEW ADD 1 legend");
    std::size_t steps = s.steps();
    EXPECT_TRUE(contains(s.ok("VIEW SET vp2 legend=whole_set"), "legend=whole_set"));
    EXPECT_EQ(s.view(0, 1).legendScope, LegendScope::WholeSet);
    EXPECT_EQ(s.steps(), steps + 1);
    EXPECT_TRUE(contains(s.ok("VIEW SET vp2 scope=drawing"), "legend=whole_drawing"));
    EXPECT_TRUE(contains(s.ok("VIEW SET vp2 legend=this_sheet"), "legend=this_sheet"));
    EXPECT_TRUE(contains(s.refused("VIEW SET vp2 legend=everywhere").message, "this_sheet"));
    EXPECT_TRUE(contains(s.refused("VIEW SET vp1 legend=whole_set").message, "legend"));

    steps = s.steps();
    const std::string gridded = s.ok("VIEW SET vp1 grid=crosses gridinterval=50");
    EXPECT_TRUE(contains(gridded, "grid=crosses gridinterval=50")) << gridded;
    EXPECT_EQ(s.steps(), steps + 1) << "both options, one step";
    EXPECT_EQ(s.view(0, 0).gridStyle, GridStyle::Crosses);
    EXPECT_DOUBLE_EQ(s.view(0, 0).gridInterval, 50.0);
    EXPECT_TRUE(contains(s.ok("VIEW SET vp1 grid=lines gridinterval=auto"),
                         "grid=lines gridinterval=auto"));
    EXPECT_TRUE(contains(s.ok("VIEW SET vp1 grid=off"), "grid=none"));
    steps = s.steps();
    EXPECT_TRUE(contains(s.refused("VIEW SET vp1 grid=dots").message, "ticks"));
    EXPECT_TRUE(contains(s.refused("VIEW SET vp1 gridinterval=-5").message, "gridinterval"));
    EXPECT_TRUE(contains(s.refused("VIEW SET vp2 grid=ticks").message, "plan"));
    EXPECT_EQ(s.steps(), steps);
}

TEST(SheetAgentVerbs, AKeyPlanTakesAGridAndStoresNoOutlines)
{
    Session s;
    s.ok("SHEET NEW PLAN");
    s.ok("VIEW ADD 1 plan scale=500 centre=0,0");
    s.ok("SHEET NEW KEY");
    const std::string added = s.ok("VIEW ADD 2 keyplan grid=ticks");
    EXPECT_TRUE(contains(added, "kind=key_plan") && contains(added, "scale=auto centre=auto"))
        << added;
    EXPECT_TRUE(contains(added, "grid=ticks"));
    EXPECT_TRUE(s.view(1, 0).marks.empty()) << "outlined live, when drawn";
}

// ---- the page setup -----------------------------------------------------------------------

TEST(SheetAgentVerbs, ThePageSetupIsPrintedAndSetInOneStep)
{
    Session s;
    s.ok("SHEET NEW PLAN");
    EXPECT_EQ(s.ok("SHEETS PAGESETUP"),
              "pagesetup style=colour lineweight=1 dpi=300 pattern=\"{set}{n:02} {name}\" "
              "filepersheet=off");
    const std::size_t steps = s.steps();
    EXPECT_EQ(s.ok("SHEETS PAGESETUP style=mono lineweight=0.7 dpi=150 pattern=\"{n} {name}\" "
                   "filepersheet=on"),
              "pagesetup style=monochrome lineweight=0.7 dpi=150 pattern=\"{n} {name}\" "
              "filepersheet=on");
    EXPECT_EQ(s.steps(), steps + 1);
    EXPECT_EQ(s.set().pageSetup.colourMode, katana::cad::PlotColourMode::Monochrome);
    EXPECT_TRUE(s.set().pageSetup.filePerSheet);
    EXPECT_TRUE(contains(s.ok("SHEETS PAGESETUP style=grey"), "style=greyscale"));
    // Refused whole, naming what: nothing changes.
    EXPECT_FALSE(s.interpreter.run("SHEETS PAGESETUP dpi=5").ok());
    EXPECT_FALSE(s.interpreter.run("SHEETS PAGESETUP style=sepia").ok());
    EXPECT_FALSE(s.interpreter.run("SHEETS PAGESETUP pattern={nope}").ok());
    EXPECT_FALSE(s.interpreter.run("SHEETS PAGESETUP paper=A1").ok());
    EXPECT_EQ(s.steps(), steps + 2);
    EXPECT_DOUBLE_EQ(s.set().pageSetup.dpi, 150.0);
}

// ---- SHEETS CHECK ---------------------------------------------------------------------------

TEST(SheetAgentVerbs, SheetsCheckPrintsAFindingALineAnAgentCanSplit)
{
    Session s;
    s.ok("SHEET NEW PLAN");
    s.ok("VIEW ADD 1 plan scale=437 centre=5000,5000");
    s.ok("SHEET NEW EMPTY");
    const std::string reply = s.ok("SHEETS CHECK");
    const std::vector<std::string> lines = linesOf(reply);
    ASSERT_GE(lines.size(), 2u) << reply;
    EXPECT_TRUE(lines[0].starts_with("checked 2 sheets: ")) << lines[0];
    bool empty = false;
    bool nonStandard = false;
    bool setLevel = false;
    for (std::size_t i = 1; i < lines.size(); ++i) {
        // severity code sheet view message="..."
        const std::string& line = lines[i];
        EXPECT_TRUE(line.starts_with("error ") || line.starts_with("warning ") ||
                    line.starts_with("info "))
            << line;
        EXPECT_TRUE(contains(line, " message=\"")) << line;
        empty = empty || line.starts_with("warning sheet.empty 2 - message=");
        nonStandard = nonStandard || line.starts_with("info scale.non-standard 1 vp1 message=");
        setLevel = setLevel || contains(line, " field.empty - - message=");
    }
    EXPECT_TRUE(empty) << reply;
    EXPECT_TRUE(nonStandard) << reply;
    EXPECT_TRUE(setLevel) << reply;

    // One sheet, and as JSON; checking changes nothing.
    const std::size_t steps = s.steps();
    const std::string one = s.ok("SHEETS CHECK sheets=2");
    EXPECT_TRUE(one.starts_with("checked 1 sheet: ")) << one;
    EXPECT_FALSE(contains(one, "scale.non-standard"));
    const auto findings = findingsFromJson(s.ok("SHEETS CHECK json"));
    ASSERT_TRUE(findings.ok());
    EXPECT_EQ(findings->size(), lines.size() - 1);
    EXPECT_EQ(s.steps(), steps);
    EXPECT_FALSE(s.interpreter.run("SHEETS CHECK sheets=9").ok());

    // The window's own checker, when it gives one.
    bool asked = false;
    s.interpreter.setSheetContext([&asked] {
        SheetVerbContext context;
        context.check = [&asked](std::span<const std::size_t> sheets) {
            asked = sheets.size() == 1 && sheets[0] == 0;
            return std::vector<Finding>{Finding{Severity::Error, "viewport.outside", 0, "s1", "vp1",
                                                {}, "off the paper", "move it"}};
        };
        return context;
    });
    EXPECT_EQ(s.ok("SHEETS CHECK sheets=1"),
              "checked 1 sheet: 1 error\nerror viewport.outside 1 vp1 message=\"off the paper\" "
              "fix=\"move it\"");
    EXPECT_TRUE(asked);
}

// ---- ARRANGE ---------------------------------------------------------------------------------

TEST(SheetAgentVerbs, ArrangeAlignDistributeAndMatchScaleAreEachOneStep)
{
    Session s;
    s.ok("SHEET NEW PLANS");
    s.ok("VIEW ADD 1 plan rect=30,40,130,120 scale=500 centre=0,0");
    s.ok("VIEW ADD 1 plan rect=100,60,200,140 scale=1000 centre=0,0");
    s.ok("VIEW ADD 1 plan rect=300,50,380,110 scale=250 centre=0,0");

    std::size_t steps = s.steps();
    const std::string arranged = s.ok("ARRANGE 1");
    EXPECT_TRUE(arranged.starts_with("arranged sheet 1 moved=")) << arranged;
    EXPECT_TRUE(contains(arranged, "overlapping= unplaced=")) << arranged;
    EXPECT_EQ(s.steps(), steps + 1);
    const Box2 a = s.view(0, 0).rect;
    const Box2 b = s.view(0, 1).rect;
    EXPECT_FALSE(a.min.x < b.max.x - 0.5 && b.min.x < a.max.x - 0.5 && a.min.y < b.max.y - 0.5 &&
                 b.min.y < a.max.y - 0.5)
        << "vp1 and vp2 no longer overlap";

    steps = s.steps();
    const std::string aligned = s.ok("ARRANGE ALIGN top vp2 vp3");
    EXPECT_TRUE(aligned.starts_with("aligned vp2,vp3 on sheet 1 edge=top moved=")) << aligned;
    EXPECT_DOUBLE_EQ(s.view(0, 1).rect.max.y, s.view(0, 2).rect.max.y);
    EXPECT_EQ(s.steps(), steps + 1);

    steps = s.steps();
    const std::string distributed = s.ok("ARRANGE DISTRIBUTE across vp1 vp2 vp3");
    EXPECT_TRUE(distributed.starts_with("distributed vp1,vp2,vp3 on sheet 1 axis=horizontal"))
        << distributed;
    EXPECT_LE(s.steps(), steps + 1);

    steps = s.steps();
    const std::string matched = s.ok("ARRANGE MATCHSCALE vp1 vp2 vp3");
    EXPECT_TRUE(matched.starts_with("matched vp2,vp3 to vp1 scale=500 changed=vp2,vp3")) << matched;
    EXPECT_DOUBLE_EQ(s.view(0, 1).scale, 500.0);
    EXPECT_DOUBLE_EQ(s.view(0, 2).scale, 500.0);
    EXPECT_EQ(s.steps(), steps + 1);

    // Refused whole: an edge it does not know, a view that is not there,
    // views on two sheets.
    s.ok("SHEET NEW OTHER");
    s.ok("VIEW ADD 2 plan");
    steps = s.steps();
    EXPECT_TRUE(contains(s.refused("ARRANGE ALIGN diagonal vp1 vp2").message, "left, right"));
    EXPECT_EQ(s.refused("ARRANGE ALIGN left vp1 vp99").code, ErrorCode::NotFound);
    EXPECT_TRUE(contains(s.refused("ARRANGE ALIGN left vp1 vp4").message, "a sheet at a time"));
    EXPECT_EQ(s.steps(), steps);
}

// ---- VIEW FIT, VIEW BESTROTATE, SHEET SUGGESTPAPER -----------------------------------------------

TEST(SheetAgentVerbs, ViewFitAndBestRotateFitAPlanToWhatItShows)
{
    Session s;
    drawRoad(s);
    s.ok("SHEET NEW PLAN");
    s.ok("VIEW ADD 1 plan rect=30,40,330,280 scale=1000 centre=200,50");
    std::size_t steps = s.steps();
    const std::string fitted = s.ok("VIEW FIT vp1");
    EXPECT_TRUE(fitted.starts_with("fitted vp1\nview id=vp1")) << fitted;
    EXPECT_EQ(s.steps(), steps + 1);
    // 400 x 100 m at 1:1000 is 400 x 100 mm: the rectangle takes the drawing's shape.
    const Box2 rect = s.view(0, 0).rect;
    EXPECT_GT(rect.width() / rect.height(), 2.5) << rect.width() << " x " << rect.height();

    // A long thin drawing turned 30 degrees: the best rotation lies it along the paper.
    s.ok("VIEW SET vp1 rotation=30 rect=30,40,330,280");
    steps = s.steps();
    const std::string turned = s.ok("VIEW BESTROTATE vp1");
    EXPECT_TRUE(turned.starts_with("rotated vp1 rotation=")) << turned;
    EXPECT_EQ(s.steps(), steps + 1);
    EXPECT_NEAR(std::fmod(std::abs(s.view(0, 0).rotation), 3.14159265358979 / 2.0), 0.0, 1e-6);

    s.ok("VIEW ADD 1 notes");
    EXPECT_TRUE(contains(s.refused("VIEW FIT vp2").message, "plans and key plans"));
    EXPECT_EQ(s.refused("VIEW BESTROTATE vp9").code, ErrorCode::NotFound);
}

TEST(SheetAgentVerbs, SuggestPaperSaysThePaperAndAppliesItOnlyWhenAsked)
{
    Session s;
    drawRoad(s);
    s.ok("SHEET NEW PLAN paper=A4");
    s.ok("VIEW ADD 1 plan scale=500 centre=200,50");
    const std::size_t steps = s.steps();
    const std::string advice = s.ok("SHEET SUGGESTPAPER 1");
    // 400 m at 1:500 is 800 mm across: more than A1's 774 mm of room, so A0.
    EXPECT_TRUE(advice.starts_with("sheet 1 view=vp1 scale=500 paper=A0 orientation=landscape"))
        << advice;
    EXPECT_EQ(s.steps(), steps) << "advice alone changes nothing";
    EXPECT_EQ(s.set().sheets[0].paper, PaperSize::A4);
    EXPECT_TRUE(contains(s.ok("SHEET SUGGESTPAPER 1 scale=1000"), "paper=A2"));

    const std::string applied = s.ok("SHEET SUGGESTPAPER 1 apply=on");
    EXPECT_TRUE(contains(applied, "paper=A0")) << applied;
    EXPECT_EQ(s.steps(), steps + 1);
    EXPECT_EQ(s.set().sheets[0].paper, PaperSize::A0);

    s.ok("SHEET NEW NOTES");
    s.ok("VIEW ADD 2 notes");
    EXPECT_TRUE(contains(s.refused("SHEET SUGGESTPAPER 2").message, "no plan"));
}

// ---- sections ---------------------------------------------------------------------------------

TEST(SheetAgentVerbs, ASectionTakesAnAutomaticScale)
{
    Session s;
    s.ok("ALIGN NEW ROAD 0,0 200,0");
    s.ok("SHEET NEW SECTIONS");
    s.ok("VIEW ADD 1 long_section");
    EXPECT_TRUE(contains(s.ok("VIEW SET vp1 scale=auto"), "scale=auto"));
    EXPECT_TRUE(s.view(0, 0).autoScale);
    EXPECT_TRUE(contains(s.ok("VIEW SET vp1 scale=500"), "scale=500"));
    EXPECT_FALSE(s.view(0, 0).autoScale);
}

// ---- HELP ---------------------------------------------------------------------------------------

TEST(SheetAgentVerbs, HelpNamesEveryVerbAndOption)
{
    const std::string help = sheetVerbHelp();
    for (const char* word :
         {"SHEETS CHECK", "SHEETS PAGESETUP", "SHEET SUGGESTPAPER", "VIEW FIT", "VIEW BESTROTATE",
          "ARRANGE n", "ARRANGE ALIGN", "ARRANGE DISTRIBUTE", "ARRANGE MATCHSCALE", "register",
          "sheet_index", "revisions=", "legend=this_sheet", "grid=none", "gridinterval=",
          "PLOTSHEETS", "format=", "style=", "folder=", "pattern="}) {
        EXPECT_TRUE(contains(help, word)) << word;
    }
    EXPECT_TRUE(isSheetVerb("arrange"));
    Session s;
    EXPECT_EQ(s.ok("HELP ARRANGE"), help);
}
