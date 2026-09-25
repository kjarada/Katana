// SHEETS APPEND path (sheet_verbs.hpp, docs/plotting.md "Sheets on the
// command line"): another set's sheets, read from the JSON file SHEETS SAVE
// wrote, put after the sheets there are as ONE undoable step - what the
// Sheets editor's Sheet Set > Append Sheets From File runs. The appended
// sheets are renumbered as a generator's are (prepareForAppend); the set
// keeps its own title block, revisions and page setup; and what cannot be
// appended changes nothing and says why.

#include <gtest/gtest.h>

#include <filesystem>
#include <fstream>
#include <set>
#include <string>
#include <vector>

#include "katana/cad/command_interpreter.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/plotting/generators.hpp"
#include "katana/cad/plotting/sheet_json.hpp"
#include "katana/cad/plotting/sheet_verbs.hpp"

namespace fs = std::filesystem;
using katana::cad::CommandInterpreter;
using katana::cad::Document;
using namespace katana::cad::plotting;
using katana::core::ErrorCode;

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
};

struct ScratchDirectory {
    fs::path path;
    explicit ScratchDirectory(const std::string& name)
        : path(fs::temp_directory_path() / ("katana-sheets-append-" + name))
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

std::string quoted(const fs::path& path)
{
    return "\"" + path.generic_string() + "\"";
}

// A set of tiles with their key plan, as GENERATE grid lays them: 400 x 200
// m at 1:500 with 10 m of overlap is six tiles and a key plan (the verb
// tests count them), saved to `file`.
void saveTiles(const fs::path& file)
{
    Session tiles;
    tiles.ok("GENERATE grid area=0,0,400,200 scale=500 overlap=10");
    tiles.ok("TITLEBLOCK organisation \"Tile Office\"");
    tiles.ok("SHEETS SAVE " + quoted(file));
}

} // namespace

TEST(SheetsAppend, AnotherSetsSheetsGoAfterTheseInOneStep)
{
    const ScratchDirectory scratch("one-step");
    const fs::path file = scratch.path / "tile set.json";
    saveTiles(file);
    const SheetSet saved = readSheetSetFile(file).value();
    ASSERT_EQ(saved.sheets.size(), 7u);

    Session s;
    s.ok("SHEET NEW COVER");
    s.ok("SHEET NEW NOTES");
    s.ok("VIEW ADD 2 notes");
    s.ok("TITLEBLOCK organisation \"Home Office\"");
    s.ok("SHEETS PAGESETUP style=grey");
    const SheetSet before = s.set();
    const std::size_t steps = s.steps();

    const std::string reply = s.ok("SHEETS APPEND " + quoted(file));
    EXPECT_EQ(reply.substr(0, reply.find('\n')),
              "appended 7 sheets from " + quoted(file) + ": 3 to 9");
    EXPECT_NE(reply.find("\nsheet 3 id="), std::string::npos) << reply;
    EXPECT_NE(reply.find("\nsheet 9 id="), std::string::npos) << reply;
    EXPECT_EQ(s.steps(), steps + 1);
    EXPECT_EQ(s.document.history().undoName(), "APPEND_SHEETS");

    // The first two are as they were; the rest are the file's, renumbered
    // as a generator's output is when it joins a set.
    ASSERT_EQ(s.set().sheets.size(), 9u);
    EXPECT_EQ(s.set().sheets[0], before.sheets[0]);
    EXPECT_EQ(s.set().sheets[1], before.sheets[1]);
    std::vector<Sheet> expected = saved.sheets;
    prepareForAppend(before, expected);
    for (std::size_t i = 0; i < expected.size(); ++i) {
        EXPECT_EQ(s.set().sheets[2 + i], expected[i]) << "sheet " << 3 + i;
    }
    // Every sheet and view id is still one of a kind.
    std::set<std::string> ids;
    std::size_t views = 0;
    for (const Sheet& sheet : s.set().sheets) {
        EXPECT_TRUE(ids.insert(sheet.id).second) << sheet.id;
        for (const Viewport& viewport : sheet.viewports) {
            EXPECT_TRUE(ids.insert(viewport.id).second) << viewport.id;
            ++views;
        }
    }
    EXPECT_EQ(ids.size(), 9u + views);
    // The set keeps its own title block and page setup.
    EXPECT_EQ(s.set().defaults, before.defaults);
    EXPECT_EQ(s.set().pageSetup, before.pageSetup);
    EXPECT_EQ(s.set().revisions, before.revisions);

    ASSERT_TRUE(s.document.undo().ok());
    EXPECT_EQ(s.set(), before);
}

