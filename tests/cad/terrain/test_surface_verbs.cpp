// The drawing's surfaces and display styles on the command line
// (cad/terrain/surface_verbs.hpp): SURFACE RENAME, STYLE, SHOW, HIDE, COPY and
// SURFACESTYLE, through the interpreter every front end runs - each change one
// undo step, each refusal leaving the drawing as it was.

#include <gtest/gtest.h>

#include <algorithm>
#include <filesystem>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/scope_verbs.hpp"
#include "katana/cad/terrain/surface_style.hpp"
#include "katana/cad/terrain/surface_verbs.hpp"
#include "katana/commands/entity_commands.hpp"
#include "katana/storage/sqlite_database.hpp"
#include "surface_test_support.hpp"

using katana::cad::CommandInterpreter;
using katana::cad::Document;
using katana::cad::SurfaceDisplayStyle;
using katana::cad::builtInSurfaceStyles;
using katana::cad::surface_testing::tiltedSquare;
using katana::cad::surface_testing::with;
using katana::core::ErrorCode;

namespace {

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

class DrawingSurfaceVerbs : public ::testing::Test {
  protected:
    void SetUp() override
    {
        // Two surfaces, as SURFACE FROM would have made them.
        auto store = with({}, "ground", tiltedSquare(100.0, 1.0), "", true, "raster dem.tif");
        store = with(std::move(store), "design", tiltedSquare(99.0), "", true, "drawing (4 entities)");
        ASSERT_TRUE(document.setSurfaces(std::move(store), "SURFACE FROM").ok());
    }

    // `line` must run; its reply.
    std::string ok(const std::string& line)
    {
        auto reply = interpreter.run(line);
        EXPECT_TRUE(reply.ok()) << line << "\n" << (reply.ok() ? "" : reply.error().describe());
        return reply.ok() ? *reply : std::string();
    }

    // `line` must be refused with `code` and change nothing.
    void refused(const std::string& line, ErrorCode code)
    {
        const std::size_t steps = document.history().undoCount();
        const auto surfaces = document.surfaces().all();
        const auto styles = document.surfaceStyles();
        auto reply = interpreter.run(line);
        ASSERT_FALSE(reply.ok()) << line << "\n" << *reply;
        EXPECT_EQ(reply.error().code, code) << line << "\n" << reply.error().describe();
        EXPECT_EQ(document.history().undoCount(), steps) << line;
        EXPECT_EQ(document.surfaces().all(), surfaces) << line;
        EXPECT_TRUE(document.surfaceStyles() == styles) << line;
    }

    Document document;
    CommandInterpreter interpreter{document};
};

} // namespace

TEST_F(DrawingSurfaceVerbs, RenameKeepsTheSurfaceAndIsOneUndoStep)
{
    const auto ground = document.surfaces().find("ground")->surface;
    const std::string reply = ok("SURFACE RENAME GROUND \"Existing Ground\"");
    EXPECT_TRUE(contains(reply, "renamed surface=ground name=\"Existing Ground\"")) << reply;
    EXPECT_TRUE(contains(reply, "surface name=\"Existing Ground\" triangles=2 points=4")) << reply;
    ASSERT_NE(document.surfaces().find("existing ground"), nullptr);
    EXPECT_EQ(document.surfaces().find("existing ground")->surface, ground);
    EXPECT_EQ(document.history().undoName(), "SURFACE RENAME");
    ASSERT_TRUE(document.undo().ok());
    EXPECT_NE(document.surfaces().find("ground"), nullptr);
    EXPECT_EQ(document.surfaces().find("Existing Ground"), nullptr);
}

TEST_F(DrawingSurfaceVerbs, ARenameTheDrawingCannotTakeIsRefused)
{
    refused("SURFACE RENAME nothing other", ErrorCode::NotFound);
    refused("SURFACE RENAME ground DESIGN", ErrorCode::AlreadyExists);
    refused("SURFACE RENAME ground", ErrorCode::InvalidArgument);
    refused("SURFACE RENAME ground \"\"", ErrorCode::InvalidArgument);
}

