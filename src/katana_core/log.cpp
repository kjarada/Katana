#include "katana/core/log.hpp"

#include <cstdio>
#include <ctime>
#include <fstream>
#include <iostream>

namespace katana::core {

std::string_view toString(LogLevel level)
{
    switch (level) {
    case LogLevel::Trace:
        return "TRACE";
    case LogLevel::Debug:
        return "DEBUG";
    case LogLevel::Info:
        return "INFO";
    case LogLevel::Warning:
        return "WARNING";
    case LogLevel::Error:
        return "ERROR";
    case LogLevel::Critical:
        return "CRITICAL";
    }
    return "UNKNOWN";
}

namespace {

std::string formatTimestamp(std::chrono::system_clock::time_point time)
{
    using namespace std::chrono;
    const auto sinceEpoch = time.time_since_epoch();
    const auto wholeSeconds = duration_cast<seconds>(sinceEpoch);
    const auto millis = duration_cast<milliseconds>(sinceEpoch - wholeSeconds).count();

    const std::time_t seconds = static_cast<std::time_t>(wholeSeconds.count());
    std::tm utc{};
#if defined(_WIN32)
    gmtime_s(&utc, &seconds);
#else
    gmtime_r(&seconds, &utc);
#endif

    // 64, not the 25 the text needs: GCC 15 sizes each %d for any int and
    // warns (format-truncation) that a 40-byte buffer could be too small.
    char buffer[64];
    std::snprintf(buffer, sizeof(buffer), "%04d-%02d-%02dT%02d:%02d:%02d.%03dZ",
                  utc.tm_year + 1900, utc.tm_mon + 1, utc.tm_mday, utc.tm_hour, utc.tm_min,
                  utc.tm_sec, static_cast<int>(millis));
    return buffer;
}

// Values containing whitespace, '=' or quotes are quoted so records stay parseable.
std::string formatFieldValue(const std::string& value)
{
    const bool needsQuotes = value.empty() || value.find_first_of(" \t\"=") != std::string::npos;
    if (!needsQuotes) {
        return value;
    }
    std::string quoted = "\"";
    for (const char ch : value) {
        if (ch == '"' || ch == '\\') {
            quoted += '\\';
        }
        quoted += ch;
    }
    quoted += '"';
    return quoted;
}

} // namespace

std::string formatRecord(const LogRecord& record)
{
    std::string line = formatTimestamp(record.time);
    line += ' ';
    line += toString(record.level);
    line += " [";
    line += record.category;
    line += "] ";
    line += record.message;
    for (const LogField& field : record.fields) {
        line += ' ';
        line += field.key;
        line += '=';
        line += formatFieldValue(field.value);
    }
    return line;
}

void Logger::addSink(Sink sink)
{
    const std::lock_guard lock(mutex_);
    sinks_.push_back(std::move(sink));
}

void Logger::setMinimumLevel(LogLevel level)
{
    const std::lock_guard lock(mutex_);
    minimumLevel_ = level;
}

LogLevel Logger::minimumLevel() const
{
    const std::lock_guard lock(mutex_);
    return minimumLevel_;
}

void Logger::log(LogLevel level, std::string_view category, std::string_view message,
                 std::initializer_list<LogField> fields)
{
    const std::lock_guard lock(mutex_);
    if (level < minimumLevel_ || sinks_.empty()) {
        return;
    }

    LogRecord record;
    record.time = std::chrono::system_clock::now();
    record.level = level;
    record.category = category;
    record.message = message;
    record.fields.assign(fields.begin(), fields.end());

    for (const Sink& sink : sinks_) {
        sink(record);
    }
}

Logger::Sink makeStderrSink()
{
    return [](const LogRecord& record) { std::cerr << formatRecord(record) << '\n'; };
}

Logger::Sink makeFileSink(std::string path)
{
    return [path = std::move(path)](const LogRecord& record) {
        std::ofstream out(path, std::ios::app);
        if (out) {
            out << formatRecord(record) << '\n';
        }
    };
}

} // namespace katana::core
