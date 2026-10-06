// The customisation's session state on the Document (customisation_state.hpp):
// what a default Document has, what installing one sets and reports, what the
// raw setters and the three part-setters do to its origin, and the record a
// project keeps of it - worked out by the open, written by the save.
//
// Every customisation here is built in code or written as text in the test;
// nothing reads a file that is not the test's own.

#include <gtest/gtest.h>

#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

#if defined(_WIN32)
#include <process.h>
#else
#include <unistd.h>
#endif

#include "katana/cad/customisation_merge.hpp"
#include "katana/cad/customisation_state.hpp"
#include "katana/cad/document.hpp"
#include "katana/storage/project_store.hpp"

namespace fs = std::filesystem;
using katana::cad::CustomisationOrigin;
using katana::cad::CustomisationSource;
using katana::cad::CustomisationState;
using katana::cad::Document;
using katana::cad::DocumentChange;
using katana::core::ErrorCode;
using katana::entity::Color;
using katana::entity::Customisation;
using katana::entity::LineStyle;
using katana::entity::SurveyRule;

namespace {

// The parts of every notification, in order.
struct Recorder {
    std::vector<std::uint32_t> calls;
    Document::ListenerHandle handle;

    explicit Recorder(Document& document)
        : handle(document.addListener(
              [this](const DocumentChange& change) { calls.push_back(change.parts); }))
    {
    }
};

LineStyle definition(const char* name, const char* source, bool symbol)
{
    LineStyle style;
    style.name = name;
    style.source = source;
    style.symbol = symbol;
    style.strokes.push_back({katana::entity::StrokeOp::Draw, katana::geometry::Point2(1.0, 0.0)});
    return style;
}

// A customisation that says everything one can: two definitions (one listed
// as a symbol), one rule, one colour of its own, its own spelling of a
// control code and one automation switch off.
Customisation site()
{
    Customisation made;
    made.name = "Site";
    made.description = "Site standards";
    made.notice = {"For this site only."};
    made.basedOn = katana::entity::CustomisationBase{"NSW", "0123456789abcdef"};
    EXPECT_TRUE(made.colours.add("sui gas", Color{1, 2, 3, 255}).ok());
    katana::entity::LineworkCodes codes;
    codes.start = "S";
    made.linework = codes;
    made.automation = katana::entity::CustomisationAutomation{false, true};
    EXPECT_TRUE(made.library.add(definition("SITE Fence", "Site", false)).ok());
    EXPECT_TRUE(made.library.add(definition("SITE Peg", "Site", true)).ok());
    SurveyRule fence;
    fence.key = "FE*";
    fence.model = "FENCES";
    fence.colour = "sui gas";
    fence.linestyle = "SITE Fence";
    EXPECT_TRUE(made.map.add(fence).ok());
    return made;
}

// One rule and nothing else: a customisation of survey codes alone.
Customisation codesOnly(const char* name)
{
    Customisation made;
    made.name = name;
    SurveyRule rule;
    rule.key = "ZZ*";
    rule.model = "OTHER";
    EXPECT_TRUE(made.map.add(rule).ok());
    return made;
}

// The process id, so that two processes running these cases at once never
// share a folder: under ctest --parallel a test program is run a case at a
// time AND, for some suites, whole again at each SIMD level. With one fixed
// name, one process removed the folder while the other held a file in it
// open, and the uncaught filesystem_error ended that run. A reused id is
// harmless: the folder is cleared before it is used.
std::string processTag()
{
#if defined(_WIN32)
    return std::to_string(_getpid());
#else
    return std::to_string(getpid());
#endif
}

// A fresh directory for one test's projects, in one process's own, removed
// when the test ends.
struct Scratch {
    fs::path root;

    explicit Scratch(const char* name)
        : root(fs::temp_directory_path() /
               ("katana-cad-tests-customisation-state-" + processTag()) / name)
    {
        fs::remove_all(root);
        fs::create_directories(root);
    }
    ~Scratch()
    {
        std::error_code ignored;
        fs::remove_all(root.parent_path(), ignored);
    }
};

// A project whose record holds `names`, made through the store itself: a
// Document's save writes the record of ITS session, so it cannot make one
// that was recorded by a session of another time.
void projectRecording(const fs::path& directory, std::vector<std::string> names)
{
    katana::storage::ProjectMetadata metadata;
    metadata.customisation = std::move(names);
    auto store = katana::storage::ProjectStore::create(directory, metadata);
    ASSERT_TRUE(store.ok()) << store.error().describe();
    const auto saved =
        store->save(katana::storage::captureModel(katana::entity::Model{}, metadata));
    ASSERT_TRUE(saved.ok()) << saved.error().describe();
}

constexpr std::uint32_t kInstalled = DocumentChange::StyleLibrary | DocumentChange::SurveyMap |
                                     DocumentChange::Customisation;

// Why `status` is a refusal; that it is one is checked here, so that a call
// which wrongly succeeded fails the test instead of throwing out of it.
ErrorCode refusalOf(const katana::core::Status& status)
{
    EXPECT_FALSE(status.ok()) << "it was not refused";
    return status.ok() ? ErrorCode::Internal : status.error().code;
}

} // namespace

// ---- nothing, until something is installed ---------------------------------------------------

