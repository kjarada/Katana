#include <gtest/gtest.h>

#include <cmath>

#include <cctype>
#include <set>

#include <filesystem>
#include <map>
#include <fstream>
#include <string>
#include <utility>

#include "katana/storage/project_store.hpp"
#include "katana/storage/sqlite_database.hpp"

using namespace katana::storage;
namespace fs = std::filesystem;
using katana::core::ErrorCode;
using katana::entity::Color;
using katana::entity::Entity;
using katana::entity::Layer;
using katana::entity::Model;
using katana::entity::PropertyDefinition;
using katana::entity::PropertyType;
using katana::entity::PropertyValue;
using katana::entity::Style;
using katana::geometry::Arc2;
using katana::geometry::Circle2;
using katana::geometry::Point2;
using katana::geometry::Polyline2;
using katana::geometry::Segment2;

namespace {

// Each test gets its own empty directory, removed afterwards.
class StorageTest : public ::testing::Test {
  protected:
    void SetUp() override
    {
        const auto* info = ::testing::UnitTest::GetInstance()->current_test_info();
        root_ = fs::temp_directory_path() / "katana-storage-tests" /
                (std::string(info->test_suite_name()) + "." + info->name());
        fs::remove_all(root_);
        fs::create_directories(root_);
    }
    void TearDown() override
    {
        std::error_code ignored;
        fs::remove_all(root_, ignored);
    }

    [[nodiscard]] fs::path projectDir() const { return root_ / "site.katana"; }

    fs::path root_;
};

using SqliteWrapper = StorageTest;
using ProjectStoreLifecycle = StorageTest;
using ProjectStoreRoundTrip = StorageTest;
using ProjectStoreMigration = StorageTest;
using ProjectStoreRecovery = StorageTest;

Model sampleModel()
{
    Model model;
    EXPECT_TRUE(model.layers.add(Layer{"Survey", Color{255, 0, 0, 255}, true, false, "dashed", 0.5}).ok());
    EXPECT_TRUE(model.layers.add(Layer{"Locked", Color{0, 0, 255, 128}, false, true}).ok());
    EXPECT_TRUE(model.styles.add(Style{"Boundary", Color{0, 255, 0, 255}, 0.7, "continuous"}).ok());
    EXPECT_TRUE(model.styles.add(Style{"ByLayer", std::nullopt, 0.25, "continuous"}).ok());
    EXPECT_TRUE(model.properties
                    .define(PropertyDefinition{"elevation", PropertyType::Real, "Ground level (m)",
                                               PropertyValue{0.0}})
                    .ok());
    EXPECT_TRUE(model.properties.define(PropertyDefinition{"code", PropertyType::Text, "", {}}).ok());

    Entity line;
    line.geometry = Segment2{Point2(500000.123456789, 5000000.987654321), Point2(1.0 / 3.0, -2e-9)};
    line.layer = "Survey";
    line.style = "Boundary";
    line.color = Color{1, 2, 3, 255};
    line.properties = {{"elevation", 101.25}, {"code", std::string("IP \"A\"")}, {"n", std::int64_t{7}}};
    line.metadata = {{"source", std::string("fieldbook.csv")}};
    EXPECT_TRUE(model.entities.add(line).ok());

    Entity hidden;
    hidden.geometry = Circle2{Point2(3, 4), 5.5};
    hidden.visible = false;
    EXPECT_TRUE(model.entities.add(hidden).ok());

    for (const katana::entity::Geometry& geometry : std::initializer_list<katana::entity::Geometry>{
             katana::entity::PointGeometry{Point2(9, 9)}, Arc2{Point2(0, 0), 2.0, 0.5, -1.5},
             Polyline2{{Point2(0, 0), Point2(4, 0), Point2(4, 3)}, true},
             katana::entity::TextGeometry{Point2(1, 1), "Ünïcödé — BM1", 2.5, 0.25},
             katana::entity::DimensionGeometry{Point2(0, 0), Point2(10, 0), 2.0, "10.00"}}) {
        Entity entity;
        entity.geometry = geometry;
        EXPECT_TRUE(model.entities.add(entity).ok());
    }
    // Retire an id so that nextEntityId is not simply "count + 1".
    const auto doomed = model.entities.add(hidden);
    EXPECT_TRUE(model.entities.remove(*doomed).ok());
    return model;
}

void overwriteWithGarbage(const fs::path& file)
{
    const auto size = fs::file_size(file);
    std::ofstream out(file, std::ios::binary | std::ios::trunc);
    const std::string garbage(static_cast<std::size_t>(size), '\x5A');
    out.write(garbage.data(), static_cast<std::streamsize>(garbage.size()));
}

} // namespace

// ---- SQLite wrapper ------------------------------------------------------------

TEST_F(SqliteWrapper, StatementsBindStepAndReportErrors)
{
    auto database = SqliteDatabase::openInMemory();
    ASSERT_TRUE(database.ok());
    ASSERT_TRUE(database->execute("CREATE TABLE t (id INTEGER PRIMARY KEY, x REAL, s TEXT)").ok());
    EXPECT_TRUE(*database->tableExists("t"));
    EXPECT_FALSE(*database->tableExists("missing"));

    auto insert = database->prepare("INSERT INTO t (id, x, s) VALUES (?1, ?2, ?3)");
    ASSERT_TRUE(insert.ok());
    ASSERT_TRUE(insert->bind(1, std::int64_t{7}).ok());
    ASSERT_TRUE(insert->bind(2, 0.1 + 0.2).ok());
    ASSERT_TRUE(insert->bind(3, std::string_view("text with 'quotes'")).ok());
    ASSERT_TRUE(insert->run().ok());

    auto select = database->prepare("SELECT id, x, s FROM t");
    ASSERT_TRUE(select.ok());
    ASSERT_TRUE(*select->step());
    EXPECT_EQ(select->columnInt64(0), 7);
    EXPECT_EQ(select->columnDouble(1), 0.1 + 0.2); // REAL columns hold doubles exactly
    EXPECT_EQ(select->columnText(2), "text with 'quotes'");
    EXPECT_FALSE(*select->step());

    const auto bad = database->execute("THIS IS NOT SQL");
    ASSERT_FALSE(bad.ok());
    EXPECT_EQ(bad.error().code, ErrorCode::DatabaseFailure);
    EXPECT_FALSE(bad.error().context.empty()); // carries SQLite's own message
    EXPECT_FALSE(database->prepare("SELECT * FROM nowhere").ok());

    // Primary key violation surfaces as an error, not a silent no-op.
    ASSERT_TRUE(insert->bind(1, std::int64_t{7}).ok());
    ASSERT_TRUE(insert->bind(2, 1.0).ok());
    ASSERT_TRUE(insert->bind(3, std::string_view("dup")).ok());
    EXPECT_FALSE(insert->run().ok());
}

TEST_F(SqliteWrapper, TransactionRollsBackUnlessCommitted)
{
    auto database = SqliteDatabase::openInMemory();
    ASSERT_TRUE(database.ok());
    ASSERT_TRUE(database->execute("CREATE TABLE t (id INTEGER)").ok());
    const auto count = [&] {
        auto statement = database->prepare("SELECT COUNT(*) FROM t");
        EXPECT_TRUE(*statement->step());
        return statement->columnInt64(0);
    };
    {
        auto transaction = SqliteTransaction::begin(*database);
        ASSERT_TRUE(transaction.ok());
        ASSERT_TRUE(database->execute("INSERT INTO t VALUES (1)").ok());
    } // destroyed without commit
    EXPECT_EQ(count(), 0);
    {
        auto transaction = SqliteTransaction::begin(*database);
        ASSERT_TRUE(transaction.ok());
        ASSERT_TRUE(database->execute("INSERT INTO t VALUES (1)").ok());
        ASSERT_TRUE(transaction->commit().ok());
        EXPECT_FALSE(transaction->commit().ok()); // already finished
    }
    EXPECT_EQ(count(), 1);
}

// ---- lifecycle -------------------------------------------------------------------

TEST_F(ProjectStoreLifecycle, CreateBuildsTheProjectLayout)
{
    ProjectMetadata metadata;
    metadata.name = "Hilltop Subdivision";
    auto store = ProjectStore::create(projectDir(), metadata);
    ASSERT_TRUE(store.ok()) << store.error().describe();

    for (const char* name : {"project.db", "backups", "terrain", "pointcloud", "assets", "cache"}) {
        EXPECT_TRUE(fs::exists(projectDir() / name)) << name;
    }
    EXPECT_TRUE(ProjectStore::isProjectDirectory(projectDir()));
    EXPECT_EQ(*store->schemaVersion(), ProjectStore::kCurrentSchemaVersion);

    const auto contents = store->load();
    ASSERT_TRUE(contents.ok());
    EXPECT_EQ(contents->metadata.name, "Hilltop Subdivision");
    EXPECT_FALSE(contents->metadata.createdUtc.empty());
    EXPECT_FALSE(contents->metadata.modifiedUtc.empty());
    EXPECT_TRUE(contents->entities.empty());
    ASSERT_EQ(contents->layers.size(), 1u);
    EXPECT_EQ(contents->layers[0].name, "0");
}

