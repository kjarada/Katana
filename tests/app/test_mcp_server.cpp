// The MCP server (src/katana_app/mcp_server.hpp, docs/mcp.md), driven message
// by message as a client would drive it, without a process. What a command
// prints is the command line's own, which the cli.* tests pin; these pin the
// protocol around it and what the tools add: the batch that stops at a failure,
// the refusal to discard unsaved work, and the state reported after each call.

#include <algorithm>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <map>
#include <string>
#include <utility>
#include <vector>

#include <gtest/gtest.h>
#include <nlohmann/json.hpp>

#include "mcp_server.hpp"
#include "session.hpp"
#include "start_environment.hpp"

namespace {

using Json = nlohmann::json;
using katana::app::Session;
using katana::app::mcp::Server;
using katana::app::tests::StartEnvironment;

// A directory of its own per test, removed afterwards.
class TempDir {
  public:
    explicit TempDir(const std::string& name)
        : path_(std::filesystem::temp_directory_path() / ("katana-mcp-" + name))
    {
        std::error_code error;
        std::filesystem::remove_all(path_, error);
        std::filesystem::create_directories(path_, error);
    }
    ~TempDir()
    {
        std::error_code error;
        std::filesystem::remove_all(path_, error);
    }
    TempDir(const TempDir&) = delete;
    TempDir& operator=(const TempDir&) = delete;

    [[nodiscard]] std::string file(const std::string& name) const
    {
        return (path_ / name).generic_string();
    }

  private:
    std::filesystem::path path_;
};

class McpServer : public ::testing::Test {
  protected:
    // nullptr: no customisation, so what the tests see does not depend on
    // whether this checkout has the reference one.
    Session session{nullptr};
    Server server{session, "9.9.9"};
    int nextId = 1;

    // A message to `other`: the fixture's server, or one of a test's own over
    // a PROGRAM's session, which the fixture's is not.
    Json requestOf(Server& other, const std::string& method, Json params = Json::object())
    {
        const Json message{
            {"jsonrpc", "2.0"}, {"id", nextId++}, {"method", method}, {"params", params}};
        const auto reply = other.handle(message.dump());
        EXPECT_TRUE(reply.has_value()) << method;
        return reply ? Json::parse(*reply) : Json();
    }

    Json request(const std::string& method, Json params = Json::object())
    {
        return requestOf(server, method, std::move(params));
    }

    Json callOf(Server& other, const std::string& tool, Json arguments = Json::object())
    {
        const Json reply = requestOf(
            other, "tools/call", Json{{"name", tool}, {"arguments", std::move(arguments)}});
        EXPECT_TRUE(reply.contains("result")) << reply.dump();
        return reply.value("result", Json::object());
    }

    Json call(const std::string& tool, Json arguments = Json::object())
    {
        return callOf(server, tool, std::move(arguments));
    }

    static std::string textOf(const Json& result)
    {
        return result["content"][0]["text"].get<std::string>();
    }

    void initializeOf(Server& other)
    {
        (void)requestOf(other, "initialize",
                        Json{{"protocolVersion", "2025-06-18"},
                             {"capabilities", Json::object()},
                             {"clientInfo", {{"name", "test"}, {"version", "1"}}}});
    }

    void initialize() { initializeOf(server); }

    // One line through katana_run_commands.
    Json runLine(const std::string& line)
    {
        return call("katana_run_commands", Json{{"commands", Json::array({line})}});
    }

    // One of the committed Katana customisation files (tests/data/customisation):
    // its path, and its path quoted for a line.
    static std::string customisationPath(const char* name)
    {
        return (std::filesystem::path(KATANA_CUSTOMISATION_DATA) / name).generic_string();
    }
    static std::string customisationFile(const char* name)
    {
        return "\"" + customisationPath(name) + "\"";
    }