TEST(CustomisationState, ADefaultDocumentHasNoCustomisationAndInstallsNoneByItself)
{
    const Document document;
    const CustomisationState& state = document.customisationState();
    EXPECT_TRUE(state == CustomisationState{});
    EXPECT_TRUE(state.name.empty());
    EXPECT_EQ(state.origin, CustomisationOrigin::None);
    EXPECT_FALSE(state.kept);
    EXPECT_TRUE(state.sources.empty());
    EXPECT_TRUE(state.colours.empty());
    EXPECT_TRUE(state.missingAtOpen.empty());
    EXPECT_TRUE(state.builtIn.empty());
    EXPECT_FALSE(state.basedOn.has_value());
    // The control codes and the switches have defaults until something says
    // otherwise: the seven spellings, and both switches on.
    EXPECT_EQ(state.linework.start, "ST");
    EXPECT_EQ(state.linework.end, "END");
    EXPECT_EQ(state.linework.close, "CL");
    EXPECT_EQ(state.linework.arcStart, "BC");
    EXPECT_EQ(state.linework.arcEnd, "EC");
    EXPECT_EQ(state.linework.join, "JPN");
    EXPECT_EQ(state.linework.rectangle, "RECT");
    EXPECT_TRUE(state.automation.codesOnSurveyImport);
    EXPECT_TRUE(state.automation.lineworkOnSurveyImport);
    // And no definitions or rules: a Document never installs a customisation
    // itself, whatever is compiled into the program.
    EXPECT_TRUE(document.styleLibrary().empty());
    EXPECT_TRUE(document.surveyMap().empty());
    EXPECT_EQ(document.customisationGeneration(), 0u);
}

TEST(CustomisationState, AnOriginIsNamedByOneOfFiveWords)
{
    EXPECT_STREQ(katana::cad::toString(CustomisationOrigin::None), "none");
    EXPECT_STREQ(katana::cad::toString(CustomisationOrigin::BuiltIn), "builtIn");
    EXPECT_STREQ(katana::cad::toString(CustomisationOrigin::Kept), "kept");
    EXPECT_STREQ(katana::cad::toString(CustomisationOrigin::Loaded), "loaded");
    EXPECT_STREQ(katana::cad::toString(CustomisationOrigin::Edited), "edited");
}

// ---- installing a whole customisation --------------------------------------------------------

TEST(CustomisationState, InstallingSetsEveryPartAndReportsTheThreeOfThemInOneNotification)
{
    Document document;
    Recorder recorder(document);

    const auto installed = document.installCustomisation(site(), CustomisationOrigin::Loaded);
    ASSERT_TRUE(installed.ok()) << installed.error().describe();

    ASSERT_EQ(recorder.calls.size(), 1u);
    EXPECT_EQ(recorder.calls[0], kInstalled);
    EXPECT_TRUE(DocumentChange{.parts = recorder.calls[0]}.changesDrawing());
    // Each generation once, from 0.
    EXPECT_EQ(document.libraryGeneration(), 1u);
    EXPECT_EQ(document.surveyMapGeneration(), 1u);
    EXPECT_EQ(document.customisationGeneration(), 1u);

    const CustomisationState& state = document.customisationState();
    EXPECT_EQ(state.name, "Site");
    EXPECT_EQ(state.origin, CustomisationOrigin::Loaded);
    EXPECT_FALSE(state.kept);
    EXPECT_EQ(state.description, "Site standards");
    EXPECT_EQ(state.notice, (std::vector<std::string>{"For this site only."}));
    ASSERT_TRUE(state.basedOn.has_value());
    EXPECT_EQ(state.basedOn->name, "NSW");
    EXPECT_EQ(state.basedOn->digest, "0123456789abcdef");
    EXPECT_EQ(state.colours.size(), 1u);
    EXPECT_EQ(state.colours.find("sui gas"), (std::optional<Color>{Color{1, 2, 3, 255}}));
    EXPECT_EQ(state.linework.start, "S");
    EXPECT_EQ(state.linework.end, "END") << "the rest of what it says, which is the defaults";
    EXPECT_FALSE(state.automation.codesOnSurveyImport);
    EXPECT_TRUE(state.automation.lineworkOnSurveyImport);
    // It lists no sources, so it is its own one source, bringing both kinds.
    EXPECT_EQ(state.sources, (std::vector<CustomisationSource>{{"Site", true, true, {}}}));
    EXPECT_TRUE(state.missingAtOpen.empty());

    EXPECT_EQ(document.styleLibrary().size(), 2u);
    ASSERT_NE(document.definitionFor("SITE Peg"), nullptr);
    EXPECT_TRUE(document.definitionFor("SITE Peg")->symbol);
    EXPECT_FALSE(document.definitionFor("SITE Fence")->symbol);
    EXPECT_EQ(document.surveyMap().size(), 1u);
    EXPECT_EQ(document.surveyMap().lookup("FE01").resolved.model, "FENCES");
}

TEST(CustomisationState, ACustomisationThatListsItsSourcesIsInstalledWithThem)
{
    // A session kept after a second customisation was merged into it lists
    // both; read back, it must be the session it was.
    Customisation merged = site();
    merged.sources = {{"NSW", true, true, {"The first notice."}},
                      {"Site", true, false, {"For this site only."}}};
    Document document;
    ASSERT_TRUE(document.installCustomisation(merged, CustomisationOrigin::Kept, true).ok());
    EXPECT_EQ(document.customisationState().sources, merged.sources);
    EXPECT_EQ(document.customisationState().origin, CustomisationOrigin::Kept);
    EXPECT_TRUE(document.customisationState().kept);
}