TEST_F(DrawingSurfaceVerbs, AStyleIsGivenByItsNameInAnyCaseAndEmptyIsTheDefault)
{
    std::string reply = ok("SURFACE STYLE ground \"cut fill\"");
    EXPECT_EQ(document.surfaces().find("ground")->style, "Cut Fill") << "as the style spells it";
    EXPECT_TRUE(contains(reply, "style=\"Cut Fill\" visible=yes")) << reply;
    reply = ok("SURFACE STYLE ground \"\"");
    EXPECT_EQ(document.surfaces().find("ground")->style, "");
    EXPECT_TRUE(contains(reply, "style=Standard")) << "drawn in the default: " << reply;
    refused("SURFACE STYLE ground \"No Such Style\"", ErrorCode::NotFound);
    refused("SURFACE STYLE nothing Contours", ErrorCode::NotFound);
}

TEST_F(DrawingSurfaceVerbs, HideAndShowChangeOnlyWhatTheViewsDrawInOneStep)
{
    const std::size_t steps = document.history().undoCount();
    std::string reply = ok("SURFACE HIDE ALL");
    EXPECT_TRUE(contains(reply, "hidden surfaces=2 changed=2")) << reply;
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_FALSE(document.surfaces().find("ground")->visible);
    EXPECT_FALSE(document.surfaces().find("design")->visible);
    // Asked again, nothing changes and no step is made.
    reply = ok("SURFACE HIDE ALL");
    EXPECT_TRUE(contains(reply, "hidden surfaces=2 changed=0")) << reply;
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    reply = ok("surface show Ground");
    EXPECT_TRUE(contains(reply, "surface name=ground")) << reply;
    EXPECT_TRUE(contains(reply, "visible=yes")) << reply;
    EXPECT_TRUE(document.surfaces().find("ground")->visible);
    EXPECT_FALSE(document.surfaces().find("design")->visible);
    // One unknown name refuses the whole line.
    refused("SURFACE SHOW design nothing", ErrorCode::NotFound);
    refused("SURFACE SHOW", ErrorCode::InvalidArgument);
}

TEST_F(DrawingSurfaceVerbs, ShowAllOfNoSurfacesIsNotAnError)
{
    document.newDocument();
    const std::string reply = ok("SURFACE SHOW ALL");
    EXPECT_TRUE(contains(reply, "shown surfaces=0 changed=0")) << reply;
    EXPECT_FALSE(document.history().canUndo());
}

TEST_F(DrawingSurfaceVerbs, ACopySharesTheTriangulationAndKeepsTheStyle)
{
    (void)ok("SURFACE STYLE ground Contours");
    const std::string reply = ok("SURFACE COPY ground \"ground before\"");
    EXPECT_TRUE(contains(reply, "copied surface=ground name=\"ground before\"")) << reply;
    const auto* copy = document.surfaces().find("ground before");
    ASSERT_NE(copy, nullptr);
    EXPECT_EQ(copy->surface, document.surfaces().find("ground")->surface);
    EXPECT_EQ(copy->style, "Contours");
    EXPECT_EQ(copy->source, "copy of ground");
    EXPECT_EQ(document.history().undoName(), "SURFACE COPY");
    refused("SURFACE COPY ground design", ErrorCode::AlreadyExists);
    refused("SURFACE COPY nothing other", ErrorCode::NotFound);
}

TEST_F(DrawingSurfaceVerbs, OnlyTheDrawingsOwnSurfaceWordsAreClaimed)
{
    using katana::cad::isSurfaceVerb;
    EXPECT_TRUE(isSurfaceVerb({"surfacestyle"}));
    EXPECT_TRUE(isSurfaceVerb({"SURFACE", "rename", "a", "b"}));
    for (const char* word : {"LIST", "INFO", "REMOVE", "FROM", "EXPORT", "EDIT", "MERGE",
                             "DIFFERENCE", "ENVELOPE"}) {
        EXPECT_FALSE(isSurfaceVerb({"SURFACE", word})) << word;
    }
    EXPECT_FALSE(isSurfaceVerb({"SURFACE"}));
}