    // The three of them in one load, as an agent loads a customisation: by
    // their text, 3 linestyles, 4 symbols, and 11 rules over 8 distinct keys.
    // The session had no name, so it takes the first one's, test_linestyles.
    void loadTheThreeCustomisationFiles()
    {
        const Json loaded =
            runLine("CUSTOMISE " + customisationFile("test_linestyles.customisation.json") + " " +
                    customisationFile("test_symbols.customisation.json") + " " +
                    customisationFile("test_survey.customisation.json"));
        ASSERT_FALSE(loaded["isError"].get<bool>()) << textOf(loaded);
    }
};

TEST_F(McpServer, InitializeAgreesTheClientsVersionAndNamesTheServer)
{
    const Json reply = request(
        "initialize", Json{{"protocolVersion", "2025-03-26"}, {"capabilities", Json::object()}});
    ASSERT_TRUE(reply.contains("result")) << reply.dump();
    EXPECT_EQ(reply["result"]["protocolVersion"], "2025-03-26");
    EXPECT_EQ(reply["result"]["serverInfo"]["name"], "katana");
    EXPECT_EQ(reply["result"]["serverInfo"]["version"], "9.9.9");
    EXPECT_TRUE(reply["result"]["capabilities"].contains("tools"));
    EXPECT_TRUE(reply["result"].contains("instructions"));
}

TEST_F(McpServer, AnUnknownProtocolVersionIsAnsweredWithTheLatest)
{
    const Json reply = request("initialize", Json{{"protocolVersion", "1999-01-01"}});
    EXPECT_EQ(reply["result"]["protocolVersion"], katana::app::mcp::kLatestProtocolVersion);
}

TEST_F(McpServer, NotificationsAreNeverAnswered)
{
    EXPECT_FALSE(server.handle(R"({"jsonrpc":"2.0","method":"notifications/initialized"})"));
}

TEST_F(McpServer, TextThatIsNotJsonIsAParseError)
{
    const auto reply = server.handle("not json");
    ASSERT_TRUE(reply);
    const Json error = Json::parse(*reply);
    EXPECT_EQ(error["error"]["code"], -32700);
    EXPECT_TRUE(error["id"].is_null());
}

TEST_F(McpServer, AnUnknownMethodIsMethodNotFound)
{
    EXPECT_EQ(request("no/such/method")["error"]["code"], -32601);
}

TEST_F(McpServer, PingIsAnsweredWithAnEmptyResult)
{
    EXPECT_EQ(request("ping")["result"], Json::object());
}

TEST_F(McpServer, EveryToolIsListedWithAnObjectSchema)
{
    initialize();
    const Json tools = request("tools/list")["result"]["tools"];
    ASSERT_TRUE(tools.is_array());
    std::vector<std::string> names;
    for (const Json& tool : tools) {
        names.push_back(tool["name"].get<std::string>());
        EXPECT_EQ(tool["inputSchema"]["type"], "object") << tool["name"];
        EXPECT_FALSE(tool["description"].get<std::string>().empty()) << tool["name"];
    }
    for (const char* expected :
         {"katana_run_commands", "katana_run_script", "katana_help", "katana_status",
          "katana_new_project", "katana_open_project", "katana_save_project",
          "katana_list_entities", "katana_describe_entity", "katana_import", "katana_export",
          "katana_undo"}) {
        EXPECT_NE(std::find(names.begin(), names.end(), expected), names.end()) << expected;
    }
}

// What an agent reads first names every UTILITY action and the scope words
// they take, so what is drawn can be graded again and written back - and a
// survey or an import drawn as services - without asking HELP first.
TEST_F(McpServer, TheCommandToolNamesEveryUtilityActionAndTheScopeWords)
{
    initialize();
    const Json tools = request("tools/list")["result"]["tools"];
    std::string description;
    for (const Json& tool : tools) {
        if (tool["name"] == "katana_run_commands") {
            description = tool["description"].get<std::string>();
        }
    }
    ASSERT_FALSE(description.empty());
    for (const char* word : {"UTILITY REPORT", "VERIFY", "CLEARANCE", "CHECK", "DRAW", "REGRADE",
                             "SCHEDULE", "HELP UTILITY", "DRAWING", "SELECTION", "AREA x0,y0,x1,y1",
                             "LAYERS a,b [ONLY]", "WHERE key=value", "MODIFY",
                             "UTILITY DRAW <scope> METHOD <method>", "FIELDS column=property"}) {
        EXPECT_NE(description.find(word), std::string::npos) << word;
    }
}

// And the survey field-file verbs, with the way to hold a point of the
// drawing, so an agent finds how to import a file with no coordinates from
// the tool list alone.
TEST_F(McpServer, TheCommandToolNamesTheSurveyFieldFileVerbs)
{
    initialize();
    const Json tools = request("tools/list")["result"]["tools"];
    std::string description;
    for (const Json& tool : tools) {
        if (tool["name"] == "katana_run_commands") {
            description = tool["description"].get<std::string>();
        }
    }
    ASSERT_FALSE(description.empty());
    for (const char* words : {"SURVEY READ <file>", "SURVEY IMPORT <file>", "SETTINGS <file>",
                              "SET key=value", "SET control=<id>;drawing;", "FORWARD"}) {
        EXPECT_NE(description.find(words), std::string::npos) << words;
    }
    // What an import does with the survey codes, and the verb that strings
    // coded points afterwards: an agent that is not told of the two words
    // cannot turn the coding off, and one not told of LINEWORK draws the
    // lines by hand. The verb's own words as HELP LINEWORK lists them, with
    // PROPERTY and CHORD since the verb took them (ledger_C3.md, 1.5: this
    // held "[ORDER number|entity] [PREVIEW]", which CHORD now stands between).
    for (const char* words :
         {"[CODES on|off] [LINEWORK on|off]", "LINEWORK [<scope>]",
          "[PROPERTY <name>] [ORDER number|entity] [CHORD <length>] [PREVIEW]", "HELP LINEWORK"}) {
        EXPECT_NE(description.find(words), std::string::npos) << words;
    }
}

TEST_F(McpServer, AnUnknownToolIsInvalidParams)
{
    EXPECT_EQ(request("tools/call", Json{{"name", "katana_nothing"}})["error"]["code"], -32602);
}

TEST_F(McpServer, CommandsDrawIntoTheLiveDrawingAndReportItsState)
{
    initialize();
    const Json result = call(
        "katana_run_commands",
        Json{{"commands",
              {"LAYER NEW Kerb #FF0000", "LAYER SET Kerb", "RECT 0,0 30,20", "CIRCLE 15,10 5"}}});
    EXPECT_FALSE(result["isError"].get<bool>()) << textOf(result);
    EXPECT_NE(textOf(result).find("> RECT 0,0 30,20\nrectangle created"), std::string::npos)
        << textOf(result);
    const Json& status = result["structuredContent"]["status"];
    EXPECT_EQ(status["entities"], 2);
    EXPECT_EQ(status["currentLayer"], "Kerb");
    EXPECT_TRUE(status["modified"].get<bool>());

    const Json list = call("katana_list_entities");
    EXPECT_NE(textOf(list).find("Polyline  layer=Kerb  vertices=4  closed  length=100  area=600"),
              std::string::npos)
        << textOf(list);
}

TEST_F(McpServer, ABatchStopsAtTheFirstFailingCommandAndSaysWhich)
{
    initialize();
    const Json result =
        call("katana_run_commands", Json{{"commands", {"CIRCLE 0,0 -1", "POINT 1,1"}}});
    EXPECT_TRUE(result["isError"].get<bool>());
    const Json& lines = result["structuredContent"]["commands"];
    ASSERT_EQ(lines.size(), 2U);
    EXPECT_FALSE(lines[0]["ok"].get<bool>());
    EXPECT_NE(lines[0]["messages"].get<std::string>().find("radius must be positive"),
              std::string::npos);
    EXPECT_TRUE(lines[1]["skipped"].get<bool>());
    EXPECT_EQ(result["structuredContent"]["status"]["entities"], 0);
}

// What an agent is told of a field file whose setup stands on RTK marks, and
// where the import puts the shots: the hand-built file of the surveyio tests,
// its positions worked by hand in src/katana_app/CMakeLists.txt beside
// cli.survey_import_rtk_setup_field_file - the marks, two GNSS positions,
// orient the setup, and each shot's offset is applied.
TEST_F(McpServer, AnAgentImportsAFieldFileWhoseSetupStandsOnRtkMarks)
{
    initialize();
    const std::string file = std::string(KATANA_SURVEYIO_DATA) + "/fld/rtk_setup.fld";
    const Json result = call("katana_run_commands",
                             Json{{"commands", {"SURVEY READ \"" + file + "\"",
                                                "SURVEY IMPORT \"" + file + "\"", "LIST"}}});
    EXPECT_FALSE(result["isError"].get<bool>()) << textOf(result);
    const Json& lines = result["structuredContent"]["commands"];
    ASSERT_EQ(lines.size(), 3U);
    const std::string read = lines[0]["output"].get<std::string>();
    EXPECT_NE(read.find("survey file=rtk_setup.fld format=opcode-field-file parser=1.2 read=18 "
                        "skipped=0 warnings=0"),
              std::string::npos)
        << read;
    // Nine observations of the setup and the marks' two GNSS positions; the
    // marks and the shots are all placed by the reduction.
    EXPECT_NE(read.find("content setups=1 observations=11 points=0 unpositioned=4"),
              std::string::npos)
        << read;
    EXPECT_NE(lines[1]["output"].get<std::string>().find(
                  "imported job=job-1 entities=4 layer=survey/points"),
              std::string::npos)
        << lines[1];
    const std::string list = lines[2]["output"].get<std::string>();
    // The whole coordinate, not the start of a longer one: the line ends
    // after it.
    const auto listed = [&list](std::string_view at) {
        const std::size_t found = list.find(at);
        const std::size_t end = found == std::string::npos ? found : found + at.size();
        return found != std::string::npos &&
               (end == list.size() || list[end] == '\n' || list[end] == '\r');
    };
    EXPECT_TRUE(listed(" at 1010.5,5000")) << list;
    EXPECT_TRUE(listed(" at 1020,4999.5")) << list;
    EXPECT_EQ(result["structuredContent"]["status"]["entities"], 4);
}

// An agent reaches a survey field file and a WKT through the lines it sends:
// SURVEY IMPORT is the session's verb (survey_verbs.hpp), and CRS SET takes
// the WKT with its quotes, which the verb once lost (test_project_crs.cpp).
// The field file is the hand-built one of the surveyio tests; its 4 points
// are worked by hand in src/katana_app/CMakeLists.txt, beside
// cli.survey_import_field_file.
TEST_F(McpServer, AnAgentImportsAFieldFileAndSetsTheSystemByItsWkt)
{
    initialize();
    const std::string file = std::string(KATANA_SURVEYIO_DATA) + "/fld/setup.fld";
    // OGC WKT1 for EPSG:7856, a name with blanks in it: without its quotes
    // PROJ cannot read it.
    const std::string wkt =
        "PROJCS[\"GDA2020 / MGA zone 56\",GEOGCS[\"GDA2020\",DATUM[\"Geocentric_Datum_of_"
        "Australia_2020\",SPHEROID[\"GRS 1980\",6378137,298.257222101]],PRIMEM[\"Greenwich\",0],"
        "UNIT[\"degree\",0.0174532925199433]],PROJECTION[\"Transverse_Mercator\"],"
        "PARAMETER[\"latitude_of_origin\",0],PARAMETER[\"central_meridian\",153],"
        "PARAMETER[\"scale_factor\",0.9996],PARAMETER[\"false_easting\",500000],"
        "PARAMETER[\"false_northing\",10000000],UNIT[\"metre\",1],AUTHORITY[\"EPSG\",\"7856\"]]";
    const Json result = call("katana_run_commands",
                             Json{{"commands", {"SURVEY IMPORT \"" + file + "\"", "CRS SET " + wkt}}});
    EXPECT_FALSE(result["isError"].get<bool>()) << textOf(result);
    const Json& lines = result["structuredContent"]["commands"];
    ASSERT_EQ(lines.size(), 2U);
    const std::string imported = lines[0]["output"].get<std::string>();
    EXPECT_NE(imported.find("survey file=setup.fld format=opcode-field-file"), std::string::npos)
        << imported;
    EXPECT_NE(imported.find("imported job=job-1 entities=4 layer=survey/points"),
              std::string::npos)
        << imported;
    EXPECT_EQ(lines[1]["output"].get<std::string>(),
              "crs id=EPSG:7856 name=\"GDA2020 / MGA zone 56\" kind=\"projected\" units=metre")
        << lines[1];
    EXPECT_EQ(result["structuredContent"]["status"]["entities"], 4);
}

// A Sokkia SDR file reaches an agent through the same lines: the registry
// gives SURVEY READ and SURVEY IMPORT every format surveyio reads. The file
// is the hand-built one of the surveyio tests; its 4 points are worked by
// hand in src/katana_app/CMakeLists.txt, beside cli.survey_import_sokkia_sdr,
// and its 8 warnings are its 7 deleted records and its coordinate order
// (option 2, read east first), said once.
TEST_F(McpServer, AnAgentReadsAndImportsASokkiaSdrFile)
{
    initialize();
    const std::string file = std::string(KATANA_SURVEYIO_DATA) + "/sdr/traverse.sdr";
    const Json result =
        call("katana_run_commands", Json{{"commands", {"SURVEY READ \"" + file + "\"",
                                                       "SURVEY IMPORT \"" + file + "\""}}});
    EXPECT_FALSE(result["isError"].get<bool>()) << textOf(result);
    const Json& lines = result["structuredContent"]["commands"];
    ASSERT_EQ(lines.size(), 2U);
    const std::string read = lines[0]["output"].get<std::string>();
    EXPECT_NE(read.find("survey file=traverse.sdr format=sokkia-sdr parser=1.0 read=40 "
                        "skipped=7 warnings=8"),
              std::string::npos)
        << read;
    EXPECT_NE(read.find("warning record=8 text=\"the header's coordinate order option is 2"),
              std::string::npos)
        << read;
    EXPECT_NE(read.find("warning record=35 text=\"a deleted record (marked 'DD'), 02NM "
                        "(station), is not imported\""),
              std::string::npos)
        << read;
    EXPECT_NE(read.find("content setups=2 observations=36 points=2 unpositioned=2"),
              std::string::npos)
        << read;
    const std::string imported = lines[1]["output"].get<std::string>();
    EXPECT_NE(imported.find("imported job=job-1 entities=4 layer=survey/points"),
              std::string::npos)
        << imported;
    EXPECT_EQ(result["structuredContent"]["status"]["entities"], 4);
}

// An agent imports a field file that gives no coordinates by holding a point
// of the drawing, as the wizard's control can: FORWARD puts CP1 there, and
// SURVEY IMPORT's SET holds it (survey_verbs.hpp). The file is traverse.sdr
// less its coordinate records; alone it places nothing. Held, it places CP2,
// T1 and T2 where src/katana_app/CMakeLists.txt works them by hand beside
// cli.survey_import_holds_a_point_of_the_drawing, and CP1 stays the
// drawing's, not drawn again.
TEST_F(McpServer, AnAgentImportsAFileWithNoCoordinatesHoldingAPointOfTheDrawing)
{
    initialize();
    const std::string file =
        std::string(KATANA_SURVEYIO_DATA) + "/sdr/traverse_without_coordinates.sdr";
    const Json result = call(
        "katana_run_commands",
        Json{{"commands",
              {"SURVEY IMPORT \"" + file + "\"", "FORWARD 500000,4999999,100 0 1 0 CP1",
               "SURVEY IMPORT \"" + file + "\" SET control=CP1;drawing;fixed;0;fixed;0;fixed;0",
               "LIST"}}});
    EXPECT_FALSE(result["isError"].get<bool>()) << textOf(result);
    const Json& lines = result["structuredContent"]["commands"];
    ASSERT_EQ(lines.size(), 4U);
    const std::string alone = lines[0]["output"].get<std::string>();
    EXPECT_NE(alone.find("imported job=job-1 entities=0 layer=survey/points"), std::string::npos)
        << alone;
    const std::string held = lines[2]["output"].get<std::string>();
    EXPECT_NE(held.find("\nsettings file= set=1 differ=1\n"
                        "setting key=control value=CP1;drawing;fixed;0;fixed;0;fixed;0\n"
                        "imported job=job-3 entities=3 layer=survey/points"),
              std::string::npos)
        << held;
    // Where CP1 was held: the drawing's point FORWARD made, entity 2 - the
    // first import's job took number 1 (a job is numbered with the next
    // entity number) - and nothing adjusted, as the defaults have it.
    EXPECT_NE(held.find("\nheld id=CP1 from=drawing entity=2 northing=5e+06 easting=5e+05 "
                        "height=100\nreduction method=radiation adjustments=0 rejected=0\n"),
              std::string::npos)
        << held;
    const std::string list = lines[3]["output"].get<std::string>();
    // The whole coordinate, not the start of a longer one: the line ends
    // after it.
    const auto listed = [&list](std::string_view at) {
        const std::size_t found = list.find(at);
        const std::size_t end = found == std::string::npos ? found : found + at.size();
        return found != std::string::npos &&
               (end == list.size() || list[end] == '\n' || list[end] == '\r');
    };
    EXPECT_TRUE(listed(" at 500000,5000000")) << list;
    EXPECT_TRUE(listed(" at 500000,5000099.99993")) << list;
    EXPECT_TRUE(listed(" at 500149.795756,5000000")) << list;
    EXPECT_TRUE(listed(" at 500149.795756,5000079.98883")) << list;
    EXPECT_EQ(result["structuredContent"]["status"]["entities"], 4);
}

// HELP sent as a command is the session's whole help, what katana_help and
// --help give: IFC is the session's, not the interpreter's, and the
// interpreter's own HELP leaves it out. (CUSTOMISE was the session's too; it
// is the interpreter's now, and its block opens with the same words.)
TEST_F(McpServer, HelpSentAsACommandIsTheWholeSessionHelp)
{
    initialize();
    const Json result = call("katana_run_commands", Json{{"commands", {"HELP", "?"}}});
    EXPECT_FALSE(result["isError"].get<bool>()) << textOf(result);
    const std::string text = textOf(result);
    // The reply's text is trimmed at its end, so the help is sought without
    // its closing newline.
    std::string help = Session::helpText();
    while (!help.empty() && help.back() == '\n') {
        help.pop_back();
    }
    const std::size_t first = text.find(help);
    ASSERT_NE(first, std::string::npos) << text;
    EXPECT_NE(text.find(help, first + help.size()), std::string::npos) << text;
    EXPECT_NE(text.find("CUSTOMISE [REPLACE]"), std::string::npos);
    EXPECT_NE(text.find("IMPORT <file.ifc>"), std::string::npos);
}

TEST_F(McpServer, ABatchToldToCarryOnRunsPastAFailure)
{
    initialize();
    const Json result =
        call("katana_run_commands",
             Json{{"commands", {"CIRCLE 0,0 -1", "POINT 1,1"}}, {"stop_on_error", false}});
    EXPECT_TRUE(result["isError"].get<bool>());
    EXPECT_EQ(result["structuredContent"]["status"]["entities"], 1);
}

TEST_F(McpServer, NewAndOpenRefuseToDiscardUnsavedChangesUnlessToldTo)
{
    initialize();
    (void)call("katana_run_commands", Json{{"commands", {"POINT 1,1"}}});

    const Json refused = call("katana_new_project");
    EXPECT_TRUE(refused["isError"].get<bool>());
    EXPECT_NE(textOf(refused).find("unsaved changes"), std::string::npos) << textOf(refused);
    EXPECT_EQ(call("katana_status")["structuredContent"]["entities"], 1);

    // A typed NEW is held to the same rule as the tool.
    EXPECT_TRUE(call("katana_run_commands", Json{{"commands", {"NEW"}}})["isError"].get<bool>());

    const Json done = call("katana_new_project", Json{{"discard_unsaved_changes", true}});
    EXPECT_FALSE(done["isError"].get<bool>()) << textOf(done);
    EXPECT_EQ(call("katana_status")["structuredContent"]["entities"], 0);
}

TEST_F(McpServer, OpeningAPolylineIsAnEditNotADiscardOfTheDrawing)
{
    // OPEN SELECTION (and OPEN #id) opens polylines (docs/drawing.md): it
    // keeps the drawing, so the unsaved-changes guard NEW and OPEN of a
    // project are held to does not refuse it.
    initialize();
    const Json result = call(
        "katana_run_commands", Json{{"commands", {"RECT 0,0 30,20", "SELECT ALL", "OPEN SELECTION"}}});
    EXPECT_FALSE(result["isError"].get<bool>()) << textOf(result);
    EXPECT_EQ(result["structuredContent"]["status"]["entities"], 1);
}

TEST_F(McpServer, AProjectSavedByOneSessionOpensInAnother)
{
    const TempDir dir("save-open");
    const std::string project = dir.file("site project");
    initialize();
    (void)call("katana_run_commands", Json{{"commands", {"RECT 0,0 30,20"}}});
    const Json saved = call("katana_save_project", Json{{"path", project}});
    ASSERT_FALSE(saved["isError"].get<bool>()) << textOf(saved);
    EXPECT_FALSE(saved["structuredContent"]["status"]["modified"].get<bool>());

    Session other(nullptr);
    Server otherServer(other, "9.9.9");
    const Json message{
        {"jsonrpc", "2.0"},
        {"id", 1},
        {"method", "tools/call"},
        {"params", {{"name", "katana_open_project"}, {"arguments", {{"path", project}}}}}};
    const Json opened = Json::parse(*otherServer.handle(message.dump()));
    ASSERT_FALSE(opened["result"]["isError"].get<bool>()) << opened.dump();
    EXPECT_EQ(other.document().model().entities.size(), 1U);
}

TEST_F(McpServer, APathWithAQuoteIsRefusedRatherThanCutShort)
{
    initialize();
    const Json result = call("katana_save_project", Json{{"path", "a\"b"}});
    EXPECT_TRUE(result["isError"].get<bool>());
    EXPECT_FALSE(session.document().hasProject());
}

TEST_F(McpServer, ACommandWithALineBreakIsRefused)
{
    initialize();
    const Json result = call("katana_run_commands", Json{{"commands", {"POINT 1,1\nERASE"}}});
    EXPECT_TRUE(result["isError"].get<bool>());
    EXPECT_EQ(session.document().model().entities.size(), 0U);
}

TEST_F(McpServer, QuitIsNotRunAndTheSessionCarriesOn)
{
    initialize();
    const Json result = call("katana_run_commands", Json{{"commands", {"QUIT", "POINT 1,1"}}});
    EXPECT_FALSE(result["isError"].get<bool>());
    EXPECT_EQ(session.document().model().entities.size(), 1U);
}

TEST_F(McpServer, DescribeEntityDescribesTheEntityItNames)
{
    // The tool sends INFO <id>, which a session with the GIS module once took
    // for INFO <file>: every call answered that the file did not exist. A
    // 10 x 5 rectangle: perimeter 30, area 50.
    initialize();
    (void)call("katana_run_commands", Json{{"commands", {"RECT 0,0 10,5"}}});
    const Json described = call("katana_describe_entity", Json{{"id", 1}});
    EXPECT_FALSE(described["isError"].get<bool>()) << textOf(described);
    EXPECT_NE(textOf(described).find(
                  "> INFO #1\n1  Polyline  layer=0  vertices=4  closed  length=30  area=50"),
              std::string::npos)
        << textOf(described);

    const Json missing = call("katana_describe_entity", Json{{"id", 2}});
    EXPECT_TRUE(missing["isError"].get<bool>());
    EXPECT_NE(textOf(missing).find("entity does not exist"), std::string::npos) << textOf(missing);
}

TEST_F(McpServer, DescribeEntityNamesTheEntityWhateverFilesTheWorkingDirectoryHolds)
{
    // Files called 1 and #1 where the server runs - a mistyped shell 2>1
    // makes the first - once turned the tool's INFO 1 into INFO <file>, "no
    // importer reads files named ''". It sends INFO #1, always the entity.
    const TempDir folder("describe-beside-files");
    for (const char* name : {"1", "#1"}) {
        std::ofstream(folder.file(name)) << "";
    }
    struct WorkingDirectory {
        std::filesystem::path before = std::filesystem::current_path();
        ~WorkingDirectory() { std::filesystem::current_path(before); }
    } restore;
    std::filesystem::current_path(folder.file("."));
    ASSERT_TRUE(std::filesystem::exists("#1"));

    initialize();
    (void)call("katana_run_commands", Json{{"commands", {"RECT 0,0 10,5"}}});
    const Json described = call("katana_describe_entity", Json{{"id", 1}});
    EXPECT_FALSE(described["isError"].get<bool>()) << textOf(described);
    EXPECT_NE(textOf(described).find("1  Polyline  layer=0  vertices=4  closed  length=30  area=50"),
              std::string::npos)
        << textOf(described);
}

TEST_F(McpServer, AScriptsIndentedNoteRunsNothing)
{
    // A line whose first non-blank is '#' is a note, as File > Run Script
    // reads one; an indented one was once a command here, so the script
    // stopped at it and the lines after it were "not run".
    const TempDir folder("indented-notes");
    std::ofstream(folder.file("notes.kcs"))
        << "# a note\r\nRECT 0,0 10,5\r\n\r\n  # an indented note\r\n\t# a tabbed one\r\n"
           "CIRCLE 5,5 1\r\nLIST\r\n";
    initialize();
    const Json ran = call("katana_run_script", Json{{"path", folder.file("notes.kcs")}});
    EXPECT_FALSE(ran["isError"].get<bool>()) << textOf(ran);
    EXPECT_EQ(session.document().model().entities.size(), 2U) << textOf(ran);
    EXPECT_EQ(textOf(ran).find("not run"), std::string::npos) << textOf(ran);
}

TEST_F(McpServer, TheWindowsOwnVerbsAreRefusedSayingWhereTheyRun)
{
    // PLOT, SNAPSHOT, ONLINE and SCRIPT run only in the desktop window; an
    // agent once met "unknown command" and could not tell a typo from a verb
    // this surface lacks. The tool's description says so too. GRID and
    // EXAGGERATION were unknown commands here until 2026-09-30; VIEWS and
    // ZOOM are refused by the interpreter itself, which has no views here.
    initialize();
    for (const char* line : {"PLOT a.pdf", "PLOTSHEETS", "SNAPSHOT a.png", "ONLINE PROVIDERS",
                             "SCRIPT a.kcs", "GRID ON", "EXAGGERATION 2", "VIEWS",
                             "VIEWS LINK 1,2", "ZOOM IN", "Z"}) {
        const Json ran = call("katana_run_commands", Json{{"commands", {line}}});
        EXPECT_TRUE(ran["isError"].get<bool>()) << line;
        EXPECT_NE(textOf(ran).find("is the desktop window's"), std::string::npos) << textOf(ran);
        EXPECT_EQ(textOf(ran).find("unknown command"), std::string::npos) << textOf(ran);
    }
    const Json tools = request("tools/list")["result"]["tools"];
    const auto run = std::find_if(tools.begin(), tools.end(), [](const Json& each) {
        return each["name"] == "katana_run_commands";
    });
    ASSERT_NE(run, tools.end());
    EXPECT_NE((*run)["description"].get<std::string>().find("desktop window's verbs"),
              std::string::npos);
}

TEST_F(McpServer, TheImportToolSaysWhatLocalDoes)
{
    // It once said LOCAL imported "in the drawing's own coordinates rather
    // than reprojecting": LOCAL moves the data, it does not keep it.
    initialize();
    const Json tools = request("tools/list")["result"]["tools"];
    const auto tool = std::find_if(tools.begin(), tools.end(), [](const Json& each) {
        return each["name"] == "katana_import";
    });
    ASSERT_NE(tool, tools.end());
    const std::string local =
        (*tool)["inputSchema"]["properties"]["local"]["description"].get<std::string>();
    EXPECT_NE(local.find("lower-left corner sits at 0,0"), std::string::npos) << local;
    EXPECT_EQ((*tool)["description"].get<std::string>().find("reprojecting"), std::string::npos);
    const Json placement = (*tool)["inputSchema"]["properties"]["placement"];
    EXPECT_EQ(placement["enum"], (Json{"keep", "local", "alongside", "offset"}));
}

TEST_F(McpServer, TheImportToolRefusesAPlacementItCannotSay)
{
    initialize();
    const auto refused = [&](const Json& arguments) {
        const Json result = call("katana_import", arguments);
        EXPECT_TRUE(result["isError"].get<bool>()) << arguments.dump();
        return textOf(result);
    };
    EXPECT_NE(refused(Json{{"path", "a.dxf"}, {"placement", "beside"}}).find("keep, local"),
              std::string::npos);
    EXPECT_NE(refused(Json{{"path", "a.dxf"}, {"placement", "offset"}, {"offset_east", 1}})
                  .find("offset_north"),
              std::string::npos);
    EXPECT_NE(refused(Json{{"path", "a.dxf"}, {"placement", "alongside"}, {"local", true}})
                  .find("say different things"),
              std::string::npos);
}

#if defined(KATANA_TEST_WITH_INTEROP)
TEST_F(McpServer, ImportLocalMovesAQuotedPathsDataToTheOrigin)
{
    // The tool quotes the path and adds LOCAL; the session once took LOCAL
    // off and left the quotes on, so local: true failed for every GIS file.
    // In a folder with a blank in its name, so the quotes matter.
    // samples/gis/parcels.geojson spans (180, 0) to (365, 165): moved as one
    // piece to put that lower-left corner at 0,0, it spans (0, 0) to
    // (185, 165).
    const TempDir dir("import local");
    const std::string copy = dir.file("site parcels.geojson");
    std::filesystem::copy_file(std::filesystem::path(KATANA_GIS_SAMPLES) / "parcels.geojson",
                               std::filesystem::path(copy));
    initialize();
    const Json imported = call("katana_import", Json{{"path", copy}, {"local", true}});
    EXPECT_FALSE(imported["isError"].get<bool>()) << textOf(imported);
    const auto bounds = session.document().model().entities.bounds();
    EXPECT_DOUBLE_EQ(bounds.min.x, 0.0);
    EXPECT_DOUBLE_EQ(bounds.min.y, 0.0);
    EXPECT_DOUBLE_EQ(bounds.max.x, 185.0);
    EXPECT_DOUBLE_EQ(bounds.max.y, 165.0);

    // Without LOCAL the same file keeps its own coordinates.
    (void)call("katana_new_project", Json{{"discard_unsaved_changes", true}});
    const Json kept = call("katana_import", Json{{"path", copy}});
    EXPECT_FALSE(kept["isError"].get<bool>()) << textOf(kept);
    EXPECT_DOUBLE_EQ(session.document().model().entities.bounds().min.x, 180.0);

    // placement offset: moved by exactly what is given, (180, 0) to (190, -20.5).
    (void)call("katana_new_project", Json{{"discard_unsaved_changes", true}});
    const Json moved = call("katana_import", Json{{"path", copy},
                                                  {"placement", "offset"},
                                                  {"offset_east", 10},
                                                  {"offset_north", -20.5}});
    EXPECT_FALSE(moved["isError"].get<bool>()) << textOf(moved);
    EXPECT_DOUBLE_EQ(session.document().model().entities.bounds().min.x, 190.0);
    EXPECT_DOUBLE_EQ(session.document().model().entities.bounds().min.y, -20.5);
}

// katana_export's cloud writes a reference point cloud to a .laz, as GIS >
// Export Point Cloud does: the scan's 40 000 points (`pdal info`), all held,
// so no sample and no warning.
TEST_F(McpServer, ExportWritesAReferenceCloudByIdOrName)
{
    initialize();
    const TempDir folder("export-cloud");
    const Json imported = call(
        "katana_import",
        Json{{"path",
              (std::filesystem::path(KATANA_GIS_SAMPLES) / "survey_scan.las").generic_string()}});
    ASSERT_FALSE(imported["isError"].get<bool>()) << textOf(imported);
    const Json exported =
        call("katana_export", Json{{"path", folder.file("scan.laz")}, {"cloud", "survey_scan"}});
    ASSERT_FALSE(exported["isError"].get<bool>()) << textOf(exported);
    const Json& record = exported["structuredContent"]["records"][0];
    EXPECT_EQ(record["record"], "exported");
    EXPECT_EQ(record["kind"], "cloud");
    EXPECT_EQ(record["points"], 40000);
    EXPECT_EQ(record["sample"], false); // yes/no is a boolean in the records
    EXPECT_TRUE(std::filesystem::exists(folder.file("scan.laz")));
    const Json byId = call("katana_export", Json{{"path", folder.file("again.las")}, {"cloud", 1}});
    EXPECT_FALSE(byId["isError"].get<bool>()) << textOf(byId);
    const Json refused =
        call("katana_export", Json{{"path", folder.file("x.las")}, {"cloud", "nothing"}});
    EXPECT_TRUE(refused["isError"].get<bool>()) << textOf(refused);
}

TEST_F(McpServer, ImportReturnsStructuredRecords)
{
    // The reply's records as objects, numbers as numbers: an agent reads the
    // entities that came in, the move LOCAL made and the reference layer's
    // id without reading text. parcels.geojson: 8 features, the spoil heaps
    // a MultiPolygon of two, so 9 entities; its lower-left corner (180, 0)
    // moved to 0,0. terrain.asc: 120 x 90 cells.
    initialize();
    const std::string parcels =
        (std::filesystem::path(KATANA_GIS_SAMPLES) / "parcels.geojson").generic_string();
    const Json imported = call("katana_import", Json{{"path", parcels}, {"placement", "local"}});
    ASSERT_FALSE(imported["isError"].get<bool>()) << textOf(imported);
    const Json records = imported["structuredContent"]["records"];
    ASSERT_GE(records.size(), 2u) << records.dump();
    EXPECT_EQ(records[0]["record"], "imported");
    EXPECT_EQ(records[0]["kind"], "vector");
    EXPECT_EQ(records[0]["entities"], 9);
    EXPECT_EQ(records[0]["bounds"], (Json{0.0, 0.0, 185.0, 165.0}));
    EXPECT_EQ(records[1]["record"], "placed");
    EXPECT_EQ(records[1]["placement"], "local");
    EXPECT_EQ(records[1]["east"], -180);
    EXPECT_EQ(imported["structuredContent"]["status"]["entities"], 9);

    const Json raster = call(
        "katana_import",
        Json{{"path", (std::filesystem::path(KATANA_GIS_SAMPLES) / "terrain.asc").generic_string()}});
    ASSERT_FALSE(raster["isError"].get<bool>()) << textOf(raster);
    const Json layer = raster["structuredContent"]["records"][1];
    EXPECT_EQ(layer["record"], "reference");
    EXPECT_EQ(layer["id"], 1);
    EXPECT_EQ(layer["kind"], "raster");
    EXPECT_EQ(layer["width"], 120);
    EXPECT_EQ(layer["height"], 90);

    // EXPORT the same way: what was written, as numbers.
    const TempDir dir("export records");
    const Json exported = call("katana_export", Json{{"path", dir.file("parcels.gpkg")}});
    ASSERT_FALSE(exported["isError"].get<bool>()) << textOf(exported);
    const Json written = exported["structuredContent"]["records"][0];
    EXPECT_EQ(written["record"], "exported");
    EXPECT_EQ(written["driver"], "GPKG");
    EXPECT_EQ(written["features"], 9);
}

TEST_F(McpServer, ImportTakesItsFilterScopeAndPreviewArgumentsAsTheWordsAPersonTypes)
{
    // tests/geo/data/lots.geojson, by hand: A and B kind=lot, 50 x 40 side by
    // side from (0, 0); C kind=corridor across both at y = 18..22. The box
    // (40, 10) - (60, 30) meets all three; kind = 'lot' leaves A and B.
    initialize();
    const std::string lots =
        (std::filesystem::path(KATANA_GIS_SAMPLES) / "../../tests/geo/data/lots.geojson")
            .lexically_normal()
            .generic_string();
    const Json previewed =
        call("katana_import", Json{{"path", lots},
                                   {"where", "kind = 'lot'"},
                                   {"area", Json{40, 10, 60, 30}},
                                   {"clip", true},
                                   {"preview", true}});
    ASSERT_FALSE(previewed["isError"].get<bool>()) << textOf(previewed);
    EXPECT_EQ(previewed["structuredContent"]["commands"][0]["command"],
              "IMPORT \"" + lots + "\" where=\"kind = 'lot'\" AREA 40,10,60,30 clip PREVIEW");
    const Json records = previewed["structuredContent"]["records"];
    ASSERT_GE(records.size(), 2u) << records.dump(); // and any warning GDAL gave
    EXPECT_EQ(records[0]["record"], "scope");
    EXPECT_EQ(records[1]["record"], "import");
    EXPECT_EQ(records[1]["features"], 2);
    EXPECT_EQ(records[1]["of"], 3);
    EXPECT_EQ(previewed["structuredContent"]["status"]["entities"], 0);

    const Json imported = call("katana_import", Json{{"path", lots},
                                                     {"fields", Json{"name"}},
                                                     {"target_layer", "site"},
                                                     {"max_features", 2}});
    ASSERT_FALSE(imported["isError"].get<bool>()) << textOf(imported);
    EXPECT_EQ(imported["structuredContent"]["commands"][0]["command"],
              "IMPORT \"" + lots + "\" fields=name target=site max=2");
    EXPECT_EQ(imported["structuredContent"]["status"]["entities"], 2);

    const Json refused =
        call("katana_import", Json{{"path", lots}, {"open_options", Json{"NOPE=1"}}});
    EXPECT_TRUE(refused["isError"].get<bool>());
    EXPECT_NE(textOf(refused).find("NOPE"), std::string::npos) << textOf(refused);
}

TEST_F(McpServer, ExportTakesTheSharedScopeAndItsOptionsAsTheWordsAPersonTypes)
{
    // tests/geo/data/lots.geojson imported: three closed polylines on layer
    // lots, the two lots and the corridor (kind=corridor).
    initialize();
    const std::string lots =
        (std::filesystem::path(KATANA_GIS_SAMPLES) / "../../tests/geo/data/lots.geojson")
            .lexically_normal()
            .generic_string();
    ASSERT_FALSE(call("katana_import", Json{{"path", lots}})["isError"].get<bool>());
    const TempDir dir("export options");
    const std::string out = dir.file("lots.gpkg");

    const Json previewed = call("katana_export", Json{{"path", out},
                                                      {"scope", "layers"},
                                                      {"layers", Json{"lots"}},
                                                      {"where", Json{"PROP=kind:lot"}},
                                                      {"layer_name", "parcels"},
                                                      {"preview", true}});
    ASSERT_FALSE(previewed["isError"].get<bool>()) << textOf(previewed);
    EXPECT_EQ(previewed["structuredContent"]["commands"][0]["command"],
              "EXPORT \"" + out + "\" LAYERS lots WHERE PROP=kind:lot layername=parcels PREVIEW");
    const Json records = previewed["structuredContent"]["records"];
    ASSERT_GE(records.size(), 2u) << records.dump();
    EXPECT_EQ(records[0]["record"], "export");
    EXPECT_EQ(records[0]["entities"], 2);
    EXPECT_FALSE(std::filesystem::exists(out));

    const Json written = call("katana_export", Json{{"path", out}, {"split_by_layer", true}});
    ASSERT_FALSE(written["isError"].get<bool>()) << textOf(written);
    EXPECT_EQ(written["structuredContent"]["records"][0]["record"], "exported");
    EXPECT_EQ(written["structuredContent"]["records"][0]["features"], 3);
    EXPECT_EQ(written["structuredContent"]["records"][0]["layers"], "lots");

    const Json refused = call("katana_export", Json{{"path", dir.file("refused.gpkg")},
                                                    {"layer_creation_options", Json{"NOPE=1"}}});
    EXPECT_TRUE(refused["isError"].get<bool>());
    EXPECT_NE(textOf(refused).find("NOPE"), std::string::npos) << textOf(refused);
}

// "where" without "scope" filters the selection, as WHERE alone does on the
// command line - the one scope parser's rule - on katana_export and on a
// katana_gdal_run input alike; it was refused here and taken there. Two
// lines drawn, one selected: one line is exported, one buffered.
TEST_F(McpServer, WhereWithoutAScopeFiltersTheSelectionAsTheLineDoes)
{
    initialize();
    (void)call("katana_run_commands", Json{{"commands",
                                            {"LINE 0,0 10,0", "LINE 0,5 10,5", "RECT 20,0 30,10",
                                             "SELECT NONE", "SELECT 1"}}});
    const TempDir dir("where alone");
    const std::string out = dir.file("lines.geojson");
    const Json exported = call("katana_export", Json{{"path", out}, {"where", Json{"TYPE=line"}}});
    ASSERT_FALSE(exported["isError"].get<bool>()) << textOf(exported);
    EXPECT_EQ(exported["structuredContent"]["commands"][0]["command"],
              "EXPORT \"" + out + "\" SELECTION WHERE TYPE=line");
    EXPECT_EQ(exported["structuredContent"]["records"][0]["features"], 1)
        << exported["structuredContent"]["records"].dump();

    const Json buffered =
        call("katana_gdal_run", Json{{"algorithm", "vector buffer"},
                                     {"arguments", {{"distance", 1}}},
                                     {"inputs", {{"input", {{"where", {"TYPE=line"}}}}}},
                                     {"output", {{"layer", "gis/b"}}}});
    ASSERT_FALSE(buffered["isError"].get<bool>()) << textOf(buffered);
    EXPECT_EQ(buffered["structuredContent"]["line"],
              "GDAL vector buffer --distance=1 FROM input SELECTION WHERE TYPE=line TO LAYER "
              "gis/b");
    EXPECT_EQ(buffered["structuredContent"]["scope"][0]["matched"], 1);

    const Json layersAlone = call("katana_export", Json{{"path", out}, {"layers", Json{"0"}}});
    EXPECT_TRUE(layersAlone["isError"].get<bool>()) << textOf(layersAlone);
}

TEST_F(McpServer, AnArgumentTheToolDoesNotDeclareIsRefusedByNameAndNothingRuns)
{
    // "split" is not katana_export's word ("split_by_layer" is). Ignored, the
    // call wrote one layer where the agent asked for one per drawing layer.
    initialize();
    const std::string lots =
        (std::filesystem::path(KATANA_GIS_SAMPLES) / "../../tests/geo/data/lots.geojson")
            .lexically_normal()
            .generic_string();
    ASSERT_FALSE(call("katana_import", Json{{"path", lots}})["isError"].get<bool>());
    const TempDir dir("undeclared argument");
    const std::string out = dir.file("lots.gpkg");
    const Json refused = call("katana_export", Json{{"path", out}, {"split", true}});
    EXPECT_TRUE(refused["isError"].get<bool>());
    EXPECT_NE(textOf(refused).find("katana_export takes no argument \"split\""), std::string::npos)
        << textOf(refused);
    EXPECT_NE(textOf(refused).find("split_by_layer"), std::string::npos) << textOf(refused);
    EXPECT_FALSE(std::filesystem::exists(out));
    // A tool that takes no arguments refuses any, saying so.
    const Json none = call("katana_status", Json{{"verbose", true}});
    EXPECT_TRUE(none["isError"].get<bool>());
    EXPECT_NE(textOf(none).find("it takes none"), std::string::npos) << textOf(none);
}

TEST_F(McpServer, DatasetInfoReturnsRecordsAndGdalsJson)
{
    // terrain.asc's header: 120 x 90 cells of 1.5 from (-5, -5), no-data
    // -9999. The tool changes nothing and reads only the file.
    initialize();
    const std::string terrain =
        (std::filesystem::path(KATANA_GIS_SAMPLES) / "terrain.asc").generic_string();
    const Json described =
        call("katana_dataset_info", Json{{"path", terrain}, {"json", true}, {"check", true}});
    ASSERT_FALSE(described["isError"].get<bool>()) << textOf(described);
    const Json& content = described["structuredContent"];
    EXPECT_EQ(content["line"], "INFO \"" + terrain + "\" CHECK");
    std::map<std::string, Json> first;
    for (const Json& record : content["records"]) {
        first.emplace(record["record"].get<std::string>(), record);
    }
    EXPECT_EQ(first["dataset"]["driver"], "AAIGrid");
    EXPECT_EQ(first["raster"]["width"], 120);
    EXPECT_EQ(first["raster"]["bounds"], (Json{-5.0, -5.0, 175.0, 130.0}));
    EXPECT_EQ(first["band"]["nodata"], -9999);
    EXPECT_EQ(first["check"]["code"], 0);
    EXPECT_EQ(content["gdal"]["raster"]["driverShortName"], "AAIGrid");
    EXPECT_EQ(session.document().model().entities.size(), 0u);
}

TEST_F(McpServer, ReferencesActsOnALayerByIdOrNameAndListsThemAfter)
{
    initialize();
    const std::string terrain =
        (std::filesystem::path(KATANA_GIS_SAMPLES) / "terrain.asc").generic_string();
    ASSERT_FALSE(call("katana_import", Json{{"path", terrain}})["isError"].get<bool>());

    const Json hidden = call("katana_references", Json{{"action", "hide"}, {"id", 1}});
    ASSERT_FALSE(hidden["isError"].get<bool>()) << textOf(hidden);
    EXPECT_EQ(hidden["structuredContent"]["line"], "REFS hide 1");
    EXPECT_EQ(hidden["structuredContent"]["records"][0]["visible"], false);
    ASSERT_EQ(hidden["structuredContent"]["references"].size(), 1u);
    EXPECT_EQ(hidden["structuredContent"]["references"][0]["name"], "terrain");

    const Json faded =
        call("katana_references", Json{{"action", "opacity"}, {"id", "terrain"}, {"value", 0.25}});
    ASSERT_FALSE(faded["isError"].get<bool>()) << textOf(faded);
    EXPECT_EQ(faded["structuredContent"]["references"][0]["opacity"], 0.25);

    // Overviews write beside the raster's file: refused unless confirmed.
    const Json refused = call("katana_references", Json{{"action", "overviews"}, {"id", 1}});
    EXPECT_TRUE(refused["isError"].get<bool>());
    EXPECT_NE(textOf(refused).find("confirm"), std::string::npos) << textOf(refused);

    const Json listed = call("katana_references");
    EXPECT_EQ(listed["structuredContent"]["line"], "REFS LIST");
    EXPECT_EQ(listed["structuredContent"]["references"][0]["id"], 1);
}
#endif

TEST_F(McpServer, UndoAndRedoStepThroughTheHistory)
{
    initialize();
    (void)call("katana_run_commands", Json{{"commands", {"POINT 1,1", "POINT 2,2"}}});
    EXPECT_EQ(call("katana_undo", Json{{"steps", 2}})["structuredContent"]["status"]["entities"],
              0);
    EXPECT_EQ(call("katana_undo", Json{{"redo", true}})["structuredContent"]["status"]["entities"],
              1);
}

// What the Sheets editor's Generate Sheets writes reaches an agent as the
// same line: here its "Rotate the drawing to fill the sheet", rotate=on, on
// a strip along 45 degrees, which a square plan shows at 1:2000.
TEST_F(McpServer, TheGenerateLineTheSheetsEditorWritesReachesTheRunCommandsTool)
{
    initialize();
    const Json result = call(
        "katana_run_commands",
        Json{{"commands",
              {"LINE -152.028,-166.170 166.170,152.028", "LINE 166.170,152.028 152.028,166.170",
               "LINE 152.028,166.170 -166.170,-152.028", "LINE -166.170,-152.028 -152.028,-166.170",
               "GENERATE fit rotate=on legend=off", "VIEW LIST"}}});
    EXPECT_FALSE(result["isError"].get<bool>()) << textOf(result);
    EXPECT_NE(textOf(result).find("generated 1 sheet: 1"), std::string::npos) << textOf(result);
    EXPECT_NE(textOf(result).find("scale=1250"), std::string::npos) << textOf(result);
}

// The Sheets editor's Sheet Set menu runs SHEETS lines; an agent appends
// another set's sheets with the same line, renumbered after these.
TEST_F(McpServer, TheSheetSetMenusAppendReachesTheRunCommandsTool)
{
    initialize();
    const TempDir dir("sheets-append");
    const std::string file = "\"" + dir.file("north set.json") + "\"";
    const Json result = call("katana_run_commands",
                             Json{{"commands",
                                   {"SHEET NEW NORTH", "SHEETS SAVE " + file, "SHEET RENAME 1 COVER",
                                    "SHEETS APPEND " + file, "SHEETS"}}});
    EXPECT_FALSE(result["isError"].get<bool>()) << textOf(result);
    EXPECT_NE(textOf(result).find("appended 1 sheet from " + file + ": 2"), std::string::npos)
        << textOf(result);
    EXPECT_NE(textOf(result).find("sheet 2 id=s2 name=\"NORTH\""), std::string::npos) << textOf(result);
}

TEST_F(McpServer, AnOlderClientGetsTheFactsAsTextOnly)
{
    (void)request("initialize", Json{{"protocolVersion", "2024-11-05"}});
    const Json result = call("katana_status");
    EXPECT_FALSE(result.contains("structuredContent"));
    EXPECT_NE(textOf(result).find("Entities: 0"), std::string::npos) << textOf(result);
}

TEST_F(McpServer, TheHelpAndStatusAreResources)
{
    initialize();
    const Json list = request("resources/list")["result"]["resources"];
    // The help, the status and - one more than there were, since 2026-10-06 -
    // the customisation (McpServer.TheCustomisationSummaryIsCadsReportAsAToolAndAsAResource):
    // that one added resource is the whole reason each count below went up
    // by one, from 3 and 2.
#if defined(KATANA_TEST_WITH_INTEROP)
    // And the formats GDAL reads and writes (McpServer.FormatsReturnsStructuredDrivers).
    ASSERT_EQ(list.size(), 4U);
#else
    ASSERT_EQ(list.size(), 3U);
#endif
    std::vector<std::string> uris;
    for (const Json& resource : list) {
        uris.push_back(resource["uri"].get<std::string>());
    }
    for (const char* uri : {"katana://help", "katana://status", "katana://customisation"}) {
        EXPECT_NE(std::find(uris.begin(), uris.end(), uri), uris.end()) << uri;
    }
    const Json help = request("resources/read", Json{{"uri", "katana://help"}});
    EXPECT_NE(help["result"]["contents"][0]["text"].get<std::string>().find("CUSTOMISE"),
              std::string::npos);
    const Json status = request("resources/read", Json{{"uri", "katana://status"}});
    EXPECT_EQ(Json::parse(status["result"]["contents"][0]["text"].get<std::string>())["entities"],
              0);
    EXPECT_EQ(request("resources/read", Json{{"uri", "katana://nothing"}})["error"]["code"],
              -32002);
}

// katana_status and katana://status are the STATUS verb's (cad/document_status.hpp),
// so an agent reads one record whichever front end it drives. The resource's
// text is written out by hand for this drawing - one rectangle, selected, one
// undo step, no project, no customisation (the session loads none) - in the
// form it has always had: every key in alphabetical order, indented by two.
TEST_F(McpServer, TheStatusToolAndResourceSayWhatTheStatusVerbSays)
{
    initialize();
    (void)call("katana_run_commands", Json{{"commands", {"RECT 0,0 30,20", "SELECT ALL"}}});
    const Json verbs = call("katana_run_commands", Json{{"commands", {"STATUS", "STATUS JSON"}}});
    const Json& lines = verbs["structuredContent"]["commands"];
    const std::string text = lines[0]["output"].get<std::string>();
    const std::string json = lines[1]["output"].get<std::string>();

    const Json tool = call("katana_status");
    EXPECT_EQ(textOf(tool), text);
    EXPECT_EQ(tool["structuredContent"], Json::parse(json));
    const Json resource = request("resources/read", Json{{"uri", "katana://status"}});
    const std::string read = resource["result"]["contents"][0]["text"].get<std::string>();
    EXPECT_EQ(read, json);
    EXPECT_EQ(read, "{\n"
                    "  \"alignments\": 0,\n"
                    "  \"currentLayer\": \"0\",\n"
                    "  \"currentStyle\": \"\",\n"
                    "  \"entities\": 1,\n"
                    "  \"layers\": 1,\n"
                    "  \"modified\": true,\n"
                    "  \"project\": null,\n"
                    "  \"redoSteps\": 0,\n"
                    "  \"selected\": 1,\n"
                    "  \"styleLibraryDefinitions\": 0,\n"
                    "  \"surveyCodeRules\": 0,\n"
                    "  \"undoSteps\": 1\n"
                    "}");
    EXPECT_EQ(text, "Project: (none - not saved to a project yet)  [unsaved changes]\n"
                    "Entities: 1  Layers: 1  Alignments: 0\n"
                    "Current layer: 0\n"
                    "Selected: 1  Undo steps: 1  Redo steps: 0\n"
                    "Customisation: 0 linestyles and symbols, 0 survey code rules");
}

// ---- the customisation ----------------------------------------------------------------------
//
// katana_customisation and katana://customisation (docs/mcp.md, "The
// customisation"). What is expected is counted from the text of the three
// committed fixtures and from the format's chapter of docs/customisation.md:
// a rule and a definition come back as the very objects their file holds.

// What an agent reads first says how a customisation is loaded and written,
// and names no file or verb of another program.
TEST_F(McpServer, TheToolListSaysHowACustomisationIsLoadedWrittenAndRead)
{
    initialize();
    const Json tools = request("tools/list")["result"]["tools"];
    std::string commands;
    Json customisation;
    for (const Json& tool : tools) {
        if (tool["name"] == "katana_run_commands") {
            commands = tool["description"].get<std::string>();
        } else if (tool["name"] == "katana_customisation") {
            customisation = tool;
        }
    }
    ASSERT_FALSE(commands.empty());
    ASSERT_TRUE(customisation.is_object());
    const std::string description = customisation["description"].get<std::string>();
    for (const char* words : {"CODE LIST", "CODE CHECK", "CUSTOMISE <file>",
                              "CUSTOMISE EXPORT <file>", "HELP CUSTOMISE"}) {
        EXPECT_NE(commands.find(words), std::string::npos) << words;
    }
    // The round trip that edits one, where the tool that only reads sends it.
    for (const char* words : {"CUSTOMISE EXPORT <file>", "CUSTOMISE <file>",
                              "CUSTOMISE REPLACE <file>", "katana_run_commands"}) {
        EXPECT_NE(description.find(words), std::string::npos) << words;
    }
    // Two things a caller cannot work out and must be told: the summary is
    // one object with none of the lists' envelope, and an entry read here
    // goes into a file only without the one member that is the tool's own
    // (McpServer.ARuleOrADefinitionTheToolGaveLoadsBackOnceTheToolsOwnMemberIsTakenOff).
    for (const char* words : {"with no part, total, offset or limit",
                              "index from a rule, kind from a definition"}) {
        EXPECT_NE(description.find(words), std::string::npos) << words;
    }
    // MAPFILE listed and checked the codes until it became CODE LIST and CODE
    // CHECK; it and the two file kinds it was named with no longer exist here.
    for (const char* gone : {"MAPFILE", ".mapfile", ".4d"}) {
        EXPECT_EQ(commands.find(gone), std::string::npos) << gone;
        EXPECT_EQ(description.find(gone), std::string::npos) << gone;
    }
    EXPECT_EQ(customisation["inputSchema"]["required"], Json::array({"part"}));
    EXPECT_EQ(customisation["inputSchema"]["properties"]["part"]["enum"],
              Json::array({"summary", "codes", "definitions", "problems"}));
    EXPECT_TRUE(customisation["annotations"]["readOnlyHint"].get<bool>());
}

TEST_F(McpServer, TheCustomisationSummaryIsCadsReportAsAToolAndAsAResource)
{
    initialize();
    // A session of no program starts with nothing, and says so.
    const Json empty = call("katana_customisation", Json{{"part", "summary"}});
    ASSERT_FALSE(empty["isError"].get<bool>()) << textOf(empty);
    EXPECT_EQ(empty["structuredContent"]["origin"], "none");
    EXPECT_EQ(empty["structuredContent"]["name"], "");
    EXPECT_EQ(empty["structuredContent"]["counts"]["definitions"], 0);
    EXPECT_EQ(empty["structuredContent"]["counts"]["rules"], 0);

    loadTheThreeCustomisationFiles();
    const Json summary = call("katana_customisation", Json{{"part", "summary"}});
    ASSERT_FALSE(summary["isError"].get<bool>()) << textOf(summary);
    const Json& content = summary["structuredContent"];
    EXPECT_EQ(content["name"], "test_linestyles");
    EXPECT_EQ(content["origin"], "loaded");
    EXPECT_EQ(content["kept"], false);
    EXPECT_EQ(content["counts"]["definitions"], 7); // 3 linestyles and 4 symbols
    EXPECT_EQ(content["counts"]["rules"], 11);
    EXPECT_EQ(content["counts"]["codes"], 8);
    // Each file is a source, by the name it declares, with what it brought.
    ASSERT_EQ(content["sources"].size(), 3U);
    EXPECT_EQ(content["sources"][0]["name"], "test_linestyles");
    EXPECT_EQ(content["sources"][0]["definitions"], true);
    EXPECT_EQ(content["sources"][0]["rules"], false);
    EXPECT_EQ(content["sources"][1]["name"], "test_symbols");
    EXPECT_EQ(content["sources"][2]["name"], "test_survey");
    EXPECT_EQ(content["sources"][2]["definitions"], false);
    EXPECT_EQ(content["sources"][2]["rules"], true);
    // The one name the rules ask for that nothing defines, as the survey
    // fixture's own comment says of its PX* rule.
    EXPECT_EQ(content["problems"]["undefined"], Json::array({"TEST Missing Symbol"}));
    // None of the three files says the settings, so they are the defaults.
    EXPECT_EQ(content["automation"]["codesOnSurveyImport"], true);
    EXPECT_EQ(content["automation"]["lineworkOnSurveyImport"], true);
    EXPECT_EQ(content["linework"]["start"], "ST");
    EXPECT_EQ(content["linework"]["rectangle"], "RECT");
    // It is cad's object and nothing beside it: the lists' envelope is not
    // put round one object, and no member of the tool's own stands among
    // cad's, so the tool, the resource and the verb give one object.
    for (const char* own : {"part", "total", "offset", "limit"}) {
        EXPECT_FALSE(content.contains(own)) << own;
    }
    // This session is no program's: it was never started with a host, so
    // nothing went wrong at a start, and nothing was kept to have been made
    // from another built-in.
    EXPECT_EQ(content["start"],
              (Json{{"keptFromAnotherBuiltIn", false}, {"problems", Json::array()}}));
    // The text is the same object, for a client without structured content.
    EXPECT_EQ(Json::parse(textOf(summary)), content);

    // The resource is that object, and both are what CUSTOMISE JSON prints.
    const Json resource = request("resources/read", Json{{"uri", "katana://customisation"}});
    ASSERT_TRUE(resource.contains("result")) << resource.dump();
    EXPECT_EQ(resource["result"]["contents"][0]["mimeType"], "application/json");
    const std::string read = resource["result"]["contents"][0]["text"].get<std::string>();
    EXPECT_EQ(Json::parse(read), content);
    const Json typed = runLine("CUSTOMISE JSON");
    ASSERT_FALSE(typed["isError"].get<bool>()) << textOf(typed);
    EXPECT_EQ(typed["structuredContent"]["commands"][0]["output"], read);

    // A summary is one object: an argument that pages or filters a list is
    // refused, not ignored.
    const Json filtered =
        call("katana_customisation", Json{{"part", "summary"}, {"filter", "tree"}});
    EXPECT_TRUE(filtered["isError"].get<bool>());
    EXPECT_NE(textOf(filtered).find("\"filter\" does not go with part \"summary\""),
              std::string::npos)
        << textOf(filtered);
}

TEST_F(McpServer, TheCodesPartGivesEachRuleAsItsFileHoldsItFilteredAndPaged)
{
    initialize();
    loadTheThreeCustomisationFiles();
    const Json all = call("katana_customisation", Json{{"part", "codes"}});
    ASSERT_FALSE(all["isError"].get<bool>()) << textOf(all);
    const Json& content = all["structuredContent"];
    EXPECT_EQ(content["part"], "codes");
    EXPECT_EQ(content["total"], 11);
    EXPECT_EQ(content["offset"], 0);
    EXPECT_EQ(content["limit"], 100);
    ASSERT_EQ(content["codes"].size(), 11U);
    // Rules 0, 7 and 10 of test_survey, member for member as its text has
    // them - a feature rule, a symbol rule with its object and an attributes
    // rule with its list - each with its place in the map.
    EXPECT_EQ(content["codes"][0], (Json{{"index", 0},
                                         {"key", "WM*"},
                                         {"sets", "feature"},
                                         {"layer", "TEST SERVICES"},
                                         {"colour", "blue"},
                                         {"draw", "line"},
                                         {"linestyle", "TEST Water Main"},
                                         {"weight", "0"},
                                         {"group", "TEST - SERVICES"},
                                         {"comment", "[WM*] Water main"}}));
    EXPECT_EQ(content["codes"][7],
              (Json{{"index", 7},
                    {"key", "AC*"},
                    {"sets", "symbol"},
                    {"comment", "[AC*] Access chamber"},
                    {"hide", false},
                    {"symbol",
                     {{"name", "TEST Survey Mark"}, {"colour", "white"}, {"size", 1.5}}}}));
    EXPECT_EQ(content["codes"][10],
              (Json{{"index", 10},
                    {"key", "*"},
                    {"sets", "attributes"},
                    {"comment", "Every code gets this"},
                    {"attributes", Json::array({Json{{"type", "text"},
                                                     {"name", "Source"},
                                                     {"value", "Katana test fixture"}}})}}));
    EXPECT_EQ(Json::parse(textOf(all)), content);

    // The filter is a substring of a rule's key, comment or layer, with
    // letter case ignored. "tree" is in the comment of the two TR* rules
    // ("[TR*] Tree") and nowhere else; "test text" is the layer of 1* and 2*;
    // "px*" is the key of the two PX* rules.
    const auto indices = [&](const std::string& filter) {
        const Json found =
            call("katana_customisation", Json{{"part", "codes"}, {"filter", filter}});
        std::vector<int> list;
        for (const Json& rule : found["structuredContent"]["codes"]) {
            list.push_back(rule["index"].get<int>());
        }
        EXPECT_EQ(found["structuredContent"]["total"], list.size()) << filter;
        return list;
    };
    EXPECT_EQ(indices("tree"), (std::vector<int>{3, 8}));
    EXPECT_EQ(indices("test text"), (std::vector<int>{4, 5}));
    EXPECT_EQ(indices("px*"), (std::vector<int>{6, 9}));
    EXPECT_EQ(indices("no rule says this"), std::vector<int>{});

    // A page: `total` is still every match, the list only the page.
    const Json page =
        call("katana_customisation", Json{{"part", "codes"}, {"offset", 9}, {"limit", 5}});
    EXPECT_EQ(page["structuredContent"]["total"], 11);
    EXPECT_EQ(page["structuredContent"]["offset"], 9);
    EXPECT_EQ(page["structuredContent"]["limit"], 5);
    ASSERT_EQ(page["structuredContent"]["codes"].size(), 2U);
    EXPECT_EQ(page["structuredContent"]["codes"][0]["index"], 9);
    EXPECT_EQ(page["structuredContent"]["codes"][1]["index"], 10);
    // Past the end is an empty page, not a refusal.
    const Json past = call("katana_customisation", Json{{"part", "codes"}, {"offset", 11}});
    EXPECT_FALSE(past["isError"].get<bool>()) << textOf(past);
    EXPECT_EQ(past["structuredContent"]["total"], 11);
    EXPECT_TRUE(past["structuredContent"]["codes"].empty());

    // What is not a page, a part, or this part's argument is refused by name.
    for (const Json& bad : {Json{{"part", "codes"}, {"limit", 0}},
                            Json{{"part", "codes"}, {"limit", 1001}},
                            Json{{"part", "codes"}, {"offset", -1}},
                            Json{{"part", "codes"}, {"name", "TEST Tree"}},
                            Json{{"part", "rules"}}, Json::object()}) {
        const Json refused = call("katana_customisation", bad);
        EXPECT_TRUE(refused["isError"].get<bool>()) << bad.dump();
        EXPECT_FALSE(refused.contains("structuredContent")) << bad.dump();
    }
    EXPECT_NE(textOf(call("katana_customisation", Json{{"part", "rules"}}))
                  .find("\"part\" is summary, codes, definitions or problems"),
              std::string::npos);
    EXPECT_NE(textOf(call("katana_customisation", Json{{"part", "codes"}, {"name", "TEST Tree"}}))
                  .find("\"name\" picks one definition: it goes with part \"definitions\""),
              std::string::npos);
}

TEST_F(McpServer, TheDefinitionsPartListsEachDefinitionAndGivesStrokesOnlyForTheOneNamed)
{
    initialize();
    loadTheThreeCustomisationFiles();
    const Json all = call("katana_customisation", Json{{"part", "definitions"}});
    ASSERT_FALSE(all["isError"].get<bool>()) << textOf(all);
    const Json& content = all["structuredContent"];
    EXPECT_EQ(content["part"], "definitions");
    EXPECT_EQ(content["total"], 7);
    ASSERT_EQ(content["definitions"].size(), 7U);
    // In name order, linestyles and symbols together; each says its kind by
    // the list of its file it sits in, its units and whether it is drawn at
    // vertices even where its file leaves them at their default (TEST Water
    // Main says no units, which is world), and the customisation it is from.
    // No entry of a list holds strokes.
    const auto entry = [](const char* name, const char* kind, const char* group,
                          const char* units, bool atVertices, const char* from) {
        return Json{{"name", name},   {"kind", kind},           {"group", group},
                    {"units", units}, {"atVertices", atVertices}, {"from", from}};
    };
    EXPECT_EQ(content["definitions"],
              Json::array({entry("TEST Dashed Kerb", "linestyle", "Test/Lines", "paper", false,
                                 "test_linestyles"),
                           entry("TEST Gate", "linestyle", "Test/Lines", "twoPoint", false,
                                 "test_linestyles"),
                           entry("TEST Survey Mark", "symbol", "Test/Marks", "world", true,
                                 "test_symbols"),
                           entry("TEST Tree", "symbol", "Test/Vegetation", "world", false,
                                 "test_symbols"),
                           entry("TEST U Turn", "symbol", "Test/Marks", "world", true,
                                 "test_symbols"),
                           entry("TEST Valve", "symbol", "Test/Marks", "world", true,
                                 "test_symbols"),
                           entry("TEST Water Main", "linestyle", "Test/Services", "world", false,
                                 "test_linestyles")}));

    // A filter is a substring of a name or a group: three sit in Test/Marks.
    const Json marks =
        call("katana_customisation", Json{{"part", "definitions"}, {"filter", "MARKS"}});
    EXPECT_EQ(marks["structuredContent"]["total"], 3);
    const Json water =
        call("katana_customisation", Json{{"part", "definitions"}, {"filter", "water"}});
    ASSERT_EQ(water["structuredContent"]["definitions"].size(), 1U);
    EXPECT_EQ(water["structuredContent"]["definitions"][0]["name"], "TEST Water Main");

    // Named, it is the one definition whole: the six facts and its strokes,
    // as test_symbols writes them - a move, a draw and an arc of radius -0.5.
    const Json one =
        call("katana_customisation", Json{{"part", "definitions"}, {"name", "TEST U Turn"}});
    ASSERT_FALSE(one["isError"].get<bool>()) << textOf(one);
    EXPECT_EQ(one["structuredContent"]["total"], 1);
    ASSERT_EQ(one["structuredContent"]["definitions"].size(), 1U);
    Json whole = entry("TEST U Turn", "symbol", "Test/Marks", "world", true, "test_symbols");
    whole["strokes"] = Json::array({Json::array({"move", 0, 0}), Json::array({"draw", 1, 0}),
                                    Json::array({"arc", -0.5, -90, 90})});
    EXPECT_EQ(one["structuredContent"]["definitions"][0], whole);

    // A name is matched as written; one nothing has is refused, naming it,
    // and so is a name together with what narrows a list.
    for (const char* missing : {"TEST Nothing", "test u turn"}) {
        const Json refused =
            call("katana_customisation", Json{{"part", "definitions"}, {"name", missing}});
        EXPECT_TRUE(refused["isError"].get<bool>()) << missing;
        EXPECT_NE(textOf(refused).find("no definition is called \"" + std::string(missing) + "\""),
                  std::string::npos)
            << textOf(refused);
    }
    const Json both = call("katana_customisation", Json{{"part", "definitions"},
                                                        {"name", "TEST U Turn"},
                                                        {"filter", "marks"}});
    EXPECT_TRUE(both["isError"].get<bool>());
    EXPECT_NE(textOf(both).find("\"filter\" does not go with \"name\""), std::string::npos)
        << textOf(both);
}

TEST_F(McpServer, TheProblemsPartIsTheRulesLintAnObjectAnIssue)
{
    // Three rules, of which one is wrong in one way: KX* is a feature rule
    // with no layer, which the lint warns of (docs/survey_coding.md, the lint
    // table, NoModel) in the words "no layer". Nothing else: red is a
    // standard colour, which needs no front end to say so, "0" is the plain
    // line and cross a symbol Katana draws itself.
    initialize();
    const TempDir dir("customisation problems");
    const std::string file = dir.file("codes.customisation.json");
    std::ofstream(file, std::ios::binary)
        << R"({"format": "katana-customisation", "version": 1, "name": "codes", "codes": [
  {"key": "KT*", "sets": "feature", "layer": "KATANA TEST", "colour": "red", "linestyle": "0"},
  {"key": "KX*", "sets": "feature", "colour": "red"},
  {"key": "KT*", "sets": "symbol", "symbol": {"name": "cross", "size": 2}}
]})";
    const Json loaded = runLine("CUSTOMISE \"" + file + "\"");
    ASSERT_FALSE(loaded["isError"].get<bool>()) << textOf(loaded);

    const Json problems = call("katana_customisation", Json{{"part", "problems"}});
    ASSERT_FALSE(problems["isError"].get<bool>()) << textOf(problems);
    const Json& content = problems["structuredContent"];
    EXPECT_EQ(content["part"], "problems");
    EXPECT_EQ(content["total"], 1);
    EXPECT_EQ(content["rules"], 3);
    EXPECT_EQ(content["cannotApply"], 0);
    EXPECT_EQ(content["warnings"], 1);
    EXPECT_EQ(content["problems"],
              Json::array({Json{{"index", 1},
                                {"key", "KX*"},
                                {"sets", "feature"},
                                {"severity", "warning"},
                                {"kind", "no layer"},
                                {"message", "a feature rule with no layer: its codes stay on "
                                            "whatever layer they are on"}}}));
    // `index` is the rule's own, the one part codes gives it.
    const Json codes = call("katana_customisation", Json{{"part", "codes"}, {"filter", "kx"}});
    ASSERT_EQ(codes["structuredContent"]["codes"].size(), 1U);
    EXPECT_EQ(codes["structuredContent"]["codes"][0]["index"], 1);

    // A filter narrows the list - by key, kind or message - and not the
    // counts of the whole lint beside it.
    const Json byKind =
        call("katana_customisation", Json{{"part", "problems"}, {"filter", "NO LAYER"}});
    EXPECT_EQ(byKind["structuredContent"]["total"], 1);
    const Json none =
        call("katana_customisation", Json{{"part", "problems"}, {"filter", "unresolved"}});
    EXPECT_EQ(none["structuredContent"]["total"], 0);
    EXPECT_TRUE(none["structuredContent"]["problems"].empty());
    EXPECT_EQ(none["structuredContent"]["warnings"], 1);
    EXPECT_EQ(none["structuredContent"]["rules"], 3);
}

