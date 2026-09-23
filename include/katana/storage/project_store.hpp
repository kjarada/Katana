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
#include <map>
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
    // The 12d customisation the drawing was drawn with: the NAMES of the
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
    static constexpr int kCurrentSchemaVersion = 9;
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
