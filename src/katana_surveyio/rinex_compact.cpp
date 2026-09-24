// Compact RINEX (Hatanaka compression) expanded back to the RINEX observation
// file it stands for, so that the observation reader (rinex.cpp) reads it
// exactly as it reads a plain file and every check it makes still applies.
//
// Specification followed: Y. Hatanaka, "A Compression Format and Tools for GNSS
// Observation Data", Bulletin of the Geographical Survey Institute, vol. 55,
// March 2008 - section 2 (the rules, and where Compact RINEX 1.0, for RINEX 2,
// differs from 3.0, for RINEX 3 and 4), appendix 1 (the recovering operation,
// equation 3), appendix 2 tables A-1 to A-3 (the records) and appendix 3 (an
// example, which the tests read).
//
// What a compact file holds:
//   * CRINEX VERS / TYPE and CRINEX PROG / DATE, then the RINEX header as it
//     was;
//   * per epoch, an epoch line: the RINEX epoch record's first 41 columns
//     (3.0) or 32 (1.0) followed by the whole satellite list on the one line,
//     TEXT-DIFFERENCED against the previous epoch line - a blank keeps the old
//     character, '&' makes it a blank, anything else replaces it. A line
//     starting '>' (3.0) or '&' (1.0) is given in full instead;
//   * the receiver clock offset on a line of its own (empty when the epoch has
//     none), as an integer in 1e-12 s (3.0) or 1e-9 s (1.0);
//   * one line per satellite: a field per observation type, separated by
//     single blanks - "M&v" starts a series of differencing order M at the
//     integer v, a bare integer is the series' next M-th difference, an empty
//     field is a blank observation - then the LLI and signal-strength flags of
//     every type as one text-differenced string. Observations are integers in
//     thousandths (F14.3 without its point), recovered by equation 3:
//     Y(m-1)_i = Y(m-1)_(i-1) + Y(m)_i, and so on down to Y(0)_i;
//   * an epoch with event flag 2 to 6 as it is, its special records after it
//     unchanged, and every series starting afresh at the next epoch (table 2).
// Where 1.0 differs (section 2.1): the flags of an observation type restart
// with that type's series rather than all together, and have no '&' for their
// blanks when they do.
//
// Why expand to text rather than count straight from the compact records: the
// observation reader's record checks, event handling and warnings then apply
// to a compact file unchanged, and what the two paths report cannot drift
// apart. The price is holding the expanded text while it is read - about three
// times the compact file - which the size cap bounds.
//
// A compact file is differential: once a line is damaged, nothing after it can
// be recovered until every series starts afresh (section 2.4). Damage is
// therefore reported once, with the lines it spoils, and expansion resumes at
// the next epoch that stands on its own.

#include <algorithm>
#include <array>
#include <charconv>
#include <cstdint>
#include <limits>
#include <string>
#include <string_view>
#include <vector>

#include "katana/core/text.hpp"
#include "rinex_internal.hpp"