// THE way an agent edits a customisation (docs/mcp.md): write the session out
// as one file, change the JSON, load it in the session's place.
TEST_F(McpServer, ACustomisationIsEditedByExportingItChangingTheJsonAndLoadingItBack)
{
    initialize();
    loadTheThreeCustomisationFiles();
    const TempDir dir("customisation round trip");
    const std::string file = dir.file("session.customisation.json");
    const Json exported = runLine("CUSTOMISE EXPORT \"" + file + "\"");
    ASSERT_FALSE(exported["isError"].get<bool>()) << textOf(exported);

    // The edit: the water main's rule, the first of the eleven, moves to a
    // layer of its own.
    Json edited;
    {
        std::ifstream in(file, std::ios::binary);
        ASSERT_TRUE(in.good());
        edited = Json::parse(in);
    }
    ASSERT_EQ(edited["codes"][0]["key"], "WM*");
    ASSERT_EQ(edited["codes"][0]["layer"], "TEST SERVICES");
    edited["codes"][0]["layer"] = "TEST WATER";
    std::ofstream(file, std::ios::binary | std::ios::trunc) << edited.dump(2);

    const Json replaced = runLine("CUSTOMISE REPLACE \"" + file + "\"");
    ASSERT_FALSE(replaced["isError"].get<bool>()) << textOf(replaced);
    const Json rule = call("katana_customisation", Json{{"part", "codes"}, {"filter", "wm*"}});
    ASSERT_EQ(rule["structuredContent"]["codes"].size(), 1U);
    EXPECT_EQ(rule["structuredContent"]["codes"][0]["index"], 0);
    EXPECT_EQ(rule["structuredContent"]["codes"][0]["layer"], "TEST WATER");
    // And nothing else went: the same seven definitions, eleven rules and
    // three sources as before the round trip.
    const Json after = call("katana_customisation", Json{{"part", "summary"}})["structuredContent"];
    EXPECT_EQ(after["counts"]["definitions"], 7);
    EXPECT_EQ(after["counts"]["rules"], 11);
    EXPECT_EQ(after["counts"]["codes"], 8);
    EXPECT_EQ(after["sources"].size(), 3U);
    EXPECT_EQ(after["name"], "test_linestyles");
}