TEST(SheetsAppend, TheSameFileAppendedTwiceGivesTwoSetsOfSheetsNoIdTwice)
{
    const ScratchDirectory scratch("twice");
    const fs::path file = scratch.path / "tiles.json";
    saveTiles(file);
    Session s;
    s.ok("SHEETS APPEND " + quoted(file));
    s.ok("SHEETS APPEND " + quoted(file));
    ASSERT_EQ(s.set().sheets.size(), 14u);
    std::set<std::string> ids;
    for (const Sheet& sheet : s.set().sheets) {
        EXPECT_TRUE(ids.insert(sheet.id).second) << sheet.id;
        for (const Viewport& viewport : sheet.viewports) {
            EXPECT_TRUE(ids.insert(viewport.id).second) << viewport.id;
        }
    }
    // The same lines give the same set every time.
    Session again;
    again.ok("SHEETS APPEND " + quoted(file));
    again.ok("SHEETS APPEND " + quoted(file));
    EXPECT_EQ(again.set(), s.set());
}

TEST(SheetsAppend, WhatCannotBeAppendedChangesNothingAndSaysWhy)
{
    const ScratchDirectory scratch("refused");
    Session s;
    s.ok("SHEET NEW PLAN");
    const SheetSet before = s.set();
    const std::size_t steps = s.steps();

    EXPECT_EQ(s.refused("SHEETS APPEND " + quoted(scratch.path / "none.json")).code,
              ErrorCode::NotFound);
    writeFile(scratch.path / "junk.json", "not json");
    EXPECT_EQ(s.refused("SHEETS APPEND " + quoted(scratch.path / "junk.json")).code,
              ErrorCode::ParseFailure);
    writeFile(scratch.path / "newer.json", R"({"format": "katana-sheets", "version": 99})");
    EXPECT_EQ(s.refused("SHEETS APPEND " + quoted(scratch.path / "newer.json")).code,
              ErrorCode::Unsupported);
    // An empty set appends nothing, which is said rather than made a step.
    ASSERT_TRUE(writeSheetSetFile(SheetSet{}, scratch.path / "empty.json").ok());
    const auto empty = s.refused("SHEETS APPEND " + quoted(scratch.path / "empty.json"));
    EXPECT_EQ(empty.code, ErrorCode::InvalidArgument);
    EXPECT_EQ(empty.message, "the file holds no sheets to append");
    EXPECT_NE(s.refused("SHEETS APPEND").message.find("usage: SHEETS"), std::string::npos);
    EXPECT_NE(s.refused("SHEETS APPEND a.json b.json").message.find("usage: SHEETS"), std::string::npos);
    EXPECT_EQ(s.set(), before);
    EXPECT_EQ(s.steps(), steps);
}

TEST(SheetsAppend, StoredSheetsThatCannotBeReadAreNotAppendedTo)
{
    // Appending to sheets that cannot be read would replace them with the
    // file's; only SHEETS LOAD replaces them, on purpose.
    const ScratchDirectory scratch("unreadable");
    const fs::path file = scratch.path / "tiles.json";
    saveTiles(file);
    Session s;
    auto metadata = s.document.metadata();
    metadata.unknownKeys.insert_or_assign("sheets", R"({"format": "katana-sheets", "version": 99})");
    s.document.setMetadata(metadata);
    EXPECT_EQ(s.refused("SHEETS APPEND " + quoted(file)).code, ErrorCode::Unsupported);
    EXPECT_EQ(s.document.metadata().unknownKeys.at("sheets"),
              R"({"format": "katana-sheets", "version": 99})");
}

TEST(SheetsAppend, TheHelpListsIt)
{
    Session s;
    EXPECT_NE(s.ok("HELP SHEETS").find("SHEETS APPEND path"), std::string::npos);
}