TEST(CustomisationState, InstallingIsNotAnEditOfTheSessionOrOfTheDrawing)
{
    Document document;
    ASSERT_TRUE(document.installCustomisation(site(), CustomisationOrigin::BuiltIn, true).ok());
    // The library and the map were set, and the origin is still what was
    // said: the raw setters would have made it Edited.
    EXPECT_EQ(document.customisationState().origin, CustomisationOrigin::BuiltIn);
    EXPECT_TRUE(document.customisationState().kept);
    // Not undoable and not a change to the drawing.
    EXPECT_FALSE(document.isModified());
    EXPECT_EQ(document.history().undoCount(), 0u);
    EXPECT_EQ(document.modelRevision(), 0u);
}

// This test pinned the opposite when it was written, earlier in this change:
// that a customisation silent about the control codes and the switches left
// the SESSION's standing. That is the merge's rule - a load that says nothing
// changes nothing (MergeCustomisation.LineworkCodesAndAutomationAreTakenOnly-
// FromACustomisationThatSaysThem) - and it was wrong for an install, which
// takes the place of the whole session: installed `kept`, the session must be
// the one the next start gives, and that start has no earlier session whose
// spellings could be left behind. The expectation below is the format's own
// defaults (entity::LineworkCodes: start "ST"; both switches on).
TEST(CustomisationState, ACustomisationSilentAboutTheControlCodesAndTheSwitchesInstallsTheDefaults)
{
    Document document;
    ASSERT_TRUE(document.installCustomisation(site(), CustomisationOrigin::Loaded).ok());
    // The session spells start "S" and has coding off. A customisation that
    // says neither is installed over it: neither stays.
    ASSERT_TRUE(
        document.installCustomisation(codesOnly("Other"), CustomisationOrigin::Loaded).ok());
    EXPECT_EQ(document.customisationState().name, "Other");
    EXPECT_EQ(document.customisationState().linework.start, "ST");
    EXPECT_TRUE(document.customisationState().linework == katana::entity::LineworkCodes{});
    EXPECT_TRUE(document.customisationState().automation.codesOnSurveyImport);
    EXPECT_TRUE(document.customisationState().automation.lineworkOnSurveyImport);
    // What it does not hold of the rest is gone: an install takes the place
    // of the session, it is not a merge.
    EXPECT_TRUE(document.styleLibrary().empty());
    EXPECT_TRUE(document.customisationState().colours.empty());
    EXPECT_TRUE(document.customisationState().description.empty());
    EXPECT_FALSE(document.customisationState().basedOn.has_value());
    EXPECT_EQ(document.customisationState().sources,
              (std::vector<CustomisationSource>{{"Other", false, true, {}}}));
}

TEST(CustomisationState, AResetToASilentBuiltInLeavesTheSessionAFreshStartWithItWouldGive)
{
    // What a reset to the built-in does, by hand. The session's control codes
    // and switches were changed, and the built-in - which says neither - is
    // installed as what the next start would give. That start is a new
    // Document installing the same customisation, so the two sessions are
    // equal whatever the first held before: start "ST" again, both switches
    // on again.
    const Customisation builtIn = codesOnly("Built In");
    Document edited;
    katana::entity::LineworkCodes spelled;
    spelled.start = "BEG";
    ASSERT_TRUE(edited.setLineworkCodes(spelled).ok());
    edited.setAutomation({false, false});
    ASSERT_TRUE(edited.installCustomisation(builtIn, CustomisationOrigin::BuiltIn, true).ok());

    Document fresh;
    ASSERT_TRUE(fresh.installCustomisation(builtIn, CustomisationOrigin::BuiltIn, true).ok());

    EXPECT_TRUE(edited.customisationState() == fresh.customisationState());
    EXPECT_EQ(edited.customisationState().linework.start, "ST");
    EXPECT_TRUE(edited.customisationState().automation.codesOnSurveyImport);
    EXPECT_TRUE(edited.customisationState().automation.lineworkOnSurveyImport);
    EXPECT_TRUE(edited.customisationState().kept);
    EXPECT_EQ(edited.customisationState().origin, CustomisationOrigin::BuiltIn);
}

TEST(CustomisationState, ALoadMergedIntoTheSessionStillLeavesTheControlCodesItDoesNotSay)
{
    // The other half of the rule: a LOAD that is silent leaves the session's
    // alone. It does so through the two calls a load makes - the session as a
    // customisation, merged, and the result installed - because the session
    // always says its own, and the merge carries them forward.
    Document document;
    katana::entity::LineworkCodes spelled;
    spelled.start = "BEG";
    ASSERT_TRUE(document.setLineworkCodes(spelled).ok());
    document.setAutomation({false, true});

    const std::vector<Customisation> loaded{codesOnly("Other")};
    const katana::cad::CustomisationMerge merge = katana::cad::mergeCustomisation(
        document.customisation(), loaded, katana::cad::LoadMode::Merge);
    ASSERT_TRUE(merge.ok());
    ASSERT_TRUE(document.installCustomisation(merge.merged, CustomisationOrigin::Loaded).ok());

    EXPECT_EQ(document.surveyMap().size(), 1u) << "the load did come in";
    EXPECT_EQ(document.customisationState().linework.start, "BEG");
    EXPECT_FALSE(document.customisationState().automation.codesOnSurveyImport);
    EXPECT_TRUE(document.customisationState().automation.lineworkOnSurveyImport);
}