// A rule and a definition come back as the objects a file holds - and one
// member more each, the tool's own: a rule's `index`, a definition's `kind`.
// A file says neither (a rule's place is its order, a definition's kind the
// list it is in) and its reader is strict, so an entry copied across whole is
// refused for that member; with it taken off, it loads. The docs said only
// "an agent that has read a rule can write one".
TEST_F(McpServer, ARuleOrADefinitionTheToolGaveLoadsBackOnceTheToolsOwnMemberIsTakenOff)
{
    initialize();
    loadTheThreeCustomisationFiles();
    const TempDir dir("customisation entries back");
    // A customisation file holding the one entry, in the list it belongs to.
    const auto fileOf = [&dir](const char* name, const char* list, const Json& entry) {
        const std::string file = dir.file(name);
        std::ofstream(file, std::ios::binary | std::ios::trunc)
            << Json{{"format", "katana-customisation"},
                    {"version", 1},
                    {"name", "edit"},
                    {list, Json::array({entry})}}
                   .dump(2);
        return "\"" + file + "\"";
    };
    const auto waterMain = [this] {
        return call("katana_customisation", Json{{"part", "codes"}, {"filter", "wm*"}})
            ["structuredContent"];
    };
    const auto uTurn = [this] {
        return call("katana_customisation", Json{{"part", "definitions"}, {"name", "TEST U Turn"}})
            ["structuredContent"]["definitions"][0];
    };

    // ---- a rule: the water main's, the first of the eleven.
    Json rule = waterMain()["codes"][0];
    ASSERT_EQ(rule["index"], 0);
    ASSERT_EQ(rule["layer"], "TEST SERVICES");
    // As the tool gives it: refused for `index`, naming the entry, and
    // nothing is loaded.
    const Json ruleRefused =
        runLine("CUSTOMISE " + fileOf("rule as given.customisation.json", "codes", rule));
    EXPECT_TRUE(ruleRefused["isError"].get<bool>());
    EXPECT_NE(textOf(ruleRefused).find("codes[0] \"WM*\": unknown member \"index\""),
              std::string::npos)
        << textOf(ruleRefused);
    EXPECT_EQ(waterMain()["codes"][0], rule);
    // Without it - and on another layer, so that the load shows - it loads,
    // and takes the place of the key's rule in that section: still rule 0,
    // still eleven rules.
    Json edited = rule;
    edited.erase("index");
    edited["layer"] = "TEST WATER";
    const Json ruleLoaded =
        runLine("CUSTOMISE " + fileOf("rule.customisation.json", "codes", edited));
    ASSERT_FALSE(ruleLoaded["isError"].get<bool>()) << textOf(ruleLoaded);
    Json expected = edited;
    expected["index"] = 0;
    const Json after = waterMain();
    EXPECT_EQ(after["total"], 1);
    ASSERT_EQ(after["codes"].size(), 1U);
    EXPECT_EQ(after["codes"][0], expected);
    EXPECT_EQ(call("katana_customisation", Json{{"part", "codes"}})["structuredContent"]["total"],
              11);

    // ---- a definition, read whole by its name.
    const Json definition = uTurn();
    ASSERT_EQ(definition["kind"], "symbol");
    ASSERT_EQ(definition["strokes"].size(), 3U);
    const Json definitionRefused = runLine(
        "CUSTOMISE " + fileOf("definition as given.customisation.json", "symbols", definition));
    EXPECT_TRUE(definitionRefused["isError"].get<bool>());
    EXPECT_NE(textOf(definitionRefused).find("unknown member \"kind\""), std::string::npos)
        << textOf(definitionRefused);
    EXPECT_NE(textOf(definitionRefused).find("TEST U Turn"), std::string::npos)
        << textOf(definitionRefused);
    EXPECT_EQ(uTurn(), definition);
    // Without `kind`, in the list that says its kind - and in another group,
    // so that the load shows - it loads in the place of the definition of its
    // name and reads back as the object that was written: a symbol, by that
    // list, and from test_symbols still, since it says so although its file
    // is called "edit". Seven definitions as before.
    Json plain = definition;
    plain.erase("kind");
    plain["group"] = "Test/Edited";
    const Json definitionLoaded =
        runLine("CUSTOMISE " + fileOf("definition.customisation.json", "symbols", plain));
    ASSERT_FALSE(definitionLoaded["isError"].get<bool>()) << textOf(definitionLoaded);
    Json moved = definition;
    moved["group"] = "Test/Edited";
    EXPECT_EQ(uTurn(), moved);
    EXPECT_EQ(call("katana_customisation", Json{{"part", "definitions"}})["structuredContent"]
                  ["total"],
              7);
}