TEST_F(ProjectStoreLifecycle, CreateRefusesToOverwriteAndOpenRequiresAProject)
{
    ASSERT_TRUE(ProjectStore::create(projectDir(), {}).ok());
    EXPECT_EQ(ProjectStore::create(projectDir(), {}).error().code, ErrorCode::AlreadyExists);
    EXPECT_EQ(ProjectStore::open(root_ / "nothing-here").error().code, ErrorCode::NotFound);
    EXPECT_FALSE(ProjectStore::isProjectDirectory(root_));
}

TEST_F(ProjectStoreLifecycle, RejectsForeignAndGarbageDatabases)
{
    fs::create_directories(projectDir());
    {
        auto foreign = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(foreign.ok());
        ASSERT_TRUE(foreign->execute("CREATE TABLE unrelated (x)").ok());
    }
    const auto notKatana = ProjectStore::open(projectDir());
    ASSERT_FALSE(notKatana.ok());
    EXPECT_EQ(notKatana.error().code, ErrorCode::DatabaseFailure);

    overwriteWithGarbage(projectDir() / "project.db");
    EXPECT_EQ(ProjectStore::open(projectDir()).error().code, ErrorCode::DatabaseFailure);
}

// ---- round trip --------------------------------------------------------------------

TEST_F(ProjectStoreRoundTrip, ModelSurvivesSaveCloseOpenLoadExactly)
{
    const Model original = sampleModel();
    // Every field set to something distinctive. Asserting one or two of them
    // would pass just as happily if the others were dropped, swapped or
    // written into each other's columns.
    ProjectMetadata metadata;
    metadata.name = "Round trip";
    metadata.description = "A description with punctuation: commas, \"quotes\" and a / slash.";
    metadata.linearUnit = "US survey foot";
    metadata.coordinateSystem = "EPSG:32630";
    metadata.createdUtc = "2019-03-04T05:06:07Z";
    metadata.modifiedUtc = "1970-01-01T00:00:00Z"; // save() must replace this
    metadata.applicationVersion = "katana-test/9.9.9";
    const std::vector<Relationship> relationships = {{7, 1, "measures", "{\"side\":\"left\"}"}};
    {
        auto store = ProjectStore::create(projectDir(), metadata);
        ASSERT_TRUE(store.ok());
        const auto status = store->save(captureModel(original, metadata, relationships));
        ASSERT_TRUE(status.ok()) << status.error().describe();
    } // connection closed

    auto reopened = ProjectStore::open(projectDir());
    ASSERT_TRUE(reopened.ok()) << reopened.error().describe();
    const auto contents = reopened->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();

    Model loaded;
    ASSERT_TRUE(applyToModel(*contents, loaded).ok());

    EXPECT_EQ(loaded.layers.all(), original.layers.all());
    EXPECT_EQ(loaded.styles.all(), original.styles.all());
    EXPECT_EQ(loaded.properties.all(), original.properties.all());
    EXPECT_EQ(loaded.entities.ids(), original.entities.ids());
    original.entities.forEach([&](const Entity& entity) {
        ASSERT_NE(loaded.entities.find(entity.id), nullptr);
        EXPECT_EQ(*loaded.entities.find(entity.id), entity) << "entity " << entity.id; // bit exact
    });
    // The retired id stays retired after a reload.
    EXPECT_EQ(loaded.entities.nextId(), original.entities.nextId());
    EXPECT_EQ(contents->relationships, relationships);

    // Six of the seven metadata fields survive unchanged...
    EXPECT_EQ(contents->metadata.name, metadata.name);
    EXPECT_EQ(contents->metadata.description, metadata.description);
    EXPECT_EQ(contents->metadata.linearUnit, metadata.linearUnit);
    EXPECT_EQ(contents->metadata.coordinateSystem, metadata.coordinateSystem);
    EXPECT_EQ(contents->metadata.createdUtc, metadata.createdUtc);
    EXPECT_EQ(contents->metadata.applicationVersion, metadata.applicationVersion);

    // ...and modifiedUtc is the one save() owns: it must have replaced the
    // sentinel with a real timestamp rather than passing it through.
    EXPECT_NE(contents->metadata.modifiedUtc, metadata.modifiedUtc)
        << "save() must stamp the modification time, not preserve it";
    EXPECT_FALSE(contents->metadata.modifiedUtc.empty());
    // The writer's format string is "%04d-%02d-%02dT%02d:%02d:%02d.%03dZ", so
    // the stamp is 24 characters: YYYY-MM-DDTHH:MM:SS.mmmZ. Checked by shape
    // rather than by length alone, so a reordering would also be caught.
    const std::string& stamp = contents->metadata.modifiedUtc;
    ASSERT_EQ(stamp.size(), 24u) << stamp;
    EXPECT_EQ(stamp[4], '-');
    EXPECT_EQ(stamp[7], '-');
    EXPECT_EQ(stamp[10], 'T');
    EXPECT_EQ(stamp[13], ':');
    EXPECT_EQ(stamp[16], ':');
    EXPECT_EQ(stamp[19], '.');
    EXPECT_EQ(stamp[23], 'Z');
    for (const std::size_t digit : {0u, 1u, 2u, 3u, 5u, 6u, 8u, 9u, 11u, 12u, 14u, 15u, 17u,
                                    18u, 20u, 21u, 22u}) {
        EXPECT_TRUE(std::isdigit(static_cast<unsigned char>(stamp[digit])))
            << "position " << digit << " of " << stamp;
    }

    // The whole struct, once the one field save() owns is accounted for. This
    // is what catches a field added later and never persisted.
    ProjectMetadata expected = metadata;
    expected.modifiedUtc = contents->metadata.modifiedUtc;
    EXPECT_EQ(contents->metadata, expected);
}

TEST_F(ProjectStoreRoundTrip, SaveIsAtomicWhenTheDatabaseRejectsARow)
{
    auto store = ProjectStore::create(projectDir(), {});
    ASSERT_TRUE(store.ok());
    const Model model = sampleModel();
    ASSERT_TRUE(store->save(captureModel(model, {})).ok());

    // Two layers with the same name violate the primary key half way through the
    // save, after the old rows were deleted inside the transaction.
    ProjectContents broken = captureModel(model, {});
    broken.entities.clear();
    broken.layers.push_back(Layer{"Survey"});
    const auto status = store->save(broken);
    ASSERT_FALSE(status.ok());
    EXPECT_EQ(status.error().code, ErrorCode::DatabaseFailure);

    const auto contents = store->load();
    ASSERT_TRUE(contents.ok());
    EXPECT_EQ(contents->entities.size(), model.entities.size()); // previous save intact
}

TEST_F(ProjectStoreRoundTrip, InvalidContentsAreNeverWrittenOrApplied)
{
    auto store = ProjectStore::create(projectDir(), {});
    ASSERT_TRUE(store.ok());
    const Model model = sampleModel();

    ProjectContents unknownLayer = captureModel(model, {});
    unknownLayer.entities[0].layer = "Nowhere";
    EXPECT_EQ(store->save(unknownLayer).error().code, ErrorCode::InvalidArgument);

    ProjectContents duplicateId = captureModel(model, {});
    duplicateId.entities[1].id = duplicateId.entities[0].id;
    EXPECT_FALSE(store->save(duplicateId).ok());

    ProjectContents staleCounter = captureModel(model, {});
    staleCounter.nextEntityId = 1;
    EXPECT_FALSE(store->save(staleCounter).ok());

    ProjectContents danglingRelationship = captureModel(model, {}, {{1, 9999, "x", ""}});
    EXPECT_FALSE(store->save(danglingRelationship).ok());

    Model target = sampleModel();
    const auto before = target.entities.ids();
    EXPECT_FALSE(applyToModel(unknownLayer, target).ok());
    EXPECT_EQ(target.entities.ids(), before); // rejected before anything was touched
}

// ---- migration -----------------------------------------------------------------------