TEST(CustomisationState, TheSessionAsOneCustomisationInstallsBackAsTheSameSession)
{
    Document first;
    ASSERT_TRUE(first.installCustomisation(site(), CustomisationOrigin::Loaded).ok());
    const Customisation session = first.customisation();

    // Everything the session has, the control codes and switches said.
    EXPECT_EQ(session.name, "Site");
    EXPECT_EQ(session.description, "Site standards");
    EXPECT_EQ(session.notice, (std::vector<std::string>{"For this site only."}));
    EXPECT_EQ(session.sources, (std::vector<CustomisationSource>{{"Site", true, true, {}}}));
    ASSERT_TRUE(session.linework.has_value());
    EXPECT_EQ(session.linework->start, "S");
    ASSERT_TRUE(session.automation.has_value());
    EXPECT_FALSE(session.automation->codesOnSurveyImport);
    EXPECT_EQ(session.library.all(), first.styleLibrary().all());
    EXPECT_EQ(session.map, first.surveyMap());
    EXPECT_EQ(session.colours, first.customisationState().colours);

    Document second;
    ASSERT_TRUE(second.installCustomisation(session, CustomisationOrigin::Loaded).ok());
    EXPECT_TRUE(second.customisationState() == first.customisationState());
    EXPECT_TRUE(second.customisation() == session);

    // And through the file it would be kept in.
    const auto written = katana::entity::customisationToJson(session);
    ASSERT_TRUE(written.ok()) << written.error().describe();
    const auto read = katana::entity::customisationFromJson(*written);
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(*read == session);
}

TEST(CustomisationState, InstallRefusesWhatCouldNotBeKeptOrRecordedAndThenChangesNothing)
{
    Document document;
    ASSERT_TRUE(document.installCustomisation(site(), CustomisationOrigin::Loaded).ok());
    const CustomisationState before = document.customisationState();
    Recorder recorder(document);

    // "none" is where nothing came from.
    EXPECT_EQ(
        refusalOf(document.installCustomisation(codesOnly("Other"), CustomisationOrigin::None)),
        ErrorCode::InvalidArgument);
    // A name a project's record could not hold: none, a path, two lines.
    for (const char* name : {"", "roads/2026", "a\\b", "two\nlines"}) {
        EXPECT_EQ(refusalOf(document.installCustomisation(codesOnly(name),
                                                          CustomisationOrigin::Loaded)),
                  ErrorCode::InvalidArgument)
            << '"' << name << '"';
    }
    // The same of a source's name.
    Customisation badSource = codesOnly("Other");
    badSource.sources = {{"fine", false, true, {}}, {"not/fine", true, false, {}}};
    EXPECT_EQ(refusalOf(document.installCustomisation(badSource, CustomisationOrigin::Loaded)),
              ErrorCode::InvalidArgument);
    // Control codes that spell two controls alike.
    Customisation badCodes = codesOnly("Other");
    katana::entity::LineworkCodes alike;
    alike.start = "X";
    alike.close = "x";
    badCodes.linework = alike;
    EXPECT_EQ(refusalOf(document.installCustomisation(badCodes, CustomisationOrigin::Loaded)),
              ErrorCode::InvalidArgument);
    // A definition whose source is a path. A project records each
    // definition's source beside the customisation's own name, and the store
    // refuses a record holding a path: installed, EVERY save would then fail.
    Customisation badDefinition = codesOnly("Other");
    ASSERT_TRUE(badDefinition.library.add(definition("ROAD Kerb", "lib/roads", false)).ok());
    const auto pathAsSource =
        document.installCustomisation(badDefinition, CustomisationOrigin::Loaded);
    EXPECT_EQ(refusalOf(pathAsSource), ErrorCode::InvalidArgument);
    if (!pathAsSource.ok()) {
        EXPECT_NE(pathAsSource.error().message.find("\"ROAD Kerb\""), std::string::npos)
            << pathAsSource.error().describe();
    }
    // What it says it is based on, with a digest that is not sixteen
    // hexadecimal digits, or a name that is a path: the kept file's writer
    // refuses both, so a KEEP of the session would fail.
    Customisation badDigest = codesOnly("Other");
    badDigest.basedOn = katana::entity::CustomisationBase{"NSW", "0123"};
    EXPECT_EQ(refusalOf(document.installCustomisation(badDigest, CustomisationOrigin::Loaded)),
              ErrorCode::InvalidArgument);
    Customisation badBase = codesOnly("Other");
    badBase.basedOn = katana::entity::CustomisationBase{"a/b", "0123456789abcdef"};
    EXPECT_EQ(refusalOf(document.installCustomisation(badBase, CustomisationOrigin::Loaded)),
              ErrorCode::InvalidArgument);

    EXPECT_TRUE(document.customisationState() == before);
    EXPECT_EQ(document.styleLibrary().size(), 2u);
    EXPECT_EQ(document.surveyMap().size(), 1u);
    EXPECT_EQ(document.customisationGeneration(), 1u);
    EXPECT_TRUE(recorder.calls.empty());
}

TEST(CustomisationState, EveryPartThatCouldNotBeKeptOrRecordedIsAFaultThatNamesThePart)
{
    // Nothing wrong: site(), and a definition made in a session, which has no
    // source and needs none.
    Customisation sound = site();
    ASSERT_TRUE(sound.library.add(definition("MINE Mark", "", true)).ok());
    EXPECT_TRUE(katana::cad::customisationFaults(sound).empty());
    Document document;
    EXPECT_TRUE(document.installCustomisation(sound, CustomisationOrigin::Loaded).ok());

    // One of each planted, so five faults, in the order they are looked for:
    // the name, a source's name, a definition's source, the control codes,
    // what it is based on. Each says which part it is about first.
    Customisation faulty = site();
    faulty.name = "roads/2026";
    faulty.sources = {{"fine", true, true, {}}, {"not\\fine", true, false, {}}};
    ASSERT_TRUE(faulty.library.add(definition("ROAD Kerb", "lib/roads", false)).ok());
    katana::entity::LineworkCodes alike;
    alike.start = "X";
    alike.close = "x";
    faulty.linework = alike;
    faulty.basedOn = katana::entity::CustomisationBase{"NSW", "0123"};

    const std::vector<katana::core::Error> faults = katana::cad::customisationFaults(faulty);
    const std::vector<std::string> parts{"its name: ", "the name of one of its sources: ",
                                         "the source of its definition \"ROAD Kerb\": ",
                                         "its linework codes: ", "its basedOn: "};
    ASSERT_EQ(faults.size(), parts.size());
    for (std::size_t i = 0; i < parts.size(); ++i) {
        EXPECT_EQ(faults[i].code, ErrorCode::InvalidArgument) << i;
        EXPECT_EQ(faults[i].message.rfind(parts[i], 0), 0u) << faults[i].describe();
    }
}