// What went wrong when the session STARTED is said once, on standard error -
// the client's log, which an agent is never shown. Here it is the agent's own
// case: it copied a rule out of katana_customisation into the file its server
// keeps the customisation in, `index` and all, and the server was started
// again. The session starts with the built-in, and the summary told it only
// `origin: builtIn`.
TEST_F(McpServer, WhatWentWrongWhenTheSessionStartedIsInTheSummaryAndTheResource)
{
    const TempDir dir("customisation start problem");
    const std::string kept = dir.file("kept.customisation.json");
    std::ofstream(kept, std::ios::binary)
        << R"({"format": "katana-customisation", "version": 1, "name": "Mine",
 "codes": [{"index": 0, "key": "WM*", "sets": "feature", "layer": "MINE"}]})";
    const std::string symbols = customisationPath("test_symbols.customisation.json");
    const StartEnvironment environment(symbols.c_str(), kept.c_str());
    testing::internal::CaptureStdout();
    testing::internal::CaptureStderr();
    Session started("katana_mcp");
    (void)testing::internal::GetCapturedStdout();
    const std::string log = testing::internal::GetCapturedStderr();
    Server own(started, "9.9.9");
    initializeOf(own);

    const Json summary = callOf(own, "katana_customisation", Json{{"part", "summary"}});
    ASSERT_FALSE(summary["isError"].get<bool>()) << textOf(summary);
    const Json& content = summary["structuredContent"];
    // The built-in started in the kept file's place: the symbol fixture.
    EXPECT_EQ(content["origin"], "builtIn");
    EXPECT_EQ(content["name"], "test_symbols");
    EXPECT_EQ(content["counts"]["definitions"], 4);
    // And why: cad's sentence, which holds the reader's own refusal - the
    // entry and the member - and the file.
    EXPECT_EQ(content["start"]["keptFromAnotherBuiltIn"], false);
    ASSERT_EQ(content["start"]["problems"].size(), 1U);
    const std::string problem = content["start"]["problems"][0].get<std::string>();
    EXPECT_TRUE(problem.starts_with(
        "the kept customisation is not read, so the built-in customisation is used: "
        "ParseFailure: codes[0] \"WM*\": unknown member \"index\""))
        << problem;
    EXPECT_NE(problem.find("kept.customisation.json"), std::string::npos) << problem;
    // It is the line the log had, and all the log had.
    EXPECT_EQ(log, "error: " + problem + "\n");
    EXPECT_EQ(Json::parse(textOf(summary)), content);

    // The resource is the same object.
    const Json resource =
        requestOf(own, "resources/read", Json{{"uri", "katana://customisation"}});
    ASSERT_TRUE(resource.contains("result")) << resource.dump();
    EXPECT_EQ(Json::parse(resource["result"]["contents"][0]["text"].get<std::string>()), content);
}