TEST_F(DrawingSurfaceVerbs, TheStylesAreListedAndTheirJsonIsTheDocumentsForm)
{
    const std::string reply = ok("SURFACESTYLE LIST");
    EXPECT_TRUE(contains(reply, "style name=Standard default=yes surfaces=2")) << reply;
    EXPECT_TRUE(contains(reply, "style name=\"Cut Fill\" default=no surfaces=0")) << reply;
    EXPECT_TRUE(contains(reply, "listed styles=7 default=Standard")) << reply;
    EXPECT_EQ(ok("SURFACESTYLE LIST JSON"), katana::cad::surfaceStylesToJson(document.surfaceStyles()));
    EXPECT_EQ(ok("SURFACESTYLE"), reply) << "LIST is what the verb alone says";
    refused("SURFACESTYLE LIST XML", ErrorCode::InvalidArgument);
}

TEST_F(DrawingSurfaceVerbs, AStyleIsMadeFromAnotherSetRenamedMadeTheDefaultAndDeleted)
{
    std::string reply = ok("SURFACESTYLE NEW Fine FROM contours interval=0.5 major=4");
    EXPECT_TRUE(contains(reply, "created style=Fine from=Contours")) << reply;
    const SurfaceDisplayStyle* fine = document.surfaceStyles().find("Fine");
    ASSERT_NE(fine, nullptr);
    EXPECT_TRUE(fine->contours.show) << "from Contours";
    EXPECT_EQ(fine->contours.interval, 0.5);
    EXPECT_EQ(fine->contours.majorEvery, 4);
    EXPECT_EQ(document.history().undoName(), "SURFACESTYLE NEW");

    // A colour by its standard name, and stops as value:colour pairs.
    reply = ok("SURFACESTYLE SET fine majorcolour=red stops=-1:#0000FF;1:#FF0000 faces=elevation");
    EXPECT_TRUE(contains(reply, "set style=Fine changed=yes")) << reply;
    fine = document.surfaceStyles().find("Fine");
    EXPECT_EQ(fine->contours.majorColour, (katana::entity::Color{255, 0, 0, 255}));
    ASSERT_EQ(fine->stops.size(), 2u);
    EXPECT_EQ(fine->stops[0].value, -1.0);
    EXPECT_EQ(fine->planFaces, katana::cad::SurfaceFaces::Elevation);

    // Renamed with a surface drawn in it: the surface follows, in one step.
    (void)ok("SURFACE STYLE ground Fine");
    const std::size_t steps = document.history().undoCount();
    reply = ok("SURFACESTYLE RENAME Fine \"Fine Contours\"");
    EXPECT_TRUE(contains(reply, "renamed style=Fine name=\"Fine Contours\" surfaces=1")) << reply;
    EXPECT_EQ(document.history().undoCount(), steps + 1);
    EXPECT_EQ(document.surfaces().find("ground")->style, "Fine Contours");

    reply = ok("SURFACESTYLE DEFAULT \"fine contours\"");
    EXPECT_TRUE(contains(reply, "default style=\"Fine Contours\"")) << reply;
    refused("SURFACESTYLE DELETE \"Fine Contours\"", ErrorCode::InvalidArgument); // the default
    (void)ok("SURFACESTYLE DEFAULT Standard");
    refused("SURFACESTYLE DELETE \"Fine Contours\"", ErrorCode::InvalidArgument); // drawn in
    (void)ok("SURFACE STYLE ground \"\"");
    reply = ok("SURFACESTYLE DELETE \"Fine Contours\"");
    EXPECT_TRUE(contains(reply, "deleted style=\"Fine Contours\" styles=7")) << reply;
    EXPECT_EQ(document.surfaceStyles().find("Fine Contours"), nullptr);
}