// ---- the change bit --------------------------------------------------------------------------

TEST(CustomisationState, ItsChangeBitIsTheEighteenthAndIsNoPartOfWhatADrawingIsDrawnFrom)
{
    EXPECT_EQ(std::uint32_t{DocumentChange::Customisation}, 1u << 18);
    EXPECT_EQ(DocumentChange::kDrawing & DocumentChange::Customisation, 0u);
    // Alone - a switch, a name - it draws nothing.
    EXPECT_FALSE(DocumentChange{.parts = DocumentChange::Customisation}.changesDrawing());
    // And no other part shares the bit.
    for (const std::uint32_t other :
         {std::uint32_t{DocumentChange::Entities}, std::uint32_t{DocumentChange::Selection},
          DocumentChange::kTables, std::uint32_t{DocumentChange::CurrentAttributes},
          std::uint32_t{DocumentChange::History}, std::uint32_t{DocumentChange::Saved},
          std::uint32_t{DocumentChange::Metadata}, std::uint32_t{DocumentChange::StyleLibrary},
          std::uint32_t{DocumentChange::SurveyMap}, std::uint32_t{DocumentChange::Replaced},
          std::uint32_t{DocumentChange::Drafting}}) {
        EXPECT_EQ(other & DocumentChange::Customisation, 0u) << other;
    }
}

TEST(CustomisationState, ANewDrawingKeepsTheCustomisationAndDoesNotReportIt)
{
    Document document;
    ASSERT_TRUE(document.installCustomisation(site(), CustomisationOrigin::BuiltIn, true).ok());
    const CustomisationState before = document.customisationState();
    Recorder recorder(document);

    document.newDocument();

    ASSERT_EQ(recorder.calls.size(), 1u);
    const DocumentChange change{.parts = recorder.calls[0]};
    EXPECT_TRUE(change.has(DocumentChange::Replaced));
    EXPECT_FALSE(change.has(DocumentChange::Customisation));
    EXPECT_FALSE(change.has(DocumentChange::StyleLibrary));
    EXPECT_TRUE(document.customisationState() == before);
    EXPECT_EQ(document.styleLibrary().size(), 2u);
    EXPECT_EQ(document.customisationGeneration(), 1u);
}

// ---- the raw setters, which an editor calls --------------------------------------------------

TEST(CustomisationState, TheRawSettersMarkTheSessionEditedAndReportTheirOwnPartAlone)
{
    Document document;
    ASSERT_TRUE(document.installCustomisation(site(), CustomisationOrigin::BuiltIn, true).ok());
    Recorder recorder(document);

    katana::entity::StyleLibrary library;
    ASSERT_TRUE(library.add(definition("MINE Mark", "", true)).ok());
    document.setStyleLibrary(library);

    // Exactly what it has always reported, in one notification.
    ASSERT_EQ(recorder.calls.size(), 1u);
    EXPECT_EQ(recorder.calls[0], std::uint32_t{DocumentChange::StyleLibrary});
    EXPECT_EQ(document.customisationState().origin, CustomisationOrigin::Edited);
    EXPECT_FALSE(document.customisationState().kept);
    // The generation is how a panel showing the origin sees it: 1 for the
    // install, 2 for the edit.
    EXPECT_EQ(document.customisationGeneration(), 2u);
    // An edit keeps the name: it is still that customisation, changed.
    EXPECT_EQ(document.customisationState().name, "Site");

    document.setSurveyMap({});
    ASSERT_EQ(recorder.calls.size(), 2u);
    EXPECT_EQ(recorder.calls[1], std::uint32_t{DocumentChange::SurveyMap});
    EXPECT_EQ(document.customisationState().origin, CustomisationOrigin::Edited);
    EXPECT_EQ(document.customisationGeneration(), 2u) << "already edited: nothing of it changed";
}

TEST(CustomisationState, EitherRawSetterMakesAnEmptySessionAnEditedOne)
{
    Document byLibrary;
    byLibrary.setStyleLibrary({});
    EXPECT_EQ(byLibrary.customisationState().origin, CustomisationOrigin::Edited);
    EXPECT_FALSE(byLibrary.customisationState().kept);

    Document byMap;
    byMap.setSurveyMap({});
    EXPECT_EQ(byMap.customisationState().origin, CustomisationOrigin::Edited);
    EXPECT_EQ(byMap.customisationGeneration(), 1u);
}

// ---- the parts set on their own --------------------------------------------------------------