TEST_F(McpServer, AKeptCustomisationMadeFromAnotherBuiltInIsSaidInTheSummary)
{
    // The kept file names what it was made from, and that is not this run's
    // built-in (test_symbols). It is the user's, so it is what starts -
    // nothing is wrong - and the summary says the program's own has moved on,
    // which the log's warning said and an agent did not see.
    const TempDir dir("customisation kept from another");
    const std::string kept = dir.file("kept.customisation.json");
    std::ofstream(kept, std::ios::binary)
        << R"({"format": "katana-customisation", "version": 1, "name": "Mine",
 "basedOn": {"name": "Some Other", "digest": "0123456789abcdef"},
 "codes": [{"key": "MN*", "sets": "feature", "layer": "MINE"}]})";
    const std::string symbols = customisationPath("test_symbols.customisation.json");
    const StartEnvironment environment(symbols.c_str(), kept.c_str());
    testing::internal::CaptureStdout();
    testing::internal::CaptureStderr();
    Session started("katana_mcp");
    (void)testing::internal::GetCapturedStdout();
    (void)testing::internal::GetCapturedStderr();
    Server own(started, "9.9.9");
    initializeOf(own);

    const Json summary = callOf(own, "katana_customisation", Json{{"part", "summary"}});
    ASSERT_FALSE(summary["isError"].get<bool>()) << textOf(summary);
    const Json& content = summary["structuredContent"];
    EXPECT_EQ(content["origin"], "kept");
    EXPECT_EQ(content["name"], "Mine");
    EXPECT_EQ(content["kept"], true);
    EXPECT_EQ(content["builtIn"], "test_symbols");
    EXPECT_EQ(content["start"],
              (Json{{"keptFromAnotherBuiltIn", true}, {"problems", Json::array()}}));
}

TEST_F(McpServer, NothingACommandPrintsLeaksOntoTheRealStreams)
{
    initialize();
    testing::internal::CaptureStdout();
    testing::internal::CaptureStderr();
    (void)call("katana_run_commands", Json{{"commands", {"POINT 1,1", "CIRCLE 0,0 -1"}}});
    EXPECT_EQ(testing::internal::GetCapturedStdout(), "");
    EXPECT_EQ(testing::internal::GetCapturedStderr(), "");
}

// What the window's Dimension Styles manager sends, an agent sends too: one
// SET of several fields is one undo step, and INFO reads the style back as a
// record of every field.
TEST_F(McpServer, DimensionStylesAreSetInOneStepAndReadBackAsARecord)
{
    initialize();
    const Json made = call("katana_run_commands",
                           Json{{"commands",
                                 {"DIMSTYLE NEW site", "DIMSTYLE SET site TEXT 3.5 HEAD Tick PAPER on",
                                  "DIMSTYLE INFO site"}}});
    ASSERT_FALSE(made["isError"].get<bool>()) << textOf(made);
    const Json& lines = made["structuredContent"]["commands"];
    ASSERT_EQ(lines.size(), 3U);
    EXPECT_NE(lines[2]["output"].get<std::string>().find(
                  "name=site text=3.5 gap=0.625 extoff=0.625 extbeyond=1.25 arrow=2.5 head=Tick"),
              std::string::npos)
        << lines[2]["output"];
    EXPECT_EQ(made["structuredContent"]["status"]["undoSteps"], 2) << "NEW, then the one SET";

    const Json undone = call("katana_undo");
    EXPECT_FALSE(undone["isError"].get<bool>());
    const Json info = call("katana_run_commands", Json{{"commands", {"DIMSTYLE INFO site"}}});
    EXPECT_NE(textOf(info).find("text=2.5"), std::string::npos) << textOf(info);
    EXPECT_NE(textOf(info).find("head=ClosedFilled"), std::string::npos) << textOf(info);
    EXPECT_NE(textOf(info).find("paper=off"), std::string::npos) << textOf(info);
}

// The line the window's Edit Text sends, sent by an agent: several keys of a
// text in one TEXTEDIT, its words over two lines, read back by
// katana_describe_entity with the break written \n.
TEST_F(McpServer, ATextIsEditedInOneStepAndDescribedWithItsLineBreaks)
{
    initialize();
    const Json made = call("katana_run_commands",
                           Json{{"commands",
                                 {"TEXTSTYLE NEW Notes paper=3.5", "TEXT 10,20 2.5 old",
                                  "TEXTEDIT 1 text=\"PIT 12\\nIL 10.50\" style=Notes justify=MC"}}});
    ASSERT_FALSE(made["isError"].get<bool>()) << textOf(made);
    EXPECT_EQ(made["structuredContent"]["status"]["undoSteps"], 3);
    // Notes is 3.5 mm on paper; at the default 1:1000 that is 3.5 m.
    const Json described = call("katana_describe_entity", Json{{"id", 1}});
    EXPECT_NE(textOf(described).find(
                  "1  Text  layer=0  \"PIT 12\\nIL 10.50\"  height=3.5  style=Notes  justify=MC"),
              std::string::npos)
        << textOf(described);
}

// What the window's rules tab sends, sent by an agent: a rule added with its
// type, previewed by name with a record per rule of the labels it would
// have, then switched off - after which it labels nothing, named or not
// (auto_label.cpp, ruleMatches).
TEST_F(McpServer, ChosenLabelRulesArePreviewedByNameWithACountForEach)
{
    initialize();
    const Json result = call(
        "katana_run_commands",
        Json{{"commands",
              {"LABELSTYLE DEFAULTS", "RECT 0,0 20,20",
               "AUTOLABEL RULE ADD lots style=\"Lot Area\" type=Polyline",
               "AUTOLABEL PREVIEW lots", "AUTOLABEL RULE SET lots enabled=off",
               "AUTOLABEL PREVIEW lots", "LABELSTYLE VALUES area"}}});
    ASSERT_FALSE(result["isError"].get<bool>()) << textOf(result);
    const Json& lines = result["structuredContent"]["commands"];
    ASSERT_EQ(lines.size(), 7U);
    // One label a target (auto_label.hpp): the rectangle once for its area.
    EXPECT_EQ(lines[3]["output"].get<std::string>(),
              "preview created=1 kept=0 removed=0 skipped=0\nrule=lots labels=1");
    EXPECT_EQ(lines[5]["output"].get<std::string>(),
              "preview created=0 kept=0 removed=0 skipped=0");
    EXPECT_NE(lines[6]["output"].get<std::string>().find("kind=area values=id,layer,code"),
              std::string::npos)
        << lines[6]["output"];
    EXPECT_EQ(result["structuredContent"]["status"]["entities"], 1) << "a preview makes nothing";
}

