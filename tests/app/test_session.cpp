// The session katana_cli and katana_mcp share (src/katana_app/session.hpp), as
// katana_cli runs a line: what goes to stdout, where a reply or a report goes,
// and what to stderr, where a refusal goes. The verbs' own text is pinned by
// the cli.* tests; this pins the streams, which a merged ctest log cannot.

#include <filesystem>
#include <fstream>
#include <string>

#include <gtest/gtest.h>

#include "session.hpp"

namespace {

using katana::app::Session;

// A directory of the test's own, removed afterwards.
struct ScratchDirectory {
    std::filesystem::path path;
    explicit ScratchDirectory(const std::string& name)
        : path(std::filesystem::temp_directory_path() / ("katana-session-" + name))
    {
        std::error_code error;
        std::filesystem::remove_all(path, error);
        std::filesystem::create_directories(path, error);
    }
    ~ScratchDirectory()
    {
        std::error_code error;
        std::filesystem::remove_all(path, error);
    }

    // Writes `text` to `name` here and returns its path, quoted for a line.
    [[nodiscard]] std::string file(const std::string& name, const std::string& text) const
    {
        const std::filesystem::path where = path / name;
        std::ofstream(where, std::ios::binary) << text;
        return "\"" + where.generic_string() + "\"";
    }
};

// What running `line` printed, stream by stream.
struct Printed {
    bool ok = false;
    std::string out;
    std::string err;
};

Printed run(Session& session, const std::string& line)
{
    testing::internal::CaptureStdout();
    testing::internal::CaptureStderr();
    Printed printed;
    printed.ok = session.run(line);
    printed.out = testing::internal::GetCapturedStdout();
    printed.err = testing::internal::GetCapturedStderr();
    return printed;
}

bool contains(const std::string& text, const std::string& part)
{
    return text.find(part) != std::string::npos;
}

} // namespace

TEST(Session, ARefusalThatCarriesAReportPrintsTheReportWhereAReportGoes)
{
    // UTILITY CHECK refuses a schedule with errors against its schema, so that
    // a script stops there, and the refusal carries the whole check. A script
    // that keeps stdout - katana_cli -c "UTILITY CHECK ..." > check.txt - must
    // have the check exactly when it failed, as before the verb moved into the
    // interpreter; the refusal itself is one line on stderr.
    const ScratchDirectory scratch("check");
    const std::string schema = scratch.file("schema.csv", "kind,attribute,value,detail,label\n"
                                                          "schema,Example Utility Schema,0.1,,\n"
                                                          "identifier,AssetId,,,\n"
                                                          "field,AssetId,Alphanumerical,Yes,Asset Id\n"
                                                          "field,Status,Domain List: Status,Yes,Status\n"
                                                          "domain,Status,Live,,\n"
                                                          "domain,Status,Dead,,\n");
    const std::string wrong =
        scratch.file("wrong.csv", "AssetId,Status,easting\nW-1,Alive,1\nW-2,,1\n");
    Session session(nullptr);

    const Printed failed = run(session, "UTILITY CHECK " + wrong + " SCHEMA " + schema);
    EXPECT_FALSE(failed.ok);
    EXPECT_EQ(failed.err, "error: InvalidArgument: the schedule does not meet the schema: 2 errors\n");
    EXPECT_TRUE(failed.out.starts_with("Delivery schema check against Example Utility Schema v0.1\n"))
        << failed.out;
    EXPECT_TRUE(contains(failed.out, "2 rows, 2 assets (AssetId): 2 errors, 0 warnings"));
    EXPECT_TRUE(contains(failed.out, "\"Alive\" is not in the domain")) << failed.out;
    EXPECT_TRUE(contains(failed.out, "Status is empty"));
    EXPECT_TRUE(failed.out.ends_with("\n"));

    // A schedule that passes is a reply, on stdout, as it always was.
    const std::string clean = scratch.file("clean.csv", "AssetId,Status,easting\nW-1,Live,1\n");
    const Printed passed = run(session, "UTILITY CHECK " + clean + " SCHEMA " + schema);
    EXPECT_TRUE(passed.ok);
    EXPECT_EQ(passed.err, "");
    EXPECT_TRUE(contains(passed.out, "1 rows, 1 assets (AssetId): 0 errors, 0 warnings"));

    // A refusal of one line is that line on stderr and nothing on stdout.
    const Printed refused = run(session, "CIRCLE 0,0 -1");
    EXPECT_FALSE(refused.ok);
    EXPECT_EQ(refused.out, "");
    EXPECT_TRUE(refused.err.starts_with("error: ")) << refused.err;
    EXPECT_EQ(refused.err.find('\n'), refused.err.size() - 1) << refused.err;
}