TEST_F(DrawingSurfaceVerbs, AStyleChangeThatCannotBeMadeChangesNothing)
{
    refused("SURFACESTYLE NEW Standard", ErrorCode::AlreadyExists);
    refused("SURFACESTYLE NEW", ErrorCode::InvalidArgument);
    refused("SURFACESTYLE NEW Fine FROM nothing", ErrorCode::NotFound);
    refused("SURFACESTYLE NEW Fine colour=red", ErrorCode::InvalidArgument); // no such key
    refused("SURFACESTYLE NEW Fine interval=0", ErrorCode::InvalidArgument); // cannot be drawn
    refused("SURFACESTYLE SET Standard opacity=half", ErrorCode::InvalidArgument);
    refused("SURFACESTYLE SET Standard opacity=1.5", ErrorCode::InvalidArgument);
    refused("SURFACESTYLE SET Standard boundarycolour=nocolour", ErrorCode::InvalidArgument);
    refused("SURFACESTYLE SET Standard stops=1:#FF0000;0:#0000FF", ErrorCode::InvalidArgument);
    refused("SURFACESTYLE SET Standard ramp=plaid", ErrorCode::InvalidArgument);
    refused("SURFACESTYLE SET Standard faces3d=none", ErrorCode::InvalidArgument);
    refused("SURFACESTYLE SET Standard", ErrorCode::InvalidArgument);
    refused("SURFACESTYLE SET nothing triangles=on", ErrorCode::NotFound);
    refused("SURFACESTYLE RENAME Standard Contours", ErrorCode::AlreadyExists);
    refused("SURFACESTYLE DELETE nothing", ErrorCode::NotFound);
    refused("SURFACESTYLE DEFAULT nothing", ErrorCode::NotFound);
    refused("SURFACESTYLE PAINT Standard", ErrorCode::InvalidArgument);
}

TEST_F(DrawingSurfaceVerbs, WhatInfoPrintsTypedBackMakesTheSameStyle)
{
    const std::string info = ok("SURFACESTYLE INFO \"Slope Bands\"");
    const std::size_t options = info.find("\noptions ");
    ASSERT_NE(options, std::string::npos) << info;
    const std::string words = info.substr(options + std::string("\noptions ").size());
    (void)ok("SURFACESTYLE NEW Again " + words);
    SurfaceDisplayStyle again = *document.surfaceStyles().find("Again");
    again.name = "Slope Bands";
    EXPECT_TRUE(again == *builtInSurfaceStyles().find("Slope Bands"));
}

TEST_F(DrawingSurfaceVerbs, OneOptionWordAtATimeLeavesTheStyleAsItWasWhenRefused)
{
    SurfaceDisplayStyle style;
    style.name = "Probe";
    const SurfaceDisplayStyle before = style;
    EXPECT_FALSE(katana::cad::setSurfaceStyleOption(style, "bands", "many").ok());
    EXPECT_FALSE(katana::cad::setSurfaceStyleOption(style, "stops", "0:#00FF00;x:#FF0000").ok());
    EXPECT_TRUE(style == before);
    EXPECT_TRUE(katana::cad::setSurfaceStyleOption(style, "min", "-2.5").ok());
    EXPECT_EQ(style.rangeMin, -2.5);
    EXPECT_TRUE(katana::cad::setSurfaceStyleOption(style, "min", "").ok());
    EXPECT_FALSE(style.rangeMin.has_value()) << "empty clears";
    EXPECT_TRUE(katana::cad::setSurfaceStyleOption(style, "stops", "").ok());
    EXPECT_TRUE(style.stops.empty());
}