TEST_F(McpServer, LabelLayoutNamesTheLabelsWithNoRoomSoAnAgentCanMoveThem)
{
    // Two numbered points at one place (1, 2), labelled in a style that may
    // not move its labels: label 4 finds no room and is named on its own
    // line; pinned elsewhere by LABEL SET, it is placed where it was put -
    // counted as displaced, being away from its own place with a leader back.
    initialize();
    const Json made = call(
        "katana_run_commands",
        Json{{"commands",
              {"LABELSTYLE NEW pt kind=point text={point} displace=off", "POINT 0,0", "SELECT 1",
               "PROP SET point P1", "POINT 0,0", "SELECT 2", "PROP SET point P2",
               "LABEL 1 2 style=pt", "LABEL LAYOUT"}}});
    ASSERT_FALSE(made["isError"].get<bool>()) << textOf(made);
    const Json& lines = made["structuredContent"]["commands"];
    const std::string layout = lines.back()["output"].get<std::string>();
    EXPECT_NE(layout.find("placed=1 displaced=0 suppressed=1"), std::string::npos) << layout;
    EXPECT_NE(layout.find("\nlabel=4 piece=0 suppressed=yes"), std::string::npos) << layout;

    const Json moved =
        call("katana_run_commands", Json{{"commands", {"LABEL SET 4 at=50,50", "LABEL LAYOUT"}}});
    ASSERT_FALSE(moved["isError"].get<bool>()) << textOf(moved);
    const std::string after =
        moved["structuredContent"]["commands"].back()["output"].get<std::string>();
    EXPECT_NE(after.find("placed=2 displaced=1 suppressed=0"), std::string::npos) << after;
    EXPECT_EQ(after.find("suppressed=yes"), std::string::npos) << after;
}

} // namespace

// What Terrain > Alignment Manager does, as an agent does it: the PIs read
// back exactly as records, rewritten as one line (one undo step), and the
// setting-out table a record a station with its key stations named.
TEST_F(McpServer, AnAgentReadsAndRewritesAnAlignmentsPIsAndItsSettingOutTable)
{
    initialize();
    const Json result =
        call("katana_run_commands",
             Json{{"commands",
                   {"ALIGN NEW road 0,0 100,0 100,100", "ALIGN PIS road",
                    "ALIGN PIS road 0,0 100,0,50 100,100", "ALIGN STATIONS road 1000"}}});
    ASSERT_FALSE(result["isError"].get<bool>()) << textOf(result);
    const Json& lines = result["structuredContent"]["commands"];
    ASSERT_EQ(lines.size(), 4U);
    EXPECT_NE(lines[1]["output"].get<std::string>().find(
                  "pi index=1 x=100 y=0 radius=0 spiral_in=0 spiral_out=0"),
              std::string::npos)
        << lines[1].dump();
    EXPECT_NE(lines[2]["output"].get<std::string>().find("alignment road now has 3 PIs"),
              std::string::npos)
        << lines[2].dump();
    const std::string table = lines[3]["output"].get<std::string>();
    for (const char* key : {"key=start", "key=TC", "key=CT", "key=end"}) {
        EXPECT_NE(table.find(key), std::string::npos) << key << "\n" << table;
    }
    const Json undone = call("katana_undo");
    EXPECT_FALSE(undone["isError"].get<bool>()) << textOf(undone);
    const Json back = call("katana_run_commands", Json{{"commands", {"ALIGN PIS road"}}});
    EXPECT_NE(back["structuredContent"]["commands"][0]["output"].get<std::string>().find(
                  "pi index=1 x=100 y=0 radius=0 "),
              std::string::npos)
        << textOf(back);
}

// What Survey > Parcel Report does, as an agent does it: the report, the deed
// wording under a name, and the labels as one undo step naming their layer.
TEST_F(McpServer, AnAgentReportsDescribesAndLabelsAParcel)
{
    initialize();
    const Json result = call("katana_run_commands",
                             Json{{"commands",
                                   {"PLINE 0,0 100,0 100,50 0,50 CLOSE", "PARCEL 1",
                                    "PARCEL 1 LEGAL Lot7", "PARCEL 1 LABEL 2"}}});
    ASSERT_FALSE(result["isError"].get<bool>()) << textOf(result);
    const Json& lines = result["structuredContent"]["commands"];
    ASSERT_EQ(lines.size(), 4U);
    EXPECT_NE(lines[1]["output"].get<std::string>().find(
                  "area 5000.000 m2 (0.500 ha), perimeter 300.000 m"),
              std::string::npos)
        << lines[1].dump();
    EXPECT_NE(lines[2]["output"].get<std::string>().find("Lot7: Beginning at E 0.000 N 0.000"),
              std::string::npos)
        << lines[2].dump();
    EXPECT_NE(lines[3]["output"].get<std::string>().find(
                  "5 labels created on layer 0, the current layer"),
              std::string::npos)
        << lines[3].dump();
    EXPECT_EQ(result["structuredContent"]["status"]["entities"], 6);
    const Json undone = call("katana_undo");
    EXPECT_EQ(undone["structuredContent"]["status"]["entities"], 1);
}

// What the Hatch Patterns tab does, as an agent does it: a pattern edited in
// one step, and a style made hatching with it in another.
TEST_F(McpServer, AnAgentEditsAHatchPatternAndMakesAStyleThatUsesIt)
{
    initialize();
    const Json result =
        call("katana_run_commands",
             Json{{"commands",
                   {"HATCH NEW brick 45 0.25", "HATCH SET brick 45 0.5 135 0.5",
                    "STYLE NEW paving HATCH brick"}}});
    ASSERT_FALSE(result["isError"].get<bool>()) << textOf(result);
    const Json& lines = result["structuredContent"]["commands"];
    ASSERT_EQ(lines.size(), 3U);
    EXPECT_EQ(lines[1]["output"].get<std::string>(), "hatch pattern brick updated (2 families)");
    EXPECT_EQ(lines[2]["output"].get<std::string>(), "style paving created");
    const Json refused = call("katana_run_commands", Json{{"commands", {"HATCH DELETE brick"}}});
    EXPECT_TRUE(refused["isError"].get<bool>()) << "a style still hatches with it";
}

#if defined(KATANA_TEST_WITH_INTEROP)
// ---- the geoprocessing tools (docs/mcp.md, "Geoprocessing tools") ---------------------------

TEST_F(McpServer, GdalCatalogueListsHillshade)
{
    initialize();
    const Json result = call("katana_gdal_catalogue", Json{{"filter", "hillshade"}, {"schemas", true}});
    ASSERT_FALSE(result["isError"].get<bool>()) << textOf(result);
    const Json& algorithms = result["structuredContent"]["algorithms"];
    ASSERT_EQ(algorithms.size(), 1U) << algorithms.dump();
    EXPECT_EQ(algorithms[0]["name"], "raster hillshade");
    EXPECT_EQ(algorithms[0]["path"], Json({"raster", "hillshade"}));
    EXPECT_EQ(algorithms[0]["policy"], "safe");
    // One algorithm: few enough for its schema to come with it.
    EXPECT_TRUE(algorithms[0]["arguments_schema"]["properties"].contains("zfactor"));
    EXPECT_TRUE(result["structuredContent"]["gdal_version"].get<std::string>().starts_with("3."));
    // A group lists its members and no schemas past 20 of them.
    const Json raster = call("katana_gdal_catalogue", Json{{"filter", "raster"}, {"schemas", true}});
    EXPECT_GT(raster["structuredContent"]["algorithms"].size(), 20U);
    EXPECT_FALSE(raster["structuredContent"]["algorithms"][0].contains("arguments_schema"));
}

TEST_F(McpServer, GdalDescribeGivesASchemaWithBounds)
{
    initialize();
    const Json result = call("katana_gdal_describe", Json{{"algorithm", "raster hillshade"}});
    ASSERT_FALSE(result["isError"].get<bool>()) << textOf(result);
    const Json& described = result["structuredContent"];
    const Json& altitude = described["arguments_schema"]["properties"]["altitude"];
    EXPECT_EQ(altitude["minimum"], 0.0);
    EXPECT_EQ(altitude["maximum"], 90.0);
    EXPECT_EQ(described["arguments_schema"]["properties"]["zfactor"]["exclusiveMinimum"], 0.0);
    bool found = false;
    for (const Json& arg : described["arguments"]) {
        if (arg["name"] == "input") {
            found = true;
            EXPECT_EQ(arg["dataset"]["kinds"], Json({"raster"}));
            EXPECT_EQ(arg["dataset"]["sources"], Json({"raster", "surface", "file"}));
        }
    }
    EXPECT_TRUE(found);
    // An alias is taken, as GDAL LIST names it.
    const Json warp = call("katana_gdal_describe", Json{{"algorithm", "raster warp"}});
    EXPECT_EQ(warp["structuredContent"]["algorithm"]["name"], "raster reproject");
}

TEST_F(McpServer, GdalRunBuildsTheLineAndReturnsOutputs)
{
    initialize();
    (void)call("katana_run_commands", Json{{"commands", {"LINE 0,0 100,0", "TEXT 5,5 2.5 \"LOT 7\""}}});
    const Json result = call(
        "katana_gdal_run",
        Json{{"algorithm", "vector buffer"},
             {"arguments", {{"distance", 1}, {"endcap-style", "flat"}}},
             {"inputs", {{"input", {{"scope", "drawing"}, {"where", {"TYPE=line"}}}}}},
             {"output", {{"layer", "gis/easement"}}}});
    ASSERT_FALSE(result["isError"].get<bool>()) << textOf(result);
    const Json& run = result["structuredContent"];
    EXPECT_EQ(run["line"], "GDAL vector buffer --distance=1 --endcap-style=flat FROM input DRAWING "
                           "WHERE TYPE=line TO LAYER gis/easement");
    EXPECT_TRUE(textOf(result).starts_with("> GDAL vector buffer"));
    ASSERT_EQ(run["scope"].size(), 1U);
    EXPECT_EQ(run["scope"][0]["matched"], 1);
    EXPECT_EQ(run["scope"][0]["used"], 1);
    ASSERT_EQ(run["outputs"].size(), 1U);
    EXPECT_EQ(run["outputs"][0]["kind"], "vector");
    EXPECT_EQ(run["outputs"][0]["layer"], "gis/easement");
    EXPECT_EQ(run["outputs"][0]["created"], 1);
    EXPECT_EQ(run["cancelled"], false);
    const Json listed = call("katana_list_entities");
    EXPECT_NE(textOf(listed).find("layer=gis/easement  vertices=4  closed  length=204  area=200"),
              std::string::npos)
        << textOf(listed);
}

// An input's schema offers only the sources it reads, and a source of
// another kind is refused before anything runs: hillshade's input reads
// rasters, so katana_gdal_describe offers it no drawing scope, and a drawing
// given to it anyway is refused naming what it takes, with nothing drawn or
// kept. GDAL answered "Unable to fetch band #1" from the worker.
TEST_F(McpServer, GdalRunRefusesASourceTheInputDoesNotRead)
{
    initialize();
    const Json described =
        call("katana_gdal_describe", Json{{"algorithm", "raster hillshade"}})["structuredContent"];
    const Json& offered = described["inputs_schema"]["properties"]["input"]["properties"];
    EXPECT_FALSE(offered.contains("scope")) << offered.dump();
    EXPECT_TRUE(offered.contains("raster")) << offered.dump();
    (void)call("katana_run_commands", Json{{"commands", {"RECT 0,0 10,10"}}});
    const Json result =
        call("katana_gdal_run", Json{{"algorithm", "raster hillshade"},
                                     {"inputs", {{"input", {{"scope", "drawing"}}}}},
                                     {"output", {{"reference", "shade"}}}});
    EXPECT_TRUE(result["isError"].get<bool>()) << textOf(result);
    EXPECT_NE(textOf(result).find("input reads raster datasets, and FROM gives it drawing data; "
                                  "it takes raster,surface,file"),
              std::string::npos)
        << textOf(result);
    EXPECT_EQ(call("katana_status")["structuredContent"]["entities"], 1);
}

TEST_F(McpServer, GdalRunQuotesAnOutputNamedLikeAKeyword)
{
    // An agent's layer "preview" was written bare: the line read PREVIEW as
    // the flag and was refused. The one quoting rule quotes it.
    initialize();
    (void)call("katana_run_commands", Json{{"commands", {"LINE 0,0 100,0"}}});
    const Json result =
        call("katana_gdal_run", Json{{"algorithm", "vector buffer"},
                                     {"arguments", {{"distance", 1}}},
                                     {"inputs", {{"input", {{"scope", "drawing"}}}}},
                                     {"output", {{"layer", "preview"}}}});
    ASSERT_FALSE(result["isError"].get<bool>()) << textOf(result);
    EXPECT_EQ(result["structuredContent"]["line"],
              "GDAL vector buffer --distance=1 FROM input DRAWING TO LAYER \"preview\"");
    EXPECT_EQ(result["structuredContent"]["outputs"][0]["layer"], "preview");
}

TEST_F(McpServer, GdalRunRefusesAConfirmAlgorithmWithoutConfirm)
{
    initialize();
    const TempDir folder("gdal-confirm");
    const std::string doomed = folder.file("doomed.txt");
    std::ofstream(doomed) << "x";
    const Json refused =
        call("katana_gdal_run", Json{{"algorithm", "vsi delete"}, {"tokens", {doomed}}});
    EXPECT_TRUE(refused["isError"].get<bool>());
    EXPECT_NE(textOf(refused).find("confirm"), std::string::npos) << textOf(refused);
    EXPECT_TRUE(std::filesystem::exists(doomed));
    const Json confirmed = call("katana_gdal_run", Json{{"algorithm", "vsi delete"},
                                                        {"tokens", {doomed}},
                                                        {"confirm", true}});
    EXPECT_FALSE(confirmed["isError"].get<bool>()) << textOf(confirmed);
    EXPECT_FALSE(std::filesystem::exists(doomed));
}