namespace katana::surveyio::rinex {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

// ---- Line map ------------------------------------------------------------------------

void LineMap::add(std::size_t expandedFirst, std::size_t compactFirst,
                  std::size_t expandedPerCompact)
{
    if (!runs_.empty()) {
        const Run& last = runs_.back();
        if (last.expandedPerCompact == expandedPerCompact && expandedFirst >= last.expandedFirst) {
            const std::size_t offset = expandedFirst - last.expandedFirst;
            const bool predicted =
                expandedPerCompact == 0
                    ? compactFirst == last.compactFirst
                    : offset % expandedPerCompact == 0 &&
                          last.compactFirst + offset / expandedPerCompact == compactFirst;
            if (predicted) {
                return;
            }
        }
    }
    runs_.push_back(Run{expandedFirst, compactFirst, expandedPerCompact});
}

std::size_t LineMap::compactLine(std::size_t expanded) const
{
    if (expanded == 0 || runs_.empty()) {
        return expanded;
    }
    auto after = std::upper_bound(runs_.begin(), runs_.end(), expanded,
                                  [](std::size_t line, const Run& run) {
                                      return line < run.expandedFirst;
                                  });
    if (after == runs_.begin()) {
        return expanded;
    }
    const Run& run = *(after - 1);
    if (run.expandedPerCompact == 0) {
        return run.compactFirst;
    }
    return run.compactFirst + (expanded - run.expandedFirst) / run.expandedPerCompact;
}

namespace {

// Table A-2 allows orders up to 9; the published compressor writes 3.
constexpr int kMaxOrder = 9;
// Satellite slots: a system letter and a two-digit number.
constexpr std::size_t kSatelliteSlots = 26 * 100;
// More observation types than any receiver records (RINEX 3.05 has about 150
// codes across all systems); a bound so a damaged header cannot ask for
// gigabytes of series.
constexpr int kMostObservationTypes = 200;

bool isDigit(char c)
{
    return c >= '0' && c <= '9';
}

std::string_view trimmedRight(std::string_view text)
{
    while (!text.empty() && text.back() == ' ') {
        text.remove_suffix(1);
    }
    return text;
}

// Section 2.2: a blank keeps the old character, '&' makes it a blank, anything
// else replaces it; a difference longer than the old text extends it.
void applyTextDifference(std::string& text, std::string_view difference)
{
    const std::size_t common = std::min(text.size(), difference.size());
    for (std::size_t i = 0; i < common; ++i) {
        const char c = difference[i];
        if (c != ' ') {
            text[i] = c == '&' ? ' ' : c;
        }
    }
    for (std::size_t i = common; i < difference.size(); ++i) {
        text.push_back(difference[i] == '&' ? ' ' : difference[i]);
    }
}

// An integer as a Fortran F field of `width` with `decimals` places, right
// aligned: 1234567 with 3 decimals is "1234.567", -353 is "-.353" - without
// the zero before the point, as Fortran writes it and as the published
// CRX2RNX restores it, so an expansion matches that tool's byte for byte.
// It is written into the `width` characters at `field`, which hold blanks,
// digits laid down from the right, so no text is built and copied. False when
// it does not fit, which a value that came out of an F14.3 field never does.
// Digits are written two at a time: the chain of divisions is most of what
// writing a value costs, and pairs halve it.
constexpr std::array<char, 200> kDigitPairs = [] {
    std::array<char, 200> pairs{};
    for (std::size_t i = 0; i < 100; ++i) {
        pairs[2 * i] = static_cast<char>('0' + i / 10);
        pairs[2 * i + 1] = static_cast<char>('0' + i % 10);
    }
    return pairs;
}();

bool writeFixed(char* field, std::size_t width, std::int64_t value, int decimals)
{
    const bool negative = value < 0;
    std::uint64_t magnitude = negative ? std::uint64_t{0} - static_cast<std::uint64_t>(value)
                                       : static_cast<std::uint64_t>(value);
    const auto writePair = [&](char*& at) {
        const std::size_t pair = static_cast<std::size_t>(magnitude % 100);
        magnitude /= 100;
        at -= 2;
        at[0] = kDigitPairs[2 * pair];
        at[1] = kDigitPairs[2 * pair + 1];
    };
    char* at = field + width;
    int left = decimals;
    for (; left >= 2; left -= 2) {
        writePair(at);
    }
    if (left == 1) {
        *--at = static_cast<char>('0' + magnitude % 10);
        magnitude /= 10;
    }
    *--at = '.';
    while (magnitude >= 10) {
        if (at - field < 2) {
            return false;
        }
        writePair(at);
    }
    if (magnitude != 0) {
        if (at == field) {
            return false;
        }
        *--at = static_cast<char>('0' + magnitude);
    }
    if (negative) {
        if (at == field) {
            return false;
        }
        *--at = '-';
    }
    return true;
}

// One numeric data series (table 1, categories B and C): the state vector of
// appendix 1 - the last value and its differences up to the series' order.
struct Series {
    std::array<std::int64_t, kMaxOrder + 1> state{};
    int order = 0;
    int values = 0; // values since the series started, saturating at `order`
    bool active = false;

    void start(int maximumOrder, std::int64_t value)
    {
        state[0] = value;
        order = maximumOrder;
        values = 1;
        active = true;
    }