TEST_F(DrawingSurfaceVerbs, AnImportersSurfacesJoinTheDrawingInTheImportsOneStepUnderFreeNames)
{
    const std::size_t steps = document.history().undoCount();
    std::vector<katana::terrain::NamedSurface> brought(2);
    brought[0].name = "ground"; // taken: becomes "ground (2)"
    brought[0].surface = tiltedSquare(50.0);
    brought[0].source = "IFC site.ifc";
    brought[1].name = "pad";
    brought[1].surface = tiltedSquare(51.0);
    auto records = katana::cad::importSurfaces(
        document, katana::commands::createPoint(katana::geometry::Point2(1.0, 1.0)),
        std::move(brought), "IMPORT site.ifc");
    ASSERT_TRUE(records.ok()) << records.error().describe();
    ASSERT_EQ(records->size(), 2u);
    EXPECT_TRUE(contains((*records)[0], "surface name=\"ground (2)\"")) << (*records)[0];
    EXPECT_TRUE(contains((*records)[1], "surface name=pad")) << (*records)[1];
    EXPECT_EQ(document.history().undoCount(), steps + 1) << "the point and both surfaces";
    EXPECT_EQ(document.model().entities.size(), 1u);
    ASSERT_TRUE(document.undo().ok());
    EXPECT_EQ(document.surfaces().all().size(), 2u) << "the two the fixture made";
    EXPECT_EQ(document.model().entities.size(), 0u);

    // None brought: the command alone, as it always ran.
    auto none = katana::cad::importSurfaces(
        document, katana::commands::createPoint(katana::geometry::Point2(2.0, 2.0)), {}, "IMPORT");
    ASSERT_TRUE(none.ok());
    EXPECT_TRUE(none->empty());
    EXPECT_EQ(document.model().entities.size(), 1u);
}

TEST_F(DrawingSurfaceVerbs, TheHelpNamesTheVerbsAndEveryKey)
{
    EXPECT_TRUE(contains(CommandInterpreter::helpText(), "SURFACE RENAME <name> <new>"));
    EXPECT_TRUE(contains(CommandInterpreter::helpText(), "SURFACESTYLE LIST [JSON]"));
    const std::string help = ok("HELP SURFACESTYLE");
    for (const auto& [key, value] : katana::cad::surfaceStyleOptions(SurfaceDisplayStyle{})) {
        EXPECT_TRUE(contains(help, key + "=")) << key;
    }
}

// ---- text the verbs refuse, and the reply that says what stands in -----------------------

TEST_F(DrawingSurfaceVerbs, NamesAndDescriptionsTypedInTheAnsiCodePageAreRefusedAndNothingChanges)
{
    // katana_cli on Windows arrives with 0xB0 for the degree sign and 0xE9 for e-acute: not
    // UTF-8, and the project stores a style set as JSON, whose writer threw out of SAVE.
    refused("SURFACESTYLE NEW Pente\xB0", ErrorCode::InvalidArgument);
    refused("SURFACESTYLE NEW Fine description=caf\xE9", ErrorCode::InvalidArgument);
    refused("SURFACESTYLE SET Contours description=caf\xE9", ErrorCode::InvalidArgument);
    refused("SURFACESTYLE RENAME Contours Pente\xB0", ErrorCode::InvalidArgument);
    refused("SURFACE RENAME ground terrain\xE9", ErrorCode::InvalidArgument);
    refused("SURFACE COPY ground caf\xE9", ErrorCode::InvalidArgument);

    // What was refused never reached the model, so a save has nothing to throw on.
    const katana::cad::surface_testing::ScratchDirectory scratch("ansi-verbs");
    EXPECT_NO_THROW({
        const auto saved = document.saveAs(scratch.path / "p.katana");
        EXPECT_TRUE(saved.ok()) << saved.error().describe();
    });
    // The same text as UTF-8 is accepted: nothing is wrong with the letters.
    (void)ok("SURFACESTYLE NEW Pente\xC2\xB0 description=caf\xC3\xA9");
    EXPECT_EQ(document.surfaceStyles().find("Pente\xC2\xB0")->description, "caf\xC3\xA9");
}

