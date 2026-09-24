#pragma once

// Private to the RINEX reader: the column and line helpers the observation
// reader (rinex.cpp) and the navigation summary (rinex_navigation.cpp) share,
// so that "which columns hold the label" has one answer in both.
//
// RINEX is a Fortran-era fixed-column text format. Every header record carries
// its label in columns 61-80 and its content in columns 1-60; data records have
// their fields at fixed columns too. The helpers below take 1-based columns,
// the way the specifications number them, so a field here can be checked
// against the specification's table by eye.

#include <cstddef>
#include <cstring>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/error.hpp"
#include "katana/surveyio/format.hpp"

namespace katana::surveyio::rinex {

// Lines of a byte buffer, one at a time, without their terminators. LF and
// CRLF both end a line: RINEX files are written on every operating system and
// copied between them. (A lone CR is not a RINEX line end; it stays in the
// line as a stray byte, which the record checks then report.) memchr finds each
// end, which is what keeps the reader at memory speed on a 200 MB file.
class LineCursor {
  public:
    explicit LineCursor(std::string_view bytes) : bytes_(bytes) {}

    // The next line, or false at the end of the bytes.
    bool next(std::string_view& line)
    {
        if (position_ >= bytes_.size()) {
            return false;
        }
        const char* begin = bytes_.data() + position_;
        const std::size_t left = bytes_.size() - position_;
        const void* found = std::memchr(begin, '\n', left);
        std::size_t length = found != nullptr
                                 ? static_cast<std::size_t>(static_cast<const char*>(found) - begin)
                                 : left;
        position_ += found != nullptr ? length + 1 : length;
        if (length > 0 && begin[length - 1] == '\r') {
            --length;
        }
        line = std::string_view(begin, length);
        ++number_;
        return true;
    }

    // The 1-based number of the line next() returned last.
    [[nodiscard]] std::size_t lineNumber() const { return number_; }
    [[nodiscard]] bool atEnd() const { return position_ >= bytes_.size(); }

    // A saved place, to look at a line and step back.
    struct Mark {
        std::size_t position = 0;
        std::size_t number = 0;
    };
    [[nodiscard]] Mark mark() const { return {position_, number_}; }
    void reset(Mark mark)
    {
        position_ = mark.position;
        number_ = mark.number;
    }

  private:
    std::string_view bytes_;
    std::size_t position_ = 0;
    std::size_t number_ = 0;
};

// Columns `first` (1-based) to `first + width - 1` of `line`, clipped to the
// line: a record whose trailing blanks were removed (the specifications allow
// it) simply has shorter fields.
[[nodiscard]] inline std::string_view columns(std::string_view line, std::size_t first,
                                              std::size_t width)
{
    if (first == 0 || first > line.size()) {
        return {};
    }
    return line.substr(first - 1, width);
}

// The header label: columns 61-80, trailing blanks removed.
[[nodiscard]] std::string_view headerLabel(std::string_view line);
// The header content: columns 1-60.
[[nodiscard]] inline std::string_view headerContent(std::string_view line)
{
    return line.substr(0, 60);
}

// A Fortran I field: blanks around it allowed, nothing else. nullopt when blank
// or not an integer.
[[nodiscard]] std::optional<long long> integerField(std::string_view field);
// A Fortran F field (".9030", "-.353", "4375274.0000"). nullopt when blank or
// not a finite number. A 'D' exponent (Fortran double precision, common in
// navigation files) is read as 'E'.
[[nodiscard]] std::optional<double> realField(std::string_view field);

// `text` as something safe to keep and show: surrounding blanks trimmed,
// control characters replaced by blanks and, when the bytes are not valid
// UTF-8 (a header typed on an instrument with its own code page), every byte
// above 127 replaced by '?'. `changed` is set when anything but trimming
// happened, so the caller can say so once.
[[nodiscard]] std::string cleanText(std::string_view text, bool* changed = nullptr);

// Whitespace-separated tokens of `text`.
[[nodiscard]] std::vector<std::string_view> tokens(std::string_view text);

// The satellite system a RINEX system letter names, in the words
// survey::GnssSession::satellitesPerSystem is keyed by; nullptr for a letter
// that names none.
[[nodiscard]] const char* systemName(char letter);

// The first line of a RINEX file, read.
struct VersionRecord {
    std::string versionText; // "3.04", as written (trimmed)
    double version = 0.0;
    int major = 0;
    char fileType = ' ';  // column 21: 'O', 'N', 'G', 'M', ...
    char system = ' ';    // column 41: 'G', 'R', 'M', ... (blank in old files)
    std::string typeText; // columns 21-40, trimmed: "OBSERVATION DATA"
};

// nullopt when the line is not a RINEX VERSION / TYPE record.
[[nodiscard]] std::optional<VersionRecord> versionRecord(std::string_view firstLine);

// ---- Compression, which is refused rather than guessed at --------------------

enum class Packing {
    Plain,
    Gzip,         // 1F 8B: .gz
    UnixCompress, // 1F 9D: .Z, the classic IGS archive packing
    Bzip2,        // "BZh"
    Zip,          // "PK\3\4"
    Hatanaka,     // Compact RINEX ("CRINEX VERS   / TYPE"), .crx / .YYd
};

[[nodiscard]] Packing packingOf(std::string_view bytes);

// The sentence a person is told for a packed file: what it is and what to do.
[[nodiscard]] std::string packingRefusal(Packing packing, std::string_view fileName);

// ---- Navigation files -----------------------------------------------------------

// The navigation files that conventionally sit beside an observation file
// called `observationFileName`, most likely first: the short-name siblings
// (site2560.24o -> site2560.24n, .24g, ...), the long-name ones
// (..._01D_30S_MO.rnx -> ..._01D_MN.rnx, _GN, _RN, ...), or "stem.nav".
[[nodiscard]] std::vector<std::string>
navigationFileCandidates(std::string_view observationFileName);

// What a broadcast navigation file holds, counted rather than stored: without a
// positioning step the orbits themselves have no use in the survey model.
struct NavigationSummary {
    std::string fileName;
    std::string versionText;
    // Broadcast ephemeris records per system name ("GPS", "GLONASS", ...).
    std::map<std::string, std::size_t> ephemerides;
    std::size_t otherMessages = 0; // RINEX 4 STO / EOP / ION records
    bool ionosphereParameters = false;
    std::string leapSeconds;
    std::size_t lines = 0;
    std::vector<ReadWarning> warnings;
};

// FileImportFailure (naming the file) when `bytes` is not a RINEX navigation
// file at all; otherwise the summary, with a warning per unreadable record.
[[nodiscard]] katana::core::Result<NavigationSummary>
summariseNavigation(std::string_view bytes, std::string_view fileName);

} // namespace katana::surveyio::rinex