// Undoes every migration above `version`, so a project can be made to look as
// though it were written by an older build.
//
// Migrations are append-only and each runs once, so a fixture that claims
// version N must actually LOOK like version N: leaving a later migration's
// table in place makes the replay fail with "table already exists", which is
// the migration machinery working correctly on a fixture that lied.
//
// This exists as one function because it was previously written out inside
// each test, and every new migration then broke every older fixture. Adding a
// migration now means adding one case here.
[[nodiscard]] katana::core::Status rewindSchemaTo(SqliteDatabase& database, int version)
{
    // Newest first: a migration is undone before the one it was built on.
    if (version < 10) {
        if (auto status = database.execute("DROP TABLE IF EXISTS survey_job_files;"
                                           "DROP TABLE IF EXISTS survey_jobs;");
            !status) {
            return status;
        }
    }
    if (version < 9) {
        if (auto status = database.execute("ALTER TABLE styles DROP COLUMN description;"
                                           "ALTER TABLE styles DROP COLUMN symbol;"
                                           "ALTER TABLE styles DROP COLUMN symbol_size;");
            !status) {
            return status;
        }
    }
    if (version < 8) {
        if (auto status = database.execute("DROP TABLE IF EXISTS alignment_pvis;"); !status) {
            return status;
        }
    }
    if (version < 7) {
        if (auto status = database.execute("DROP TABLE IF EXISTS alignment_pis;"
                                           "DROP TABLE IF EXISTS alignments;");
            !status) {
            return status;
        }
    }
    if (version < 6) {
        if (auto status = database.execute("DROP TABLE IF EXISTS hatch_families;"
                                           "DROP TABLE IF EXISTS hatch_patterns;"
                                           "ALTER TABLE layers DROP COLUMN hatch_pattern;"
                                           "ALTER TABLE styles DROP COLUMN hatch_pattern;");
            !status) {
            return status;
        }
    }
    if (version < 5) {
        if (auto status = database.execute("DROP TABLE IF EXISTS dimension_styles;"
                                           "ALTER TABLE layers DROP COLUMN dimension_style;");
            !status) {
            return status;
        }
    }
    if (version < 4) {
        if (auto status = database.execute("DROP TABLE IF EXISTS linetype_elements;"
                                           "DROP TABLE IF EXISTS linetypes;");
            !status) {
            return status;
        }
    }
    return database.setUserVersion(version);
}

TEST_F(ProjectStoreMigration, UpgradesAVersion1ProjectAndBacksItUpFirst)
{
    // A project exactly as Katana schema version 1 wrote it (no relationships table).
    fs::create_directories(projectDir());
    {
        auto v1 = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(v1.ok());
        ASSERT_TRUE(v1->setApplicationId(0x4B544E41).ok());
        ASSERT_TRUE(v1->execute(R"sql(
            CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
            CREATE TABLE layers (name TEXT PRIMARY KEY, color TEXT NOT NULL, visible INTEGER NOT NULL,
                locked INTEGER NOT NULL, linetype TEXT NOT NULL, line_weight REAL NOT NULL) WITHOUT ROWID;
            CREATE TABLE styles (name TEXT PRIMARY KEY, color TEXT, line_weight REAL NOT NULL,
                linetype TEXT NOT NULL) WITHOUT ROWID;
            CREATE TABLE property_definitions (name TEXT PRIMARY KEY, type TEXT NOT NULL,
                description TEXT NOT NULL, default_value TEXT) WITHOUT ROWID;
            CREATE TABLE entities (id INTEGER PRIMARY KEY, type TEXT NOT NULL,
                layer TEXT NOT NULL REFERENCES layers(name), style TEXT NOT NULL, color TEXT,
                visible INTEGER NOT NULL, geometry TEXT NOT NULL, properties TEXT NOT NULL,
                metadata TEXT NOT NULL);
            INSERT INTO metadata VALUES ('name', 'Legacy site'), ('next_entity_id', '6');
            INSERT INTO layers VALUES ('0', '#FFFFFF', 1, 0, 'continuous', 0.25);
            INSERT INTO entities VALUES (5, 'Circle', '0', '', NULL, 1,
                '{"type":"Circle","center":[1.5,2.5],"radius":4.0}', '{}', '{}');
        )sql")
                        .ok());
        ASSERT_TRUE(v1->setUserVersion(1).ok());
    }

    auto store = ProjectStore::open(projectDir());
    ASSERT_TRUE(store.ok()) << store.error().describe();
    EXPECT_EQ(*store->schemaVersion(), ProjectStore::kCurrentSchemaVersion);
    EXPECT_EQ(store->listBackups().size(), 1u); // taken before the schema was touched

    const auto contents = store->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    EXPECT_EQ(contents->metadata.name, "Legacy site");
    ASSERT_EQ(contents->entities.size(), 1u);
    EXPECT_EQ(contents->entities[0].id, 5u);
    EXPECT_EQ(std::get<Circle2>(contents->entities[0].geometry), (Circle2{Point2(1.5, 2.5), 4.0}));
    EXPECT_EQ(contents->nextEntityId, 6u);

    // The pre-migration backup is still a version 1 database.
    auto backup = SqliteDatabase::open(store->listBackups().front(),
                                       SqliteDatabase::OpenMode::ReadOnly);
    ASSERT_TRUE(backup.ok());
    EXPECT_EQ(*backup->userVersion(), 1);
    EXPECT_FALSE(*backup->tableExists("relationships"));
}

TEST_F(ProjectStoreMigration, RefusesProjectsFromANewerKatana)
{
    { ASSERT_TRUE(ProjectStore::create(projectDir(), {}).ok()); }
    {
        auto raw = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(raw.ok());
        ASSERT_TRUE(raw->setUserVersion(ProjectStore::kCurrentSchemaVersion + 1).ok());
    }
    const auto opened = ProjectStore::open(projectDir());
    ASSERT_FALSE(opened.ok());
    EXPECT_EQ(opened.error().code, ErrorCode::Unsupported);
}

// ---- backup & recovery --------------------------------------------------------------------

TEST_F(ProjectStoreRecovery, BackupsArePrunedToTheNewest)
{
    auto store = ProjectStore::create(projectDir(), {});
    ASSERT_TRUE(store.ok());

    // Every path in creation order, so the test knows which two SHOULD survive
    // rather than only checking that two did. Four backups taken back to back
    // land in the same millisecond, so this also exercises the collision
    // suffix and the ordering that has to parse it.
    std::vector<fs::path> made;
    for (int i = 0; i < 4; ++i) {
        const auto path = store->backup(/*keep=*/2);
        ASSERT_TRUE(path.ok()) << path.error().describe();
        EXPECT_TRUE(fs::exists(*path)) << "backup " << i << " was not written";
        made.push_back(*path);
    }
    ASSERT_EQ(std::set<fs::path>(made.begin(), made.end()).size(), made.size())
        << "each backup must get its own file, even within one millisecond";

    const auto backups = store->listBackups();
    ASSERT_EQ(backups.size(), 2u);
    // BOTH survivors are identified, newest first: asserting only front() would
    // pass with any arbitrary second file.
    EXPECT_EQ(backups[0], made[3]);
    EXPECT_EQ(backups[1], made[2]);

    // The pruned ones are gone from DISK, not merely absent from the listing -
    // a prune that only stopped listing them would leak a file per save.
    EXPECT_FALSE(fs::exists(made[0])) << "the oldest backup should have been deleted";
    EXPECT_FALSE(fs::exists(made[1]));
    EXPECT_TRUE(fs::exists(made[2]));
    EXPECT_TRUE(fs::exists(made[3]));

    // And what survived is usable. A prune that kept a truncated file would
    // satisfy every assertion above and still lose the project.
    for (const fs::path& kept : backups) {
        auto database = SqliteDatabase::open(kept, SqliteDatabase::OpenMode::ReadOnly);
        ASSERT_TRUE(database.ok()) << kept.string() << ": " << database.error().describe();
        const auto version = database->userVersion();
        ASSERT_TRUE(version.ok());
        EXPECT_EQ(*version, ProjectStore::kCurrentSchemaVersion);
    }
}

TEST_F(ProjectStoreRecovery, TheKeepCountIsHonouredAtItsEdges)
{
    auto store = ProjectStore::create(projectDir(), {});
    ASSERT_TRUE(store.ok());

    // keep larger than the number taken: nothing is pruned.
    for (int i = 0; i < 3; ++i) {
        ASSERT_TRUE(store->backup(/*keep=*/10).ok());
    }
    EXPECT_EQ(store->listBackups().size(), 3u);

    // keep = 1 collapses to the newest, which must be the one just made.
    const auto newest = store->backup(/*keep=*/1);
    ASSERT_TRUE(newest.ok()) << newest.error().describe();
    const auto one = store->listBackups();
    ASSERT_EQ(one.size(), 1u);
    EXPECT_EQ(one.front(), *newest);
    EXPECT_TRUE(fs::exists(*newest));

    // keep = 0 removes even the backup just taken. That is a strange thing to
    // ask for, but it must do what it says rather than quietly keep one.
    const auto discarded = store->backup(/*keep=*/0);
    ASSERT_TRUE(discarded.ok()) << discarded.error().describe();
    EXPECT_TRUE(store->listBackups().empty());
    EXPECT_FALSE(fs::exists(*discarded));
}

