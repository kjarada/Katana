#pragma once

// Minimal RAII wrapper around SQLite (PLAN.MD Phase 07, Rule 4).
//
// sqlite3.h is included only by the implementation; no SQLite type appears
// here. Every call reports failures as DatabaseFailure with SQLite's own
// message. A connection and its statements belong to one thread.

#include <cstdint>
#include <filesystem>
#include <memory>
#include <optional>
#include <cstddef>
#include <span>
#include <string>
#include <vector>
#include <string_view>

#include "katana/core/error.hpp"

namespace katana::storage {

class SqliteDatabase;

// A prepared statement. Parameter indices start at 1, column indices at 0.
class SqliteStatement {
  public:
    SqliteStatement(SqliteStatement&&) noexcept;
    SqliteStatement& operator=(SqliteStatement&&) noexcept;
    ~SqliteStatement();

    [[nodiscard]] katana::core::Status bind(int index, std::int64_t value);
    [[nodiscard]] katana::core::Status bind(int index, double value);
    [[nodiscard]] katana::core::Status bind(int index, std::string_view value);
    [[nodiscard]] katana::core::Status bind(int index, bool value);
    // Binary column. SQLite copies the bytes, so `value` need not outlive this.
    [[nodiscard]] katana::core::Status bind(int index, std::span<const std::byte> value);
    [[nodiscard]] katana::core::Status bindNull(int index);

    // true: a row is available. false: the statement finished.
    [[nodiscard]] katana::core::Result<bool> step();
    // Runs a statement that returns no rows, then resets it for reuse.
    [[nodiscard]] katana::core::Status run();
    void reset();

    [[nodiscard]] bool columnIsNull(int column) const;
    [[nodiscard]] std::int64_t columnInt64(int column) const;
    [[nodiscard]] double columnDouble(int column) const;
    [[nodiscard]] std::string columnText(int column) const;
    // Empty for a NULL column or a zero-length blob; columnIsNull separates
    // the two, which matters because a missing geometry and an empty one are
    // different failures.
    [[nodiscard]] std::vector<std::byte> columnBlob(int column) const;

    // BORROWED views of the same two columns, for a value that is parsed and
    // discarded within the row: they point straight at SQLite's own row buffer
    // and copy nothing, which is what makes loading 50k entities not allocate a
    // string and a vector per row.
    //
    // LIFETIME. SQLite owns the bytes. A view is invalidated by the next
    // step(), reset() or run() on the statement, by destroying it, and by
    // reading the SAME column through a different accessor (SQLite converts the
    // value in place). So: take the view, consume it, move on - never store one
    // past the row it came from, and take columnText()/columnBlob() when the
    // value has to outlive the row.
    [[nodiscard]] std::string_view columnTextView(int column) const;
    [[nodiscard]] std::span<const std::byte> columnBlobSpan(int column) const;

  private:
    friend class SqliteDatabase;
    struct Impl;
    explicit SqliteStatement(std::unique_ptr<Impl> impl);
    std::unique_ptr<Impl> impl_;
};

class SqliteDatabase {
  public:
    enum class OpenMode { ReadWriteCreate, ReadWrite, ReadOnly };

    [[nodiscard]] static katana::core::Result<SqliteDatabase>
    open(const std::filesystem::path& path, OpenMode mode = OpenMode::ReadWriteCreate);
    [[nodiscard]] static katana::core::Result<SqliteDatabase> openInMemory();

    SqliteDatabase(SqliteDatabase&&) noexcept;
    SqliteDatabase& operator=(SqliteDatabase&&) noexcept;
    ~SqliteDatabase();

    // Runs one or more statements that return no rows.
    [[nodiscard]] katana::core::Status execute(std::string_view sql);
    [[nodiscard]] katana::core::Result<SqliteStatement> prepare(std::string_view sql);

    [[nodiscard]] katana::core::Status begin(); // BEGIN IMMEDIATE
    [[nodiscard]] katana::core::Status commit();
    [[nodiscard]] katana::core::Status rollback();

    [[nodiscard]] katana::core::Result<int> userVersion();
    [[nodiscard]] katana::core::Status setUserVersion(int version);
    [[nodiscard]] katana::core::Result<std::int64_t> applicationId();
    [[nodiscard]] katana::core::Status setApplicationId(std::int32_t id);
    [[nodiscard]] katana::core::Result<bool> tableExists(std::string_view name);

    // Empty optional when the file is sound, otherwise SQLite's description of
    // the damage (PRAGMA integrity_check).
    [[nodiscard]] katana::core::Result<std::optional<std::string>> integrityProblems();

    // Consistent snapshot through SQLite's online backup API; safe while the
    // connection is in use.
    [[nodiscard]] katana::core::Status backupTo(const std::filesystem::path& destination);

  private:
    struct Impl;
    explicit SqliteDatabase(std::unique_ptr<Impl> impl);
    std::unique_ptr<Impl> impl_;
};

// Rolls the transaction back unless commit() succeeded.
class SqliteTransaction {
  public:
    [[nodiscard]] static katana::core::Result<SqliteTransaction> begin(SqliteDatabase& database);
    SqliteTransaction(SqliteTransaction&& other) noexcept;
    SqliteTransaction& operator=(SqliteTransaction&&) = delete;
    ~SqliteTransaction();

    [[nodiscard]] katana::core::Status commit();

  private:
    explicit SqliteTransaction(SqliteDatabase& database) : database_(&database) {}
    SqliteDatabase* database_;
};

} // namespace katana::storage
