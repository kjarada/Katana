#pragma once

// Project persistence (PLAN.MD Phase 07).
//
// A project is a directory:
//
//   <name>.katana/
//   ├── project.db     structured data (SQLite): entities, layers, styles,
//   │                  property definitions, relationships, metadata
//   ├── backups/       timestamped snapshots of project.db
//   ├── terrain/       large datasets live in files, not in SQLite rows
//   ├── pointcloud/
//   ├── assets/
//   └── cache/         disposable; safe to delete
//
// Guarantees
//   Atomic save    save() is one SQLite transaction. A crash or power loss mid
//                  save leaves the previous state intact (rollback journal).
//   Exact values   doubles round-trip bit for bit.
//   Versioning     PRAGMA user_version holds the schema version. Older projects
//                  are migrated step by step on open(), after an automatic
//                  backup. Projects from a newer Katana are refused, not guessed at.
//   Recovery       open() runs an integrity check; recover() restores the newest
//                  sound backup and keeps the damaged file for forensics.
//
// save() rewrites the whole model. That is simple and correct; incremental saves
// driven by command change events are a later optimisation (measure first).
//
// Threading: a ProjectStore belongs to one thread.

#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <map>
#include <memory>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/model.hpp"
#include "katana/storage/survey_job.hpp"

namespace katana::storage {

struct ProjectMetadata {
    std::string name = "Untitled";
    std::string description{};
    std::string linearUnit = "metre";
    std::string coordinateSystem{}; // e.g. "EPSG:32630"; empty for local coordinates
    std::string createdUtc{};       // ISO 8601; filled in by create()
    std::string modifiedUtc{};      // ISO 8601; refreshed by save()
    std::string applicationVersion{};
    // The customisation the drawing was drawn with: the NAMES of the
    // linestyle, symbol and map files that were loaded, in load order - never
    // their paths, because a project travels between machines and a path
    // says whose disk it was made on. A record, not a reference: nothing is
    // loaded from it, and a name here need not exist where the project is
    // opened. Stored as one `customisation` key with the names separated by
    // line feeds, which is why save() refuses a name that is empty or holds
    // a line break or a path separator.
    std::vector<std::string> customisation{};
    // Keys a NEWER Katana wrote that this one does not read, kept and written
    // back as they were, so opening and saving a project in an older build
    // does not strip what a newer one recorded. Never a key this build reads:
    // save() refuses one, rather than let it overwrite the field.
    std::map<std::string, std::string> unknownKeys{};

    friend bool operator==(const ProjectMetadata&, const ProjectMetadata&) = default;
};

// Directed, typed link between two entities (a dimension and what it measures,
// a label and its subject...). Rows disappear with either entity.
struct Relationship {
    katana::entity::EntityId from = katana::entity::kInvalidEntityId;
    katana::entity::EntityId to = katana::entity::kInvalidEntityId;
    std::string kind{};
    std::string data{};

    friend bool operator==(const Relationship&, const Relationship&) = default;
};

// Everything persisted for a project, as plain values.
struct ProjectContents {
    ProjectMetadata metadata{};
    std::vector<katana::entity::Layer> layers{};
    std::vector<katana::entity::Style> styles{};
    std::vector<katana::entity::Linetype> linetypes{};
    std::vector<katana::entity::DimensionStyle> dimensionStyles{};
    std::vector<katana::entity::HatchPattern> hatchPatterns{};
    std::vector<katana::entity::Alignment> alignments{};
    std::vector<katana::entity::PropertyDefinition> propertyDefinitions{};
    std::vector<katana::entity::Entity> entities{};
    std::vector<Relationship> relationships{};
    katana::entity::EntityId nextEntityId = 1;
    // The survey jobs (survey_job.hpp), in the order they were created. Saved
    // and loaded in the same transaction as the entities they created, so a
    // job can never name entities a crash left unsaved. Schema version 10.
    // save() refuses a job without an id, two jobs of one id, and a job too
    // large for the database (ProjectStore::checkSurveyJobSize); a project
    // from before schema 10 loads with none.
    std::vector<SurveyJob> surveyJobs{};
};

[[nodiscard]] ProjectContents captureModel(const katana::entity::Model& model,
                                           ProjectMetadata metadata,
                                           std::vector<Relationship> relationships = {});

// Replaces the content of `model`. The contents are checked first, so a failure
// leaves the model untouched.
[[nodiscard]] katana::core::Status applyToModel(const ProjectContents& contents,
                                                katana::entity::Model& model);

// The same, CONSUMING the contents. Opening a project loads a ProjectContents,
// applies it and throws it away, and the copy in between was 22.6% of every
// heap allocation a 50,000-entity open made - an Entity carries two std::maps
// and a geometry variant holding vectors, and each one was duplicated into the
// model and destroyed moments later.
//
// `contents` is moved-from on return, on success AND on failure: the vectors
// keep their size, but every element in them has been emptied into the model,
// so the contents must not be read or applied a second time. `model` is still
// untouched when this fails, so a failed open leaves the caller's drawing as it
// was - but take anything you still need out of the contents (the metadata)
// BEFORE calling this.
[[nodiscard]] katana::core::Status applyToModel(ProjectContents&& contents,
                                                katana::entity::Model& model);

struct RecoveryReport {
    bool restored = false;                     // false: project.db was already sound
    std::filesystem::path restoredFrom{};      // backup that was used
    std::filesystem::path damagedFileKeptAs{}; // where the corrupt database went
    std::string problems{};                    // what the integrity check reported
};

class ProjectStore {
  public:
    // 3: geometry stored as a binary blob rather than JSON text. See the
    //    migration table in project_store.cpp and docs/storage.md for why.
    // 4: linetype definitions (Phase 09).
    // 5: dimension styles, and Layer::dimensionStyle (Phase 09).
    // 6: hatch patterns, and Layer/Style::hatchPattern (Phase 09).
    // 7: alignments, stored as their PI definitions (Phase 21).
    // 8: design profiles on alignments, as PVIs (Phase 21).
    // 9: a style's description and point symbol.
    // 10: survey jobs - the raw bytes of each imported field file and its
    //     siblings, its reduction settings, report and placed points.
    static constexpr int kCurrentSchemaVersion = 10;
    static constexpr std::size_t kDefaultBackupsToKeep = 10;