TEST_F(ProjectStoreRecovery, RestoresTheNewestSoundBackupAndKeepsTheDamagedFile)
{
    const Model model = sampleModel();
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
        ASSERT_TRUE(store->backup().ok()); // older, sound
        const auto newer = store->backup();
        ASSERT_TRUE(newer.ok());
        overwriteWithGarbage(*newer); // the newest backup is damaged as well
    }
    overwriteWithGarbage(projectDir() / "project.db");
    ASSERT_FALSE(ProjectStore::open(projectDir()).ok());

    const auto report = ProjectStore::recover(projectDir());
    ASSERT_TRUE(report.ok()) << report.error().describe();
    EXPECT_TRUE(report->restored);
    EXPECT_FALSE(report->problems.empty());
    EXPECT_TRUE(fs::exists(report->damagedFileKeptAs)); // evidence is never deleted

    auto store = ProjectStore::open(projectDir());
    ASSERT_TRUE(store.ok()) << store.error().describe();
    const auto contents = store->load();
    ASSERT_TRUE(contents.ok());
    EXPECT_EQ(contents->entities.size(), model.entities.size());
}

TEST_F(ProjectStoreRecovery, SoundProjectsAreLeftAloneAndMissingBackupsAreReported)
{
    { ASSERT_TRUE(ProjectStore::create(projectDir(), {}).ok()); }
    const auto sound = ProjectStore::recover(projectDir());
    ASSERT_TRUE(sound.ok());
    EXPECT_FALSE(sound->restored);

    overwriteWithGarbage(projectDir() / "project.db");
    const auto hopeless = ProjectStore::recover(projectDir());
    ASSERT_FALSE(hopeless.ok());
    EXPECT_EQ(hopeless.error().code, ErrorCode::DatabaseFailure);
    EXPECT_TRUE(fs::exists(projectDir() / "project.db")); // untouched when nothing can replace it
}

// ---- regressions: four ways the store used to lose data --------------------------------------

// applyToModel() used to reset the caller's model before populating it, and
// reset it again if any insertion failed - so a project that passed
// validateContents but tripped a table rule destroyed the drawing that was
// already open. docs/cad.md documents the opposite, and Document::open relies
// on it: "a failed open leaves the current drawing exactly as it was".
TEST_F(ProjectStoreRoundTrip, AFailedLoadLeavesTheCallersModelUntouched)
{
    Model existing = sampleModel();
    const auto idsBefore = existing.entities.ids();
    const auto layersBefore = existing.layers.all().size();
    ASSERT_FALSE(idsBefore.empty());

    // Contents that pass validateContents but that the tables reject: a style
    // named twice cannot be added twice.
    ProjectContents contents;
    contents.layers.push_back(Layer{std::string(katana::entity::kDefaultLayerName)});
    contents.styles.push_back(Style{"Duplicate"});
    contents.styles.push_back(Style{"Duplicate"});
    contents.nextEntityId = 1;

    const auto status = applyToModel(contents, existing);
    ASSERT_FALSE(status.ok()) << "a duplicate style should be refused";

    // The drawing that was open must still be there, in full.
    EXPECT_EQ(existing.entities.ids(), idsBefore);
    EXPECT_EQ(existing.layers.all().size(), layersBefore);
}

// The observer belongs to the model's OWNER, not to the contents being loaded.
// Committing the staged contents with a plain move-assignment would carry the
// staged model's empty observer across and silently stop every change
// notification the Document depends on.
TEST_F(ProjectStoreRoundTrip, LoadingKeepsTheCallersEntityObserver)
{
    Model model;
    int events = 0;
    model.entities.setObserver([&events](const katana::entity::ChangeEvent&) { ++events; });

    const auto status = applyToModel(captureModel(sampleModel(), {}), model);
    ASSERT_TRUE(status.ok()) << status.error().describe();
    EXPECT_FALSE(model.entities.empty());

    const int afterLoad = events;
    EXPECT_GT(afterLoad, 0) << "the wholesale replacement should report a Cleared event";

    // And the observer must still be live afterwards.
    Entity point;
    point.geometry = katana::entity::PointGeometry{Point2(1.0, 2.0)};
    ASSERT_TRUE(model.entities.add(std::move(point)).ok());
    EXPECT_GT(events, afterLoad) << "the caller's observer was replaced by the loaded model's";
}

// A database whose process was killed mid-commit has a hot rollback journal
// beside it. SQLite replays that journal on the next WRITABLE open, after which
// the file is sound. Inspecting read-only cannot replay it and reports
// SQLITE_READONLY_ROLLBACK, which used to be taken for corruption - so recover()
// threw away a perfectly recoverable database for an older backup.
TEST_F(ProjectStoreRecovery, ACrashInterruptedDatabaseIsNotMistakenForDamage)
{
    const Model model = sampleModel();
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
    }

    // Simulate the crash: a leftover journal beside an otherwise intact file.
    // Its header is deliberately not a valid journal, which is exactly what a
    // torn write leaves behind; SQLite discards such a journal on open.
    const fs::path journal = projectDir() / "project.db-journal";
    {
        std::ofstream out(journal, std::ios::binary);
        ASSERT_TRUE(out.good());
        const std::string rubbish(512, '\0');
        out.write(rubbish.data(), static_cast<std::streamsize>(rubbish.size()));
    }
    ASSERT_TRUE(fs::exists(journal));

    // The store must open, and the drawing must still be all there.
    auto reopened = ProjectStore::open(projectDir());
    ASSERT_TRUE(reopened.ok()) << "a hot journal was taken for corruption: "
                               << reopened.error().describe();
    const auto loaded = reopened->load();
    ASSERT_TRUE(loaded.ok()) << loaded.error().describe();
    EXPECT_EQ(loaded->entities.size(), model.entities.size());
}

// recover() must move the damaged file's rollback journal aside, or SQLite
// replays it over the restored data on the next open. When project.db was
// missing entirely, damagedFileKeptAs is empty and concat() produced the
// RELATIVE name "-journal": the journal was moved into the working directory
// and left sitting beside the restored database.
TEST_F(ProjectStoreRecovery, TheDamagedJournalIsMovedAsideAndNeverIntoTheWorkingDirectory)
{
    const Model model = sampleModel();
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
        ASSERT_TRUE(store->backup().ok());
    }

    // project.db is gone, but its journal is not: the case that produced a
    // relative rename target.
    fs::remove(projectDir() / "project.db");
    const fs::path journal = projectDir() / "project.db-journal";
    { std::ofstream out(journal, std::ios::binary); out << "stale journal"; }

    const fs::path strayInCwd = fs::current_path() / "-journal";
    std::error_code ignored;
    fs::remove(strayInCwd, ignored);

    const auto report = ProjectStore::recover(projectDir());
    ASSERT_TRUE(report.ok()) << report.error().describe();
    EXPECT_TRUE(report->restored);

    EXPECT_FALSE(fs::exists(journal)) << "the stale journal was left beside the restored database";
    EXPECT_FALSE(fs::exists(strayInCwd))
        << "the journal was renamed to a relative path and landed in the working directory";
    fs::remove(strayInCwd, ignored);

    // And the restored database opens.
    EXPECT_TRUE(ProjectStore::open(projectDir()).ok());
}

// "Newest backup" used to be decided by filename, so anything else dropped into
// backups/ could outrank the real snapshots - and the collision suffix sorted
// BACKWARDS, because '.' (0x2E) is above '-' (0x2D), making "...T120000.db"
// beat the later "...T120000-1.db".
TEST_F(ProjectStoreRecovery, BackupOrderIgnoresStrayFilesAndHandlesTheCollisionSuffix)
{
    auto store = ProjectStore::create(projectDir(), {});
    ASSERT_TRUE(store.ok());
    ASSERT_TRUE(store->save(captureModel(sampleModel(), {})).ok());

    const auto first = store->backup(/*keep=*/50);
    ASSERT_TRUE(first.ok()) << first.error().describe();

    const fs::path backupDir = projectDir() / "backups";
    // A name that beats every real snapshot on a plain descending filename sort.
    const fs::path stray = backupDir / "zzz-not-ours.db";
    fs::copy_file(*first, stray);
    // A hand-made copy that is not even a database.
    { std::ofstream out(backupDir / "scratch.db"); out << "not a database"; }

    const auto listed = store->listBackups();
    for (const fs::path& path : listed) {
        EXPECT_NE(path.filename(), stray.filename())
            << "a file this store never wrote was offered as a backup";
        EXPECT_NE(path.filename().string(), "scratch.db");
    }
    ASSERT_FALSE(listed.empty());

    // The collision suffix: same second, higher sequence, therefore newer.
    const std::string stem = first->stem().string();
    const fs::path collision = backupDir / (stem + "-1.db");
    fs::copy_file(*first, collision);
    const auto reordered = store->listBackups();
    ASSERT_GE(reordered.size(), 2u);
    EXPECT_EQ(reordered.front().filename(), collision.filename())
        << "the later collision backup must rank above the one it collided with";
}

