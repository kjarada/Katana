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
#include <filesystem>
#include <memory>
#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/entity/model.hpp"

namespace katana::storage {

struct ProjectMetadata {
    std::string name = "Untitled";
    std::string description{};
    std::string linearUnit = "metre";
    std::string coordinateSystem{}; // e.g. "EPSG:32630"; empty for local coordinates
    std::string createdUtc{};       // ISO 8601; filled in by create()
    std::string modifiedUtc{};      // ISO 8601; refreshed by save()
    std::string applicationVersion{};

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
    std::vector<katana::entity::PropertyDefinition> propertyDefinitions{};
    std::vector<katana::entity::Entity> entities{};
    std::vector<Relationship> relationships{};
    katana::entity::EntityId nextEntityId = 1;
};

[[nodiscard]] ProjectContents captureModel(const katana::entity::Model& model,
                                           ProjectMetadata metadata,
                                           std::vector<Relationship> relationships = {});

// Replaces the content of `model`. The contents are checked first, so a failure
// leaves the model untouched.
[[nodiscard]] katana::core::Status applyToModel(const ProjectContents& contents,
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
    // migration table in project_store.cpp and docs/storage.md for why.
    static constexpr int kCurrentSchemaVersion = 3;
    static constexpr std::size_t kDefaultBackupsToKeep = 10;

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