TEST(CustomisationState, AColourTableAlsoMovesTheLibraryGenerationAndReportsTheLibrary)
{
    Document document;
    ASSERT_TRUE(document.installCustomisation(site(), CustomisationOrigin::BuiltIn, true).ok());
    Recorder recorder(document);

    katana::entity::ColourTable colours;
    ASSERT_TRUE(colours.add("sui gas", Color{200, 100, 0, 255}).ok());
    document.setColourTable(colours);

    ASSERT_EQ(recorder.calls.size(), 1u);
    EXPECT_EQ(recorder.calls[0],
              std::uint32_t{DocumentChange::Customisation | DocumentChange::StyleLibrary});
    EXPECT_TRUE(DocumentChange{.parts = recorder.calls[0]}.changesDrawing())
        << "a pen inside a definition is drawn in another colour now";
    // What is baked from the library - a sprite, a thumbnail - holds the old
    // colour, and is dropped only when this moves.
    EXPECT_EQ(document.libraryGeneration(), 2u);
    EXPECT_EQ(document.customisationGeneration(), 2u);
    EXPECT_EQ(document.surveyMapGeneration(), 1u);
    EXPECT_EQ(document.customisationState().colours, colours);
    EXPECT_EQ(document.customisationState().origin, CustomisationOrigin::Edited);
    EXPECT_FALSE(document.customisationState().kept);

    // The table it already has is no change at all.
    document.setColourTable(colours);
    EXPECT_EQ(recorder.calls.size(), 1u);
    EXPECT_EQ(document.libraryGeneration(), 2u);
    EXPECT_EQ(document.customisationGeneration(), 2u);
}

TEST(CustomisationState, TheControlCodesAreCheckedAndReportedAsTheCustomisationAlone)
{
    Document document;
    Recorder recorder(document);

    katana::entity::LineworkCodes codes;
    codes.start = "BEGIN";
    codes.rectangle = ""; // an empty spelling switches the control off
    ASSERT_TRUE(document.setLineworkCodes(codes).ok());
    ASSERT_EQ(recorder.calls.size(), 1u);
    EXPECT_EQ(recorder.calls[0], std::uint32_t{DocumentChange::Customisation});
    EXPECT_EQ(document.customisationState().linework, codes);
    EXPECT_EQ(document.customisationState().origin, CustomisationOrigin::Edited);
    EXPECT_EQ(document.customisationGeneration(), 1u);
    EXPECT_EQ(document.libraryGeneration(), 0u);

    // A spelling with a blank could never be matched: refused, nothing moved.
    katana::entity::LineworkCodes blank = codes;
    blank.end = "THE END";
    EXPECT_EQ(refusalOf(document.setLineworkCodes(blank)), ErrorCode::InvalidArgument);
    // Two controls spelled alike, whatever the case.
    katana::entity::LineworkCodes alike = codes;
    alike.close = "begin";
    EXPECT_EQ(refusalOf(document.setLineworkCodes(alike)), ErrorCode::InvalidArgument);
    EXPECT_EQ(document.customisationState().linework, codes);
    // The codes it already has are no change.
    ASSERT_TRUE(document.setLineworkCodes(codes).ok());
    EXPECT_EQ(recorder.calls.size(), 1u);
    EXPECT_EQ(document.customisationGeneration(), 1u);
}

TEST(CustomisationState, TheSwitchesAndWhetherItIsKeptAreReportedAsTheCustomisationAlone)
{
    Document document;
    ASSERT_TRUE(document.installCustomisation(site(), CustomisationOrigin::BuiltIn, true).ok());
    Recorder recorder(document);

    // site() has coding off and linework on; turn linework off too.
    document.setAutomation({false, false});
    ASSERT_EQ(recorder.calls.size(), 1u);
    EXPECT_EQ(recorder.calls[0], std::uint32_t{DocumentChange::Customisation});
    EXPECT_FALSE(document.customisationState().automation.lineworkOnSurveyImport);
    EXPECT_EQ(document.customisationState().origin, CustomisationOrigin::Edited);
    EXPECT_FALSE(document.customisationState().kept) << "it differs from what was kept";
    document.setAutomation({false, false});
    EXPECT_EQ(recorder.calls.size(), 1u) << "the switches it already has";

    // Written to the kept file: kept again, and still an edited built-in.
    document.setCustomisationKept(true);
    ASSERT_EQ(recorder.calls.size(), 2u);
    EXPECT_EQ(recorder.calls[1], std::uint32_t{DocumentChange::Customisation});
    EXPECT_TRUE(document.customisationState().kept);
    EXPECT_EQ(document.customisationState().origin, CustomisationOrigin::Edited);
    document.setCustomisationKept(true);
    EXPECT_EQ(recorder.calls.size(), 2u);
    // 1 install, 2 switches, 3 kept.
    EXPECT_EQ(document.customisationGeneration(), 3u);
}

// ---- what a project records ------------------------------------------------------------------

TEST(CustomisationState, ASaveWritesTheRecordWithoutTheDrawingEverBeingModifiedByIt)
{
    const Scratch scratch("save-writes-the-record");
    // Scoped: the project is opened again below, by another Document.
    {
        Document document;
        ASSERT_TRUE(document.installCustomisation(site(), CustomisationOrigin::Loaded).ok());
        EXPECT_FALSE(document.isModified()) << "a customisation is not an edit of the drawing";
        EXPECT_TRUE(document.metadata().customisation.empty())
            << "nothing is recorded until a save";

        // A save with nowhere to go: refused, and the drawing is still
        // untouched. (The record was once written through setMetadata before
        // the save, which left exactly this drawing asking to be saved.)
        EXPECT_EQ(refusalOf(document.save()), ErrorCode::InvalidState);
        EXPECT_FALSE(document.isModified());
        EXPECT_TRUE(document.metadata().customisation.empty());

        // Somewhere to go: the record is the one name the session is drawn
        // with.
        Recorder recorder(document);
        const auto saved = document.saveAs(scratch.root / "site.katana");
        ASSERT_TRUE(saved.ok()) << saved.error().describe();
        EXPECT_FALSE(document.isModified());
        EXPECT_EQ(document.metadata().customisation, (std::vector<std::string>{"Site"}));

        // Saved again, the record is the same and only the save is reported.
        ASSERT_TRUE(document.save().ok());
        EXPECT_EQ(recorder.calls.back(), std::uint32_t{DocumentChange::Saved});
        EXPECT_FALSE(document.isModified());
    }

    // And it is IN the project, not only in this Document.
    Document reopened;
    ASSERT_TRUE(reopened.open(scratch.root / "site.katana").ok());
    EXPECT_EQ(reopened.metadata().customisation, (std::vector<std::string>{"Site"}));
}