TEST_F(ProjectStoreMigration, SavingAMigratedProjectConvertsItsGeometryToBlobs)
{
    // A schema-1 project keeps its JSON until it is next saved; after that the
    // blob is the record. Both readings must give the same geometry, or the
    // act of saving would silently change a drawing.
    fs::create_directories(projectDir());
    {
        auto v1 = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(v1.ok());
        ASSERT_TRUE(v1->setApplicationId(0x4B544E41).ok());
        ASSERT_TRUE(v1->execute(R"sql(
            CREATE TABLE metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
            CREATE TABLE layers (name TEXT PRIMARY KEY, color TEXT NOT NULL, visible INTEGER NOT NULL,
                locked INTEGER NOT NULL, linetype TEXT NOT NULL, line_weight REAL NOT NULL) WITHOUT ROWID;
            CREATE TABLE styles (name TEXT PRIMARY KEY, color TEXT, line_weight REAL NOT NULL,
                linetype TEXT NOT NULL) WITHOUT ROWID;
            CREATE TABLE property_definitions (name TEXT PRIMARY KEY, type TEXT NOT NULL,
                description TEXT NOT NULL, default_value TEXT) WITHOUT ROWID;
            CREATE TABLE entities (id INTEGER PRIMARY KEY, type TEXT NOT NULL,
                layer TEXT NOT NULL REFERENCES layers(name), style TEXT NOT NULL, color TEXT,
                visible INTEGER NOT NULL, geometry TEXT NOT NULL, properties TEXT NOT NULL,
                metadata TEXT NOT NULL);
            INSERT INTO metadata VALUES ('name', 'Converted'), ('next_entity_id', '9');
            INSERT INTO layers VALUES ('0', '#FFFFFF', 1, 0, 'continuous', 0.25);
            INSERT INTO entities VALUES (5, 'Circle', '0', '', NULL, 1,
                '{"type":"Circle","center":[1.5,2.5],"radius":4.0}', '{}', '{}');
        )sql")
                        .ok());
        ASSERT_TRUE(v1->setUserVersion(1).ok());
    }

    const Circle2 expected{Point2(1.5, 2.5), 4.0};

    // Open (migrates the schema, still reads JSON) and save (writes blobs).
    {
        auto store = ProjectStore::open(projectDir());
        ASSERT_TRUE(store.ok()) << store.error().describe();
        const auto contents = store->load();
        ASSERT_TRUE(contents.ok()) << contents.error().describe();
        ASSERT_EQ(contents->entities.size(), 1u);
        EXPECT_EQ(std::get<Circle2>(contents->entities[0].geometry), expected);
        ASSERT_TRUE(store->save(*contents).ok());
    }

    // The blob is now populated and the legacy column emptied.
    {
        auto database = SqliteDatabase::open(projectDir() / "project.db",
                                             SqliteDatabase::OpenMode::ReadOnly);
        ASSERT_TRUE(database.ok());
        auto row = database->prepare("SELECT geometry, geometry_blob FROM entities WHERE id = 5");
        ASSERT_TRUE(row.ok());
        const auto stepped = row->step();
        ASSERT_TRUE(stepped.ok());
        ASSERT_TRUE(*stepped);
        EXPECT_TRUE(row->columnText(0).empty()) << "the JSON column is no longer written";
        EXPECT_FALSE(row->columnIsNull(1)) << "the blob must have been written";
        EXPECT_GE(row->columnBlob(1).size(), 2u);
    }

    // And it still reads back as the same circle.
    auto reopened = ProjectStore::open(projectDir());
    ASSERT_TRUE(reopened.ok()) << reopened.error().describe();
    const auto contents = reopened->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    ASSERT_EQ(contents->entities.size(), 1u);
    EXPECT_EQ(std::get<Circle2>(contents->entities[0].geometry), expected);
    EXPECT_EQ(contents->metadata.name, "Converted");
}

TEST_F(ProjectStoreMigration, WhereBothEncodingsArePresentTheBlobWins)
{
    // A half-migrated row - JSON from before, blob from a later save - must
    // read the blob, which is the newer of the two. The JSON here is
    // deliberately a DIFFERENT circle, so a reader taking the wrong column
    // fails loudly instead of happening to agree.
    const Model model = sampleModel();
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
    }
    {
        auto database = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(database.ok());
        ASSERT_TRUE(database
                        ->execute("UPDATE entities SET geometry = "
                                  "'{\"type\":\"Circle\",\"center\":[999.0,999.0],\"radius\":1.0}'")
                        .ok());
    }

    auto reopened = ProjectStore::open(projectDir());
    ASSERT_TRUE(reopened.ok()) << reopened.error().describe();
    const auto contents = reopened->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();

    Model loaded;
    ASSERT_TRUE(applyToModel(*contents, loaded).ok());
    model.entities.forEach([&](const Entity& entity) {
        const Entity* back = loaded.entities.find(entity.id);
        ASSERT_NE(back, nullptr);
        EXPECT_EQ(back->geometry, entity.geometry)
            << "entity " << entity.id << " was read from the stale JSON column";
    });
}

TEST_F(ProjectStoreRoundTrip, LinetypeDefinitionsSurviveSaveAndReload)
{
    katana::entity::Linetype dashed;
    dashed.name = "dashed";
    dashed.description = "Dashed  __  __  __";
    dashed.pattern = {katana::entity::LinetypeElement{1.0},
                      katana::entity::LinetypeElement{-0.5}};

    katana::entity::Linetype dashDot;
    dashDot.name = "dashdot";
    dashDot.description = "Dash dot  __ . __ .";
    // Includes a dot (exactly zero), which is the element a naive
    // "store the sign" encoding loses.
    dashDot.pattern = {katana::entity::LinetypeElement{2.0},
                       katana::entity::LinetypeElement{-0.25},
                       katana::entity::LinetypeElement{0.0},
                       katana::entity::LinetypeElement{-0.25}};

    Model model;
    ASSERT_TRUE(model.linetypes.add(dashed).ok());
    ASSERT_TRUE(model.linetypes.add(dashDot).ok());

    Layer fence;
    fence.name = "fence";
    fence.linetype = "dashed";
    ASSERT_TRUE(model.layers.add(fence).ok());

    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
    }

    auto reopened = ProjectStore::open(projectDir());
    ASSERT_TRUE(reopened.ok()) << reopened.error().describe();
    EXPECT_EQ(*reopened->schemaVersion(), ProjectStore::kCurrentSchemaVersion);
    const auto contents = reopened->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();

    Model loaded;
    ASSERT_TRUE(applyToModel(*contents, loaded).ok());

    // Element ORDER is the thing a per-row table can get wrong, and reversing
    // it turns a dash-dot into a dot-dash while every length still matches.
    const auto* back = loaded.linetypes.find("dashdot");
    ASSERT_NE(back, nullptr);
    EXPECT_EQ(back->description, dashDot.description);
    ASSERT_EQ(back->pattern.size(), 4u);
    for (std::size_t i = 0; i < back->pattern.size(); ++i) {
        EXPECT_DOUBLE_EQ(back->pattern[i].length, dashDot.pattern[i].length) << "element " << i;
    }
    EXPECT_TRUE(back->pattern[2].isDot()) << "a zero-length dot must survive as a dot";

    EXPECT_EQ(*loaded.linetypes.find("dashed"), dashed);
    EXPECT_TRUE(loaded.linetypes.contains("continuous")) << "the built-in must still be there";
    EXPECT_EQ(loaded.layers.find("fence")->linetype, "dashed");
}

TEST_F(ProjectStoreMigration, AProjectFromBeforeLinetypesOpensWithItsLayerPatternsIntact)
{
    // Layer::linetype has always been a free string with no table behind it, so
    // an older project can name a pattern that has no definition. Opening it
    // must keep the name - it is what the layer says - and resolve to solid,
    // which is exactly how it already drew.
    const Model model = sampleModel();
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
    }
    {
        auto database = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(database.ok());
        ASSERT_TRUE(database->execute("UPDATE layers SET linetype = 'hidden'").ok());
        ASSERT_TRUE(rewindSchemaTo(*database, 3).ok());
    }

    auto reopened = ProjectStore::open(projectDir());
    ASSERT_TRUE(reopened.ok()) << reopened.error().describe();
    EXPECT_EQ(*reopened->schemaVersion(), ProjectStore::kCurrentSchemaVersion);

    const auto contents = reopened->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    Model loaded;
    ASSERT_TRUE(applyToModel(*contents, loaded).ok());

    EXPECT_EQ(loaded.layers.find("0")->linetype, "hidden")
        << "the name the layer carries must be kept, not rewritten to continuous";
    EXPECT_FALSE(loaded.linetypes.contains("hidden"))
        << "no empty definition may be invented, or it would shadow a real one later";
    EXPECT_TRUE(loaded.linetypes.contains("continuous"));
}

