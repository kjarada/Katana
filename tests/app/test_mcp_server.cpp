// The MCP server (src/katana_app/mcp_server.hpp, docs/mcp.md), driven message
// by message as a client would drive it, without a process. What a command
// prints is the command line's own, which the cli.* tests pin; these pin the
// protocol around it and what the tools add: the batch that stops at a failure,
// the refusal to discard unsaved work, and the state reported after each call.

#include <algorithm>
#include <filesystem>
#include <string>
#include <vector>

#include <gtest/gtest.h>
#include <nlohmann/json.hpp>

#include "mcp_server.hpp"
#include "session.hpp"

namespace {

using Json = nlohmann::json;
using katana::app::Session;
using katana::app::mcp::Server;

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

    Json request(const std::string& method, Json params = Json::object())
    {
        const Json message{
            {"jsonrpc", "2.0"}, {"id", nextId++}, {"method", method}, {"params", params}};
        const auto reply = server.handle(message.dump());
        EXPECT_TRUE(reply.has_value()) << method;
        return reply ? Json::parse(*reply) : Json();
    }

    Json call(const std::string& tool, Json arguments = Json::object())
    {
        const Json reply =
            request("tools/call", Json{{"name", tool}, {"arguments", std::move(arguments)}});
        EXPECT_TRUE(reply.contains("result")) << reply.dump();
        return reply.value("result", Json::object());
    }

    static std::string textOf(const Json& result)
    {
        return result["content"][0]["text"].get<std::string>();
    }

    void initialize()
    {
        (void)request("initialize", Json{{"protocolVersion", "2025-06-18"},
                                         {"capabilities", Json::object()},
                                         {"clientInfo", {{"name", "test"}, {"version", "1"}}}});
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
                  "> INFO 1\n1  Polyline  layer=0  vertices=4  closed  length=30  area=50"),
              std::string::npos)
        << textOf(described);

    const Json missing = call("katana_describe_entity", Json{{"id", 2}});
    EXPECT_TRUE(missing["isError"].get<bool>());
    EXPECT_NE(textOf(missing).find("entity does not exist"), std::string::npos) << textOf(missing);
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
    ASSERT_EQ(list.size(), 2U);
    const Json help = request("resources/read", Json{{"uri", "katana://help"}});
    EXPECT_NE(help["result"]["contents"][0]["text"].get<std::string>().find("CUSTOMISE"),
              std::string::npos);
    const Json status = request("resources/read", Json{{"uri", "katana://status"}});
    EXPECT_EQ(Json::parse(status["result"]["contents"][0]["text"].get<std::string>())["entities"],
              0);
    EXPECT_EQ(request("resources/read", Json{{"uri", "katana://nothing"}})["error"]["code"],
              -32002);
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

} // namespace