TEST_F(DrawingSurfaceVerbs, AnImportedNameWithAQuoteIsMadeNameableSoTheSurfaceCanBeAddressed)
{
    // A TIN named PAD "A" in a 12d archive, or an IFC string: the tokenizer has no escape inside
    // quotes, so no line could ever name it - listed, never removed, renamed or styled.
    std::vector<katana::terrain::NamedSurface> brought(2);
    brought[0].name = "PAD \"A\"";
    brought[0].surface = tiltedSquare(50.0);
    brought[1].name = "PAD \"A\"";
    brought[1].surface = tiltedSquare(51.0);
    auto records = katana::cad::importSurfaces(document, nullptr, std::move(brought), "IMPORT");
    ASSERT_TRUE(records.ok()) << records.error().describe();
    ASSERT_EQ(records->size(), 2u);
    EXPECT_TRUE(contains((*records)[0], "surface name=\"PAD 'A'\"")) << (*records)[0];
    EXPECT_TRUE(contains((*records)[1], "surface name=\"PAD 'A' (2)\"")) << (*records)[1];
    // And a line can now name it.
    EXPECT_TRUE(contains(ok("SURFACE HIDE \"PAD 'A'\""), "hidden surfaces=1 changed=1"));
    (void)ok("SURFACE RENAME \"PAD 'A'\" pad");
    EXPECT_NE(document.surfaces().find("pad"), nullptr);
}

TEST_F(DrawingSurfaceVerbs, HideOfASurfaceNamedAllHidesOnlyThatSurface)
{
    // The tokenizer drops quotes, so SURFACE HIDE "ALL" - what the Surfaces panel sends for the
    // check box of a surface called ALL - reached the verb as the keyword and hid every surface.
    (void)ok("SURFACE COPY ground ALL");
    const std::string hidden = ok("SURFACE HIDE \"ALL\"");
    EXPECT_TRUE(contains(hidden, "hidden surfaces=1 changed=1")) << hidden;
    EXPECT_TRUE(document.surfaces().find("ground")->visible);
    EXPECT_TRUE(document.surfaces().find("design")->visible);
    EXPECT_FALSE(document.surfaces().find("ALL")->visible);
    const std::string shown = ok("SURFACE SHOW all");
    EXPECT_TRUE(contains(shown, "shown surfaces=1 changed=1")) << shown;
    EXPECT_TRUE(document.surfaces().find("ALL")->visible);
    // Several names still work, ALL among them as a name.
    (void)ok("SURFACE HIDE ground ALL");
    EXPECT_FALSE(document.surfaces().find("ground")->visible);
    EXPECT_FALSE(document.surfaces().find("ALL")->visible);
    EXPECT_TRUE(document.surfaces().find("design")->visible);
    // With no surface of that name the keyword means all, as it always did.
    ASSERT_TRUE(document.surfaces().find("ALL") != nullptr);
    (void)ok("SURFACE RENAME ALL everything");
    EXPECT_TRUE(contains(ok("SURFACE SHOW ALL"), "shown surfaces=3 changed=2"));
}

TEST_F(DrawingSurfaceVerbs, ADescriptionsBackslashesSurviveWhatInfoPrintsTypedBack)
{
    // INFO prints the description through recordValue, which doubles each backslash; the verb
    // read it plain, so each trip doubled them again. A description is read as a typed text.
    (void)ok("SURFACESTYLE NEW Dir description=C:\\dir");
    EXPECT_EQ(document.surfaceStyles().find("Dir")->description, "C:\\dir") << "one backslash";
    for (const std::string description : {"C:\\dir", "a\\\\nb", "x\\", "plain", ""}) {
        auto styles = document.surfaceStyles();
        std::ranges::find_if(styles.styles, [](const SurfaceDisplayStyle& each) {
            return each.name == "Dir";
        })->description = description;
        ASSERT_TRUE(document.setSurfaceStyles(std::move(styles)).ok());
        const std::string info = ok("SURFACESTYLE INFO Dir");
        const std::size_t options = info.find("\noptions ");
        ASSERT_NE(options, std::string::npos) << info;
        (void)ok("SURFACESTYLE NEW Back " + info.substr(options + std::string("\noptions ").size()));
        EXPECT_EQ(document.surfaceStyles().find("Back")->description, description) << description;
        (void)ok("SURFACESTYLE DELETE Back");
    }
    // A backslash and an n typed on a line is a line break, which a description may not hold;
    // the refusal says what to type instead.
    const auto broken = interpreter.run("SURFACESTYLE NEW Line description=a\\nb");
    ASSERT_FALSE(broken.ok());
    EXPECT_EQ(broken.error().code, ErrorCode::InvalidArgument);
    EXPECT_TRUE(contains(broken.error().message, "two backslashes")) << broken.error().message;
}