TEST_F(ProjectStoreRoundTrip, DimensionStylesAndTheLayersThatNameThemSurvive)
{
    katana::entity::DimensionStyle site;
    site.name = "site";
    site.textHeight = 1.8;
    site.textGap = 0.4;
    site.extensionOffset = 0.2;
    site.extensionBeyond = 0.9;
    site.arrowSize = 1.5;
    site.arrowHead = katana::entity::ArrowHead::Dot;
    site.unitScale = 1000.0;
    site.prefix = "~";
    site.suffix = " mm";
    site.decimals = 1;
    site.roundTo = 0.5;
    site.suppressTrailingZeros = true;

    Model model;
    ASSERT_TRUE(model.dimensionStyles.add(site).ok());

    Layer dims;
    dims.name = "dims";
    dims.dimensionStyle = "site";
    ASSERT_TRUE(model.layers.add(dims).ok());

    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
    }

    auto reopened = ProjectStore::open(projectDir());
    ASSERT_TRUE(reopened.ok()) << reopened.error().describe();
    EXPECT_EQ(*reopened->schemaVersion(), ProjectStore::kCurrentSchemaVersion);
    const auto contents = reopened->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();

    Model loaded;
    ASSERT_TRUE(applyToModel(*contents, loaded).ok());

    // The whole struct, so a field added later and never persisted fails here
    // rather than silently reverting to its default on every save.
    const auto* back = loaded.dimensionStyles.find("site");
    ASSERT_NE(back, nullptr);
    EXPECT_EQ(*back, site);

    EXPECT_EQ(loaded.layers.find("dims")->dimensionStyle, "site");
    EXPECT_TRUE(loaded.dimensionStyles.contains("Standard")) << "the built-in must still be there";
}

TEST_F(ProjectStoreMigration, AProjectFromBeforeDimensionStylesGetsTheDefaultAndKeepsWorking)
{
    const Model model = sampleModel();
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
    }
    {
        auto database = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(database.ok());
        ASSERT_TRUE(rewindSchemaTo(*database, 4).ok());
    }

    auto reopened = ProjectStore::open(projectDir());
    ASSERT_TRUE(reopened.ok()) << reopened.error().describe();
    EXPECT_EQ(*reopened->schemaVersion(), ProjectStore::kCurrentSchemaVersion);

    const auto contents = reopened->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    Model loaded;
    ASSERT_TRUE(applyToModel(*contents, loaded).ok());

    // Every layer names no style, which means the document default - the same
    // behaviour the project already had, rather than an invented style.
    for (const Layer& layer : loaded.layers.all()) {
        EXPECT_TRUE(layer.dimensionStyle.empty()) << layer.name;
    }
    EXPECT_TRUE(loaded.dimensionStyles.contains("Standard"));
    EXPECT_EQ(loaded.dimensionStyles.size(), 1u);
}

TEST_F(ProjectStoreMigration, AStylesDescriptionAndSymbolSurviveSavingTwiceAndAnOldProjectHasNone)
{
    Model model = sampleModel();
    katana::entity::Style marked;
    marked.name = "TOPO Natural Surface Point";
    marked.description = "colour: shade 48";
    marked.symbol = "cross";
    marked.symbolSize = 1.5;
    ASSERT_TRUE(model.styles.add(marked).ok());
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok()) << "the second save is the one that finds a missing DELETE";
    }
    {
        auto reopened = ProjectStore::open(projectDir());
        ASSERT_TRUE(reopened.ok()) << reopened.error().describe();
        const auto contents = reopened->load();
        ASSERT_TRUE(contents.ok());
        Model loaded;
        ASSERT_TRUE(applyToModel(*contents, loaded).ok());
        const katana::entity::Style* reloaded = loaded.styles.find(marked.name);
        ASSERT_NE(reloaded, nullptr);
        EXPECT_EQ(*reloaded, marked) << "a field was lost or reordered on the way through SQLite";
    }

    // A project from before schema 9: every style comes back with no
    // description, no symbol and the default mark size - as it was.
    {
        auto database = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(database.ok());
        ASSERT_TRUE(rewindSchemaTo(*database, 8).ok());
    }
    auto old = ProjectStore::open(projectDir());
    ASSERT_TRUE(old.ok()) << old.error().describe();
    EXPECT_EQ(*old->schemaVersion(), ProjectStore::kCurrentSchemaVersion);
    const auto contents = old->load();
    ASSERT_TRUE(contents.ok());
    Model loaded;
    ASSERT_TRUE(applyToModel(*contents, loaded).ok());
    const katana::entity::Style* plain = loaded.styles.find(marked.name);
    ASSERT_NE(plain, nullptr);
    EXPECT_TRUE(plain->description.empty());
    EXPECT_EQ(plain->symbol, katana::entity::kNoSymbol);
    EXPECT_EQ(plain->symbolSize, 0.0);
    EXPECT_EQ(plain->lineWeight, marked.lineWeight) << "the older columns are untouched";
}

TEST_F(ProjectStoreMigration, AProjectFromBeforeHatchPatternsOpensUnhatched)
{
    const Model model = sampleModel();
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
    }
    {
        auto database = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(database.ok());
        ASSERT_TRUE(rewindSchemaTo(*database, 5).ok());
    }

    auto reopened = ProjectStore::open(projectDir());
    ASSERT_TRUE(reopened.ok()) << reopened.error().describe();
    EXPECT_EQ(*reopened->schemaVersion(), ProjectStore::kCurrentSchemaVersion);

    const auto contents = reopened->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    Model loaded;
    ASSERT_TRUE(applyToModel(*contents, loaded).ok());

    // The point of the migration's default: a drawing made before hatching
    // existed must look exactly as it did, so every layer comes back naming
    // the built-in that draws no fill, and no other pattern is invented.
    for (const Layer& layer : loaded.layers.all()) {
        EXPECT_EQ(layer.hatchPattern, katana::entity::kNoHatch) << layer.name;
    }
    for (const katana::entity::Style& style : loaded.styles.all()) {
        EXPECT_TRUE(style.hatchPattern.empty()) << style.name; // ByLayer
    }
    EXPECT_TRUE(loaded.hatchPatterns.contains(katana::entity::kNoHatch));
    EXPECT_EQ(loaded.hatchPatterns.size(), 1u);
}

TEST_F(ProjectStoreRoundTrip, AHatchPatternSurvivesSaveAndReopenWithEveryFamily)
{
    Model model = sampleModel();

    katana::entity::HatchPattern brick;
    brick.name = "brick";
    brick.description = "Running bond";
    // Two families with different angles, spacings and offsets, so that a
    // writer which dropped a column, or a reader which mixed two of them up,
    // could not pass by coincidence.
    brick.families.push_back(katana::entity::HatchLineFamily{0.0, 0.25, 0.0});
    brick.families.push_back(katana::entity::HatchLineFamily{1.25, 0.5, 0.125});
    ASSERT_TRUE(model.hatchPatterns.add(brick));

    katana::entity::HatchPattern solid;
    solid.name = "concrete";
    solid.solid = true;
    ASSERT_TRUE(model.hatchPatterns.add(solid));

    Layer hatched;
    hatched.name = "paving";
    hatched.hatchPattern = "brick";
    ASSERT_TRUE(model.layers.add(hatched));

    katana::entity::Style style;
    style.name = "slab";
    style.hatchPattern = "concrete";
    ASSERT_TRUE(model.styles.add(style));

    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
        // Saved twice: save clears and rewrites every table it owns, so a
        // second save is what catches a missing DELETE and its primary-key
        // conflict. That is exactly how the linetype and dimension style
        // tables each failed when they were added.
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
    }

    auto reopened = ProjectStore::open(projectDir());
    ASSERT_TRUE(reopened.ok()) << reopened.error().describe();
    const auto contents = reopened->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    Model loaded;
    ASSERT_TRUE(applyToModel(*contents, loaded).ok());

    const katana::entity::HatchPattern* reloaded = loaded.hatchPatterns.find("brick");
    ASSERT_NE(reloaded, nullptr);
    EXPECT_EQ(*reloaded, brick) << "a field was lost or reordered on the way through SQLite";

    const katana::entity::HatchPattern* reloadedSolid = loaded.hatchPatterns.find("concrete");
    ASSERT_NE(reloadedSolid, nullptr);
    EXPECT_TRUE(reloadedSolid->solid);
    EXPECT_TRUE(reloadedSolid->families.empty());

    ASSERT_NE(loaded.layers.find("paving"), nullptr);
    EXPECT_EQ(loaded.layers.find("paving")->hatchPattern, "brick");
    ASSERT_NE(loaded.styles.find("slab"), nullptr);
    EXPECT_EQ(loaded.styles.find("slab")->hatchPattern, "concrete");
}

