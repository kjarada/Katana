#include "katana/storage/project_store.hpp"

#include <algorithm>
#include <cctype>
#include <chrono>
#include <cstdio>
#include <iterator>
#include <ctime>
#include <set>
#include <system_error>
#include <type_traits>
#include <utility>

#include "katana/entity/entity_geometry.hpp"
#include "katana/entity/geometry_blob.hpp"
#include "katana/entity/serialization.hpp"
#include "katana/storage/sqlite_database.hpp"

namespace katana::storage {

namespace fs = std::filesystem;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::entity::Entity;
using katana::entity::EntityId;
using katana::entity::Layer;
using katana::entity::PropertyDefinition;
using katana::entity::PropertyType;
using katana::entity::Style;

namespace {

// "KTNA" — marks a SQLite file as a Katana project (PRAGMA application_id).
constexpr std::int32_t kApplicationId = 0x4B544E41;
constexpr const char* kDatabaseFile = "project.db";
constexpr const char* kBackupDirectory = "backups";
constexpr const char* kDataDirectories[] = {"terrain", "pointcloud", "assets", "cache"};

struct Migration {
    int toVersion;
    const char* sql;
};

// Append only. Never edit a released migration: projects in the field have
// already run it. Schema changes are new entries.
constexpr Migration kMigrations[] = {
    {1, R"sql(
        CREATE TABLE metadata (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        ) WITHOUT ROWID;

        CREATE TABLE layers (
            name        TEXT PRIMARY KEY,
            color       TEXT NOT NULL,
            visible     INTEGER NOT NULL,
            locked      INTEGER NOT NULL,
            linetype    TEXT NOT NULL,
            line_weight REAL NOT NULL
        ) WITHOUT ROWID;

        CREATE TABLE styles (
            name        TEXT PRIMARY KEY,
            color       TEXT,
            line_weight REAL NOT NULL,
            linetype    TEXT NOT NULL
        ) WITHOUT ROWID;

        CREATE TABLE property_definitions (
            name          TEXT PRIMARY KEY,
            type          TEXT NOT NULL,
            description   TEXT NOT NULL,
            default_value TEXT
        ) WITHOUT ROWID;

        CREATE TABLE entities (
            id         INTEGER PRIMARY KEY,
            type       TEXT NOT NULL,
            layer      TEXT NOT NULL REFERENCES layers(name),
            style      TEXT NOT NULL,
            color      TEXT,
            visible    INTEGER NOT NULL,
            geometry   TEXT NOT NULL,
            properties TEXT NOT NULL,
            metadata   TEXT NOT NULL
        );
        CREATE INDEX entities_by_layer ON entities(layer);
        CREATE INDEX entities_by_type ON entities(type);
    )sql"},
    {2, R"sql(
        CREATE TABLE relationships (
            id          INTEGER PRIMARY KEY,
            from_entity INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
            to_entity   INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
            kind        TEXT NOT NULL,
            data        TEXT NOT NULL DEFAULT ''
        );
        CREATE INDEX relationships_by_from ON relationships(from_entity);
        CREATE INDEX relationships_by_to ON relationships(to_entity);
    )sql"},
    // Geometry as a binary blob instead of JSON text. Parsing the JSON was 602
    // of the 891 ms it took to open a 50 000-entity project - 68%, against 289
    // ms for SQLite itself - which is why the encoding changed and the database
    // did not (docs/storage.md).
    //
    // The column is ADDED rather than replacing `geometry`, and is NULL for
    // every row written before this migration. A load reads the blob when there
    // is one and falls back to the JSON when there is not, so an existing
    // project opens untouched and is converted the next time it is saved. The
    // old column stays until a migration that rewrites every row can be
    // justified; dropping it now would mean rewriting the whole table on open.
    {3, R"sql(
        ALTER TABLE entities ADD COLUMN geometry_blob BLOB;
    )sql"},
    // Linetype definitions. The elements go in their own table rather than an
    // encoded string in one column, because the order matters and a delimited
    // list of signed doubles is a parser waiting to be written twice.
    //
    // Layer::linetype and Style::linetype have always been free strings with no
    // table behind them, so an existing project may name a pattern that does
    // not exist. That is NOT repaired here: resolution treats an unknown
    // linetype as continuous, which is how those projects already drew, and
    // inventing an empty definition for every name found would shadow a real
    // built-in of the same name for ever after.
    {4, R"sql(
        CREATE TABLE linetypes (
            name        TEXT PRIMARY KEY,
            description TEXT NOT NULL
        ) WITHOUT ROWID;

        CREATE TABLE linetype_elements (
            linetype TEXT NOT NULL REFERENCES linetypes(name) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            length   REAL NOT NULL,
            PRIMARY KEY (linetype, position)
        ) WITHOUT ROWID;
    )sql"},
    // Dimension styles, and the layer column that names one. A layer with an
    // empty dimension_style uses the document default, which is how every
    // project written before this migration behaves - so nothing is invented
    // for existing rows.
    {5, R"sql(
        CREATE TABLE dimension_styles (
            name              TEXT PRIMARY KEY,
            text_height       REAL NOT NULL,
            text_gap          REAL NOT NULL,
            extension_offset  REAL NOT NULL,
            extension_beyond  REAL NOT NULL,
            arrow_size        REAL NOT NULL,
            arrow_head        TEXT NOT NULL,
            unit_scale        REAL NOT NULL,
            prefix            TEXT NOT NULL,
            suffix            TEXT NOT NULL,
            decimals          INTEGER NOT NULL,
            round_to          REAL NOT NULL,
            suppress_zeros    INTEGER NOT NULL
        ) WITHOUT ROWID;

        ALTER TABLE layers ADD COLUMN dimension_style TEXT NOT NULL DEFAULT '';
    )sql"},
    // Hatch patterns, and the layer and style columns that name one. The layer
    // column defaults to the built-in that draws no fill and the style column
    // to empty, which means ByLayer - so every project written before this
    // migration keeps drawing exactly as it did, and nothing acquires a hatch
    // it never asked for.
    {6, R"sql(
        CREATE TABLE hatch_patterns (
            name        TEXT PRIMARY KEY,
            description TEXT NOT NULL,
            solid       INTEGER NOT NULL
        ) WITHOUT ROWID;

        CREATE TABLE hatch_families (
            pattern  TEXT NOT NULL REFERENCES hatch_patterns(name) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            angle    REAL NOT NULL,
            spacing  REAL NOT NULL,
            line_offset REAL NOT NULL,
            PRIMARY KEY (pattern, position)
        ) WITHOUT ROWID;

        ALTER TABLE layers ADD COLUMN hatch_pattern TEXT NOT NULL DEFAULT 'none';
        ALTER TABLE styles ADD COLUMN hatch_pattern TEXT NOT NULL DEFAULT '';
    )sql"},
    // Alignments, stored as the PI definition and nothing else. The elements
    // are derived on load by geometry::solveAlignment, so a file can never
    // hold an alignment with a gap in it, and a better solver later applies
    // to every existing file for free.
    {7, R"sql(
        CREATE TABLE alignments (
            name          TEXT PRIMARY KEY,
            description   TEXT NOT NULL,
            start_station REAL NOT NULL
        ) WITHOUT ROWID;

        CREATE TABLE alignment_pis (
            alignment  TEXT NOT NULL REFERENCES alignments(name) ON DELETE CASCADE,
            position   INTEGER NOT NULL,
            x          REAL NOT NULL,
            y          REAL NOT NULL,
            radius     REAL NOT NULL,
            spiral_in  REAL NOT NULL,
            spiral_out REAL NOT NULL,
            PRIMARY KEY (alignment, position)
        ) WITHOUT ROWID;
    )sql"},
    // The design profile on an alignment, as its PVIs. An alignment with no
    // rows here has no profile - which is what every alignment written before
    // this migration has, so nothing is invented.
    {8, R"sql(
        CREATE TABLE alignment_pvis (
            alignment    TEXT NOT NULL REFERENCES alignments(name) ON DELETE CASCADE,
            position     INTEGER NOT NULL,
            station      REAL NOT NULL,
            elevation    REAL NOT NULL,
            curve_length REAL NOT NULL,
            PRIMARY KEY (alignment, position)
        ) WITHOUT ROWID;
    )sql"},
    // A style's description and, for points, its symbol (PLAN.MD 20.2). The
    // defaults are exactly what every style written before this had: no
    // description, no symbol, the viewport's own mark size - so an old
    // drawing draws as it did.
    {9, R"sql(
        ALTER TABLE styles ADD COLUMN description TEXT NOT NULL DEFAULT '';
        ALTER TABLE styles ADD COLUMN symbol TEXT NOT NULL DEFAULT '';
        ALTER TABLE styles ADD COLUMN symbol_size REAL NOT NULL DEFAULT 0;
    )sql"},
};

std::string toUtf8(const fs::path& path)
{
    const auto utf8 = path.u8string();
    return std::string(utf8.begin(), utf8.end());
}

// `compact` gives a file-name-safe form: 20260919T101530-123.
std::string utcTimestamp(bool compact)
{
    using namespace std::chrono;
    const auto now = system_clock::now();
    const auto wholeSeconds = time_point_cast<seconds>(now);
    const auto millis = duration_cast<milliseconds>(now - wholeSeconds).count();
    const std::time_t time = system_clock::to_time_t(wholeSeconds);
    std::tm utc{};
#if defined(_WIN32)
    gmtime_s(&utc, &time);
#else
    gmtime_r(&time, &utc);
#endif
    char buffer[48];
    std::snprintf(buffer, sizeof(buffer),
                  compact ? "%04d%02d%02dT%02d%02d%02d-%03d" : "%04d-%02d-%02dT%02d:%02d:%02d.%03dZ",
                  utc.tm_year + 1900, utc.tm_mon + 1, utc.tm_mday, utc.tm_hour, utc.tm_min,
                  utc.tm_sec, static_cast<int>(millis));
    return buffer;
}

std::string_view toString(PropertyType type)
{
    switch (type) {
    case PropertyType::Boolean:
        return "Boolean";
    case PropertyType::Integer:
        return "Integer";
    case PropertyType::Real:
        return "Real";
    case PropertyType::Text:
        return "Text";
    }
    return "Text";
}

Result<PropertyType> propertyTypeFromString(const std::string& text)
{
    for (const PropertyType type : {PropertyType::Boolean, PropertyType::Integer,
                                    PropertyType::Real, PropertyType::Text}) {
        if (toString(type) == text) {
            return type;
        }
    }
    return makeError(ErrorCode::DatabaseFailure, "unknown property type in project", text);
}

// Shared by save() and applyToModel(): a project that fails this is never
// written and never loaded into a model.
// Every metadata key this build reads and writes. Any other key in a
// project was written by a newer build, and is kept (unknownKeys).
constexpr std::string_view kMetadataKeys[] = {
    "name",        "description",  "linear_unit",         "coordinate_system", "created_utc",
    "modified_utc", "application_version", "next_entity_id", "customisation",
};

[[nodiscard]] bool isMetadataKey(std::string_view key)
{
    return std::find(std::begin(kMetadataKeys), std::end(kMetadataKeys), key) !=
           std::end(kMetadataKeys);
}

// The customisation names are stored one per line (see ProjectMetadata), so
// a name that would split or vanish on the way back is refused here rather
// than read back as a different list.
Status validateMetadata(const ProjectMetadata& metadata)
{
    for (const std::string& name : metadata.customisation) {
        if (name.empty() || name.find_first_of("\n\r/\\") != std::string::npos) {
            return makeError(ErrorCode::InvalidArgument,
                             "a customisation entry must be a file name: not empty, and with no "
                             "line break or path separator",
                             "customisation=\"" + name + "\"");
        }
    }
    for (const auto& [key, value] : metadata.unknownKeys) {
        if (isMetadataKey(key)) {
            return makeError(ErrorCode::InvalidArgument,
                             "an unknown metadata key names a field this build writes itself",
                             "key=" + key);
        }
    }
    return {};
}

Status validateContents(const ProjectContents& contents)
{
    if (auto status = validateMetadata(contents.metadata); !status) {
        return status;
    }
    std::set<std::string> layerNames{std::string(katana::entity::kDefaultLayerName)};
    for (const Layer& layer : contents.layers) {
        if (layer.name.empty()) {
            return makeError(ErrorCode::InvalidArgument, "project contains a layer without a name");
        }
        layerNames.insert(layer.name);
    }
    std::set<std::string> styleNames;
    for (const Style& style : contents.styles) {
        styleNames.insert(style.name);
    }
    std::set<EntityId> ids;
    EntityId highest = 0;
    for (const Entity& entity : contents.entities) {
        const std::string context = "id=" + std::to_string(entity.id);
        if (entity.id == katana::entity::kInvalidEntityId || !ids.insert(entity.id).second) {
            return makeError(ErrorCode::InvalidArgument, "entity id is missing or duplicated",
                             context);
        }
        if (layerNames.count(entity.layer) == 0) {
            return makeError(ErrorCode::InvalidArgument, "entity refers to an unknown layer",
                             context + " layer=" + entity.layer);
        }
        if (!entity.style.empty() && styleNames.count(entity.style) == 0) {
            return makeError(ErrorCode::InvalidArgument, "entity refers to an unknown style",
                             context + " style=" + entity.style);
        }
        if (auto status = katana::entity::validate(entity.geometry); !status) {
            return makeError(status.error().code, status.error().message, context);
        }
        highest = std::max(highest, entity.id);
    }
    if (contents.nextEntityId <= highest) {
        return makeError(ErrorCode::InvalidArgument,
                         "next entity id would collide with an existing entity",
                         "next=" + std::to_string(contents.nextEntityId));
    }
    for (const Relationship& relationship : contents.relationships) {
        if (ids.count(relationship.from) == 0 || ids.count(relationship.to) == 0) {
            return makeError(ErrorCode::InvalidArgument,
                             "relationship refers to an entity that is not in the project",
                             "kind=" + relationship.kind);
        }
    }
    return {};
}

Status migrate(SqliteDatabase& database, int fromVersion)
{
    for (const Migration& migration : kMigrations) {
        if (migration.toVersion <= fromVersion) {
            continue;
        }
        auto transaction = SqliteTransaction::begin(database);
        if (!transaction) {
            return transaction.error();
        }
        if (auto status = database.execute(migration.sql); !status) {
            return makeError(ErrorCode::DatabaseFailure, "schema migration failed",
                             "to=" + std::to_string(migration.toVersion) + " " +
                                 status.error().context);
        }
        if (auto status = database.setUserVersion(migration.toVersion); !status) {
            return status;
        }
        if (auto status = transaction->commit(); !status) {
            return status;
        }
    }
    return {};
}

// A database that opens, carries the Katana application id and passes the
// integrity check. Returns the problem description otherwise.
Result<std::optional<std::string>> inspectDatabase(const fs::path& file)
{
    // Read-WRITE, not read-only. A database whose process was killed mid-commit
    // has a hot rollback journal beside it, and SQLite replays that journal when
    // the file is next opened for writing - after which it is perfectly sound.
    // A read-only connection cannot replay it and fails with
    // SQLITE_READONLY_ROLLBACK, which is indistinguishable from real corruption
    // here: recover() would then throw away a recoverable database and restore
    // an older backup, losing every change since.
    //
    // Read-only is still tried as a fallback, for genuinely unwritable media.
    auto database = SqliteDatabase::open(file, SqliteDatabase::OpenMode::ReadWrite);
    if (!database) {
        auto readOnly = SqliteDatabase::open(file, SqliteDatabase::OpenMode::ReadOnly);
        if (!readOnly) {
            return std::optional<std::string>{readOnly.error().describe()};
        }
        database = std::move(readOnly);
    }
    const auto applicationId = database->applicationId();
    if (!applicationId) {
        return std::optional<std::string>{applicationId.error().describe()};
    }
    if (*applicationId != kApplicationId) {
        return std::optional<std::string>{"not a Katana project database"};
    }
    auto problems = database->integrityProblems();
    if (!problems) {
        return std::optional<std::string>{problems.error().describe()};
    }
    return *problems;
}

// Backups are written as "project-<compact UTC>.db", and "project-<compact
// UTC>-<n>.db" when several land in the same second. Parsing that back into
// (timestamp, sequence) is what lets them be ordered correctly; ordering by
// filename alone gets the collision case BACKWARDS, because '.' (0x2E) sorts
// above '-' (0x2D) and so "...T120000.db" outranks the later "...T120000-1.db".
struct BackupName {
    std::string timestamp;
    long sequence = 0;
};

std::optional<BackupName> parseBackupName(const fs::path& file)
{
    static constexpr std::string_view kPrefix = "project-";
    // utcTimestamp(true) produces "YYYYMMDDTHHMMSS-mmm" - fixed width, and it
    // ALREADY contains a dash, before the milliseconds. So the collision suffix
    // cannot be found by looking for the first '-' (that finds the milliseconds)
    // nor safely by the last one. The timestamp is a known shape, so it is
    // matched as one and whatever follows must be the suffix.
    static constexpr std::size_t kTimestampLength = 19; // YYYYMMDDTHHMMSS-mmm
    const std::string stem = file.stem().string();
    if (file.extension() != ".db" || stem.size() < kPrefix.size() + kTimestampLength ||
        stem.compare(0, kPrefix.size(), kPrefix) != 0) {
        return std::nullopt;
    }
    const std::string rest = stem.substr(kPrefix.size());

    BackupName parsed;
    parsed.timestamp = rest.substr(0, kTimestampLength);
    for (std::size_t i = 0; i < kTimestampLength; ++i) {
        const char ch = parsed.timestamp[i];
        const bool valid = i == 8    ? ch == 'T'
                           : i == 15 ? ch == '-'
                                     : std::isdigit(static_cast<unsigned char>(ch)) != 0;
        if (!valid) {
            return std::nullopt;
        }
    }

    if (rest.size() > kTimestampLength) {
        // Only "-<digits>", the suffix added when two backups land in the same
        // millisecond, may follow.
        if (rest[kTimestampLength] != '-') {
            return std::nullopt;
        }
        const std::string sequence = rest.substr(kTimestampLength + 1);
        if (sequence.empty() || sequence.find_first_not_of("0123456789") != std::string::npos) {
            return std::nullopt;
        }
        parsed.sequence = std::stol(sequence);
    }
    return parsed;
}

std::vector<fs::path> backupsNewestFirst(const fs::path& projectDirectory)
{
    // Only files this class actually wrote are considered. Any other .db
    // dropped into backups/ - a copy someone made by hand, an editor's stray
    // save - would otherwise be offered as a restore candidate and could
    // outrank every real snapshot on name alone.
    std::vector<std::pair<BackupName, fs::path>> backups;
    std::error_code ignored;
    for (const auto& entry : fs::directory_iterator(projectDirectory / kBackupDirectory, ignored)) {
        if (!entry.is_regular_file()) {
            continue;
        }
        if (auto parsed = parseBackupName(entry.path())) {
            backups.emplace_back(*parsed, entry.path());
        }
    }
    std::sort(backups.begin(), backups.end(), [](const auto& a, const auto& b) {
        if (a.first.timestamp != b.first.timestamp) {
            return a.first.timestamp > b.first.timestamp;
        }
        return a.first.sequence > b.first.sequence;
    });

    std::vector<fs::path> ordered;
    ordered.reserve(backups.size());
    for (auto& [name, path] : backups) {
        ordered.push_back(std::move(path));
    }
    return ordered;
}

} // namespace

// ---- model <-> contents ---------------------------------------------------------------

ProjectContents captureModel(const katana::entity::Model& model, ProjectMetadata metadata,
                             std::vector<Relationship> relationships)
{
    ProjectContents contents;
    contents.metadata = std::move(metadata);
    contents.layers = model.layers.all();
    contents.styles = model.styles.all();
    contents.linetypes = model.linetypes.all();
    contents.dimensionStyles = model.dimensionStyles.all();
    contents.hatchPatterns = model.hatchPatterns.all();
    contents.alignments = model.alignments.all();
    contents.propertyDefinitions = model.properties.all();
    contents.entities.reserve(model.entities.size());
    model.entities.forEach([&](const Entity& entity) { contents.entities.push_back(entity); });
    contents.relationships = std::move(relationships);
    contents.nextEntityId = model.entities.nextId();
    return contents;
}

namespace {

// One body for both applyToModel overloads. `Contents` deduces to a const
// lvalue reference for the copying one and to a value for the consuming one;
// `take` is then the identity or std::move, so the staging loops below read the
// same either way and neither overload copies more than its caller asked for.
//
// Written as a template rather than as two functions because the loops ARE the
// function: duplicating forty lines of insertion order and built-in-name rules
// would be a second place for them to drift apart.
//
// Every table below takes its item BY VALUE (NamedTable::add,
// PropertyTable::define, EntityDatabase::insert), so `take` reaches all the way
// in. update() takes a const reference and cannot, but it only ever applies to
// the four built-in names.
template <typename Contents> Status applyContents(Contents&& contents, katana::entity::Model& model)
{
    constexpr bool kConsume = !std::is_lvalue_reference_v<Contents&&>;
    const auto take = []<typename T>(T& value) -> decltype(auto) {
        if constexpr (kConsume) {
            return std::move(value);
        } else {
            return (value);
        }
    };

    if (auto status = validateContents(contents); !status) {
        return status;
    }
    // Built ASIDE and committed only once every insertion has succeeded.
    //
    // validateContents() does not cover everything the tables themselves
    // enforce - line weights, empty or duplicate names, property defaults - so
    // an insertion here really can fail on a file that passed validation. The
    // previous version reset the caller's model first and reset it again on
    // failure, which destroyed the open drawing; Document::open documents the
    // opposite ("a failed open leaves the current drawing exactly as it was")
    // and relies on it.
    katana::entity::Model staged;
    for (auto& layer : contents.layers) {
        auto status = layer.name == katana::entity::kDefaultLayerName
                          ? staged.layers.update(layer)
                          : staged.layers.add(take(layer));
        if (!status) {
            return status;
        }
    }
    for (auto& style : contents.styles) {
        if (auto status = staged.styles.add(take(style)); !status) {
            return status;
        }
    }
    for (auto& linetype : contents.linetypes) {
        // "continuous" is built in, so a stored one updates rather than adds -
        // the same rule layer "0" follows above.
        auto status = linetype.name == katana::entity::kContinuousLinetype
                          ? staged.linetypes.update(linetype)
                          : staged.linetypes.add(take(linetype));
        if (!status) {
            return status;
        }
    }
    for (auto& style : contents.dimensionStyles) {
        auto status = style.name == katana::entity::kDefaultDimensionStyleName
                          ? staged.dimensionStyles.update(style)
                          : staged.dimensionStyles.add(take(style));
        if (!status) {
            return status;
        }
    }
    for (auto& pattern : contents.hatchPatterns) {
        // "none" is built in, so a stored one updates rather than adds - the
        // same rule layer "0" and the continuous linetype follow above.
        auto status = pattern.name == katana::entity::kNoHatch
                          ? staged.hatchPatterns.update(pattern)
                          : staged.hatchPatterns.add(take(pattern));
        if (!status) {
            return status;
        }
    }
    for (auto& alignment : contents.alignments) {
        // add() solves the definition, so a file holding an alignment that
        // cannot be built is refused here with the PI named, not opened and
        // drawn wrong.
        if (auto status = staged.alignments.add(take(alignment)); !status) {
            return status;
        }
    }
    for (auto& definition : contents.propertyDefinitions) {
        if (auto status = staged.properties.define(take(definition)); !status) {
            return status;
        }
    }
    for (auto& entity : contents.entities) {
        if (auto status = staged.entities.insert(take(entity)); !status) {
            return status;
        }
    }
    staged.entities.reserveIdsBelow(contents.nextEntityId);

    // Nothing below can fail, so this is the commit point.
    model.adoptContents(std::move(staged));
    return {};
}

} // namespace

Status applyToModel(const ProjectContents& contents, katana::entity::Model& model)
{
    return applyContents(contents, model);
}

Status applyToModel(ProjectContents&& contents, katana::entity::Model& model)
{
    return applyContents(std::move(contents), model);
}

// ---- ProjectStore -----------------------------------------------------------------------

struct ProjectStore::Impl {
    fs::path directory;
    SqliteDatabase database;
};

ProjectStore::ProjectStore(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}
ProjectStore::ProjectStore(ProjectStore&&) noexcept = default;
ProjectStore& ProjectStore::operator=(ProjectStore&&) noexcept = default;
ProjectStore::~ProjectStore() = default;

bool ProjectStore::isProjectDirectory(const fs::path& directory)
{
    std::error_code ignored;
    return fs::is_regular_file(directory / kDatabaseFile, ignored);
}

Result<ProjectStore> ProjectStore::create(const fs::path& projectDirectory,
                                          const ProjectMetadata& metadata)
{
    if (isProjectDirectory(projectDirectory)) {
        return makeError(ErrorCode::AlreadyExists, "a project already exists in this directory",
                         toUtf8(projectDirectory));
    }
    std::error_code failure;
    fs::create_directories(projectDirectory / kBackupDirectory, failure);
    for (const char* name : kDataDirectories) {
        if (!failure) {
            fs::create_directories(projectDirectory / name, failure);
        }
    }
    if (failure) {
        return makeError(ErrorCode::DatabaseFailure, "could not create the project directories",
                         toUtf8(projectDirectory) + ": " + failure.message());
    }

    auto database = SqliteDatabase::open(projectDirectory / kDatabaseFile);
    if (!database) {
        return database.error();
    }
    if (auto status = database->setApplicationId(kApplicationId); !status) {
        return status.error();
    }
    if (auto status = migrate(*database, 0); !status) {
        return status.error();
    }

    ProjectStore store(std::make_unique<Impl>(Impl{projectDirectory, std::move(*database)}));
    ProjectContents empty;
    empty.metadata = metadata;
    empty.metadata.createdUtc = utcTimestamp(false);
    empty.layers.push_back(Layer{});
    if (auto status = store.save(empty); !status) {
        return status.error();
    }
    return store;
}

Result<ProjectStore> ProjectStore::open(const fs::path& projectDirectory)
{
    if (!isProjectDirectory(projectDirectory)) {
        return makeError(ErrorCode::NotFound, "no Katana project in this directory",
                         toUtf8(projectDirectory));
    }
    const fs::path file = projectDirectory / kDatabaseFile;
    const auto problems = inspectDatabase(file);
    if (!problems) {
        return problems.error();
    }
    if (problems->has_value()) {
        return makeError(ErrorCode::DatabaseFailure,
                         "project database is damaged; ProjectStore::recover() can restore a backup",
                         **problems);
    }

    auto database = SqliteDatabase::open(file, SqliteDatabase::OpenMode::ReadWrite);
    if (!database) {
        return database.error();
    }
    const auto version = database->userVersion();
    if (!version) {
        return version.error();
    }
    if (*version > kCurrentSchemaVersion) {
        return makeError(ErrorCode::Unsupported,
                         "project was written by a newer version of Katana",
                         "schema=" + std::to_string(*version) +
                             " supported=" + std::to_string(kCurrentSchemaVersion));
    }

    ProjectStore store(std::make_unique<Impl>(Impl{projectDirectory, std::move(*database)}));
    if (*version < kCurrentSchemaVersion) {
        if (auto saved = store.backup(); !saved) { // never migrate without a way back
            return saved.error();
        }
        if (auto status = migrate(store.impl_->database, *version); !status) {
            return status.error();
        }
    }
    return store;
}

Result<RecoveryReport> ProjectStore::recover(const fs::path& projectDirectory)
{
    const fs::path file = projectDirectory / kDatabaseFile;
    RecoveryReport report;

    std::error_code ignored;
    if (fs::exists(file, ignored)) {
        const auto problems = inspectDatabase(file);
        if (!problems) {
            return problems.error();
        }
        if (!problems->has_value()) {
            return report; // sound: nothing to do
        }
        report.problems = **problems;
    } else {
        report.problems = "project.db is missing";
    }

    for (const fs::path& candidate : backupsNewestFirst(projectDirectory)) {
        const auto candidateProblems = inspectDatabase(candidate);
        if (!candidateProblems || candidateProblems->has_value()) {
            continue; // this backup is damaged too; try an older one
        }
        std::error_code failure;
        if (fs::exists(file, ignored)) {
            report.damagedFileKeptAs =
                projectDirectory / (std::string(kDatabaseFile) + ".damaged-" + utcTimestamp(true));
            fs::rename(file, report.damagedFileKeptAs, failure);
            if (failure) {
                return makeError(ErrorCode::DatabaseFailure,
                                 "could not move the damaged database aside", failure.message());
            }
        }
        // A rollback journal of the damaged file must never be left beside the
        // restored one: SQLite would replay it over the good data on the next
        // open, corrupting the file that was just recovered.
        const fs::path journal = projectDirectory / (std::string(kDatabaseFile) + "-journal");
        if (fs::exists(journal, ignored)) {
            // damagedFileKeptAs is empty when project.db was missing entirely,
            // and concat() on an empty path yields the RELATIVE name "-journal",
            // which moves the journal into the working directory and leaves it
            // beside the restored database. The target is always built inside
            // projectDirectory instead.
            const fs::path journalKeptAs =
                report.damagedFileKeptAs.empty()
                    ? projectDirectory / (std::string(kDatabaseFile) + ".damaged-" +
                                          utcTimestamp(true) + "-journal")
                    : fs::path(report.damagedFileKeptAs).concat("-journal");
            fs::rename(journal, journalKeptAs, failure);
            if (failure) {
                // Restoring on top of a journal that is still there would be
                // worse than not restoring at all.
                return makeError(ErrorCode::DatabaseFailure,
                                 "could not move the damaged database's rollback journal aside",
                                 failure.message());
            }
        }
        fs::copy_file(candidate, file, fs::copy_options::overwrite_existing, failure);
        if (failure) {
            return makeError(ErrorCode::DatabaseFailure, "could not restore the backup",
                             failure.message());
        }
        report.restored = true;
        report.restoredFrom = candidate;
        return report;
    }
    return makeError(ErrorCode::DatabaseFailure, "no usable backup was found",
                     toUtf8(projectDirectory) + ": " + report.problems);
}

const fs::path& ProjectStore::directory() const
{
    return impl_->directory;
}

fs::path ProjectStore::databasePath() const
{
    return impl_->directory / kDatabaseFile;
}

Result<int> ProjectStore::schemaVersion()
{
    return impl_->database.userVersion();
}

std::vector<fs::path> ProjectStore::listBackups() const
{
    return backupsNewestFirst(impl_->directory);
}

Result<fs::path> ProjectStore::backup(std::size_t keep)
{
    const fs::path directory = impl_->directory / kBackupDirectory;
    std::error_code failure;
    fs::create_directories(directory, failure);
    if (failure) {
        return makeError(ErrorCode::DatabaseFailure, "could not create the backup directory",
                         failure.message());
    }
    fs::path target = directory / ("project-" + utcTimestamp(true) + ".db");
    for (int suffix = 1; fs::exists(target, failure); ++suffix) { // same millisecond
        target = directory / ("project-" + utcTimestamp(true) + "-" + std::to_string(suffix) + ".db");
    }
    if (auto status = impl_->database.backupTo(target); !status) {
        fs::remove(target, failure);
        return status.error();
    }
    const auto backups = backupsNewestFirst(impl_->directory);
    for (std::size_t i = keep; i < backups.size(); ++i) {
        fs::remove(backups[i], failure);
    }
    return target;
}

namespace {

Status writeMetadata(SqliteDatabase& database, const ProjectContents& contents)
{
    // REPLACE, because save() does not empty this table: a key this build
    // does not know stays where a newer build put it, and every key it does
    // know is written on every save, so none can go stale.
    auto insert =
        database.prepare("INSERT OR REPLACE INTO metadata (key, value) VALUES (?1, ?2)");
    if (!insert) {
        return insert.error();
    }
    const ProjectMetadata& m = contents.metadata;
    std::string customisation;
    for (const std::string& name : m.customisation) {
        customisation += (customisation.empty() ? "" : "\n") + name;
    }
    const std::pair<std::string_view, std::string> rows[] = {
        {"name", m.name},
        {"description", m.description},
        {"linear_unit", m.linearUnit},
        {"coordinate_system", m.coordinateSystem},
        {"created_utc", m.createdUtc},
        {"modified_utc", utcTimestamp(false)},
        {"application_version", m.applicationVersion},
        {"next_entity_id", std::to_string(contents.nextEntityId)},
        {"customisation", std::move(customisation)},
    };
    // One row per key the reader knows: a key added to one list and not the
    // other would be read back as unknown, or never written.
    static_assert(sizeof(rows) / sizeof(rows[0]) == std::size(kMetadataKeys));
    const auto write = [&insert](std::string_view key, std::string_view value) {
        Status status = insert->bind(1, key);
        if (status) {
            status = insert->bind(2, value);
        }
        if (status) {
            status = insert->run();
        }
        return status;
    };
    for (const auto& [key, value] : rows) {
        if (auto status = write(key, value); !status) {
            return status;
        }
    }
    for (const auto& [key, value] : m.unknownKeys) {
        if (auto status = write(key, value); !status) {
            return status;
        }
    }
    return {};
}

// Binds a nullable colour column.
Status bindColor(SqliteStatement& statement, int index,
                 const std::optional<katana::entity::Color>& color)
{
    return color ? statement.bind(index, std::string_view(color->toHex()))
                 : statement.bindNull(index);
}

// Chains bind calls, stopping at the first failure.
class Binder {
  public:
    explicit Binder(SqliteStatement& statement) : statement_(statement) {}