    // The most bytes one survey job may take in a project: its own row (the
    // field file, its names, settings, report and packed point lists) and,
    // separately, each file kept with it. SQLite refuses a string, a BLOB or
    // a row over its SQLITE_MAX_LENGTH - 1,000,000,000 bytes by default, and
    // as MSYS2 builds it - while a reader accepts a field file of up to 1 GiB
    // (surveyio::kMaxSurveyFileBytes). Without this a job could be imported
    // that no save could write, and every later save of the whole project
    // would fail. A round figure, well under the library's limit; save() also
    // honours the connection's own limit, should a build of SQLite have a
    // lower one.
    static constexpr std::uint64_t kMaxSurveyJobBytes = 900'000'000;

    // The bytes `job` takes in its own row: every column's content, without
    // the few bytes of SQLite's record header.
    [[nodiscard]] static std::uint64_t surveyJobRowBytes(const SurveyJob& job);

    // InvalidArgument, with a sentence naming the job's field file and saying
    // what to do instead, when the job's row or one of its files' rows would
    // be over `limit` bytes. The survey job commands call it before they
    // change anything; save() calls it for every job, so a job too large is
    // refused before a byte is written, not by SQLite halfway through.
    [[nodiscard]] static katana::core::Status
    checkSurveyJobSize(const SurveyJob& job, std::uint64_t limit = kMaxSurveyJobBytes);

    // Creates the directory layout and an empty database. Fails if a project
    // already exists there.
    [[nodiscard]] static katana::core::Result<ProjectStore>
    create(const std::filesystem::path& projectDirectory, const ProjectMetadata& metadata);

    // Opens an existing project: verifies it is a Katana database, checks its
    // integrity and migrates older schemas (backing up first).
    [[nodiscard]] static katana::core::Result<ProjectStore>
    open(const std::filesystem::path& projectDirectory);

    // Restores the newest backup that passes an integrity check when project.db
    // is missing, unreadable or corrupt. Never deletes data.
    [[nodiscard]] static katana::core::Result<RecoveryReport>
    recover(const std::filesystem::path& projectDirectory);

    [[nodiscard]] static bool isProjectDirectory(const std::filesystem::path& directory);

    ProjectStore(ProjectStore&&) noexcept;
    ProjectStore& operator=(ProjectStore&&) noexcept;
    ~ProjectStore();

    [[nodiscard]] katana::core::Status save(const ProjectContents& contents);
    [[nodiscard]] katana::core::Result<ProjectContents> load();

    // Snapshot into backups/, pruned to the newest `keep` files. Returns the path.
    [[nodiscard]] katana::core::Result<std::filesystem::path>
    backup(std::size_t keep = kDefaultBackupsToKeep);
    [[nodiscard]] std::vector<std::filesystem::path> listBackups() const; // newest first

    [[nodiscard]] katana::core::Result<int> schemaVersion();
    [[nodiscard]] const std::filesystem::path& directory() const;
    [[nodiscard]] std::filesystem::path databasePath() const;

  private:
    struct Impl;
    explicit ProjectStore(std::unique_ptr<Impl> impl);
    std::unique_ptr<Impl> impl_;
};

} // namespace katana::storage