// ---- alignments (schema 7) ---------------------------------------------------------

namespace {

katana::entity::Alignment mainRoad()
{
    katana::entity::Alignment road;
    road.name = "road";
    road.description = "Main road";
    road.horizontal.startStation = 1000.0;
    const double d = std::acos(-1.0) / 6.0; // 30 degrees
    road.horizontal.pis = {
        katana::geometry::AlignmentPI{katana::geometry::Point2(0, 0)},
        katana::geometry::AlignmentPI{katana::geometry::Point2(400, 0), 300.0, 90.0, 90.0},
        katana::geometry::AlignmentPI{
            katana::geometry::Point2(400 + 300 * std::cos(d), 300 * std::sin(d))},
    };
    return road;
}

} // namespace

TEST_F(ProjectStoreMigration, AProjectFromBeforeAlignmentsOpensWithNone)
{
    const Model model = sampleModel();
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
    }
    {
        auto database = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(database.ok());
        ASSERT_TRUE(rewindSchemaTo(*database, 6).ok());
    }
    auto reopened = ProjectStore::open(projectDir());
    ASSERT_TRUE(reopened.ok()) << reopened.error().describe();
    EXPECT_EQ(*reopened->schemaVersion(), ProjectStore::kCurrentSchemaVersion);
    const auto contents = reopened->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    Model loaded;
    ASSERT_TRUE(applyToModel(*contents, loaded).ok());
    EXPECT_EQ(loaded.alignments.size(), 0u) << "nothing should be invented for an old file";
}

TEST_F(ProjectStoreRoundTrip, AnAlignmentSurvivesSaveAndReopenWithEveryPI)
{
    Model model = sampleModel();
    const katana::entity::Alignment road = mainRoad();
    ASSERT_TRUE(model.alignments.add(road));
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
        // Twice: save clears and rewrites every table it owns, and a missing
        // DELETE only shows as a primary-key conflict on the second save.
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
    }
    auto reopened = ProjectStore::open(projectDir());
    ASSERT_TRUE(reopened.ok()) << reopened.error().describe();
    const auto contents = reopened->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    Model loaded;
    ASSERT_TRUE(applyToModel(*contents, loaded).ok());

    const katana::entity::Alignment* reloaded = loaded.alignments.find("road");
    ASSERT_NE(reloaded, nullptr) << "dropped somewhere between capture and adoptContents";
    // Exact equality: SQLite REAL is a double, so every coordinate, radius,
    // spiral length and the start station must come back bit for bit.
    EXPECT_EQ(*reloaded, road);
}

TEST_F(ProjectStoreRoundTrip, AFileHoldingAnAlignmentThatCannotBeBuiltIsRefusedNamingThePI)
{
    // Nothing in the writer can produce this - add() refuses it - so it can
    // only arrive by a hand edit, another tool, or a future bug. Whichever it
    // is, opening the file must say so rather than draw the alignment wrong
    // (PLAN.MD section 36).
    Model model = sampleModel();
    ASSERT_TRUE(model.alignments.add(mainRoad()));
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
    }
    {
        auto database = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(database.ok());
        // 500 m spirals on R = 300 each use 0.833 rad; the corner has 0.524.
        ASSERT_TRUE(database
                        ->execute("UPDATE alignment_pis SET spiral_in = 500, spiral_out = 500"
                                  " WHERE alignment = 'road' AND position = 1")
                        .ok());
    }
    auto reopened = ProjectStore::open(projectDir());
    ASSERT_TRUE(reopened.ok()) << reopened.error().describe();
    const auto contents = reopened->load();
    ASSERT_TRUE(contents.ok()) << "reading rows is fine; building them is what must fail";
    Model loaded;
    const auto applied = applyToModel(*contents, loaded);
    ASSERT_FALSE(applied.ok());
    EXPECT_EQ(applied.error().code, ErrorCode::InvalidGeometry);
    EXPECT_NE(applied.error().describe().find("PI 1"), std::string::npos)
        << applied.error().describe();
    EXPECT_EQ(loaded.alignments.size(), 0u) << "a refused load must leave the model untouched";
}

// ---- design profiles (schema 8) ----------------------------------------------------

namespace {

katana::entity::Alignment mainRoadWithProfile()
{
    katana::entity::Alignment road = mainRoad();
    // Stations lie within the road's 1000 to ~1247 chainage: a crest of
    // 60 m at 1120.
    road.vertical.emplace();
    road.vertical->pvis = {katana::geometry::ProfilePVI{1000.0, 50.0},
                           katana::geometry::ProfilePVI{1120.0, 53.0, 60.0},
                           katana::geometry::ProfilePVI{1240.0, 51.0}};
    return road;
}

} // namespace

TEST_F(ProjectStoreRoundTrip, AProfileOnAnAlignmentSurvivesSaveAndReopen)
{
    Model model = sampleModel();
    const katana::entity::Alignment road = mainRoadWithProfile();
    ASSERT_TRUE(model.alignments.add(road));
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok()); // the second-save check
    }
    auto reopened = ProjectStore::open(projectDir());
    ASSERT_TRUE(reopened.ok()) << reopened.error().describe();
    const auto contents = reopened->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    Model loaded;
    ASSERT_TRUE(applyToModel(*contents, loaded).ok());
    const katana::entity::Alignment* reloaded = loaded.alignments.find("road");
    ASSERT_NE(reloaded, nullptr);
    ASSERT_TRUE(reloaded->vertical.has_value());
    EXPECT_EQ(*reloaded, road);
}

TEST_F(ProjectStoreRoundTrip, AnAlignmentWithoutAProfileComesBackWithoutOne)
{
    // The reader creates the profile on its first row. An alignment with no
    // rows must NOT come back with an empty profile, which would then fail
    // to solve and refuse the whole load.
    Model model = sampleModel();
    ASSERT_TRUE(model.alignments.add(mainRoad()));
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
    }
    auto reopened = ProjectStore::open(projectDir());
    ASSERT_TRUE(reopened.ok());
    const auto contents = reopened->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    Model loaded;
    ASSERT_TRUE(applyToModel(*contents, loaded).ok());
    ASSERT_NE(loaded.alignments.find("road"), nullptr);
    EXPECT_FALSE(loaded.alignments.find("road")->vertical.has_value());
}

TEST_F(ProjectStoreMigration, AProjectFromBeforeProfilesKeepsItsAlignmentsUnprofiled)
{
    Model model = sampleModel();
    ASSERT_TRUE(model.alignments.add(mainRoadWithProfile()));
    {
        auto store = ProjectStore::create(projectDir(), {});
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, {})).ok());
    }
    {
        auto database = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(database.ok());
        ASSERT_TRUE(rewindSchemaTo(*database, 7).ok()); // drops the profile table
    }
    auto reopened = ProjectStore::open(projectDir());
    ASSERT_TRUE(reopened.ok()) << reopened.error().describe();
    EXPECT_EQ(*reopened->schemaVersion(), ProjectStore::kCurrentSchemaVersion);
    const auto contents = reopened->load();
    ASSERT_TRUE(contents.ok()) << contents.error().describe();
    Model loaded;
    ASSERT_TRUE(applyToModel(*contents, loaded).ok());
    const katana::entity::Alignment* road = loaded.alignments.find("road");
    ASSERT_NE(road, nullptr) << "the alignment itself predates the migration and must survive";
    EXPECT_FALSE(road->vertical.has_value());
}

// ---- the consuming overload is the same function ---------------------------------------------