TEST(CustomisationState, ASaveThatFailsPartWayLeavesTheRecordAndTheDrawingAsTheyWere)
{
    // The record is written INTO what is saved and becomes the metadata's
    // only once the save has succeeded, so a save the project store refuses
    // leaves no trace. (Written through setMetadata before the save, as it
    // once was, it stayed, and the untouched drawing asked to be saved.) The
    // refusal is arranged with the one thing a store refuses of a record - a
    // name that is a path - as a definition's source, given through the raw
    // setter, which checks nothing: an install would have refused it.
    const Scratch scratch("save-fails-part-way");
    const fs::path project = scratch.root / "site.katana";
    katana::entity::StyleLibrary unrecordable;
    ASSERT_TRUE(unrecordable.add(definition("ROAD Kerb", "lib/roads", false)).ok());
    {
        Document document;
        ASSERT_TRUE(document.installCustomisation(site(), CustomisationOrigin::Loaded).ok());
        ASSERT_TRUE(document.saveAs(project).ok());
        ASSERT_EQ(document.metadata().customisation, (std::vector<std::string>{"Site"}));

        document.setStyleLibrary(unrecordable);
        ASSERT_FALSE(document.isModified()) << "the library is session data, not the drawing";
        const katana::storage::ProjectMetadata before = document.metadata();
        Recorder recorder(document);

        // By hand: the record would be "Site" (a source that brought rules)
        // and then "lib/roads" (a definition's source no load accounts for),
        // and the store takes no name with a '/'.
        const auto refused = document.save();
        EXPECT_EQ(refusalOf(refused), ErrorCode::InvalidArgument);
        if (!refused.ok()) {
            EXPECT_NE(refused.error().context.find("lib/roads"), std::string::npos)
                << refused.error().describe();
        }
        EXPECT_TRUE(document.metadata() == before) << "nothing of the save is left in it";
        EXPECT_EQ(document.metadata().customisation, (std::vector<std::string>{"Site"}));
        EXPECT_FALSE(document.isModified());
        EXPECT_TRUE(recorder.calls.empty()) << "nothing was saved, so nothing is reported";
    }
    {
        // The same of a first save, which has no project yet - and has none
        // after it.
        Document unsaved;
        unsaved.setStyleLibrary(unrecordable);
        EXPECT_EQ(refusalOf(unsaved.saveAs(scratch.root / "never.katana")),
                  ErrorCode::InvalidArgument);
        EXPECT_TRUE(unsaved.metadata().customisation.empty());
        EXPECT_FALSE(unsaved.isModified());
        EXPECT_FALSE(unsaved.hasProject());
    }

    // The project still holds what the save that worked wrote.
    Document reopened;
    ASSERT_TRUE(reopened.open(project).ok());
    EXPECT_EQ(reopened.metadata().customisation, (std::vector<std::string>{"Site"}));
}

TEST(CustomisationState, AnOpenWorksOutWhatIsNotLoadedAndANewDrawingForgetsIt)
{
    const Scratch scratch("open-works-out-missing");
    const fs::path project = scratch.root / "recorded.katana";
    projectRecording(project, {"Site", "old symbols.4d"});

    {
        // Nothing loaded: both recorded names are missing, in the project's
        // order, and the open says the customisation's state changed.
        Document empty;
        Recorder recorder(empty);
        ASSERT_TRUE(empty.open(project).ok());
        EXPECT_EQ(empty.customisationState().missingAtOpen,
                  (std::vector<std::string>{"Site", "old symbols.4d"}));
        ASSERT_EQ(recorder.calls.size(), 1u);
        EXPECT_TRUE(DocumentChange{.parts = recorder.calls[0]}.has(DocumentChange::Replaced));
        EXPECT_TRUE(DocumentChange{.parts = recorder.calls[0]}.has(DocumentChange::Customisation));
        EXPECT_EQ(empty.customisationGeneration(), 1u);

        // A new drawing recorded nothing, so it is missing nothing.
        empty.newDocument();
        EXPECT_TRUE(empty.customisationState().missingAtOpen.empty());
        ASSERT_EQ(recorder.calls.size(), 2u);
        EXPECT_TRUE(DocumentChange{.parts = recorder.calls[1]}.has(DocumentChange::Customisation));
        EXPECT_EQ(empty.customisationGeneration(), 2u);
        // And a second new drawing has nothing to forget.
        empty.newDocument();
        EXPECT_FALSE(
            DocumentChange{.parts = recorder.calls[2]}.has(DocumentChange::Customisation));
        EXPECT_EQ(empty.customisationGeneration(), 2u);
    }
    {
        // "Site" loaded: only the other name is missing.
        Document withSite;
        ASSERT_TRUE(withSite.installCustomisation(site(), CustomisationOrigin::Loaded).ok());
        ASSERT_TRUE(withSite.open(project).ok());
        EXPECT_EQ(withSite.customisationState().missingAtOpen,
                  (std::vector<std::string>{"old symbols.4d"}));
        // The customisation itself is kept across the open.
        EXPECT_EQ(withSite.customisationState().name, "Site");
        EXPECT_EQ(withSite.styleLibrary().size(), 2u);
    }
}