    // Equation 3: this epoch's M-th difference back to its value. False on an
    // overflow, which only a damaged file produces.
    bool advance(std::int64_t difference)
    {
        const int m = std::min(values, order);
        state[static_cast<std::size_t>(m)] = difference;
        for (int j = m - 1; j >= 0; --j) {
            const auto at = static_cast<std::size_t>(j);
            if (__builtin_add_overflow(state[at], state[at + 1], &state[at])) {
                return false;
            }
        }
        if (values < order) {
            ++values;
        }
        return true;
    }
};

// A numeric field read: blank, a start ("M&v") or a difference.
struct NumericField {
    enum class Kind { Blank, Start, Difference, Unreadable } kind = Kind::Blank;
    int order = 0;
    std::int64_t value = 0;
};

NumericField readNumericField(std::string_view field)
{
    NumericField read;
    if (field.empty()) {
        return read;
    }
    std::string_view number = field;
    if (field.size() >= 2 && field[1] == '&') {
        if (!isDigit(field[0])) {
            read.kind = NumericField::Kind::Unreadable;
            return read;
        }
        read.order = field[0] - '0';
        number = field.substr(2);
        read.kind = NumericField::Kind::Start;
    } else {
        read.kind = NumericField::Kind::Difference;
    }
    const char* end = number.data() + number.size();
    const auto [stop, error] = std::from_chars(number.data(), end, read.value);
    if (number.empty() || error != std::errc{} || stop != end) {
        read.kind = NumericField::Kind::Unreadable;
    }
    return read;
}

constexpr std::uint8_t kContinued = 0;
constexpr std::uint8_t kStarted = 1;
constexpr std::uint8_t kBlank = 2;

struct SatelliteState {
    std::uint64_t lastEpoch = 0; // the epoch it was last expanded in; 0 = never
    std::vector<Series> series;
    std::string flags; // LLI and signal strength, two characters per type
};

class Expander {
  public:
    Expander(std::string_view bytes, std::string_view fileName, std::size_t maxBytes)
        : bytes_(bytes), fileName_(fileName), maxBytes_(maxBytes), cursor_(bytes),
          satellites_(kSatelliteSlots)
    {
    }

    Result<CompactExpansion> run();

  private:
    katana::core::Status readHeader();
    void noteObservationTypes(std::string_view line);
    bool expandEpoch(std::string_view line, std::size_t record);
    bool expandEvent(int flag, int count, std::size_t record);
    bool expandSatellite(std::string_view id, std::string_view text, std::size_t record);
    void appendEpochLine(const std::int64_t* clock, int count, std::size_t record);
    // Why the epoch being expanded cannot be, found at the line last read.
    bool fail(std::string why)
    {
        why_ = std::move(why);
        failedLine_ = cursor_.lineNumber();
        return false;
    }
    void restartAllSeries()
    {
        // Every satellite is then "not in the previous epoch", which table 2
        // makes a new series; and the clock starts afresh.
        epoch_ += 2;
        clock_.active = false;
    }
    // An expanded line, written straight into the expansion; an epoch that
    // then fails is cut off again (discard), so only whole epochs remain.
    void emitLine(std::string_view line, std::size_t compactRecord, std::size_t perCompact)
    {
        ++pendingLines_;
        pendingRuns_.push_back({expanded_ + pendingLines_, compactRecord, perCompact});
        out_.text.append(trimmedRight(line));
        out_.text.push_back('\n');
    }
    void commit()
    {
        for (const PendingRun& run : pendingRuns_) {
            out_.lines.add(run.expanded, run.compact, run.perCompact);
        }
        expanded_ += pendingLines_;
        pendingRuns_.clear();
        pendingLines_ = 0;
        epochStart_ = out_.text.size();
    }
    void discard()
    {
        out_.text.resize(epochStart_);
        pendingRuns_.clear();
        pendingLines_ = 0;
    }
    void reportDamage(std::size_t lastRecord);

    std::string_view bytes_;
    std::string fileName_;
    std::size_t maxBytes_;
    LineCursor cursor_;
    CompactExpansion out_{};

