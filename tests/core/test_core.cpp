#include <gtest/gtest.h>

#include <string>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/core/log.hpp"

using namespace katana::core;

namespace {

Result<int> parsePositive(int value)
{
    if (value <= 0) {
        return makeError(ErrorCode::InvalidArgument, "value must be positive",
                         "value=" + std::to_string(value));
    }
    return value;
}

} // namespace

TEST(CoreResult, HoldsValueOnSuccess)
{
    const Result<int> result = parsePositive(7);
    ASSERT_TRUE(result.ok());
    EXPECT_EQ(result.value(), 7);
    EXPECT_EQ(*result, 7);
    EXPECT_THROW((void)result.error(), BadResultAccess);
}

TEST(CoreResult, HoldsStructuredErrorOnFailure)
{
    const Result<int> result = parsePositive(-3);
    ASSERT_FALSE(result.ok());
    EXPECT_EQ(result.error().code, ErrorCode::InvalidArgument);
    EXPECT_EQ(result.error().describe(), "InvalidArgument: value must be positive [value=-3]");
    EXPECT_EQ(result.valueOr(42), 42);
    EXPECT_THROW((void)result.value(), BadResultAccess);
}

TEST(CoreResult, StatusDefaultsToSuccess)
{
    const Status okStatus;
    EXPECT_TRUE(okStatus.ok());

    const Status failed = makeError(ErrorCode::DatabaseFailure, "disk full");
    ASSERT_FALSE(failed.ok());
    EXPECT_EQ(failed.error().describe(), "DatabaseFailure: disk full");
}

TEST(CoreResult, EveryErrorCodeHasAName)
{
    for (int raw = 0; raw <= static_cast<int>(ErrorCode::Internal); ++raw) {
        EXPECT_NE(toString(static_cast<ErrorCode>(raw)), "Unknown") << "code " << raw;
    }
}

TEST(CoreLog, DeliversStructuredRecordsToSinks)
{
    Logger logger;
    std::vector<LogRecord> records;
    logger.addSink([&records](const LogRecord& record) { records.push_back(record); });

    logger.info("command", "executed", {{"name", "CREATE_LINE"}, {"entities", "1"}});

    ASSERT_EQ(records.size(), 1u);
    EXPECT_EQ(records[0].level, LogLevel::Info);
    EXPECT_EQ(records[0].category, "command");
    ASSERT_EQ(records[0].fields.size(), 2u);
    EXPECT_EQ(records[0].fields[0].key, "name");
    EXPECT_EQ(records[0].fields[0].value, "CREATE_LINE");
}

TEST(CoreLog, FiltersBelowMinimumLevel)
{
    Logger logger;
    int delivered = 0;
    logger.addSink([&delivered](const LogRecord&) { ++delivered; });

    logger.debug("storage", "ignored at default Info level");
    EXPECT_EQ(delivered, 0);

    logger.setMinimumLevel(LogLevel::Trace);
    logger.trace("storage", "now visible");
    logger.critical("storage", "always visible");
    EXPECT_EQ(delivered, 2);
}

TEST(CoreLog, FormatsRecordWithQuotedFieldValues)
{
    LogRecord record;
    record.time = std::chrono::system_clock::time_point{}; // Unix epoch
    record.level = LogLevel::Warning;
    record.category = "import";
    record.message = "skipped rows";
    record.fields = {{"file", "site survey.csv"}, {"rows", "3"}};

    EXPECT_EQ(formatRecord(record), "1970-01-01T00:00:00.000Z WARNING [import] skipped rows "
                                    "file=\"site survey.csv\" rows=3");
}