TEST_F(McpServer, GdalRunRefusesAPipelineThatChangesExistingDataUnlessToldTo)
{
    initialize();
    const TempDir folder("gdal-pipeline");
    const std::string source = folder.file("a.tif");
    const std::string victim = folder.file("b.tif");
    for (const auto& [file, burn] : {std::pair{source, "7"}, std::pair{victim, "1"}}) {
        const Json made =
            call("katana_gdal_run",
                 Json{{"algorithm", "raster create"},
                      {"tokens", {"--size", "3,3", "--bbox", "0,0,3,3", "--burn", burn, file}}});
        ASSERT_FALSE(made["isError"].get<bool>()) << textOf(made);
    }
    const auto bytes = [](const std::string& path) {
        std::ifstream in(path, std::ios::binary);
        return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
    };
    const std::string before = bytes(victim);

    // An update step, as one quoted word: confirm.
    const Json update =
        call("katana_gdal_run", Json{{"algorithm", "raster pipeline"},
                                     {"tokens", {"read " + source + " ! update " + victim}}});
    EXPECT_TRUE(update["isError"].get<bool>());
    EXPECT_NE(textOf(update).find("confirm"), std::string::npos) << textOf(update);
    EXPECT_EQ(bytes(victim), before);
    // --overwrite inside the pipeline: overwrite.
    const Json replace = call(
        "katana_gdal_run", Json{{"algorithm", "pipeline"},
                                {"tokens", {"read " + source + " ! write --overwrite " + victim}}});
    EXPECT_TRUE(replace["isError"].get<bool>());
    EXPECT_NE(textOf(replace).find("OVERWRITE"), std::string::npos) << textOf(replace);
    EXPECT_EQ(bytes(victim), before);

    const Json confirmed =
        call("katana_gdal_run", Json{{"algorithm", "raster pipeline"},
                                     {"tokens", {"read " + source + " ! update " + victim}},
                                     {"confirm", true}});
    EXPECT_FALSE(confirmed["isError"].get<bool>()) << textOf(confirmed);
    const std::string updated = bytes(victim);
    EXPECT_NE(updated, before);
    const Json replaced = call(
        "katana_gdal_run", Json{{"algorithm", "pipeline"},
                                {"tokens", {"read " + source + " ! write --overwrite " + victim}},
                                {"overwrite", true}});
    EXPECT_FALSE(replaced["isError"].get<bool>()) << textOf(replaced);
}

TEST_F(McpServer, GdalRunRefusesAnArgumentTheAlgorithmHasNot)
{
    initialize();
    const Json refused = call("katana_gdal_run", Json{{"algorithm", "raster hillshade"},
                                                      {"arguments", {{"no-such", 1}}}});
    EXPECT_TRUE(refused["isError"].get<bool>());
    EXPECT_NE(textOf(refused).find("no-such"), std::string::npos);
}

// ---- I2: katana_formats and katana://formats ----

TEST_F(McpServer, FormatsReturnsStructuredDrivers)
{
    initialize();
    const Json result = call("katana_formats", Json{{"kind", "vector"},
                                                    {"capability", "write"},
                                                    {"filter", "flatgeobuf"}});
    ASSERT_FALSE(result["isError"].get<bool>()) << textOf(result);
    const Json& formats = result["structuredContent"]["formats"];
    ASSERT_EQ(formats.size(), 1U) << formats.dump();
    EXPECT_EQ(formats[0]["driver"], "FlatGeobuf");
    EXPECT_EQ(formats[0]["write"], Json({"vector"}));
    EXPECT_EQ(formats[0]["extensions"], Json({"fgb"}));
    EXPECT_TRUE(result["structuredContent"]["gdal_version"].get<std::string>().starts_with("3."));
    // The text is the verb's records.
    EXPECT_NE(textOf(result).find("format driver=FlatGeobuf kind=vector"), std::string::npos);

    // One driver's options, as IMPORT's oo= and EXPORT's co= are checked.
    const Json gpkg = call("katana_formats", Json{{"driver", "GPKG"}});
    ASSERT_FALSE(gpkg["isError"].get<bool>()) << textOf(gpkg);
    bool listAll = false;
    for (const Json& option : gpkg["structuredContent"]["open_options"]) {
        if (option["name"] == "LIST_ALL_TABLES") {
            listAll = true;
            EXPECT_EQ(option["choices"], Json({"AUTO", "YES", "NO"}));
        }
    }
    EXPECT_TRUE(listAll);
    EXPECT_TRUE(call("katana_formats", Json{{"driver", "NoSuchDriver"}})["isError"].get<bool>());
    EXPECT_TRUE(call("katana_formats", Json{{"kind", "both"}})["isError"].get<bool>());

    // The resource is FORMATS JSON: every format, the same objects.
    const Json resource = request("resources/read", Json{{"uri", "katana://formats"}});
    const Json all = Json::parse(resource["result"]["contents"][0]["text"].get<std::string>());
    ASSERT_TRUE(all.is_array());
    const auto found = std::ranges::find_if(all, [](const Json& format) {
        return format["driver"] == "FlatGeobuf";
    });
    ASSERT_NE(found, all.end());
    EXPECT_EQ(*found, formats[0]);
}

// katana_terrain_list: what SURFACE LIST JSON says, as structured content.
// terrain.asc's least and greatest values, read from the text grid, are
// 24.892 and 38.819, and a surface of all its 10 800 cells spans exactly
// them; the grid is read as Float32, within 2e-6 of the text at that size.
TEST_F(McpServer, TerrainListGivesTheSessionsSurfacesAndRasters)
{
    initialize();
    const std::string terrain = std::string(KATANA_GIS_SAMPLES) + "/terrain.asc";
    const Json empty = call("katana_terrain_list");
    ASSERT_FALSE(empty["isError"].get<bool>()) << textOf(empty);
    EXPECT_TRUE(empty["structuredContent"]["surfaces"].empty());
    const Json made = call("katana_run_commands",
                           Json{{"commands", {"IMPORT \"" + terrain + "\"",
                                              "SURFACE FROM RASTER 1 NAME ground"}}});
    ASSERT_FALSE(made["isError"].get<bool>()) << textOf(made);
    const Json listed = call("katana_terrain_list");
    ASSERT_FALSE(listed["isError"].get<bool>()) << textOf(listed);
    const Json& content = listed["structuredContent"];
    ASSERT_EQ(content["surfaces"].size(), 1U) << content.dump();
    EXPECT_EQ(content["surfaces"][0]["name"], "ground");
    EXPECT_EQ(content["surfaces"][0]["points"], 10800);
    EXPECT_NEAR(content["surfaces"][0]["zmin"].get<double>(), 24.892, 2e-6);
    EXPECT_NEAR(content["surfaces"][0]["zmax"].get<double>(), 38.819, 2e-6);
    ASSERT_EQ(content["rasters"].size(), 1U);
    EXPECT_EQ(content["rasters"][0]["id"], 1);
    EXPECT_EQ(content["rasters"][0]["width"], 120);
    EXPECT_EQ(content["rasters"][0]["cell"], 1.5);
    EXPECT_NE(textOf(listed).find("surface ground: "), std::string::npos) << textOf(listed);
}

// T1: CONTOUR is a session line, so the command tool draws contours as the
// window's Terrain > Analysis > Contours does. terrain.asc's heights run
// from 24.892 to 38.819, so the whole metres in it are 25 to 38: 14 levels.
TEST_F(McpServer, ContoursOfASurfaceAreDrawnThroughTheCommandTool)
{
    initialize();
    const std::string terrain = std::string(KATANA_GIS_SAMPLES) + "/terrain.asc";
    const Json drawn = call("katana_run_commands",
                            Json{{"commands", {"SURFACE FROM FILE \"" + terrain + "\" NAME ground",
                                               "CONTOUR SURFACE ground interval=1"}}});
    ASSERT_FALSE(drawn["isError"].get<bool>()) << textOf(drawn);
    const Json& lines = drawn["structuredContent"]["commands"];
    ASSERT_EQ(lines.size(), 2U);
    const std::string reply = lines[1]["output"].get<std::string>();
    EXPECT_NE(reply.find("contours method=tin cell= levels=14 "), std::string::npos) << reply;
    EXPECT_NE(reply.find("layer=terrain/contours"), std::string::npos) << reply;
}

// T2: a shading is a derived reference raster, which katana_terrain_list
// then lists with the line that made it.
TEST_F(McpServer, AShadingMadeThroughTheCommandToolIsListedAsADerivedRaster)
{
    initialize();
    const std::string terrain = std::string(KATANA_GIS_SAMPLES) + "/terrain.asc";
    const Json shaded = call("katana_run_commands",
                             Json{{"commands", {"RASTER SHADE FILE \"" + terrain + "\""}}});
    ASSERT_FALSE(shaded["isError"].get<bool>()) << textOf(shaded);
    const Json listed = call("katana_terrain_list");
    ASSERT_FALSE(listed["isError"].get<bool>()) << textOf(listed);
    const Json& rasters = listed["structuredContent"]["rasters"];
    ASSERT_EQ(rasters.size(), 1U) << rasters.dump();
    EXPECT_EQ(rasters[0]["name"], "terrain-hillshade");
    EXPECT_EQ(rasters[0]["role"], "derived");
    EXPECT_NE(rasters[0]["derived_from"].get<std::string>().find("RASTER SHADE"),
              std::string::npos);
}

// T3: slope classes through the command tool, one undo step. terrain.asc's
// heights span 24.892 to 38.819 on 1.5 m cells, so Horn's gradient, a
// weighted difference of at most 4 x 13.927 m over 8 x 1.5 m, is at most
// 4.64 along each axis: under 657 % however it falls. A break at 1000 puts
// all 120 x 90 cells of 2.25 m2 in [0, 1000): 24 300 m2.
TEST_F(McpServer, SlopeClassesMadeThroughTheCommandToolAreOneUndoStep)
{
    initialize();
    const std::string terrain = std::string(KATANA_GIS_SAMPLES) + "/terrain.asc";
    const Json made = call("katana_run_commands",
                           Json{{"commands", {"RASTER SLOPE FILE \"" + terrain + "\" classes=1000"}}});
    ASSERT_FALSE(made["isError"].get<bool>()) << textOf(made);
    const std::string reply = made["structuredContent"]["commands"][0]["output"].get<std::string>();
    EXPECT_NE(reply.find("class name=0-1000 from=0 to=1000 unit=percent area=24300.000"),
              std::string::npos)
        << reply;
    const Json undone = call("katana_run_commands", Json{{"commands", {"UNDO", "LIST"}}});
    ASSERT_FALSE(undone["isError"].get<bool>()) << textOf(undone);
    EXPECT_NE(textOf(undone).find("0 entities"), std::string::npos) << textOf(undone);
}

// T4: statistics by area through the command tool, written on the drawn lot
// in place. A 40 x 30 m lot on terrain.asc's 1.5 m cells covers
// 1200 / 2.25 = 533.333 cells by fractional coverage, wherever it lies, and
// the reply's zone record says so.
TEST_F(McpServer, StatisticsByAreaThroughTheCommandToolAreWrittenOnTheLot)
{
    initialize();
    const std::string terrain = std::string(KATANA_GIS_SAMPLES) + "/terrain.asc";
    const Json made = call("katana_run_commands",
                           Json{{"commands", {"RECT 10,10 50,40",
                                              "RASTER ZONAL FILE \"" + terrain +
                                                  "\" DRAWING stats=count"}}});
    ASSERT_FALSE(made["isError"].get<bool>()) << textOf(made);
    const std::string reply = made["structuredContent"]["commands"][1]["output"].get<std::string>();
    EXPECT_NE(reply.find("zone entity=1 count=533.33"), std::string::npos) << reply;
    EXPECT_NE(reply.find("target=in-place created=0 updated=1"), std::string::npos) << reply;
}

// T5: a sight line through the command tool. Two points a metre apart on
// terrain.asc (1.5 m cells, heights changing by centimetres over a cell):
// the eye 1.7 m up sees ground a metre off, whatever the slope between.
TEST_F(McpServer, ALineOfSightThroughTheCommandToolSaysWhetherTheTargetIsSeen)
{
    initialize();
    const std::string terrain = std::string(KATANA_GIS_SAMPLES) + "/terrain.asc";
    const Json looked = call("katana_run_commands",
                             Json{{"commands", {"LOS FILE \"" + terrain +
                                                "\" OBSERVER 50,50 TARGET 51,50"}}});
    ASSERT_FALSE(looked["isError"].get<bool>()) << textOf(looked);
    const std::string reply =
        looked["structuredContent"]["commands"][0]["output"].get<std::string>();
    EXPECT_NE(reply.find("sight visible=yes observer=50,50 target=51,50 distance=1.000"),
              std::string::npos)
        << reply;
}
#endif

// IFC through the one door an agent has: the lines File > Export IFC and
// Import IFC write (docs/ifc.md), each answered in key=value records the
// agent can read - the preview's objects, the file written, the file read.
TEST_F(McpServer, AnAgentPreviewsExportsDescribesAndImportsIfcByTheDialogsLines)
{
    initialize();
    const TempDir dir("ifc");
    const std::string file = dir.file("site plan.ifc");
    const Json exported =
        call("katana_run_commands",
             Json{{"commands",
                   {"LAYER NEW Survey/Kerb", "LAYER SET Survey/Kerb", "PL 0,0 10,0 20,5",
                    "EXPORT \"" + file + "\" PREVIEW", "EXPORT \"" + file + "\" NOSURFACES",
                    "INFO \"" + file + "\"", "IFC RULES \"" + dir.file("rules.csv") + "\""}}});
    ASSERT_FALSE(exported["isError"].get<bool>()) << textOf(exported);
    const std::string text = textOf(exported);
    EXPECT_NE(text.find("ifc previewed file=\"site plan.ifc\" schema=IFC4X3_ADD2"),
              std::string::npos)
        << text;
    EXPECT_NE(text.find("object from=\"layer Survey/Kerb\" count=1 class=\"IfcKerb\""),
              std::string::npos);
    EXPECT_NE(text.find("ifc exported file=\"site plan.ifc\""), std::string::npos);
    EXPECT_NE(text.find("ifc described file=\"site plan.ifc\" schema=IFC4X3_ADD2 crs=\"\" "
                        "entities=1"),
              std::string::npos);
    EXPECT_NE(text.find("ifc rules file=\"rules.csv\" rules="), std::string::npos);
    EXPECT_TRUE(std::filesystem::exists(dir.file("rules.csv")));

    ASSERT_FALSE(
        call("katana_new_project", Json{{"discard_unsaved_changes", true}})["isError"].get<bool>());
    const Json imported =
        call("katana_run_commands",
             Json{{"commands", {"IMPORT \"" + file + "\" NOALIGNMENTS KEEPCRS"}}});
    ASSERT_FALSE(imported["isError"].get<bool>()) << textOf(imported);
    EXPECT_NE(textOf(imported).find("ifc imported file=\"site plan.ifc\" schema=IFC4X3_ADD2 "
                                    "crs=\"\" entities=1 alignments=0"),
              std::string::npos)
        << textOf(imported);
    EXPECT_EQ(imported["structuredContent"]["status"]["entities"], 1);
}

#if defined(KATANA_TEST_WITH_INTEROP)
// katana_export of an .ifc hands its records as objects under their whole
// kind: "ifc exported", with a file field - read as the kind "ifc" and a
// field "exported file" until the kind became every word before the first
// key=.
TEST_F(McpServer, AnIfcExportsRecordIsItsWholeKindWithItsFields)
{
    initialize();
    const TempDir dir("ifc-records");
    (void)call("katana_run_commands", Json{{"commands", {"PL 0,0 10,0 20,5"}}});
    const Json exported = call("katana_export", Json{{"path", dir.file("site.ifc")}});
    ASSERT_FALSE(exported["isError"].get<bool>()) << textOf(exported);
    const Json& records = exported["structuredContent"]["records"];
    ASSERT_FALSE(records.empty()) << exported.dump();
    EXPECT_EQ(records[0]["record"], "ifc exported") << records.dump();
    EXPECT_EQ(records[0]["file"], "site.ifc") << records.dump();
    EXPECT_FALSE(records[0].contains("exported file")) << records.dump();
}
#endif
