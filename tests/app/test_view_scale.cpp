#include <gtest/gtest.h>
#include <nlohmann/json.hpp>

#include <iostream>
#include <sstream>
#include <string>

#include "katana/cad/view_set.hpp"
#include "mcp_server.hpp"
#include "session.hpp"

namespace {

using Json = nlohmann::json;

class CapturedViewScaleSession {
  public:
    CapturedViewScaleSession()
        : outBefore_(std::cout.rdbuf(out.rdbuf())), errBefore_(std::cerr.rdbuf(err.rdbuf()))
    {
    }
    ~CapturedViewScaleSession()
    {
        std::cout.rdbuf(outBefore_);
        std::cerr.rdbuf(errBefore_);
    }

    std::ostringstream out;
    std::ostringstream err;

  private:
    std::streambuf* outBefore_;
    std::streambuf* errBefore_;
};

Json call(katana::app::mcp::Server& server, const Json& commands)
{
    const auto response = server.handle(
        Json{{"jsonrpc", "2.0"}, {"id", 2}, {"method", "tools/call"},
             {"params", {{"name", "katana_run_commands"}, {"arguments", {{"commands", commands}}}}}}
            .dump());
    EXPECT_TRUE(response.has_value());
    return response ? Json::parse(*response).value("result", Json::object()) : Json::object();
}

} // namespace

TEST(ViewScaleSession, TheCliSessionReadsChangesAndUndoesASavedViewsPaperScale)
{
    katana::app::Session session(nullptr);
    ASSERT_TRUE(katana::cad::setViewPaperScales(session.document(), {{4, 500.0}}).ok());
    CapturedViewScaleSession captured;
    ASSERT_TRUE(session.run("VIEWS SCALE LIST"));
    ASSERT_TRUE(session.run("VIEWS SCALE 4 1:250"));
    ASSERT_TRUE(session.run("UNDO"));
    ASSERT_TRUE(session.run("VIEWS SCALE 4"));
    EXPECT_TRUE(captured.err.str().empty()) << captured.err.str();
    const std::string output = captured.out.str();
    EXPECT_NE(output.find("view=4 kind=plan paper_scale=500\nviews=1"), std::string::npos);
    EXPECT_NE(output.find("view=4 kind=plan paper_scale=250 changed=yes"), std::string::npos);
    EXPECT_TRUE(output.ends_with("view=4 kind=plan paper_scale=500\n"));
}

TEST(ViewScaleMcp, TheCommandToolChangesOnlyAKnownSavedViewAndReportsUndo)
{
    katana::app::Session session(nullptr);
    ASSERT_TRUE(katana::cad::setViewPaperScales(session.document(), {{4, 500.0}}).ok());
    const double annotation = session.document().annotationScale();
    katana::app::mcp::Server server(session, "test");
    ASSERT_TRUE(server.handle(
        R"({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{}}})")
                    .has_value());
    const Json result =
        call(server, {"VIEWS SCALE LIST", "VIEWS SCALE 4 1:250", "UNDO", "VIEWS SCALE 4"});
    ASSERT_TRUE(result.contains("isError")) << result.dump();
    EXPECT_FALSE(result.at("isError").get<bool>()) << result.dump();
    const auto& lines = result.at("structuredContent").at("commands");
    ASSERT_EQ(lines.size(), 4U);
    EXPECT_NE(lines[0].at("output").get<std::string>().find("paper_scale=500"),
              std::string::npos);
    EXPECT_NE(lines[1].at("output").get<std::string>().find("paper_scale=250 changed=yes"),
              std::string::npos);
    EXPECT_NE(lines[3].at("output").get<std::string>().find("paper_scale=500"),
              std::string::npos);
    EXPECT_EQ(session.document().annotationScale(), annotation);
    EXPECT_EQ(session.document().model().entities.size(), 0U);

    const Json unknown = call(server, {"VIEWS SCALE 8 500", "POINT 1,1"});
    ASSERT_TRUE(unknown.contains("isError")) << unknown.dump();
    EXPECT_TRUE(unknown.at("isError").get<bool>());
    EXPECT_TRUE(unknown.at("structuredContent").at("commands")[1].at("skipped").get<bool>());
    EXPECT_EQ(session.document().model().entities.size(), 0U);
    const auto scales = katana::cad::viewPaperScales(session.document());
    ASSERT_TRUE(scales.ok());
    EXPECT_EQ(*scales, (katana::cad::ViewPaperScales{{4, 500.0}}));
}
