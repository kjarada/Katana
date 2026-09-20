#include "katana/storage/sqlite_database.hpp"

#include <sqlite3.h>

#include <utility>

namespace katana::storage {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;

namespace {

std::string toUtf8(const std::filesystem::path& path)
{
    const auto utf8 = path.u8string();
    return std::string(utf8.begin(), utf8.end());
}

katana::core::Error sqliteError(sqlite3* connection, std::string_view action)
{
    std::string message = "SQLite failed to ";
    message += action;
    return makeError(ErrorCode::DatabaseFailure, std::move(message),
                     connection != nullptr ? sqlite3_errmsg(connection) : "no connection");
}

} // namespace

// ---- SqliteStatement ---------------------------------------------------------------

struct SqliteStatement::Impl {
    sqlite3* connection = nullptr;
    sqlite3_stmt* statement = nullptr;

    ~Impl() { sqlite3_finalize(statement); }

    Status check(int code, std::string_view action) const
    {
        if (code != SQLITE_OK) {
            return sqliteError(connection, action);
        }
        return {};
    }
};

SqliteStatement::SqliteStatement(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}
SqliteStatement::SqliteStatement(SqliteStatement&&) noexcept = default;
SqliteStatement& SqliteStatement::operator=(SqliteStatement&&) noexcept = default;
SqliteStatement::~SqliteStatement() = default;

Status SqliteStatement::bind(int index, std::int64_t value)
{
    return impl_->check(sqlite3_bind_int64(impl_->statement, index, value), "bind an integer");
}

Status SqliteStatement::bind(int index, double value)
{
    return impl_->check(sqlite3_bind_double(impl_->statement, index, value), "bind a real");
}

Status SqliteStatement::bind(int index, std::string_view value)
{
    // A default-constructed string_view has a NULL data() pointer, and SQLite
    // binds a null pointer as SQL NULL rather than as empty text. A caller that
    // wrote `std::string_view{}` meaning "empty" would silently violate a NOT
    // NULL constraint, or worse, store NULL where the schema allows it and have
    // the difference surface much later. Empty text is bound as empty text.
    static constexpr char kEmpty[] = "";
    const char* data = value.data() != nullptr ? value.data() : kEmpty;

    // SQLITE_TRANSIENT: SQLite copies the text, so the caller's buffer may die.
    return impl_->check(sqlite3_bind_text64(impl_->statement, index, data, value.size(),
                                            SQLITE_TRANSIENT, SQLITE_UTF8),
                        "bind text");
}

Status SqliteStatement::bind(int index, bool value)
{
    return bind(index, static_cast<std::int64_t>(value ? 1 : 0));
}

Status SqliteStatement::bind(int index, std::span<const std::byte> value)
{
    // SQLITE_TRANSIENT for the same reason as text: SQLite copies, so the
    // caller's buffer may die immediately afterwards.
    //
    // A zero-length blob is bound through sqlite3_bind_zeroblob rather than
    // bind_blob64 with a null pointer, which SQLite would store as NULL - and
    // NULL is how this schema says "no blob here, read the JSON column".
    if (value.empty()) {
        return impl_->check(sqlite3_bind_zeroblob(impl_->statement, index, 0), "bind empty blob");
    }
    return impl_->check(sqlite3_bind_blob64(impl_->statement, index, value.data(),
                                            static_cast<sqlite3_uint64>(value.size()),
                                            SQLITE_TRANSIENT),
                        "bind blob");
}

Status SqliteStatement::bindNull(int index)
{
    return impl_->check(sqlite3_bind_null(impl_->statement, index), "bind null");
}

Result<bool> SqliteStatement::step()
{
    const int code = sqlite3_step(impl_->statement);
    if (code == SQLITE_ROW) {
        return true;
    }
    if (code == SQLITE_DONE) {
        return false;
    }
    return sqliteError(impl_->connection, "execute a statement");
}

Status SqliteStatement::run()
{
    const auto row = step();
    reset();
    if (!row) {
        return row.error();
    }
    return {};
}

void SqliteStatement::reset()
{
    sqlite3_reset(impl_->statement);
    sqlite3_clear_bindings(impl_->statement);
}

bool SqliteStatement::columnIsNull(int column) const
{
    return sqlite3_column_type(impl_->statement, column) == SQLITE_NULL;
}

std::int64_t SqliteStatement::columnInt64(int column) const
{
    return sqlite3_column_int64(impl_->statement, column);
}

double SqliteStatement::columnDouble(int column) const
{
    return sqlite3_column_double(impl_->statement, column);
}

std::string SqliteStatement::columnText(int column) const
{
    const unsigned char* text = sqlite3_column_text(impl_->statement, column);
    const int size = sqlite3_column_bytes(impl_->statement, column);
    return text != nullptr ? std::string(reinterpret_cast<const char*>(text),
                                         static_cast<std::size_t>(size))
                           : std::string{};
}

std::vector<std::byte> SqliteStatement::columnBlob(int column) const
{
    const void* bytes = sqlite3_column_blob(impl_->statement, column);
    const int size = sqlite3_column_bytes(impl_->statement, column);
    if (bytes == nullptr || size <= 0) {
        return {};
    }
    const auto* first = static_cast<const std::byte*>(bytes);
    return std::vector<std::byte>(first, first + size);
}

// ---- SqliteDatabase ----------------------------------------------------------------

struct SqliteDatabase::Impl {
    sqlite3* connection = nullptr;
    ~Impl() { sqlite3_close(connection); }
};

SqliteDatabase::SqliteDatabase(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}
SqliteDatabase::SqliteDatabase(SqliteDatabase&&) noexcept = default;
SqliteDatabase& SqliteDatabase::operator=(SqliteDatabase&&) noexcept = default;
SqliteDatabase::~SqliteDatabase() = default;

Result<SqliteDatabase> SqliteDatabase::open(const std::filesystem::path& path, OpenMode mode)
{
    int flags = SQLITE_OPEN_FULLMUTEX;
    switch (mode) {
    case OpenMode::ReadWriteCreate:
        flags |= SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE;
        break;
    case OpenMode::ReadWrite:
        flags |= SQLITE_OPEN_READWRITE;
        break;
    case OpenMode::ReadOnly:
        flags |= SQLITE_OPEN_READONLY;
        break;
    }

    auto impl = std::make_unique<Impl>();
    const std::string utf8Path = toUtf8(path);
    if (sqlite3_open_v2(utf8Path.c_str(), &impl->connection, flags, nullptr) != SQLITE_OK) {
        auto error = sqliteError(impl->connection, "open the database");
        error.context += " path=" + utf8Path;
        return error;
    }
    sqlite3_extended_result_codes(impl->connection, 1);
    sqlite3_busy_timeout(impl->connection, 5000);

    SqliteDatabase database(std::move(impl));
    if (mode != OpenMode::ReadOnly) {
        // FULL: a committed save survives power loss. Rollback journal rather than
        // WAL so projects behave on network shares and stay a single file at rest.
        if (auto status = database.execute(
                "PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL; PRAGMA journal_mode = DELETE;");
            !status) {
            return status.error();
        }
    }
    return database;
}

Result<SqliteDatabase> SqliteDatabase::openInMemory()
{
    return open(":memory:");
}

Status SqliteDatabase::execute(std::string_view sql)
{
    const std::string text(sql);
    char* message = nullptr;
    if (sqlite3_exec(impl_->connection, text.c_str(), nullptr, nullptr, &message) != SQLITE_OK) {
        std::string detail = message != nullptr ? message : "unknown error";
        sqlite3_free(message);
        return makeError(ErrorCode::DatabaseFailure, "SQLite failed to execute SQL",
                         std::move(detail));
    }
    return {};
}

Result<SqliteStatement> SqliteDatabase::prepare(std::string_view sql)
{
    auto impl = std::make_unique<SqliteStatement::Impl>();
    impl->connection = impl_->connection;
    if (sqlite3_prepare_v2(impl_->connection, sql.data(), static_cast<int>(sql.size()),
                           &impl->statement, nullptr) != SQLITE_OK) {
        return sqliteError(impl_->connection, "prepare a statement");
    }
    return SqliteStatement(std::move(impl));
}

Status SqliteDatabase::begin()
{
    return execute("BEGIN IMMEDIATE");
}

Status SqliteDatabase::commit()
{
    return execute("COMMIT");
}

Status SqliteDatabase::rollback()
{
    return execute("ROLLBACK");
}

namespace {

Result<std::int64_t> queryInteger(SqliteDatabase& database, std::string_view sql)
{
    auto statement = database.prepare(sql);
    if (!statement) {
        return statement.error();
    }
    const auto row = statement->step();
    if (!row) {
        return row.error();
    }
    if (!*row) {
        return makeError(ErrorCode::DatabaseFailure, "query returned no row", std::string(sql));
    }
    return statement->columnInt64(0);
}

} // namespace

Result<int> SqliteDatabase::userVersion()
{
    const auto value = queryInteger(*this, "PRAGMA user_version");
    if (!value) {
        return value.error();
    }
    return static_cast<int>(*value);
}

Status SqliteDatabase::setUserVersion(int version)
{
    return execute("PRAGMA user_version = " + std::to_string(version));
}

Result<std::int64_t> SqliteDatabase::applicationId()
{
    return queryInteger(*this, "PRAGMA application_id");
}

Status SqliteDatabase::setApplicationId(std::int32_t id)
{
    return execute("PRAGMA application_id = " + std::to_string(id));
}

Result<bool> SqliteDatabase::tableExists(std::string_view name)
{
    auto statement =
        prepare("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1");
    if (!statement) {
        return statement.error();
    }
    if (auto status = statement->bind(1, name); !status) {
        return status.error();
    }
    const auto row = statement->step();
    if (!row) {
        return row.error();
    }
    return *row && statement->columnInt64(0) > 0;
}

Result<std::optional<std::string>> SqliteDatabase::integrityProblems()
{
    auto statement = prepare("PRAGMA integrity_check");
    if (!statement) {
        return statement.error();
    }
    std::string problems;
    while (true) {
        const auto row = statement->step();
        if (!row) {
            return row.error();
        }
        if (!*row) {
            break;
        }
        const std::string line = statement->columnText(0);
        if (line != "ok") {
            problems += problems.empty() ? "" : "; ";
            problems += line;
        }
    }
    if (problems.empty()) {
        return std::optional<std::string>{};
    }
    return std::optional<std::string>{std::move(problems)};
}

Status SqliteDatabase::backupTo(const std::filesystem::path& destination)
{
    auto target = open(destination);
    if (!target) {
        return target.error();
    }
    sqlite3_backup* backup =
        sqlite3_backup_init(target->impl_->connection, "main", impl_->connection, "main");
    if (backup == nullptr) {
        return sqliteError(target->impl_->connection, "start a backup");
    }
    const int stepCode = sqlite3_backup_step(backup, -1); // -1: copy everything in one pass
    const int finishCode = sqlite3_backup_finish(backup);
    if (stepCode != SQLITE_DONE || finishCode != SQLITE_OK) {
        return sqliteError(target->impl_->connection, "complete a backup");
    }
    return {};
}

// ---- SqliteTransaction -------------------------------------------------------------

Result<SqliteTransaction> SqliteTransaction::begin(SqliteDatabase& database)
{
    if (auto status = database.begin(); !status) {
        return status.error();
    }
    return SqliteTransaction(database);
}

SqliteTransaction::SqliteTransaction(SqliteTransaction&& other) noexcept
    : database_(std::exchange(other.database_, nullptr))
{
}

SqliteTransaction::~SqliteTransaction()
{
    if (database_ != nullptr) {
        (void)database_->rollback();
    }
}

Status SqliteTransaction::commit()
{
    if (database_ == nullptr) {
        return makeError(ErrorCode::InvalidState, "transaction is no longer active");
    }
    auto status = database_->commit();
    if (status) {
        database_ = nullptr;
    }
    return status;
}

} // namespace katana::storage