    bool versionOne_ = false;   // Compact RINEX 1.0: RINEX 2
    std::size_t listColumn_ = 0; // 0-based start of the satellite list in an epoch line
    int typesV2_ = 0;
    std::array<int, 26> typesV3_{};

    std::string epochLine_{}; // the last epoch line, expanded: what differences apply to
    bool haveEpochLine_ = false;
    Series clock_{};
    std::uint64_t epoch_ = 1;
    std::vector<SatelliteState> satellites_;

    // The epoch being expanded, kept apart until it has expanded whole.
    struct PendingRun {
        std::size_t expanded;
        std::size_t compact;
        std::size_t perCompact;
    };
    std::size_t epochStart_ = 0; // where the epoch being expanded starts in the text
    std::vector<PendingRun> pendingRuns_{};
    std::size_t pendingLines_ = 0;
    std::size_t expanded_ = 0; // lines committed
    std::string satelliteLine_{};    // one satellite's RINEX record, reused
    std::string epochText_{};        // an epoch record line, reused
    // What each of a satellite's fields was this epoch: kContinued, kStarted
    // or kBlank.
    std::vector<std::uint8_t> fieldKinds_{};

    std::string why_{};
    std::size_t failedLine_ = 0;
    std::size_t damagedFrom_ = 0; // first line of a damaged stretch, 0 = none
    std::size_t damageLine_ = 0;  // the line the damage was found at
    std::string damageWhy_{};
};

katana::core::Status Expander::readHeader()
{
    std::string_view line;
    cursor_.next(line);
    const std::string_view crxVersion = katana::core::trimmed(columns(line, 1, 20));
    if (headerLabel(line) != "CRINEX VERS   / TYPE" && headerLabel(line) != "CRINEX VERS / TYPE") {
        return makeError(ErrorCode::FileImportFailure,
                         fileName_ + " is not Compact RINEX: its first line is not CRINEX VERS "
                                     "/ TYPE");
    }
    if (crxVersion != "1.0" && crxVersion != "3.0") {
        return makeError(ErrorCode::Unsupported,
                         fileName_ + " is Compact RINEX version " + std::string(crxVersion) +
                             "; Katana expands versions 1.0 (RINEX 2) and 3.0 (RINEX 3 and 4). "
                             "Convert it with CRX2RNX and import the RINEX file it writes.");
    }
    versionOne_ = crxVersion == "1.0";
    listColumn_ = versionOne_ ? 32 : 41;
    out_.description = "Compact RINEX " + std::string(crxVersion);

    if (!cursor_.next(line) || headerLabel(line) != "CRINEX PROG / DATE") {
        return makeError(ErrorCode::FileImportFailure,
                         fileName_ + " is not whole Compact RINEX: its second line is not "
                                     "CRINEX PROG / DATE");
    }
    const std::string program = cleanText(columns(line, 1, 40));
    const std::string written = cleanText(columns(line, 41, 20));
    if (!program.empty()) {
        out_.description += ", written by " + program;
    }
    if (!written.empty()) {
        out_.description += " on " + written;
    }

    if (!cursor_.next(line)) {
        return makeError(ErrorCode::FileImportFailure,
                         fileName_ + " ends after its Compact RINEX header, before the RINEX "
                                     "header it should hold");
    }
    const std::optional<VersionRecord> version = versionRecord(line);
    if (!version) {
        return makeError(ErrorCode::FileImportFailure,
                         fileName_ + " is Compact RINEX, but its third line is not the RINEX "
                                     "VERSION / TYPE record of the file it compresses");
    }
    const bool matches = versionOne_ ? version->major == 2
                                     : (version->major == 3 || version->major == 4);
    if (!matches) {
        return makeError(ErrorCode::FileImportFailure,
                         fileName_ + " is Compact RINEX " + std::string(crxVersion) +
                             ", which holds RINEX " + (versionOne_ ? "2" : "3 and 4") +
                             ", but the file it compresses says RINEX " + version->versionText);
    }
    // The observation reader reads the RINEX header from here: line 3 of the
    // compact file is line 1 of the expansion.
    out_.lines.add(1, 3, 1);
    std::size_t lines = 0;
    do {
        ++lines;
        out_.text.append(trimmedRight(line));
        out_.text.push_back('\n');
        noteObservationTypes(line);
        if (headerLabel(line) == "END OF HEADER") {
            expanded_ = lines;
            epochStart_ = out_.text.size();
            return {};
        }
    } while (cursor_.next(line));
    return makeError(ErrorCode::FileImportFailure,
                     fileName_ + " is Compact RINEX, but the RINEX header inside it has no END "
                                 "OF HEADER");
}

// How many observation types each system has: a satellite line has one field
// per type. Read from the header and from event records that redefine them.
void Expander::noteObservationTypes(std::string_view line)
{
    const std::string_view label = headerLabel(line);
    if (versionOne_ && label == "# / TYPES OF OBSERV") {
        if (const std::optional<long long> count = integerField(columns(line, 1, 6))) {
            typesV2_ = static_cast<int>(std::clamp<long long>(*count, 0, kMostObservationTypes + 1));
        }
    } else if (!versionOne_ && label == "SYS / # / OBS TYPES" && !line.empty() && line[0] != ' ') {
        const char system = line[0];
        const std::optional<long long> count = integerField(columns(line, 4, 3));
        if (system >= 'A' && system <= 'Z' && count) {
            typesV3_[static_cast<std::size_t>(system - 'A')] =
                static_cast<int>(std::clamp<long long>(*count, 0, kMostObservationTypes + 1));
        }
    }
}

void Expander::appendEpochLine(const std::int64_t* clock, int count, std::size_t record)
{
    std::string& line = epochText_;
    if (!versionOne_) {
        // RINEX 3 (table A3): columns 1-35, six reserved, the clock in 42-56.
        line.assign(std::string_view(epochLine_).substr(0, std::min<std::size_t>(41, epochLine_.size())));
        if (clock != nullptr) {
            line.resize(41 + 15, ' ');
            writeFixed(line.data() + 41, 15, *clock, 12);
        }
        emitLine(line, record, 1);
        return;
    }
    // RINEX 2 (table A2): columns 1-32, twelve satellites per line (33-68),
    // the clock in 69-80 of the first line, continuation lines of 32 blanks.
    const std::string_view list = std::string_view(epochLine_).substr(listColumn_);
    const int lines = count > 12 ? (count + 11) / 12 : 1;
    for (int i = 0; i < lines; ++i) {
        if (i == 0) {
            line.assign(std::string_view(epochLine_).substr(0, 32));
            line.resize(32, ' ');
        } else {
            line.assign(32, ' ');
        }
        const std::size_t first = static_cast<std::size_t>(i) * 36;
        const std::size_t left = static_cast<std::size_t>(count) * 3 - first;
        line.append(list.substr(first, std::min<std::size_t>(36, left)));
        if (i == 0 && clock != nullptr) {
            line.resize(68 + 12, ' ');
            writeFixed(line.data() + 68, 12, *clock, 9);
        }
        // Every line of the epoch record comes from the one compact line.
        emitLine(line, record, 0);
    }
}

bool Expander::expandEvent(int flag, int count, std::size_t record)
{
    // Events (flags 2-6) are written as they are (table A-2); a RINEX 2 cycle
    // slip record (flag 6) keeps its satellite list on the epoch line, and
    // each of its satellites one line per five types, as in the RINEX file.
    if (flag == 6 && versionOne_) {
        if (epochLine_.size() < listColumn_ + static_cast<std::size_t>(count) * 3) {
            return fail("the cycle slip record lists " + std::to_string(count) +
                        " satellites but its satellite list is shorter");
        }
        appendEpochLine(nullptr, count, record);
    } else {
        emitLine(std::string_view(epochLine_).substr(0, std::min(listColumn_, epochLine_.size())),
                 record, 1);
    }
    const int perSatellite = versionOne_ && flag == 6 ? std::max(1, (typesV2_ + 4) / 5) : 1;
    const long long records = static_cast<long long>(count) * perSatellite;
    std::string_view line;
    for (long long i = 0; i < records; ++i) {
        if (!cursor_.next(line)) {
            return fail("the file ends before the " + std::to_string(records) +
                        " records this event (flag " + std::to_string(flag) + ") promises");
        }
        emitLine(line, cursor_.lineNumber(), 1);
        noteObservationTypes(line);
    }
    restartAllSeries();
    return true;
}

bool Expander::expandEpoch(std::string_view line, std::size_t record)
{
    // The epoch line: given in full, or as its difference from the last one.
    const bool full = versionOne_ ? (!line.empty() && line[0] == '&')
                                  : (!line.empty() && line[0] == '>');
    if (full) {
        epochLine_.assign(line);
        if (versionOne_) {
            epochLine_[0] = ' ';
        }
        haveEpochLine_ = true;
    } else if (!haveEpochLine_) {
        return fail("an epoch line given only as its difference from an earlier one, with no "
                    "earlier one to apply it to");
    } else {
        applyTextDifference(epochLine_, line);
    }

    const std::size_t flagAt = versionOne_ ? 28 : 31;
    if (epochLine_.size() <= flagAt || !isDigit(epochLine_[flagAt])) {
        return fail("its epoch line has no event flag where one should be (column " +
                    std::to_string(flagAt + 1) + ")");
    }
    const int flag = epochLine_[flagAt] - '0';
    const std::optional<long long> counted =
        integerField(std::string_view(epochLine_).substr(flagAt + 1, 3));
    if (!counted || *counted < 0) {
        return fail("its epoch line has no satellite or record count after the event flag");
    }
    const int count = static_cast<int>(std::min<long long>(*counted, 999));
    if (flag > 6) {
        return fail("event flag " + std::to_string(flag) + " is not one RINEX defines (0 to 6)");
    }
    if (flag >= 2) {
        return expandEvent(flag, count, record);
    }

    if (epochLine_.size() < listColumn_ + static_cast<std::size_t>(count) * 3) {
        return fail("the epoch lists " + std::to_string(count) +
                    " satellites but its satellite list is shorter");
    }

    // The clock offset line (table A-2): empty, "M&v" or a difference.
    std::string_view clockLine;
    if (!cursor_.next(clockLine)) {
        return fail("the file ends after an epoch line, before its clock offset line");
    }
    const NumericField clock = readNumericField(trimmedRight(clockLine));
    switch (clock.kind) {
    case NumericField::Kind::Blank:
        clock_.active = false;
        break;
    case NumericField::Kind::Start:
        clock_.start(clock.order, clock.value);
        break;
    case NumericField::Kind::Difference:
        if (!clock_.active) {
            return fail("a receiver clock offset given as a difference, with no earlier value "
                        "to apply it to");
        }
        if (!clock_.advance(clock.value)) {
            return fail("the receiver clock offset cannot be recovered: its differences "
                        "overflow");
        }
        break;
    case NumericField::Kind::Unreadable:
        return fail("the receiver clock offset line is not a number");
    }
    ++epoch_;
    appendEpochLine(clock_.active ? &clock_.state[0] : nullptr, count, record);

    std::string_view satelliteLine;
    for (int i = 0; i < count; ++i) {
        const LineCursor::Mark mark = cursor_.mark();
        if (!cursor_.next(satelliteLine)) {
            return fail("the file ends inside the epoch: " + std::to_string(count - i) +
                        " of its " + std::to_string(count) + " satellite lines are missing");
        }
        const bool epochStarts = versionOne_ ? (!satelliteLine.empty() && satelliteLine[0] == '&')
                                             : (!satelliteLine.empty() && satelliteLine[0] == '>');
        if (epochStarts) {
            cursor_.reset(mark);
            return fail("the epoch lists " + std::to_string(count) + " satellites but only " +
                        std::to_string(i) + " lines follow it");
        }
        const std::string_view id =
            std::string_view(epochLine_).substr(listColumn_ + static_cast<std::size_t>(i) * 3, 3);
        if (!expandSatellite(id, satelliteLine, cursor_.lineNumber())) {
            return false;
        }
    }
    return true;
}

bool Expander::expandSatellite(std::string_view id, std::string_view text, std::size_t record)
{
    const std::string_view line = trimmedRight(text);
    // RINEX 2 lets a blank system letter mean GPS (2.11 section 5.1).
    const char system = id[0] == ' ' && versionOne_ ? 'G' : id[0];
    if (system < 'A' || system > 'Z' || !(isDigit(id[1]) || id[1] == ' ') || !isDigit(id[2])) {
        return fail("the satellite list holds '" + std::string(id) +
                    "', which is not a satellite (a system letter and a number)");
    }
    const int number = (id[1] == ' ' ? 0 : (id[1] - '0') * 10) + (id[2] - '0');
    const int types = versionOne_ ? typesV2_ : typesV3_[static_cast<std::size_t>(system - 'A')];
    if (types <= 0 || types > kMostObservationTypes) {
        return fail("satellite " + std::string(id) + ": the header declares " +
                    (types <= 0 ? std::string("no") : std::string("too many")) +
                    " observation types for its system, so its line cannot be expanded");
    }
    SatelliteState& satellite =
        satellites_[static_cast<std::size_t>(system - 'A') * 100 + static_cast<std::size_t>(number)];
    if (satellite.lastEpoch == epoch_) {
        return fail("satellite " + std::string(id) + " appears twice in one epoch");
    }
    const auto typeCount = static_cast<std::size_t>(types);
    if (satellite.lastEpoch + 1 != epoch_ || satellite.series.size() != typeCount) {
        // Not in the previous epoch: every series starts afresh (table 2).
        satellite.series.assign(typeCount, Series{});
        satellite.flags.assign(typeCount * 2, ' ');
    }
    satellite.lastEpoch = epoch_;

    // The numeric fields: the first `types` blanks separate them (table A-2,
    // note 3); what follows the last is the flags' difference text.
    // The RINEX record is laid out blank - F14.3, I1, I1 per type, after the
    // satellite in RINEX 3 - and each value written into its place.
    fieldKinds_.assign(typeCount, kContinued);
    const std::size_t prefix = versionOne_ ? 0 : 3;
    satelliteLine_.assign(prefix + typeCount * 16, ' ');
    char* const fields = satelliteLine_.data() + prefix;
    if (!versionOne_) {
        satelliteLine_.replace(0, 3, id);
    }
    std::size_t at = 0;
    for (std::size_t k = 0; k < typeCount; ++k) {
        std::string_view field;
        if (at < line.size()) {
            const std::size_t end = line.find(' ', at);
            const std::size_t stop = end == std::string_view::npos ? line.size() : end;
            field = line.substr(at, stop - at);
            at = stop + 1;
        } else {
            at = line.size() + 1;
        }
        Series& series = satellite.series[k];
        const NumericField read = readNumericField(field);
        switch (read.kind) {
        case NumericField::Kind::Blank:
            series.active = false;
            fieldKinds_[k] = kBlank;
            continue;
        case NumericField::Kind::Start:
            series.start(read.order, read.value);
            fieldKinds_[k] = kStarted;
            break;
        case NumericField::Kind::Difference:
            if (!series.active) {
                return fail("satellite " + std::string(id) + ", observation " +
                            std::to_string(k + 1) +
                            ": a difference with no earlier value to apply it to");
            }
            if (!series.advance(read.value)) {
                return fail("satellite " + std::string(id) + ", observation " +
                            std::to_string(k + 1) + ": its differences overflow");
            }
            break;
        case NumericField::Kind::Unreadable:
            return fail("satellite " + std::string(id) + ", observation " + std::to_string(k + 1) +
                        ": '" + std::string(field) + "' is not a number");
        }
        if (!writeFixed(fields + k * 16, 14, series.state[0], 3)) {
            return fail("satellite " + std::string(id) + ", observation " + std::to_string(k + 1) +
                        ": the recovered value does not fit an F14.3 field");
        }
    }

    // The flags. In 1.0 a type's flags restart with its series (section 2.1).
    if (versionOne_) {
        for (std::size_t k = 0; k < typeCount; ++k) {
            if (fieldKinds_[k] == kStarted) {
                satellite.flags[2 * k] = ' ';
                satellite.flags[2 * k + 1] = ' ';
            }
        }
    }
    const std::string_view flags = at < line.size() ? line.substr(at) : std::string_view{};
    if (flags.size() > typeCount * 2) {
        return fail("satellite " + std::string(id) + ": more flag characters than its " +
                    std::to_string(typeCount) + " observation types have");
    }
    applyTextDifference(satellite.flags, flags);

    for (std::size_t k = 0; k < typeCount; ++k) {
        // 1.0 cannot carry flags without their observation (section 2.1): a
        // blank observation has blank flags, as CRX2RNX writes it.
        if (versionOne_ && fieldKinds_[k] == kBlank) {
            continue;
        }
        fields[k * 16 + 14] = satellite.flags[2 * k];
        fields[k * 16 + 15] = satellite.flags[2 * k + 1];
    }
    if (!versionOne_) {
        emitLine(satelliteLine_, record, 1);
        return true;
    }
    // RINEX 2: five types (80 columns) per line.
    const std::size_t lines = (typeCount + 4) / 5;
    const std::string_view whole = satelliteLine_;
    for (std::size_t l = 0; l < lines; ++l) {
        emitLine(whole.substr(l * 80, 80), record, lines);
    }
    return true;
}

void Expander::reportDamage(std::size_t lastRecord)
{
    if (damagedFrom_ == 0) {
        return;
    }
    const std::size_t lines = lastRecord >= damagedFrom_ ? lastRecord - damagedFrom_ + 1 : 1;
    out_.linesSkipped += lines;
    std::string message = "the compact data cannot be expanded here: " + damageWhy_ + ". ";
    message += lines == 1 ? "Line " + std::to_string(damagedFrom_) + " is left out"
                          : "Lines " + std::to_string(damagedFrom_) + " to " +
                                std::to_string(damagedFrom_ + lines - 1) +
                                " are left out (Compact RINEX is differential: nothing after "
                                "damage can be recovered until every series starts afresh)";
    out_.warnings.push_back(ReadWarning{fileName_, damageLine_, std::move(message)});
    damagedFrom_ = 0;
    damageLine_ = 0;
    damageWhy_.clear();
}

Result<CompactExpansion> Expander::run()
{
    if (katana::core::Status header = readHeader(); !header.ok()) {
        return header.error();
    }
    // Compact RINEX is typically a third to a sixth of the RINEX it holds.
    out_.text.reserve(std::min(maxBytes_, bytes_.size() * 4));
    std::string_view line;
    while (cursor_.next(line)) {
        const std::size_t record = cursor_.lineNumber();
        if (line.empty() && cursor_.atEnd()) {
            continue; // an editor's last blank line
        }
        const bool full = versionOne_ ? (!line.empty() && line[0] == '&')
                                      : (!line.empty() && line[0] == '>');
        if (damagedFrom_ != 0 && !full) {
            continue; // still inside a damaged stretch: only a full epoch can end it
        }
        if (!versionOne_ && !line.empty() && line[0] == '&') {
            // Table A-3: an optional record, reserved for future use; readers
            // are to skip it, and it does not restart anything.
            ++out_.optionalLines;
            continue;
        }
        if (expandEpoch(line, record)) {
            reportDamage(record - 1);
            commit();
            if (out_.text.size() > maxBytes_) {
                return makeError(ErrorCode::FileImportFailure,
                                 fileName_ + " expands to more than " +
                                     std::to_string(maxBytes_ >> 20) +
                                     " MB of RINEX, more than Katana reads from one file");
            }
            continue;
        }
        discard();
        haveEpochLine_ = false;
        restartAllSeries();
        if (damagedFrom_ == 0) {
            damagedFrom_ = record;
            damageLine_ = std::max(record, failedLine_);
            damageWhy_ = std::move(why_);
        }
    }
    reportDamage(cursor_.lineNumber());
    out_.compactLines = cursor_.lineNumber();
    return std::move(out_);
}

} // namespace

Result<CompactExpansion> expandCompactRinex(std::string_view bytes, std::string_view fileName,
                                            std::size_t maxExpandedBytes)
{
    Expander expander(bytes, fileName, maxExpandedBytes);
    return expander.run();
}

} // namespace katana::surveyio::rinex