TEST(CustomisationState, ASaveKeepsTheNamesTheOpenFoundMissingUntilTheyAreLoaded)
{
    const Scratch scratch("save-keeps-missing");
    const fs::path project = scratch.root / "site.katana";
    {
        Document author;
        ASSERT_TRUE(author.installCustomisation(site(), CustomisationOrigin::Loaded).ok());
        ASSERT_TRUE(author.saveAs(project).ok());
    }

    // Another session, with other codes and without "Site". By hand: the open
    // finds "Site" missing; an unedited save records what this session is
    // drawn with, "Other", and keeps the name it could not judge, after it.
    Document other;
    ASSERT_TRUE(other.installCustomisation(codesOnly("Other"), CustomisationOrigin::Loaded).ok());
    ASSERT_TRUE(other.open(project).ok());
    EXPECT_EQ(other.customisationState().missingAtOpen, (std::vector<std::string>{"Site"}));
    {
        Recorder recorder(other);
        ASSERT_TRUE(other.save().ok());
        EXPECT_EQ(other.metadata().customisation, (std::vector<std::string>{"Other", "Site"}));
        // The record the save wrote is the metadata's now, and is reported.
        ASSERT_EQ(recorder.calls.size(), 1u);
        EXPECT_EQ(recorder.calls[0],
                  std::uint32_t{DocumentChange::Saved | DocumentChange::Metadata});
        EXPECT_FALSE(other.isModified());
    }

    // "Site" installed in the place of "Other": it is loaded now, so it is no
    // longer missing; and "Other", which this session loaded and then
    // replaced, it has judged - the record lets it go.
    ASSERT_TRUE(other.installCustomisation(site(), CustomisationOrigin::Loaded).ok());
    EXPECT_TRUE(other.customisationState().missingAtOpen.empty());
    ASSERT_TRUE(other.save().ok());
    EXPECT_EQ(other.metadata().customisation, (std::vector<std::string>{"Site"}));
}

TEST(CustomisationState, TheBuiltInsEarlierFileNamesInARecordAreAnsweredByTheNameTheHostGave)
{
    const Scratch scratch("earlier-names");
    const fs::path project = scratch.root / "old.katana";
    const std::vector<std::string> general{"linestyles.4d", "survey_codes.mapfile",
                                           "survey_codes_names.mapfile", "symbols.4d"};
    projectRecording(project, general);

    {
        // The same customisation loaded, but no host has said it is the
        // built-in: the four names are four files this session does not have.
        Document untold;
        ASSERT_TRUE(untold.installCustomisation(site(), CustomisationOrigin::Loaded).ok());
        ASSERT_TRUE(untold.open(project).ok());
        EXPECT_EQ(untold.customisationState().missingAtOpen, general);
    }
    {
        // Told that the built-in is called "Site": the four files it once was
        // are answered by it, nothing is missing, and the save records the
        // one name the drawing is drawn with now.
        Document told;
        ASSERT_TRUE(told.installCustomisation(site(), CustomisationOrigin::BuiltIn, true).ok());
        Recorder recorder(told);
        told.setBuiltInCustomisationName("Site");
        ASSERT_EQ(recorder.calls.size(), 1u);
        EXPECT_EQ(recorder.calls[0], std::uint32_t{DocumentChange::Customisation});
        EXPECT_EQ(told.customisationState().builtIn, "Site");

        ASSERT_TRUE(told.open(project).ok());
        EXPECT_TRUE(told.customisationState().missingAtOpen.empty());
        ASSERT_TRUE(told.save().ok());
        EXPECT_EQ(told.metadata().customisation, (std::vector<std::string>{"Site"}));
    }
}

// ---- a load installed through the raw setters ------------------------------------------------

TEST(CustomisationState, ALoadRecordedByItsSourcesJoinsTheSessionsAndComesOffWhatIsMissing)
{
    const Scratch scratch("record-a-load");
    const fs::path project = scratch.root / "two.katana";
    projectRecording(project, {"a.4d", "b.mapfile"});

    Document document;
    ASSERT_TRUE(document.open(project).ok());
    ASSERT_EQ(document.customisationState().missingAtOpen,
              (std::vector<std::string>{"a.4d", "b.mapfile"}));
    Recorder recorder(document);

    // What a front end that read the files itself does: the two raw setters,
    // then the sources of what it read.
    document.setStyleLibrary({});
    document.recordCustomisationLoad({{"a.4d", true, false, {}}}, false, false);

    ASSERT_EQ(recorder.calls.size(), 2u);
    EXPECT_EQ(recorder.calls[1], std::uint32_t{DocumentChange::Customisation});
    EXPECT_EQ(document.customisationState().sources,
              (std::vector<CustomisationSource>{{"a.4d", true, false, {}}}));
    // A load, not an edit - the setter before it had said "edited".
    EXPECT_EQ(document.customisationState().origin, CustomisationOrigin::Loaded);
    EXPECT_FALSE(document.customisationState().kept);
    EXPECT_EQ(document.customisationState().missingAtOpen,
              (std::vector<std::string>{"b.mapfile"}));

    // A Replace by survey codes takes no library's place.
    document.recordCustomisationLoad({{"c.mapfile", false, true, {}}}, false, true);
    EXPECT_EQ(document.customisationState().sources,
              (std::vector<CustomisationSource>{{"a.4d", true, false, {}},
                                                {"c.mapfile", false, true, {}}}));
}
