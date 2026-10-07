// STUB. Task T7 writes the journal of include/katana/update/journal.hpp. The
// constructor is real (it only stores its arguments); everything that touches
// the format or the disk is T7's.

#include "katana/update/journal.hpp"

#include <utility>

#include "not_implemented.hpp"

namespace katana::update {

std::string_view toString(JournalOp)
{
    detail::notYet("toString(JournalOp)", "T7");
}

core::Result<JournalOp> parseJournalOp(std::string_view)
{
    return detail::notImplemented("parseJournalOp", "T7");
}

bool needsRecovery(JournalState)
{
    detail::notYet("needsRecovery", "T7");
}

std::uint32_t crc32(std::string_view)
{
    detail::notYet("crc32", "T7");
}

std::string encodeRecord(std::uint64_t, const JournalRecord&)
{
    detail::notYet("encodeRecord", "T7");
}

ParsedJournal parseJournal(std::string_view)
{
    detail::notYet("parseJournal", "T7");
}

Journal::Journal(FsOps& fs, std::filesystem::path path) : fs_(fs), path_(std::move(path)) {}

core::Status Journal::append(std::span<const JournalRecord>)
{
    return detail::notImplemented("Journal::append", "T7");
}

core::Result<ParsedJournal> Journal::load() const
{
    return detail::notImplemented("Journal::load", "T7");
}

} // namespace katana::update