    template <typename T> Binder& operator()(int index, const T& value)
    {
        if (status_) {
            status_ = statement_.bind(index, value);
        }
        return *this;
    }
    Binder& color(int index, const std::optional<katana::entity::Color>& value)
    {
        if (status_) {
            status_ = bindColor(statement_, index, value);
        }
        return *this;
    }
    [[nodiscard]] Status run()
    {
        if (!status_) {
            statement_.reset();
            return std::move(status_);
        }
        return statement_.run();
    }

  private:
    SqliteStatement& statement_;
    Status status_;
};

} // namespace

Status ProjectStore::save(const ProjectContents& contents)
{
    if (auto status = validateContents(contents); !status) {
        return status;
    }
    SqliteDatabase& database = impl_->database;
    auto transaction = SqliteTransaction::begin(database);
    if (!transaction) {
        return transaction.error();
    }
    // Every table a save rewrites must be cleared here, or the SECOND save of a
    // project fails on a primary key it already wrote. A round-trip test that
    // saves once does not catch that; CadDocument.SaveReopenAndModifiedFlag
    // does, which is why it saves twice.
    //
    // The metadata table is the one exception, and is not cleared: it holds
    // keys a newer build wrote, which an older build's save must not strip.
    // writeMetadata replaces every key this build knows instead.
    //
    // linetype_elements is deleted explicitly rather than left to the ON DELETE
    // CASCADE: foreign keys are only enforced when the pragma is on, so relying
    // on the cascade would make correctness depend on a connection setting.
    if (auto status = database.execute("DELETE FROM relationships; DELETE FROM entities;"
                                       "DELETE FROM property_definitions; DELETE FROM styles;"
                                       "DELETE FROM linetype_elements; DELETE FROM linetypes;"
                                       "DELETE FROM dimension_styles;"
                                       "DELETE FROM hatch_families; DELETE FROM hatch_patterns;"
                                       "DELETE FROM alignment_pvis; DELETE FROM alignment_pis; DELETE FROM alignments;"
                                       "DELETE FROM layers;");
        !status) {
        return status;
    }
    if (auto status = writeMetadata(database, contents); !status) {
        return status;
    }

    auto insertLayer = database.prepare(
        "INSERT INTO layers (name, color, visible, locked, linetype, line_weight,"
        " dimension_style, hatch_pattern) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)");
    if (!insertLayer) {
        return insertLayer.error();
    }
    bool hasDefaultLayer = false;
    for (const Layer& layer : contents.layers) {
        hasDefaultLayer = hasDefaultLayer || layer.name == katana::entity::kDefaultLayerName;
        if (auto status = Binder(*insertLayer)(1, std::string_view(layer.name))(
                              2, std::string_view(layer.color.toHex()))(3, layer.visible)(
                              4, layer.locked)(5, std::string_view(layer.linetype))(
                              6, layer.lineWeight)(7, std::string_view(layer.dimensionStyle))(
                              8, std::string_view(layer.hatchPattern))
                              .run();
            !status) {
            return status;
        }
    }
    if (!hasDefaultLayer) { // entities may legitimately refer to it
        const Layer fallback;
        if (auto status = Binder(*insertLayer)(1, std::string_view(fallback.name))(
                              2, std::string_view(fallback.color.toHex()))(3, fallback.visible)(
                              4, fallback.locked)(5, std::string_view(fallback.linetype))(
                              6, fallback.lineWeight)(
                              7, std::string_view(fallback.dimensionStyle))(
                              8, std::string_view(fallback.hatchPattern))
                              .run();
            !status) {
            return status;
        }
    }

    auto insertStyle = database.prepare(
        "INSERT INTO styles (name, color, line_weight, linetype, hatch_pattern, description,"
        " symbol, symbol_size) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)");
    if (!insertStyle) {
        return insertStyle.error();
    }
    for (const Style& style : contents.styles) {
        if (auto status = Binder(*insertStyle)(1, std::string_view(style.name))
                              .color(2, style.color)(3, style.lineWeight)(
                                  4, std::string_view(style.linetype))(
                                  5, std::string_view(style.hatchPattern))(
                                  6, std::string_view(style.description))(
                                  7, std::string_view(style.symbol))(8, style.symbolSize)
                              .run();
            !status) {
            return status;
        }
    }

    auto insertDefinition = database.prepare(
        "INSERT INTO property_definitions (name, type, description, default_value)"
        " VALUES (?1, ?2, ?3, ?4)");
    if (!insertDefinition) {
        return insertDefinition.error();
    }
    for (const PropertyDefinition& definition : contents.propertyDefinitions) {
        Binder binder(*insertDefinition);
        binder(1, std::string_view(definition.name))(2, toString(definition.type))(
            3, std::string_view(definition.description));
        Status status;
        if (definition.defaultValue) {
            auto json = katana::entity::propertiesToJson({{"value", *definition.defaultValue}});
            if (!json) {
                return json.error();
            }
            status = binder(4, std::string_view(*json)).run();
        } else {
            status = insertDefinition->bindNull(4);
            if (status) {
                status = binder.run();
            }
        }
        if (!status) {
            return status;
        }
    }

    auto insertDimensionStyle = database.prepare(
        "INSERT INTO dimension_styles (name, text_height, text_gap, extension_offset,"
        " extension_beyond, arrow_size, arrow_head, unit_scale, prefix, suffix, decimals,"
        " round_to, suppress_zeros)"
        " VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)");
    if (!insertDimensionStyle) {
        return insertDimensionStyle.error();
    }
    for (const katana::entity::DimensionStyle& style : contents.dimensionStyles) {
        if (auto status =
                Binder(*insertDimensionStyle)(1, std::string_view(style.name))(
                    2, style.textHeight)(3, style.textGap)(4, style.extensionOffset)(
                    5, style.extensionBeyond)(6, style.arrowSize)(
                    7, katana::entity::toString(style.arrowHead))(8, style.unitScale)(
                    9, std::string_view(style.prefix))(10, std::string_view(style.suffix))(
                    11, static_cast<std::int64_t>(style.decimals))(12, style.roundTo)(
                    13, style.suppressTrailingZeros)
                    .run();
            !status) {
            return makeError(status.error().code, status.error().message,
                             "dimension style=" + style.name + " " + status.error().context);
        }
    }

    auto insertLinetype =
        database.prepare("INSERT INTO linetypes (name, description) VALUES (?1, ?2)");
    if (!insertLinetype) {
        return insertLinetype.error();
    }
    auto insertElement = database.prepare(
        "INSERT INTO linetype_elements (linetype, position, length) VALUES (?1, ?2, ?3)");
    if (!insertElement) {
        return insertElement.error();
    }
    for (const katana::entity::Linetype& linetype : contents.linetypes) {
        if (auto status = Binder(*insertLinetype)(1, std::string_view(linetype.name))(
                              2, std::string_view(linetype.description))
                              .run();
            !status) {
            return makeError(status.error().code, status.error().message,
                             "linetype=" + linetype.name + " " + status.error().context);
        }
        for (std::size_t i = 0; i < linetype.pattern.size(); ++i) {
            if (auto status = Binder(*insertElement)(1, std::string_view(linetype.name))(
                                  2, static_cast<std::int64_t>(i))(
                                  3, linetype.pattern[i].length)
                                  .run();
                !status) {
                return makeError(status.error().code, status.error().message,
                                 "linetype=" + linetype.name + " element=" + std::to_string(i));
            }
        }
    }

    auto insertPattern = database.prepare(
        "INSERT INTO hatch_patterns (name, description, solid) VALUES (?1, ?2, ?3)");
    if (!insertPattern) {
        return insertPattern.error();
    }
    auto insertFamily = database.prepare(
        "INSERT INTO hatch_families (pattern, position, angle, spacing, line_offset)"
        " VALUES (?1, ?2, ?3, ?4, ?5)");
    if (!insertFamily) {
        return insertFamily.error();
    }
    for (const katana::entity::HatchPattern& pattern : contents.hatchPatterns) {
        if (auto status = Binder(*insertPattern)(1, std::string_view(pattern.name))(
                              2, std::string_view(pattern.description))(3, pattern.solid)
                              .run();
            !status) {
            return makeError(status.error().code, status.error().message,
                             "hatch pattern=" + pattern.name + " " + status.error().context);
        }
        for (std::size_t i = 0; i < pattern.families.size(); ++i) {
            const katana::entity::HatchLineFamily& family = pattern.families[i];
            if (auto status = Binder(*insertFamily)(1, std::string_view(pattern.name))(
                                  2, static_cast<std::int64_t>(i))(3, family.angle)(
                                  4, family.spacing)(5, family.offset)
                                  .run();
                !status) {
                return makeError(status.error().code, status.error().message,
                                 "hatch pattern=" + pattern.name +
                                     " family=" + std::to_string(i));
            }
        }
    }

    auto insertAlignment = database.prepare(
        "INSERT INTO alignments (name, description, start_station) VALUES (?1, ?2, ?3)");
    if (!insertAlignment) {
        return insertAlignment.error();
    }
    auto insertPI = database.prepare(
        "INSERT INTO alignment_pis (alignment, position, x, y, radius, spiral_in, spiral_out)"
        " VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)");
    if (!insertPI) {
        return insertPI.error();
    }
    auto insertPVI = database.prepare(
        "INSERT INTO alignment_pvis (alignment, position, station, elevation, curve_length)"
        " VALUES (?1, ?2, ?3, ?4, ?5)");
    if (!insertPVI) {
        return insertPVI.error();
    }
    for (const katana::entity::Alignment& alignment : contents.alignments) {
        if (auto status = Binder(*insertAlignment)(1, std::string_view(alignment.name))(
                              2, std::string_view(alignment.description))(
                              3, alignment.horizontal.startStation)
                              .run();
            !status) {
            return makeError(status.error().code, status.error().message,
                             "alignment=" + alignment.name + " " + status.error().context);
        }
        for (std::size_t i = 0; i < alignment.horizontal.pis.size(); ++i) {
            const katana::geometry::AlignmentPI& pi = alignment.horizontal.pis[i];
            if (auto status = Binder(*insertPI)(1, std::string_view(alignment.name))(
                                  2, static_cast<std::int64_t>(i))(3, pi.point.x)(4, pi.point.y)(
                                  5, pi.radius)(6, pi.spiralIn)(7, pi.spiralOut)
                                  .run();
                !status) {
                return makeError(status.error().code, status.error().message,
                                 "alignment=" + alignment.name + " pi=" + std::to_string(i));
            }
        }
        if (alignment.vertical.has_value()) {
            for (std::size_t i = 0; i < alignment.vertical->pvis.size(); ++i) {
                const katana::geometry::ProfilePVI& pvi = alignment.vertical->pvis[i];
                if (auto status = Binder(*insertPVI)(1, std::string_view(alignment.name))(
                                      2, static_cast<std::int64_t>(i))(3, pvi.station)(
                                      4, pvi.elevation)(5, pvi.curveLength)
                                      .run();
                    !status) {
                    return makeError(status.error().code, status.error().message,
                                     "alignment=" + alignment.name +
                                         " pvi=" + std::to_string(i));
                }
            }
        }
    }

    auto insertEntity = database.prepare(
        "INSERT INTO entities (id, type, layer, style, color, visible, geometry, properties, "
        "metadata, geometry_blob)"
        " VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)");
    if (!insertEntity) {
        return insertEntity.error();
    }
    for (const Entity& entity : contents.entities) {
        // A write that cannot be encoded fails the save cleanly instead of
        // aborting the process partway through the transaction.
        //
        // Geometry goes in as a binary blob; the legacy `geometry` TEXT column
        // is written empty. Storing the JSON as well would double the cost of
        // a save for a column nothing reads any more - geometryToJson is still
        // there for exports and for inspecting a database by hand. Rows
        // written before schema 3 keep their JSON and are read from it until
        // the project is next saved.
        auto geometry = katana::entity::geometryToBlob(entity.geometry);
        if (!geometry) {
            return geometry.error();
        }
        auto properties = katana::entity::propertiesToJson(entity.properties);
        if (!properties) {
            return properties.error();
        }
        auto metadata = katana::entity::propertiesToJson(entity.metadata);
        if (!metadata) {
            return metadata.error();
        }
        if (auto status = Binder(*insertEntity)(1, static_cast<std::int64_t>(entity.id))(
                              2, katana::entity::toString(entity.type()))(
                              3, std::string_view(entity.layer))(4, std::string_view(entity.style))
                              .color(5, entity.color)(6, entity.visible)(
                                  7, std::string_view{})(8, std::string_view(*properties))(
                                  9, std::string_view(*metadata))(
                                  10, std::span<const std::byte>(*geometry))
                              .run();
            !status) {
            return makeError(status.error().code, status.error().message,
                             "entity id=" + std::to_string(entity.id) + " " +
                                 status.error().context);
        }
    }

    auto insertRelationship = database.prepare(
        "INSERT INTO relationships (from_entity, to_entity, kind, data) VALUES (?1, ?2, ?3, ?4)");
    if (!insertRelationship) {
        return insertRelationship.error();
    }
    for (const Relationship& relationship : contents.relationships) {
        if (auto status = Binder(*insertRelationship)(1, static_cast<std::int64_t>(relationship.from))(
                              2, static_cast<std::int64_t>(relationship.to))(
                              3, std::string_view(relationship.kind))(
                              4, std::string_view(relationship.data))
                              .run();
            !status) {
            return status;
        }
    }
    return transaction->commit();
}

Result<ProjectContents> ProjectStore::load()
{
    SqliteDatabase& database = impl_->database;
    ProjectContents contents;

    // Runs `sql` and calls `onRow` for every row; stops at the first failure.
    const auto forEachRow = [&](const char* sql, auto onRow) -> Status {
        auto statement = database.prepare(sql);
        if (!statement) {
            return statement.error();
        }
        while (true) {
            const auto row = statement->step();
            if (!row) {
                return row.error();
            }
            if (!*row) {
                return {};
            }
            if (auto status = onRow(*statement); !status) {
                return status;
            }
        }
    };
    const auto readColor = [](SqliteStatement& row, int column,
                              std::optional<katana::entity::Color>& out) -> Status {
        if (row.columnIsNull(column)) {
            out.reset();
            return {};
        }
        const auto color = katana::entity::Color::fromHex(row.columnTextView(column));
        if (!color) {
            return color.error();
        }
        out = *color;
        return {};
    };

    Status status = forEachRow("SELECT key, value FROM metadata", [&](SqliteStatement& row) -> Status {
        const std::string key = row.columnText(0);
        std::string value = row.columnText(1);
        ProjectMetadata& m = contents.metadata;
        if (key == "name") {
            m.name = std::move(value);
        } else if (key == "description") {
            m.description = std::move(value);
        } else if (key == "linear_unit") {
            m.linearUnit = std::move(value);
        } else if (key == "coordinate_system") {
            m.coordinateSystem = std::move(value);
        } else if (key == "created_utc") {
            m.createdUtc = std::move(value);
        } else if (key == "modified_utc") {
            m.modifiedUtc = std::move(value);
        } else if (key == "application_version") {
            m.applicationVersion = std::move(value);
        } else if (key == "next_entity_id") {
            try {
                contents.nextEntityId = std::stoull(value);
            } catch (const std::exception&) {
                return makeError(ErrorCode::DatabaseFailure, "next_entity_id is not a number", value);
            }
        } else if (key == "customisation") {
            m.customisation.clear();
            std::size_t start = 0;
            while (start < value.size()) {
                const std::size_t end = std::min(value.find('\n', start), value.size());
                m.customisation.push_back(value.substr(start, end - start));
                start = end + 1;
            }
        } else {
            // A key from a newer build: kept, so this build's save writes it
            // back. Every key this build knows is handled above.
            m.unknownKeys.emplace(key, std::move(value));
        }
        return {};
    });
    if (!status) {
        return status.error();
    }

    status = forEachRow(
        "SELECT name, color, visible, locked, linetype, line_weight, dimension_style,"
        " hatch_pattern FROM layers ORDER BY name",
        [&](SqliteStatement& row) -> Status {
            Layer layer;
            layer.name = row.columnText(0);
            const auto color = katana::entity::Color::fromHex(row.columnText(1));
            if (!color) {
                return color.error();
            }
            layer.color = *color;
            layer.visible = row.columnInt64(2) != 0;
            layer.locked = row.columnInt64(3) != 0;
            layer.linetype = row.columnText(4);
            layer.lineWeight = row.columnDouble(5);
            layer.dimensionStyle = row.columnText(6);
            layer.hatchPattern = row.columnText(7);
            contents.layers.push_back(std::move(layer));
            return {};
        });
    if (!status) {
        return status.error();
    }

    status = forEachRow(
        "SELECT name, text_height, text_gap, extension_offset, extension_beyond, arrow_size,"
        " arrow_head, unit_scale, prefix, suffix, decimals, round_to, suppress_zeros"
        " FROM dimension_styles ORDER BY name",
        [&](SqliteStatement& row) -> Status {
            katana::entity::DimensionStyle style;
            style.name = row.columnText(0);
            style.textHeight = row.columnDouble(1);
            style.textGap = row.columnDouble(2);
            style.extensionOffset = row.columnDouble(3);
            style.extensionBeyond = row.columnDouble(4);
            style.arrowSize = row.columnDouble(5);
            auto head = katana::entity::arrowHeadFromString(row.columnText(6));
            if (!head) {
                return makeError(head.error().code, head.error().message,
                                 "dimension style=" + style.name);
            }
            style.arrowHead = *head;
            style.unitScale = row.columnDouble(7);
            style.prefix = row.columnText(8);
            style.suffix = row.columnText(9);
            style.decimals = static_cast<int>(row.columnInt64(10));
            style.roundTo = row.columnDouble(11);
            style.suppressTrailingZeros = row.columnInt64(12) != 0;
            contents.dimensionStyles.push_back(std::move(style));
            return {};
        });
    if (!status) {
        return status.error();
    }

    status = forEachRow(
        "SELECT name, description FROM linetypes ORDER BY name",
        [&](SqliteStatement& row) -> Status {
            katana::entity::Linetype linetype;
            linetype.name = row.columnText(0);
            linetype.description = row.columnText(1);
            contents.linetypes.push_back(std::move(linetype));
            return {};
        });
    if (!status) {
        return status.error();
    }
    // Elements in one pass, ordered by position, matched to the definition by
    // name. A pattern whose definition row is missing is dropped rather than
    // resurrected under an empty name.
    status = forEachRow(
        "SELECT linetype, length FROM linetype_elements ORDER BY linetype, position",
        [&](SqliteStatement& row) -> Status {
            const std::string name = row.columnText(0);
            const auto found = std::find_if(
                contents.linetypes.begin(), contents.linetypes.end(),
                [&name](const katana::entity::Linetype& l) { return l.name == name; });
            if (found == contents.linetypes.end()) {
                return makeError(ErrorCode::ParseFailure,
                                 "a linetype element names a definition that is not there", name);
            }
            found->pattern.push_back(katana::entity::LinetypeElement{row.columnDouble(1)});
            return {};
        });
    if (!status) {
        return status.error();
    }

    status = forEachRow(
        "SELECT name, description, solid FROM hatch_patterns ORDER BY name",
        [&](SqliteStatement& row) -> Status {
            katana::entity::HatchPattern pattern;
            pattern.name = row.columnText(0);
            pattern.description = row.columnText(1);
            pattern.solid = row.columnInt64(2) != 0;
            contents.hatchPatterns.push_back(std::move(pattern));
            return {};
        });
    if (!status) {
        return status.error();
    }

    // Ordered by position so the families come back in the order they were
    // written; a crosshatch is two families and their order is part of the
    // definition, not an accident of the table.
    status = forEachRow(
        "SELECT pattern, angle, spacing, line_offset FROM hatch_families"
        " ORDER BY pattern, position",
        [&](SqliteStatement& row) -> Status {
            const std::string name = row.columnText(0);
            const auto found = std::lower_bound(
                contents.hatchPatterns.begin(), contents.hatchPatterns.end(), name,
                [](const katana::entity::HatchPattern& pattern, const std::string& wanted) {
                    return pattern.name < wanted;
                });
            if (found == contents.hatchPatterns.end() || found->name != name) {
                // The foreign key makes this unreachable; reporting rather than
                // dropping the row is what stops a future schema change from
                // silently losing families (PLAN.MD section 36).
                return makeError(ErrorCode::DatabaseFailure,
                                 "a hatch family names a pattern that was not read", name);
            }
            katana::entity::HatchLineFamily family;
            family.angle = row.columnDouble(1);
            family.spacing = row.columnDouble(2);
            family.offset = row.columnDouble(3);
            found->families.push_back(family);
            return {};
        });
    if (!status) {
        return status.error();
    }

    status = forEachRow(
        "SELECT name, description, start_station FROM alignments ORDER BY name",
        [&](SqliteStatement& row) -> Status {
            katana::entity::Alignment alignment;
            alignment.name = row.columnText(0);
            alignment.description = row.columnText(1);
            alignment.horizontal.startStation = row.columnDouble(2);
            contents.alignments.push_back(std::move(alignment));
            return {};
        });
    if (!status) {
        return status.error();
    }

    // Ordered by position: the PI order IS the alignment.
    status = forEachRow(
        "SELECT alignment, x, y, radius, spiral_in, spiral_out FROM alignment_pis"
        " ORDER BY alignment, position",
        [&](SqliteStatement& row) -> Status {
            const std::string name = row.columnText(0);
            const auto found = std::lower_bound(
                contents.alignments.begin(), contents.alignments.end(), name,
                [](const katana::entity::Alignment& alignment, const std::string& wanted) {
                    return alignment.name < wanted;
                });
            if (found == contents.alignments.end() || found->name != name) {
                return makeError(ErrorCode::DatabaseFailure,
                                 "an alignment PI names an alignment that was not read", name);
            }
            katana::geometry::AlignmentPI pi;
            pi.point = katana::geometry::Point2(row.columnDouble(1), row.columnDouble(2));
            pi.radius = row.columnDouble(3);
            pi.spiralIn = row.columnDouble(4);
            pi.spiralOut = row.columnDouble(5);
            found->horizontal.pis.push_back(pi);
            return {};
        });
    if (!status) {
        return status.error();
    }

    // The profile exists only when it has rows: the first row creates it, so
    // an alignment with none keeps `vertical` empty rather than acquiring a
    // profile of no PVIs that would then fail to solve.
    status = forEachRow(
        "SELECT alignment, station, elevation, curve_length FROM alignment_pvis"
        " ORDER BY alignment, position",
        [&](SqliteStatement& row) -> Status {
            const std::string name = row.columnText(0);
            const auto found = std::lower_bound(
                contents.alignments.begin(), contents.alignments.end(), name,
                [](const katana::entity::Alignment& alignment, const std::string& wanted) {
                    return alignment.name < wanted;
                });
            if (found == contents.alignments.end() || found->name != name) {
                return makeError(ErrorCode::DatabaseFailure,
                                 "a profile PVI names an alignment that was not read", name);
            }
            if (!found->vertical.has_value()) {
                found->vertical.emplace();
            }
            found->vertical->pvis.push_back(katana::geometry::ProfilePVI{
                row.columnDouble(1), row.columnDouble(2), row.columnDouble(3)});
            return {};
        });
    if (!status) {
        return status.error();
    }

    status = forEachRow("SELECT name, color, line_weight, linetype, hatch_pattern, description,"
                        " symbol, symbol_size FROM styles ORDER BY name",
                        [&](SqliteStatement& row) -> Status {
                            Style style;
                            style.name = row.columnText(0);
                            if (auto colorStatus = readColor(row, 1, style.color); !colorStatus) {
                                return colorStatus;
                            }
                            style.lineWeight = row.columnDouble(2);
                            style.linetype = row.columnText(3);
                            style.hatchPattern = row.columnText(4);
                            style.description = row.columnText(5);
                            style.symbol = row.columnText(6);
                            style.symbolSize = row.columnDouble(7);
                            contents.styles.push_back(std::move(style));
                            return {};
                        });
    if (!status) {
        return status.error();
    }

    status = forEachRow(
        "SELECT name, type, description, default_value FROM property_definitions ORDER BY name",
        [&](SqliteStatement& row) -> Status {
            PropertyDefinition definition;
            definition.name = row.columnText(0);
            const auto type = propertyTypeFromString(row.columnText(1));
            if (!type) {
                return type.error();
            }
            definition.type = *type;
            definition.description = row.columnText(2);
            if (!row.columnIsNull(3)) {
                auto parsed = katana::entity::propertiesFromJson(row.columnText(3));
                if (!parsed) {
                    return parsed.error();
                }
                const auto found = parsed->find("value");
                if (found != parsed->end()) {
                    definition.defaultValue = found->second;
                }
            }
            contents.propertyDefinitions.push_back(std::move(definition));
            return {};
        });
    if (!status) {
        return status.error();
    }

    // Sized up front: every reallocation of this vector moves every Entity in
    // it, and an Entity carries two std::maps. COUNT(*) is one extra b-tree
    // walk; contents.nextEntityId would be free but is only an upper bound, and
    // a drawing that has had most of its entities deleted would reserve for the
    // ids rather than for the rows.
    std::int64_t entityCount = 0;
    status = forEachRow("SELECT COUNT(*) FROM entities", [&](SqliteStatement& row) -> Status {
        entityCount = row.columnInt64(0);
        return {};
    });
    if (!status) {
        return status.error();
    }
    if (entityCount > 0) {
        contents.entities.reserve(static_cast<std::size_t>(entityCount));
    }

    status = forEachRow(
        "SELECT id, layer, style, color, visible, geometry, properties, metadata, geometry_blob"
        " FROM entities ORDER BY id",
        [&](SqliteStatement& row) -> Status {
            Entity entity;
            entity.id = static_cast<EntityId>(row.columnInt64(0));
            // Built only when something fails: on a sound project this ran once
            // per row - 50k string allocations - to describe an error that
            // never happened.
            const auto context = [&entity] { return "entity id=" + std::to_string(entity.id); };
            // The text and blob columns below are BORROWED from SQLite's row
            // buffer and parsed before the statement steps on; see
            // SqliteStatement::columnTextView for the lifetime rule.
            entity.layer = row.columnTextView(1);
            entity.style = row.columnTextView(2);
            if (auto colorStatus = readColor(row, 3, entity.color); !colorStatus) {
                return colorStatus;
            }
            entity.visible = row.columnInt64(4) != 0;
            // The blob is the record from schema 3 onwards; the JSON column is
            // read only for rows written before it, which are converted the
            // next time the project is saved. Preferring the blob when both
            // are present means a half-migrated database still reads the
            // newer of the two rather than the staler.
            if (!row.columnIsNull(8)) {
                auto geometry = katana::entity::geometryFromBlob(row.columnBlobSpan(8));
                if (!geometry) {
                    return makeError(geometry.error().code, geometry.error().message,
                                     context() + " " + geometry.error().context);
                }
                entity.geometry = std::move(*geometry);
            } else {
                auto geometry = katana::entity::geometryFromJson(row.columnTextView(5));
                if (!geometry) {
                    return makeError(geometry.error().code, geometry.error().message,
                                     context() + " " + geometry.error().context);
                }
                entity.geometry = std::move(*geometry);
            }
            auto properties = katana::entity::propertiesFromJson(row.columnTextView(6));
            if (!properties) {
                return makeError(properties.error().code, properties.error().message, context());
            }
            auto metadata = katana::entity::propertiesFromJson(row.columnTextView(7));
            if (!metadata) {
                return makeError(metadata.error().code, metadata.error().message, context());
            }
            entity.properties = std::move(*properties);
            entity.metadata = std::move(*metadata);
            contents.entities.push_back(std::move(entity));
            return {};
        });
    if (!status) {
        return status.error();
    }

    status = forEachRow("SELECT from_entity, to_entity, kind, data FROM relationships ORDER BY id",
                        [&](SqliteStatement& row) -> Status {
                            contents.relationships.push_back(
                                Relationship{static_cast<EntityId>(row.columnInt64(0)),
                                             static_cast<EntityId>(row.columnInt64(1)),
                                             row.columnText(2), row.columnText(3)});
                            return {};
                        });
    if (!status) {
        return status.error();
    }
    return contents;
}

} // namespace katana::storage
