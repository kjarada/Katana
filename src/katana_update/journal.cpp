// The apply's journal (include/katana/update/journal.hpp): the record format, the reader
// that stops at the first line a crash could have torn, and the writer that goes through
// FsOps so the fault loops see every write.

#include "katana/update/journal.hpp"

#include <array>
#include <cstddef>
#include <utility>

namespace katana::update {

namespace {

constexpr std::array<std::pair<JournalOp, std::string_view>, 12> kOperations{{
    {JournalOp::Plan, "PLAN"},
    {JournalOp::Add, "ADD"},
    {JournalOp::Replace, "REPLACE"},
    {JournalOp::Remove, "REMOVE"},
    {JournalOp::MakeDirectory, "MKDIR"},
    {JournalOp::Committing, "COMMITTING"},
    {JournalOp::MovedOld, "MOVED_OLD"},
    {JournalOp::MovedNew, "MOVED_NEW"},
    {JournalOp::CommittedPendingHealth, "COMMITTED_PENDING_HEALTH"},
    {JournalOp::RollingBack, "ROLLING_BACK"},
    {JournalOp::RolledBack, "ROLLED_BACK"},
    {JournalOp::Healthy, "HEALTHY"},
}};

// The reflected polynomial of IEEE 802.3, 0xEDB88320: the CRC-32 of zlib, gzip and PNG.
constexpr std::array<std::uint32_t, 256> makeCrcTable()
{
    std::array<std::uint32_t, 256> table{};
    for (std::uint32_t i = 0; i < 256; ++i) {
        std::uint32_t value = i;
        for (int bit = 0; bit < 8; ++bit) {
            value = (value & 1U) != 0 ? (0xEDB88320U ^ (value >> 1)) : (value >> 1);
        }
        table[i] = value;
    }
    return table;
}

constexpr std::array<std::uint32_t, 256> kCrcTable = makeCrcTable();

constexpr std::string_view kHexUpper = "0123456789ABCDEF";
constexpr std::string_view kHexLower = "0123456789abcdef";

bool isSafeByte(char c)
{
    return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') ||
           c == '.' || c == '_' || c == '/' || c == '~' || c == '-';
}

std::string encodeArgument(std::string_view argument)
{
    std::string out;
    out.reserve(argument.size());
    for (const char c : argument) {
        if (isSafeByte(c)) {
            out.push_back(c);
        } else {
            const auto byte = static_cast<unsigned char>(c);
            out.push_back('%');
            out.push_back(kHexUpper[byte >> 4]);
            out.push_back(kHexUpper[byte & 0x0F]);
        }
    }
    return out;
}

int hexValue(char c)
{
    if (c >= '0' && c <= '9') {
        return c - '0';
    }
    if (c >= 'A' && c <= 'F') {
        return c - 'A' + 10;
    }
    if (c >= 'a' && c <= 'f') {
        return c - 'a' + 10;
    }
    return -1;
}

// False for a '%' not followed by two hex digits: such a token was not written by
// encodeArgument, so the line is not one of ours.
bool decodeArgument(std::string_view token, std::string& out)
{
    out.clear();
    out.reserve(token.size());
    for (std::size_t i = 0; i < token.size(); ++i) {
        if (token[i] != '%') {
            out.push_back(token[i]);
            continue;
        }
        if (i + 2 >= token.size()) {
            return false;
        }
        const int high = hexValue(token[i + 1]);
        const int low = hexValue(token[i + 2]);
        if (high < 0 || low < 0) {
            return false;
        }
        out.push_back(static_cast<char>(high * 16 + low));
        i += 2;
    }
    return true;
}

std::string crcText(std::string_view body)
{
    const std::uint32_t crc = crc32(body);
    std::string text(8, '0');
    for (int digit = 0; digit < 8; ++digit) {
        text[static_cast<std::size_t>(7 - digit)] = kHexLower[(crc >> (4 * digit)) & 0x0F];
    }
    return text;
}

bool isDecimal(std::string_view text)
{
    if (text.empty()) {
        return false;
    }
    for (const char c : text) {
        if (c < '0' || c > '9') {
            return false;
        }
    }
    return true;
}

// One complete line (without its LF) -> a record, or false when it is not a valid next
// record of the journal.
bool parseLine(std::string_view line, std::uint64_t expectedNumber, JournalRecord& record)
{
    const std::size_t lastSpace = line.rfind(' ');
    if (lastSpace == std::string_view::npos) {
        return false;
    }
    const std::string_view body = line.substr(0, lastSpace);
    const std::string_view crc = line.substr(lastSpace + 1);
    if (crc.size() != 8 || crc != crcText(body)) {
        return false;
    }

    // Tokens split on single spaces, empty ones kept: an empty argument is "%"-free
    // nothing between two spaces.
    std::vector<std::string_view> tokens;
    std::size_t start = 0;
    while (true) {
        const std::size_t space = body.find(' ', start);
        if (space == std::string_view::npos) {
            tokens.push_back(body.substr(start));
            break;
        }
        tokens.push_back(body.substr(start, space - start));
        start = space + 1;
    }
    if (tokens.size() < 2 || !isDecimal(tokens[0]) ||
        tokens[0] != std::to_string(expectedNumber)) {
        return false;
    }
    const core::Result<JournalOp> op = parseJournalOp(tokens[1]);
    if (!op.ok()) {
        return false;
    }
    record.op = op.value();
    record.args.clear();
    for (std::size_t i = 2; i < tokens.size(); ++i) {
        std::string argument;
        if (!decodeArgument(tokens[i], argument)) {
            return false;
        }
        record.args.push_back(std::move(argument));
    }
    return true;
}

JournalState stateAfter(JournalState current, JournalOp op)
{
    switch (op) {
    case JournalOp::Plan:
        return JournalState::Planned;
    case JournalOp::Committing:
        return JournalState::Committing;
    case JournalOp::CommittedPendingHealth:
        return JournalState::CommittedPendingHealth;
    case JournalOp::RollingBack:
        return JournalState::RollingBack;
    case JournalOp::RolledBack:
        return JournalState::RolledBack;
    case JournalOp::Healthy:
        return JournalState::Healthy;
    case JournalOp::Add:
    case JournalOp::Replace:
    case JournalOp::Remove:
    case JournalOp::MakeDirectory:
    case JournalOp::MovedOld:
    case JournalOp::MovedNew:
        return current;
    }
    return current;
}

} // namespace

std::string_view toString(JournalOp op)
{
    for (const auto& [value, name] : kOperations) {
        if (value == op) {
            return name;
        }
    }
    return "UNKNOWN";
}

core::Result<JournalOp> parseJournalOp(std::string_view text)
{
    for (const auto& [value, name] : kOperations) {
        if (name == text) {
            return value;
        }
    }
    return core::makeError(core::ErrorCode::ParseFailure, "not a journal operation",
                           std::string(text));
}

bool needsRecovery(JournalState state)
{
    return state == JournalState::Planned || state == JournalState::Committing ||
           state == JournalState::RollingBack;
}

std::uint32_t crc32(std::string_view bytes)
{
    std::uint32_t crc = 0xFFFFFFFFU;
    for (const char c : bytes) {
        crc = kCrcTable[(crc ^ static_cast<unsigned char>(c)) & 0xFFU] ^ (crc >> 8);
    }
    return crc ^ 0xFFFFFFFFU;
}

std::string encodeRecord(std::uint64_t n, const JournalRecord& record)
{
    std::string line = std::to_string(n);
    line += ' ';
    line += toString(record.op);
    for (const std::string& argument : record.args) {
        line += ' ';
        line += encodeArgument(argument);
    }
    const std::string crc = crcText(line);
    line += ' ';
    line += crc;
    line += '\n';
    return line;
}

ParsedJournal parseJournal(std::string_view text)
{
    ParsedJournal parsed;
    std::size_t position = 0;
    while (position < text.size()) {
        const std::size_t end = text.find('\n', position);
        if (end == std::string_view::npos) {
            break; // no LF: the write that was torn
        }
        JournalRecord record;
        if (!parseLine(text.substr(position, end - position), parsed.records.size() + 1, record)) {
            break;
        }
        parsed.state = stateAfter(parsed.state, record.op);
        parsed.records.push_back(std::move(record));
        position = end + 1;
    }
    parsed.tornTail = position < text.size();
    return parsed;
}

Journal::Journal(FsOps& fs, std::filesystem::path path) : fs_(fs), path_(std::move(path)) {}

core::Status Journal::append(std::span<const JournalRecord> group)
{
    if (group.empty()) {
        return {};
    }
    // Learn where the journal stands the first time, because a crash may have left a torn
    // last line: records written after it would be unreadable (parsing stops at the first
    // bad line), so a journal with a torn tail is rewritten from its good prefix instead.
    std::string rewritten;
    bool rewrite = false;
    std::uint64_t next = next_;
    if (next == 0) {
        const core::Result<ParsedJournal> existing = load();
        if (!existing.ok()) {
            return existing.error();
        }
        next = existing.value().records.size() + 1;
        if (existing.value().tornTail) {
            rewrite = true;
            std::uint64_t number = 1;
            for (const JournalRecord& record : existing.value().records) {
                rewritten += encodeRecord(number++, record);
            }
        }
    }
    std::string bytes;
    for (const JournalRecord& record : group) {
        bytes += encodeRecord(next++, record);
    }
    const core::Status written = rewrite ? fs_.writeDurable(path_, rewritten + bytes)
                                         : fs_.appendDurable(path_, bytes);
    // The numbers move on only for a group that was written. One that failed may have left
    // part of its bytes behind, and what the file holds is then learned again by the next
    // append (which rewrites from the good prefix): carrying on from `next` would leave a gap
    // in the numbers, and the read stops at the first gap.
    next_ = written.ok() ? next : 0;
    return written;
}

core::Result<ParsedJournal> Journal::load() const
{
    const core::Result<std::string> bytes = fs_.readFile(path_);
    if (!bytes.ok()) {
        if (bytes.error().code == core::ErrorCode::NotFound) {
            return ParsedJournal{};
        }
        return bytes.error();
    }
    return parseJournal(bytes.value());
}

} // namespace katana::update
