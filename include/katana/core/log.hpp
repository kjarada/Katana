#pragma once

// Structured logging (PLAN.MD section 37).
//
// A Logger is an ordinary object that is passed to the subsystems that need it;
// there is no hidden global logger. Records carry a level, a category (the
// subsystem: "command", "storage", "survey", ...), a message and key/value
// fields. Sinks decide how to render them.
//
// Threading: log() may be called concurrently; sink invocation is serialised.
// Privacy: callers log identifiers and counts, never coordinate payloads.

#include <chrono>
#include <functional>
#include <initializer_list>
#include <mutex>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace katana::core {

enum class LogLevel { Trace, Debug, Info, Warning, Error, Critical };

[[nodiscard]] std::string_view toString(LogLevel level);

struct LogField {
    std::string key;
    std::string value;
};

struct LogRecord {
    std::chrono::system_clock::time_point time;
    LogLevel level = LogLevel::Info;
    std::string category;
    std::string message;
    std::vector<LogField> fields;
};

// Renders `2026-09-19T10:15:30.123Z INFO  [command] message key=value ...`.
[[nodiscard]] std::string formatRecord(const LogRecord& record);

class Logger {
  public:
    using Sink = std::function<void(const LogRecord&)>;

    Logger() = default;
    Logger(const Logger&) = delete;
    Logger& operator=(const Logger&) = delete;

    void addSink(Sink sink);
    void setMinimumLevel(LogLevel level);
    [[nodiscard]] LogLevel minimumLevel() const;

    void log(LogLevel level, std::string_view category, std::string_view message,
             std::initializer_list<LogField> fields = {});

    void trace(std::string_view category, std::string_view message,
               std::initializer_list<LogField> fields = {})
    {
        log(LogLevel::Trace, category, message, fields);
    }
    void debug(std::string_view category, std::string_view message,
               std::initializer_list<LogField> fields = {})
    {
        log(LogLevel::Debug, category, message, fields);
    }
    void info(std::string_view category, std::string_view message,
              std::initializer_list<LogField> fields = {})
    {
        log(LogLevel::Info, category, message, fields);
    }
    void warning(std::string_view category, std::string_view message,
                 std::initializer_list<LogField> fields = {})
    {
        log(LogLevel::Warning, category, message, fields);
    }
    void error(std::string_view category, std::string_view message,
               std::initializer_list<LogField> fields = {})
    {
        log(LogLevel::Error, category, message, fields);
    }
    void critical(std::string_view category, std::string_view message,
                  std::initializer_list<LogField> fields = {})
    {
        log(LogLevel::Critical, category, message, fields);
    }

  private:
    mutable std::mutex mutex_;
    std::vector<Sink> sinks_;
    LogLevel minimumLevel_ = LogLevel::Info;
};

// Sink writing formatted records to stderr.
[[nodiscard]] Logger::Sink makeStderrSink();

// Sink appending formatted records to a file. The file is opened per record so
// that a crash never loses buffered lines; intended for low-volume logs.
[[nodiscard]] Logger::Sink makeFileSink(std::string path);

} // namespace katana::core