// applyToModel has two overloads: one that copies the contents and one that
// consumes them, which is what Document::open uses so that opening a drawing
// does not duplicate every entity on its way into the model. They must be the
// SAME function, so this builds a project with something in EVERY table
// applyToModel fills - including the four whose built-in names take the
// update() path that cannot move - applies one copy of it each way, and
// compares the two models value for value.
//
// A weaker test would compare only the entity ids: this compares whole values,
// because the failure a move introduces is a field left behind, not a record.
TEST_F(ProjectStoreRoundTrip, ConsumingTheContentsBuildsExactlyTheModelCopyingThemDoes)
{
    Model rich = sampleModel();

    katana::entity::Linetype dashed;
    dashed.name = "dashed";
    dashed.description = "Dashed  __  __  __";
    dashed.pattern = {katana::entity::LinetypeElement{0.5}, katana::entity::LinetypeElement{-0.25},
                      katana::entity::LinetypeElement{0.0}, katana::entity::LinetypeElement{-0.25}};
    ASSERT_TRUE(rich.linetypes.add(dashed).ok());

    katana::entity::DimensionStyle metric;
    metric.name = "metric";
    metric.suffix = " m";
    metric.decimals = 2;
    ASSERT_TRUE(rich.dimensionStyles.add(metric).ok());

    katana::entity::HatchPattern brick;
    brick.name = "brick";
    brick.description = "Running bond";
    brick.families.push_back(katana::entity::HatchLineFamily{0.0, 0.25, 0.0});
    brick.families.push_back(katana::entity::HatchLineFamily{1.25, 0.5, 0.125});
    ASSERT_TRUE(rich.hatchPatterns.add(brick).ok());

    ASSERT_TRUE(rich.alignments.add(mainRoadWithProfile()).ok());

    // captureModel carries the built-in names too - layer "0", "continuous",
    // "none" and the default dimension style - so the update() path, the one
    // that cannot move, is exercised as well.
    ProjectContents contents = captureModel(rich, {});
    ASSERT_FALSE(contents.entities.empty());
    ASSERT_FALSE(contents.alignments.empty());
    ASSERT_FALSE(contents.propertyDefinitions.empty());
    const ProjectContents duplicate = contents; // the identical input, kept whole

    Model copied;
    ASSERT_TRUE(applyToModel(duplicate, copied).ok());
    Model moved;
    ASSERT_TRUE(applyToModel(std::move(contents), moved).ok());

    // Every table applyToModel fills. Each of these types has a defaulted
    // operator==, so this compares all of their fields, not their names.
    EXPECT_EQ(copied.layers.all(), moved.layers.all());
    EXPECT_EQ(copied.styles.all(), moved.styles.all());
    EXPECT_EQ(copied.linetypes.all(), moved.linetypes.all());
    EXPECT_EQ(copied.dimensionStyles.all(), moved.dimensionStyles.all());
    EXPECT_EQ(copied.hatchPatterns.all(), moved.hatchPatterns.all());
    EXPECT_EQ(copied.alignments.all(), moved.alignments.all());
    EXPECT_EQ(copied.properties.all(), moved.properties.all());

    const auto ids = copied.entities.ids();
    ASSERT_EQ(ids, moved.entities.ids());
    ASSERT_FALSE(ids.empty());
    for (const auto id : ids) {
        const Entity* left = copied.entities.find(id);
        const Entity* right = moved.entities.find(id);
        ASSERT_NE(left, nullptr) << "entity " << id;
        ASSERT_NE(right, nullptr) << "entity " << id;
        EXPECT_EQ(*left, *right) << "entity " << id << " differs between the two overloads";
    }
    // The retired id in sampleModel() makes this more than "count + 1".
    EXPECT_EQ(copied.entities.nextId(), moved.entities.nextId());
}

// The consuming overload leaves the caller's model untouched when it fails,
// exactly as the copying one does - Document::open relies on that and only then
// installs the metadata it took out beforehand. What it does NOT promise is the
// state of the contents, which are moved-from by then.
TEST_F(ProjectStoreRoundTrip, AFailedConsumingLoadAlsoLeavesTheCallersModelUntouched)
{
    Model existing = sampleModel();
    const auto idsBefore = existing.entities.ids();
    const auto layersBefore = existing.layers.all();
    ASSERT_FALSE(idsBefore.empty());

    ProjectContents contents;
    contents.layers.push_back(Layer{std::string(katana::entity::kDefaultLayerName)});
    contents.styles.push_back(Style{"Duplicate"});
    contents.styles.push_back(Style{"Duplicate"});
    contents.nextEntityId = 1;

    const auto status = applyToModel(std::move(contents), existing);
    ASSERT_FALSE(status.ok()) << "a duplicate style should be refused";

    EXPECT_EQ(existing.entities.ids(), idsBefore);
    EXPECT_EQ(existing.layers.all(), layersBefore);
}

// ---- metadata ----------------------------------------------------------------------

namespace {

// The metadata rows as the file holds them, read past the store.
std::map<std::string, std::string> metadataRows(const fs::path& database)
{
    std::map<std::string, std::string> rows;
    auto db = SqliteDatabase::open(database);
    EXPECT_TRUE(db.ok());
    if (!db) {
        return rows;
    }
    auto select = db->prepare("SELECT key, value FROM metadata");
    EXPECT_TRUE(select.ok());
    while (select) {
        const auto more = select->step();
        EXPECT_TRUE(more.ok());
        if (!more || !*more) {
            break;
        }
        rows.emplace(select->columnText(0), select->columnText(1));
    }
    return rows;
}

} // namespace

TEST_F(ProjectStoreRoundTrip, AMetadataKeyANewerBuildWroteSurvivesOpenSaveSave)
{
    const Model model = sampleModel();
    ProjectMetadata metadata;
    metadata.name = "Forward";
    {
        auto store = ProjectStore::create(projectDir(), metadata);
        ASSERT_TRUE(store.ok());
        ASSERT_TRUE(store->save(captureModel(model, metadata)).ok());
    }
    // What a newer Katana would have written beside the keys this one knows.
    {
        auto db = SqliteDatabase::open(projectDir() / "project.db");
        ASSERT_TRUE(db.ok());
        ASSERT_TRUE(db->execute("INSERT INTO metadata VALUES ('symbol_rotation_mode', 'north')").ok());
    }

    auto store = ProjectStore::open(projectDir());
    ASSERT_TRUE(store.ok());
    auto opened = store->load();
    ASSERT_TRUE(opened.ok());
    EXPECT_EQ(opened->metadata.unknownKeys,
              (std::map<std::string, std::string>{{"symbol_rotation_mode", "north"}}))
        << "read, and kept to be written back";
    // Saved twice, as a session does: the second save is the one a table
    // that is not emptied first would fail on.
    ASSERT_TRUE(store->save(captureModel(model, opened->metadata)).ok());
    ASSERT_TRUE(store->save(captureModel(model, opened->metadata)).ok());
    // And saved by a caller that never looked at the metadata it loaded: the
    // key is still not this build's to delete.
    ASSERT_TRUE(store->save(captureModel(model, metadata)).ok());

    const auto again = store->load();
    ASSERT_TRUE(again.ok());
    EXPECT_EQ(again->metadata.unknownKeys,
              (std::map<std::string, std::string>{{"symbol_rotation_mode", "north"}}));
    EXPECT_EQ(again->metadata.name, "Forward");
    // 9 keys this build writes + the 1 it does not know = 10 rows, each once.
    EXPECT_EQ(metadataRows(projectDir() / "project.db").size(), 10u);
}

TEST_F(ProjectStoreRoundTrip, TheCustomisationAProjectWasDrawnWithRoundTrips)
{
    const Model model = sampleModel();
    ProjectMetadata metadata;
    // A name may hold anything a file name can - a semicolon, spaces,
    // non-ASCII - and the order is the load order, not name order.
    metadata.customisation = {"user_symbols_ÿ.4d", "linestyles; v2.4d", "survey codes.mapfile"};
    auto store = ProjectStore::create(projectDir(), metadata);
    ASSERT_TRUE(store.ok());
    ASSERT_TRUE(store->save(captureModel(model, metadata)).ok());
    ASSERT_TRUE(store->save(captureModel(model, metadata)).ok());
    const auto contents = store->load();
    ASSERT_TRUE(contents.ok());
    EXPECT_EQ(contents->metadata.customisation, metadata.customisation);
    EXPECT_TRUE(contents->metadata.unknownKeys.empty()) << "customisation is a key this build knows";

    metadata.customisation.clear();
    ASSERT_TRUE(store->save(captureModel(model, metadata)).ok());
    EXPECT_TRUE(store->load()->metadata.customisation.empty()) << "an empty list, not one empty name";
}

TEST_F(ProjectStoreRoundTrip, MetadataThatWouldNotReadBackAsItWasIsRefused)
{
    auto store = ProjectStore::create(projectDir(), {});
    ASSERT_TRUE(store.ok());
    const Model model = sampleModel();
    const auto refused = [&](const ProjectMetadata& metadata) {
        const auto status = store->save(captureModel(model, metadata));
        return !status.ok() && status.error().code == ErrorCode::InvalidArgument;
    };
    for (const char* name : {"", "C:/Customisation/user.4d", "folder\\user.4d", "two\nnames.4d"}) {
        ProjectMetadata metadata;
        metadata.customisation = {name};
        EXPECT_TRUE(refused(metadata)) << "\"" << name << "\"";
    }
    ProjectMetadata shadowing;
    shadowing.unknownKeys = {{"name", "Not the name"}};
    EXPECT_TRUE(refused(shadowing)) << "an unknown key may not overwrite a field";
}