namespace {

// A project whose `surface_styles` metadata the test chose, opened in `document`.
void openWithStoredStyles(katana::cad::Document& document, const std::string& stored,
                          const std::string& name)
{
    static const katana::cad::surface_testing::ScratchDirectory scratch("stored-styles");
    const std::filesystem::path project = scratch.path / (name + ".katana");
    {
        katana::cad::Document made;
        ASSERT_TRUE(made.saveAs(project).ok());
    }
    {
        auto raw = katana::storage::SqliteDatabase::open(project / "project.db");
        ASSERT_TRUE(raw.ok());
        ASSERT_TRUE(raw->execute("UPDATE metadata SET value = '" + stored +
                                 "' WHERE key = 'surface_styles'")
                        .ok());
    }
    ASSERT_TRUE(document.open(project).ok());
}

} // namespace

TEST_F(DrawingSurfaceVerbs, TheJsonOfStylesThatOnlyStandInSaysSoAndAHealthySetDoesNot)
{
    EXPECT_FALSE(contains(ok("SURFACESTYLE LIST JSON"), "standing_in")) << "a read set carries no note";

    Document newer;
    CommandInterpreter newerRun{newer};
    openWithStoredStyles(newer, R"({"format":"katana-surface-styles","version":99,"styles":[]})",
                         "newer");
    auto reply = newerRun.run("SURFACESTYLE LIST JSON");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_TRUE(contains(*reply, "\"standing_in\":{\"code\":\"Unsupported\"")) << *reply;
    EXPECT_TRUE(contains(*reply, "\"editable\":false")) << *reply;
    // The set itself is still there to read, in the form a save writes.
    EXPECT_TRUE(contains(*reply, "\"format\":\"katana-surface-styles\"")) << *reply;

    Document damaged;
    CommandInterpreter damagedRun{damaged};
    openWithStoredStyles(damaged, "{not json", "damaged");
    reply = damagedRun.run("SURFACESTYLE LIST JSON");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_TRUE(contains(*reply, "\"standing_in\":{\"code\":\"ParseFailure\"")) << *reply;
    EXPECT_TRUE(contains(*reply, "\"editable\":true")) << *reply;
    // What a save writes into the project never carries the note.
    EXPECT_FALSE(contains(katana::cad::surfaceStylesToJson(damaged.surfaceStyles()), "standing_in"));
}

TEST_F(DrawingSurfaceVerbs, ASetThatChangesNoFieldOnUnreadableStylesSaysItReplacedThem)
{
    Document damaged;
    CommandInterpreter damagedRun{damaged};
    openWithStoredStyles(damaged, "{not json", "damaged-set");
    const std::size_t steps = damaged.history().undoCount();
    // Standard already shows its boundary, so no field of the style moves.
    auto reply = damagedRun.run("SURFACESTYLE SET Standard boundary=on");
    ASSERT_TRUE(reply.ok()) << reply.error().describe();
    EXPECT_TRUE(contains(*reply, "set style=Standard changed=yes replaced=unreadable-styles"))
        << *reply;
    EXPECT_EQ(damaged.history().undoCount(), steps + 1) << "and the step it recorded is real";
    EXPECT_TRUE(damaged.surfaceStylesStatus().ok()) << "the built-ins are now the project's own";
    // A second one, on a set that reads, is changed=no as it always was.
    reply = damagedRun.run("SURFACESTYLE SET Standard boundary=on");
    ASSERT_TRUE(reply.ok());
    EXPECT_TRUE(contains(*reply, "set style=Standard changed=no\n")) << *reply;
    EXPECT_FALSE(contains(*reply, "replaced=")) << *reply;
}
