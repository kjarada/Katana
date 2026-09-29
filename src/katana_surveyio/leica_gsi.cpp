// Leica GSI-8 and GSI-16: the probe, the reader and the format's registration.
// What an import produces is described in include/katana/surveyio/leica.hpp.
//
// THE SPECIFICATION is Leica Geosystems, "GSI ONLINE for Leica TPS and DNA",
// November 2003: the data word and the word information table on pages 5-6,
// the TPS1000/1100/2000/5000 word indices in the PUT and GET tables on pages
// 35-37, the DNA (levelling) words on pages 40-42; its SET/CONF tables list
// the instrument settings no word records (171, the horizontal circle's
// direction). Where a code block is stored relative to its point is another
// such setting, from Leica's "TPS1200 Technical Reference Manual", version
// 5.0 (see codesRecordedAfterPoints). A word is
//
//     WI  info  sign  data  blank
//     positions 1-2   word index (a DNA level also writes three-digit ones)
//     position  3     no significance - except in words 11 and 41, where
//                     positions 3-6 are the block number
//     position  4     automatic index (the vertical compensator), angle words
//     position  5     input mode: 0 measured, 1 keyed in, 2 / 3 measured with
//                     the horizontal correction on / off, 4 computed on board
//     position  6     units: 0 m (last digit 1 mm), 1 ft (1/1000 ft), 2 gon,
//                     3 decimal degrees, 4 sexagesimal, 5 mil, 6 m (1/10 mm),
//                     7 ft (1/10000 ft), 8 m (1/100 mm)
//     position  7     sign
//     then 8 (GSI-8) or 16 (GSI-16) characters of data, right justified and
//     padded with '0'. A GSI-16 block begins with '*'.
//
// THE DECIMAL POINT IS NOT WRITTEN: the unit digit places it
// ("81..00+00005387" is 5.387 m, "21.102+17920860" is 179.20860 gon). Angles
// have five decimals in gon and degrees (GET 21/22 examples) and in the
// sexagesimal DDD.MMSSs; a mil angle has four, the only scale at which a full
// circle of 6400 mil fits eight digits as 400 gon and 360 degrees do at five.
// A file that writes a point anyway (the owner's "84..00+8085.844") is read as
// written, and the import says it met one.
//
// SIXTY SECONDS. A sexagesimal word's minutes and seconds run to 59: GSI
// ONLINE's GET examples ("21.104+12149400" is 121 49 40.0) show nothing else,
// and it says nothing of 60. A writer that rounds the seconds on their own,
// without carrying into the minutes, writes 60 for a value within its
// rounding of the next minute. 60.0 seconds IS the next minute, exactly, so a word whose
// seconds are 60 with nothing after them is read as the next minute, carried
// in whole numbers before the angle becomes a real, and the import says once
// how many there were and where. That holds only where the word itself writes
// both digits of the seconds in their place: at its block's full width, or
// four digits after a written point. A short or long word is aligned from the
// right and a point's missing digits are supplied as zeros, and a 60 that
// alignment or padding made is refused. A traverse the owner sent has 24 such
// words; 19 agree with the other face of their round, and the other 5 with the
// same face's other readings of their target, only when read so
// (docs/survey.md, "Leica GSI"). Seconds past 60.0 and minutes past 59 are
// still refused: no file has shown either.
//
// A circle reading (words 21 and 22) runs to one full circle - the unit
// digit's 400 gon, 360 degrees or 6400 mil - and a word past that is refused,
// not wrapped. A full circle itself is the circle's zero, reached by rounding
// up at the top of the circle.
//
// A SIGN comes with every measured value (page 6: "+: Positive value, -:
// Negative value"), and what a negative one means differs by circle. Word 22
// is read as a zenith angle, and no zenith angle is negative: an instrument
// writes one when it is set to read V from the horizon or in percent (page 9,
// the TPS100 series' SET 44, "V angle READING": 0 Zenith, 1 Horizontal, 2
// Slope in percent), below the horizon. So a negative word 22 is refused at
// its record; wrapped onto the circle, -005 00 00 would be a face-right zenith
// of 5 degrees, the shot drawn near the vertical and half a turn round from
// where it was measured. No setting gives word 21 another meaning - 171 only
// turns the circle clockwise or counterclockwise, 178 and 179 switch the Hz
// compensator and collimation - so a negative one is the direction it names,
// counted the other way from the circle's zero: -090 00 00 is read as 270 00
// 00. -0 is zero in either word.
//
// Everything here streams over one string_view: no regex, std::from_chars for
// every number, and nothing allocated per record beyond the values it produces.

#include <array>
#include <charconv>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <deque>
#include <functional>
#include <map>
#include <numbers>
#include <optional>
#include <string>
#include <string_view>
#include <system_error>
#include <utility>
#include <vector>

#include "katana/math/unit_ratio.hpp"
#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/format.hpp"
#include "katana/surveyio/leica.hpp"

namespace katana::surveyio {

namespace {

namespace survey = katana::survey;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

// The version of THIS code (format.hpp). Bump it when a file would import
// differently, so the SourceRecords of old imports say which reading they had.
// 1.1 reads 60-second angle words as the next minute, an all-zero code or
// remark word as empty, a backsight the file positions later, and code
// information in a point's own block; it refuses a circle reading past a full
// circle, a negative vertical reading, and a sexagesimal word with no digit,
// all of which 1.0 read as angles. (The last two joined 1.1 before a release
// or a push had carried it.)
constexpr const char* kParserVersion = "1.1";

constexpr double kPi = std::numbers::pi;
constexpr double kTwoPi = 2.0 * std::numbers::pi;

// Record-level warnings listed one by one before the rest are only counted. A
// damaged file can have a problem on every line, and a list of a million
// warnings is not one a person reads; the count still says how many there were.
constexpr std::size_t kMaxListedWarnings = 1000;

// Two coordinates for one point that agree to half the last digit of a
// millimetre word are the same coordinates written twice.
constexpr double kSamePositionTolerance = 0.0005;

// A note for the file names the records it is about, up to this many, and
// counts the rest: enough to find each in a job with a few dozen, without a
// message of thousands of numbers when a writer did something on every line.
constexpr std::size_t kListedRecords = 100;

// ---- Words -----------------------------------------------------------------------

bool isDigit(char c)
{
    return c >= '0' && c <= '9';
}

bool isBlank(char c)
{
    return c == ' ' || c == '\t';
}

struct Word {
    int index = 0;         // the word index: 11, 21, ... or 330 for a DNA word
    std::string_view info; // positions 3-6 (4-6 after a three-digit index)
    char sign = '+';
    std::string_view data; // 8 or 16 characters in a well-formed file
};

// Positions 4, 5 and 6 of the word, '.' where the file leaves them empty.
char infoAt(const Word& word, std::size_t position)
{
    // `info` starts at position 3, or at position 4 after a three-digit index.
    const std::size_t first = word.info.size() == 4 ? 3 : 4;
    if (position < first || position - first >= word.info.size()) {
        return '.';
    }
    return word.info[position - first];
}

char unitDigit(const Word& word)
{
    return infoAt(word, 6);
}

char inputMode(const Word& word)
{
    return infoAt(word, 5);
}

// One blank-separated token as a GSI word, or nullopt when it is not one.
std::optional<Word> parseWord(std::string_view token)
{
    if (token.size() < 8 || !isDigit(token[0]) || !isDigit(token[1])) {
        return std::nullopt;
    }
    Word word;
    word.index = (token[0] - '0') * 10 + (token[1] - '0');
    std::size_t infoStart = 2;
    // Words 11 and 41 carry the block number in positions 3-6, so a digit
    // there is theirs; anywhere else a third digit is a DNA three-digit index
    // (page 40: "a two or three character Word Index").
    if (word.index != 11 && word.index != 41 && isDigit(token[2])) {
        word.index = word.index * 10 + (token[2] - '0');
        infoStart = 3;
    }
    word.sign = token[6];
    if (word.sign != '+' && word.sign != '-') {
        return std::nullopt;
    }
    word.info = token.substr(infoStart, 6 - infoStart);
    word.data = token.substr(7);
    // 16 is GSI-16's width; more than that is not a GSI word and could not be
    // held in the integer the number is read into.
    if (word.data.empty() || word.data.size() > 16) {
        return std::nullopt;
    }
    return word;
}

// Text data without the '0' padding the specification right-justifies it
// with: "0000A110" is point A110, "00000013" is code 13 (GET 11 and 41).
std::string_view textOf(std::string_view data)
{
    const std::size_t first = data.find_first_not_of('0');
    return first == std::string_view::npos ? data.substr(data.size() - 1) : data.substr(first);
}

struct Number {
    double value = 0.0;
    bool explicitPoint = false; // the file wrote a decimal point itself
};

// The digits of a numeric word, signed; the unit digit's scale is NOT applied
// unless the file wrote its own point.
std::optional<Number> numberOf(std::string_view data, char sign)
{
    std::size_t points = 0;
    for (const char c : data) {
        if (c == '.') {
            ++points;
        } else if (!isDigit(c)) {
            return std::nullopt;
        }
    }
    if (points > 1 || data == ".") {
        return std::nullopt;
    }
    Number number;
    const char* end = data.data() + data.size();
    if (points == 1) {
        const auto [stop, error] =
            std::from_chars(data.data(), end, number.value, std::chars_format::fixed);
        if (error != std::errc{} || stop != end) {
            return std::nullopt;
        }
        number.explicitPoint = true;
    } else {
        std::int64_t digits = 0; // at most 16 digits: parseWord refuses more
        const auto [stop, error] = std::from_chars(data.data(), end, digits);
        if (error != std::errc{} || stop != end) {
            return std::nullopt;
        }
        number.value = static_cast<double>(digits);
    }
    if (sign == '-') {
        number.value = -number.value;
    }
    return number;
}

// Division by an exact power of ten, so that an integer word becomes the double
// nearest its decimal value: 8085844 / 1000 is the same double as "8085.844".
constexpr std::array<double, 6> kPowersOfTen{1.0, 10.0, 100.0, 1000.0, 10000.0, 100000.0};

double wrapToCircle(double radians)
{
    double wrapped = std::fmod(radians, kTwoPi);
    if (wrapped < 0.0) {
        wrapped += kTwoPi;
    }
    return wrapped >= kTwoPi ? 0.0 : wrapped;
}

// What a word index is, for the messages about words this reader does not use.
const char* wordName(int index)
{
    switch (index) {
    case 17:
        return "date";
    case 25:
        return "horizontal circle difference";
    case 26:
        return "offset";
    case 27:
        return "vertical correction";
    case 28:
        return "horizontal correction";
    case 52:
        return "number of measurements and standard deviation";
    case 53:
        return "average";
    case 95:
        return "instrument temperature";
    default:
        break;
    }
    if (index >= 300) {
        return "a digital level's word"; // DNA: staff readings, levelling results
    }
    return "not defined for total stations";
}

// ---- One block, as its words give it ---------------------------------------------

// A sexagesimal word whose seconds were 60, read as the next minute (SIXTY
// SECONDS, at the top): enough to show it as written and as read.
struct SixtySeconds {
    int word = 0;
    bool negative = false;
    std::int64_t degrees = 0; // as written
    int minutes = 0;          // as written; the seconds were 60
    std::size_t zeros = 0;    // the digits written after the seconds, all 0
};

// "word 22, 084 01 60.0, read as 084 02 00.0"
std::string sixtyText(const SixtySeconds& sixty)
{
    const auto angle = [&sixty](std::int64_t degrees, int minutes, int seconds) {
        const std::string whole = std::to_string(degrees);
        std::string text = sixty.negative ? "-" : "";
        text += std::string(whole.size() < 3 ? 3 - whole.size() : 0, '0') + whole;
        for (const int part : {minutes, seconds}) {
            text += ' ';
            text += static_cast<char>('0' + part / 10);
            text += static_cast<char>('0' + part % 10);
        }
        if (sixty.zeros != 0) {
            text += '.' + std::string(sixty.zeros, '0');
        }
        return text;
    };
    const bool nextDegree = sixty.minutes == 59;
    return "word " + std::to_string(sixty.word) + ", " + angle(sixty.degrees, sixty.minutes, 60) +
           ", read as " +
           angle(sixty.degrees + (nextDegree ? 1 : 0), nextDegree ? 0 : sixty.minutes + 1, 0);
}

struct Block {
    std::size_t record = 0;
    bool wide = false; // GSI-16
    int kind = 0;      // 11: a point's block; 41: a code block
    std::string_view name; // the point id, or the code of a code block

    std::optional<double> hz;
    std::optional<double> v;
    // Set where hz or v was a 60-second word; counted once the value is kept.
    std::optional<SixtySeconds> hzSixty;
    std::optional<SixtySeconds> vSixty;
    std::optional<double> slope;
    std::optional<double> horizontal;
    std::optional<double> heightDifference;
    char hzInputMode = '.';
    char vIndex = '.';

    std::optional<double> e, n, h; // 81-83: the target
    char targetInputMode = '.';
    std::optional<double> e0, n0, h0; // 84-86: the station
    char stationInputMode = '.';
    std::optional<double> reflectorHeight;  // 87
    std::optional<double> instrumentHeight; // 88
    std::string_view heightText;            // letters in word 87 or 88
    int heightTextWord = 0;

    std::optional<double> ppm;
    std::optional<double> prismConstant; // metres

    std::array<std::string_view, 9> remarks{};  // 71..79, read in a point's block
    std::array<std::string_view, 8> codeInfo{}; // 42..49, in a code block or a point's
    std::string_view serial;                    // 12
    std::string_view type;                      // 13
    std::string_view time18;
    std::string_view time19;
    bool measurementWords = false; // any measurement word, for a code block's check
    bool levelling = false;        // holds a digital level's three-digit word

    [[nodiscard]] bool measures() const
    {
        return hz || v || slope || horizontal || heightDifference;
    }
    [[nodiscard]] bool stationCoordinates() const { return e0 || n0 || h0; }
    [[nodiscard]] bool targetCoordinates() const { return e || n || h; }
};

// A count of one kind of thing the file does many times, reported ONCE with
// where it first happened rather than once per record.
struct Tally {
    std::size_t count = 0;
    std::size_t firstRecord = 0;
    std::string example;

    void add(std::size_t record, std::string_view text = {})
    {
        if (count++ == 0) {
            firstRecord = record;
            example = std::string(text);
        }
    }

    // add(), with an example that has to be built: `make` runs for the first
    // occurrence only, so a thousand of them cost one string.
    template <class Make>
    void addMade(std::size_t record, const Make& make)
    {
        if (count == 0) {
            add(record, make());
        } else {
            add(record);
        }
    }
};

// The records a note for the file is about: the first kListedRecords named,
// the rest counted. A record added twice in a row is one record.
struct RecordList {
    std::vector<std::size_t> listed;
    std::size_t count = 0;

    void add(std::size_t record)
    {
        if (count != 0 && record == last_) {
            return;
        }
        last_ = record;
        ++count;
        if (listed.size() < kListedRecords) {
            listed.push_back(record);
        }
    }

    // "record 4", "records 4, 5", "records 4, 5, ... and 20 more"
    [[nodiscard]] std::string text() const
    {
        std::string out = count == 1 ? "record " : "records ";
        for (std::size_t i = 0; i < listed.size(); ++i) {
            out += (i == 0 ? "" : ", ") + std::to_string(listed[i]);
        }
        if (count > listed.size()) {
            out += " and " + std::to_string(count - listed.size()) + " more";
        }
        return out;
    }

  private:
    std::size_t last_ = 0;
};

std::string plural(std::size_t count, std::string_view one, std::string_view many)
{
    return std::to_string(count) + " " + std::string(count == 1 ? one : many);
}

std::string metresText(double metres)
{
    std::array<char, 64> buffer{};
    const auto [end, error] = std::to_chars(buffer.data(), buffer.data() + buffer.size(), metres,
                                            std::chars_format::fixed, 4);
    if (error != std::errc{}) {
        return "?";
    }
    std::string text(buffer.data(), end);
    while (text.size() > 1 && text.back() == '0' && text[text.size() - 2] != '.') {
        text.pop_back();
    }
    return text;
}

std::size_t lineFeeds(std::string_view bytes)
{
    std::size_t count = 0;
    const char* at = bytes.data();
    const char* const end = bytes.data() + bytes.size();
    while (at != end) {
        const void* found = std::memchr(at, '\n', static_cast<std::size_t>(end - at));
        if (found == nullptr) {
            break;
        }
        ++count;
        at = static_cast<const char*>(found) + 1;
    }
    return count;
}

// The kind of block a line holds, judged by its first word as readBlock judges
// it: 11 (a point's block), 41 (a code block), or 0 for anything else - a
// blank or unreadable line, or a digital level's special code block, which
// codes no point (GsiReader::codeBlock).
int blockKindOf(std::string_view line)
{
    std::size_t position = 0;
    while (true) {
        while (position < line.size() && isBlank(line[position])) {
            ++position;
        }
        if (position >= line.size()) {
            return 0;
        }
        std::size_t end = position;
        while (end < line.size() && !isBlank(line[end])) {
            ++end;
        }
        std::string_view token = line.substr(position, end - position);
        position = end;
        if (token.front() == '*') {
            token.remove_prefix(1);
        }
        if (token == "\x1a") {
            continue; // a DOS end-of-file mark, as readBlock skips it
        }
        const std::optional<Word> word = parseWord(token);
        if (!word) {
            return 0;
        }
        if (word->index == 41) {
            return textOf(word->data).front() == '?' ? 0 : 41;
        }
        return word->index == 11 ? 11 : 0;
    }
}

// Whether the file's code blocks were recorded AFTER their points. GSI does
// not say: it is the instrument's <Rec Free Code: Before Point / After Point>
// setting (Leica TPS1200 Technical Reference Manual, version 5.0, 16.3 "Coding
// & Linework Settings" and 8.4 "Quick Coding"), and no word carries it. The
// one sign a file gives is at its ends. Recorded before its point, a code
// block comes first when the first point is coded and never comes last - a
// code with no point after it is an unfinished record. Recorded after, the
// file must begin with a point and ends with a code whenever its last point
// is coded. So a file that begins with a point block and ends with a code
// block is read as After Point; any other as Before Point. Only the first and
// the last blocks are looked at, so this costs a line or two, not a pass.
bool codesRecordedAfterPoints(std::string_view bytes)
{
    int first = 0;
    for (std::string_view rest = bytes; !rest.empty() && first == 0;) {
        const std::size_t end = rest.find_first_of("\r\n");
        first = blockKindOf(rest.substr(0, end));
        rest = end == std::string_view::npos ? std::string_view{} : rest.substr(end + 1);
    }
    if (first != 11) {
        return false;
    }
    int last = 0;
    for (std::string_view rest = bytes; !rest.empty() && last == 0;) {
        const std::size_t cut = rest.find_last_of("\r\n");
        last = blockKindOf(cut == std::string_view::npos ? rest : rest.substr(cut + 1));
        rest = cut == std::string_view::npos ? std::string_view{} : rest.substr(0, cut);
    }
    return last == 41;
}

std::string shown(std::string_view text)
{
    std::string out = "'";
    for (const char c : text.substr(0, 40)) {
        const auto byte = static_cast<unsigned char>(c);
        out += (byte < 0x20 || byte == 0x7F) ? '?' : c;
    }
    return out + (text.size() > 40 ? "...'" : "'");
}

// Point id -> slot, keyed by views into the file's own bytes (or into ids the
// reader made up, which it keeps). Open addressing in one vector: a job names a
// new point on nearly every line, and a node-based map spent more time in the
// allocator for those than the reader spent reading them.
class IdIndex {
  public:
    static constexpr std::size_t kAbsent = static_cast<std::size_t>(-1);

    [[nodiscard]] std::size_t find(std::string_view id) const
    {
        if (entries_.empty()) {
            return kAbsent;
        }
        const std::size_t mask = entries_.size() - 1;
        for (std::size_t at = std::hash<std::string_view>{}(id) & mask;; at = (at + 1) & mask) {
            const Entry& entry = entries_[at];
            if (entry.value == kAbsent) {
                return kAbsent;
            }
            if (entry.key == id) {
                return entry.value;
            }
        }
    }

    // `id` must outlive the index.
    void insert(std::string_view id, std::size_t value)
    {
        if ((size_ + 1) * 2 > entries_.size()) {
            grow();
        }
        place(Entry{id, value}, std::hash<std::string_view>{}(id));
        ++size_;
    }

  private:
    struct Entry {
        std::string_view key;
        std::size_t value = kAbsent;
    };

    void place(const Entry& entry, std::size_t hash)
    {
        const std::size_t mask = entries_.size() - 1;
        std::size_t at = hash & mask;
        while (entries_[at].value != kAbsent) {
            at = (at + 1) & mask;
        }
        entries_[at] = entry;
    }

    void grow()
    {
        std::vector<Entry> old(std::max<std::size_t>(entries_.size() * 2, 1024));
        old.swap(entries_);
        for (const Entry& entry : old) {
            if (entry.value != kAbsent) {
                place(entry, std::hash<std::string_view>{}(entry.key));
            }
        }
    }

    std::vector<Entry> entries_;
    std::size_t size_ = 0;
};

// A point as the file builds it up, before it is known whether it has a
// position (it may be named by a shot first and given coordinates later).
struct PointSlot {
    std::string id;
    bool positioned = false;
    double northing = 0.0;
    double easting = 0.0;
    std::optional<double> elevation;
    survey::CoordinateSource coordinateSource = survey::CoordinateSource::Unknown;
    std::string code;
    std::map<std::string, std::string> metadata;
    std::size_t record = 0; // where the point was first named, or given its position
    bool wide = false;      // ... and whether that block was GSI-16
    std::size_t setups = 0; // setups made on it so far, for their ids (stationIdFor)
    // 1 + the setup whose shot gave the position (words 81-83, which the
    // instrument computed with that setup's orientation), else 0: such a
    // position cannot orient that setup (GsiReader::shotBlock, the backsight).
    std::size_t positionedByShotOf = 0;
};

// A setup whose first shot was to a point with no position yet: it is the
// setup's backsight if the file positions the point later (GsiReader::finish).
struct FirstShot {
    std::size_t station = 0; // index into project.stations
    std::size_t slot = 0;
};

struct CodeBlock {
    std::string code;
    std::array<std::string, 8> info;
    std::size_t record = 0;
};

// A point's place in the run of points that makes the features, settled once
// the point can get no more codes (GsiReader::pointCode).
struct FeatureStep {
    std::size_t slot = 0;
    std::size_t record = 0;
    bool wide = false;
};

// A setup that is only a setup once a measurement follows it (see stationBlock).
struct PendingSetup {
    std::size_t slot = 0;
    std::optional<double> instrumentHeight;
    std::size_t record = 0;
    bool wide = false;
    survey::SurveyTimestamp time;
    std::string timeWithoutYear;
};

class GsiReader {
  public:
    GsiReader(std::string_view fileName, const ReadOptions& options)
        : fileName_(fileName), precision_(options.precision)
    {
        narrowSource_.manufacturer = "Leica";
        narrowSource_.format = "Leica GSI";
        narrowSource_.formatVersion = "GSI-8";
        narrowSource_.fileName = fileName_;
        wideSource_ = narrowSource_;
        wideSource_.formatVersion = "GSI-16";
    }

    void read(std::string_view bytes)
    {
        bytes = withoutByteOrderMark(bytes);
        // A block names at most one point, so the line count bounds the
        // points; reserving once spares the doubling copies of a vector that
        // is most of a coordinate list's memory. Counting costs one memchr pass.
        slots_.reserve(lineFeeds(bytes) + 1);
        codesAfterPoints_ = codesRecordedAfterPoints(bytes);
        std::size_t record = 0;
        std::size_t start = 0;
        while (start < bytes.size()) {
            std::size_t stop = start;
            while (stop < bytes.size() && bytes[stop] != '\n' && bytes[stop] != '\r') {
                ++stop;
            }
            ++record;
            readBlock(bytes.substr(start, stop - start), record);
            // "\r\n" ends one line, not two.
            if (stop + 1 < bytes.size() && bytes[stop] == '\r' && bytes[stop + 1] == '\n') {
                ++stop;
            }
            start = stop + 1;
        }
    }

    ReadResult finish();

  private:
    // ---- Diagnostics --------------------------------------------------------------

    void warn(std::size_t record, std::string message)
    {
        if (result_.warnings.size() < kMaxListedWarnings) {
            result_.warnings.push_back(ReadWarning{fileName_, record, std::move(message)});
        } else {
            ++unlistedWarnings_;
        }
    }

    void warnWord(const Block& block, const Word& word, std::string_view problem)
    {
        // A '-' is shown, being part of the value written; the '+' nearly
        // every word carries is not.
        const std::string written =
            word.sign == '-' ? "-" + std::string(word.data) : std::string(word.data);
        warn(block.record, "word " + std::to_string(word.index) + " " + shown(written) + " " +
                               std::string(problem) + "; that value was not read");
    }

    const survey::SourceRecord& templateFor(bool wide) const
    {
        return wide ? wideSource_ : narrowSource_;
    }

    survey::SourceRecord sourceAt(std::size_t record, bool wide) const
    {
        survey::SourceRecord source = templateFor(wide);
        source.recordNumber = record;
        return source;
    }

    // ---- Values -------------------------------------------------------------------

    void noteLinearUnit(survey::LinearUnit unit, std::size_t record)
    {
        if (linearUnit_ == survey::LinearUnit::Unknown) {
            linearUnit_ = unit;
        } else if (unit != linearUnit_) {
            mixedLinearUnits_.add(record);
        }
    }

    void noteAngularUnit(survey::AngularUnit unit, std::size_t record)
    {
        if (angularUnit_ == survey::AngularUnit::Unknown) {
            angularUnit_ = unit;
        } else if (unit != angularUnit_) {
            mixedAngularUnits_.add(record);
        }
    }

    std::optional<Number> numberIn(const Block& block, const Word& word)
    {
        std::optional<Number> value = numberOf(word.data, word.sign);
        if (!value) {
            warnWord(block, word, "is not a number");
        } else if (value->explicitPoint) {
            explicitPoints_.add(block.record, word.data);
        }
        return value;
    }

    // A length word in metres. `unitWhenEmpty` is the unit a word may leave
    // unstated, where the specification's own example does (words 58 and 59).
    std::optional<double> lengthOf(const Block& block, const Word& word, char unitWhenEmpty = 0)
    {
        char unit = unitDigit(word);
        if (unit == '.' && unitWhenEmpty != 0) {
            unit = unitWhenEmpty;
        }
        int decimals = 0;
        bool feet = false;
        switch (unit) {
        case '0':
            decimals = 3;
            break;
        case '1':
            decimals = 3;
            feet = true;
            break;
        case '6':
            decimals = 4;
            break;
        case '7':
            decimals = 4;
            feet = true;
            break;
        case '8':
            decimals = 5;
            break;
        default:
            warnWord(block, word,
                     "has unit digit '" + std::string(1, unit) + "', which is not a length unit");
            return std::nullopt;
        }
        const std::optional<Number> value = numberIn(block, word);
        if (!value) {
            return std::nullopt;
        }
        // Counted apart from angles: a file that writes its own points in
        // lengths and leaves a few to the unit digit has probably lost a
        // character in those few (finish() names them).
        if (value->explicitPoint) {
            ++pointedLengths_;
        } else {
            unpointedLengths_.add(block.record, word.data);
        }
        double metres = value->explicitPoint ? value->value
                                             : value->value / kPowersOfTen[static_cast<std::size_t>(
                                                                  decimals)];
        if (feet) {
            // GSI says "feet" and not which. The US survey foot: see finish().
            feet_.add(block.record);
            noteLinearUnit(survey::LinearUnit::UsSurveyFeet, block.record);
            metres = katana::math::toMetres(metres, katana::math::units::kUsSurveyFoot);
        } else {
            noteLinearUnit(survey::LinearUnit::Metres, block.record);
        }
        return metres;
    }

    // A circle reading (word 21 or 22) in radians, as read: not wrapped, not
    // turned into a zenith. `sixty` is set when it was a 60-second word read as
    // the next minute (SIXTY SECONDS, at the top), and empty otherwise. A
    // negative word 22 is refused here; a negative word 21 is returned as it
    // is, and shotBlock wraps it onto the circle (SIGN, at the top).
    std::optional<double> angleOf(const Block& block, const Word& word,
                                  std::optional<SixtySeconds>& sixty)
    {
        sixty.reset();
        const char unit = unitDigit(word);
        if (unit == '4') {
            return sexagesimal(block, word, sixty);
        }
        double toRadians = 0.0;
        std::size_t decimals = 5;
        double fullCircle = 0.0;
        const char* circle = "";
        survey::AngularUnit declared = survey::AngularUnit::Unknown;
        switch (unit) {
        case '2':
            toRadians = kPi / 200.0;
            fullCircle = 400.0;
            circle = "400 gon";
            declared = survey::AngularUnit::Gons;
            break;
        case '3':
            toRadians = kPi / 180.0;
            fullCircle = 360.0;
            circle = "360 degrees";
            declared = survey::AngularUnit::DecimalDegrees;
            break;
        case '5':
            toRadians = kTwoPi / 6400.0;
            decimals = 4;
            fullCircle = 6400.0;
            circle = "6400 mil";
            declared = survey::AngularUnit::Mils;
            break;
        default:
            warnWord(block, word,
                     "has unit digit '" + std::string(1, unit) + "', which is not an angle unit");
            return std::nullopt;
        }
        const std::optional<Number> value = numberIn(block, word);
        if (!value) {
            return std::nullopt;
        }
        const double inUnit =
            value->explicitPoint ? value->value : value->value / kPowersOfTen[decimals];
        // The unit digit names the circle (page 6: 400 gon, 360 degrees
        // decimal or sexagesimal, 6400 mil), and no circle reading is past
        // it: such a word is damaged, and wrapping it onto the circle would
        // hide that. The full circle itself is the zero, as a rounding up at
        // the top of the circle writes it.
        if (std::abs(inUnit) > fullCircle) {
            warnWord(block, word,
                     "is past a full circle (" + std::string(circle) + "), which no circle "
                     "reading is");
            return std::nullopt;
        }
        if (refusedAsNegativeZenith(block, word, inUnit)) {
            return std::nullopt;
        }
        noteAngularUnit(declared, block.record);
        return inUnit * toRadians;
    }

    // A vertical reading below zero is no zenith angle (SIGN, at the top):
    // refused at its record, and counted for the note on the vertical angle
    // setting in finish(). `value` is the reading in any unit; -0 is not below
    // zero, and is read.
    bool refusedAsNegativeZenith(const Block& block, const Word& word, double value)
    {
        if (word.index != 22 || value >= 0.0) {
            return false;
        }
        warnWord(block, word,
                 "is negative, which no zenith angle is: an instrument set to read V from the "
                 "horizon or in percent (GSI ONLINE SET 44) writes such a value below the horizon");
        negativeZeniths_.add(block.record);
        return true;
    }

    // DDDMMSSs: degrees, minutes, seconds and tenths - DDD.MMSSs with the point
    // unwritten. A file that writes the point is read the same way around it.
    // Seconds of 60.0 are the next minute where the word writes them in their
    // place (SIXTY SECONDS, at the top), and `sixty` then says so.
    std::optional<double> sexagesimal(const Block& block, const Word& word,
                                      std::optional<SixtySeconds>& sixty)
    {
        // A word with no digit - "." - writes no angle. The missing digits
        // supplied below would make it 000 00 00.0, a reading like any other;
        // numberOf refuses the same word in the other units.
        if (word.data.find_first_of("0123456789") == std::string_view::npos) {
            warnWord(block, word, "is not a sexagesimal angle: it has no digit");
            return std::nullopt;
        }
        std::string_view whole = word.data;
        std::string_view fraction;
        const std::size_t point = whole.find('.');
        if (point != std::string_view::npos) {
            explicitPoints_.add(block.record, word.data);
            fraction = whole.substr(point + 1);
            whole = whole.substr(0, point);
        } else if (whole.size() > 5) {
            fraction = whole.substr(whole.size() - 5);
            whole = whole.substr(0, whole.size() - 5);
        } else {
            fraction = whole;
            whole = {};
        }
        const auto digitsOf = [](std::string_view text, std::size_t from, std::size_t count) {
            int value = 0;
            for (std::size_t i = from; i < from + count; ++i) {
                // Missing digits on the right are zeros: "123.45" is 123 45 00.
                const char c = i < text.size() ? text[i] : '0';
                if (!isDigit(c)) {
                    return -1;
                }
                value = value * 10 + (c - '0');
            }
            return value;
        };
        // Without a written point a short word is right-aligned: "12345" is
        // 0 degrees 12' 34.5", so the five places are filled from the left.
        std::string padded;
        if (point == std::string_view::npos && fraction.size() < 5) {
            padded.assign(5 - fraction.size(), '0');
            padded += fraction;
            fraction = padded;
        }
        std::int64_t degrees = 0;
        for (const char c : whole) {
            if (!isDigit(c)) {
                warnWord(block, word, "is not a sexagesimal angle");
                return std::nullopt;
            }
            degrees = degrees * 10 + (c - '0');
        }
        int minutes = digitsOf(fraction, 0, 2);
        int seconds = digitsOf(fraction, 2, 2);
        // Tenths and anything finer, as the decimal part of the seconds.
        double finer = 0.0;
        double scale = 0.1;
        bool pastWholeSeconds = false; // a digit other than 0 after the seconds
        for (std::size_t i = 4; i < fraction.size(); ++i, scale /= 10.0) {
            if (!isDigit(fraction[i])) {
                warnWord(block, word, "is not a sexagesimal angle");
                return std::nullopt;
            }
            finer += (fraction[i] - '0') * scale;
            pastWholeSeconds = pastWholeSeconds || fraction[i] != '0';
        }
        if (minutes < 0 || seconds < 0) {
            warnWord(block, word, "is not a sexagesimal angle");
            return std::nullopt;
        }
        if (minutes >= 60) {
            warnWord(block, word, "is not a sexagesimal angle: the minutes run to 59");
            return std::nullopt;
        }
        if (seconds > 60 || (seconds == 60 && pastWholeSeconds)) {
            warnWord(block, word,
                     "is not a sexagesimal angle: the seconds run to 59, and to 60.0 only where "
                     "a writer rounded up without carrying the minute");
            return std::nullopt;
        }
        std::optional<SixtySeconds> carried;
        if (seconds == 60) {
            // Only a 60 the word writes itself, both digits in their place. A
            // word not its block's width was aligned from the right above, and
            // a written point's missing digits were supplied as zeros: either
            // can make a 60 of digits that were never the seconds.
            const std::size_t width = block.wide ? 16U : 8U;
            if (point != std::string_view::npos && fraction.size() < 4) {
                warnWord(block, word,
                         "is not a sexagesimal angle: it writes one digit of the seconds after "
                         "its point, and 60 seconds are read as the next minute only where both "
                         "are written");
                return std::nullopt;
            }
            if (point == std::string_view::npos && word.data.size() != width) {
                warnWord(block, word,
                         "is not a sexagesimal angle: 60 seconds are read as the next minute "
                         "only in a word of its block's width, " +
                             std::to_string(width) +
                             " characters, where each digit is in its place, and this one "
                             "has " +
                             std::to_string(word.data.size()));
                return std::nullopt;
            }
            carried = SixtySeconds{word.index, word.sign == '-', degrees, minutes,
                                   fraction.size() - 4};
            // Carried in whole numbers, so that the angle is the same double
            // as the word written with its carry made; adding 60/3600 of a
            // degree as a real is not.
            seconds = 0;
            if (++minutes == 60) {
                minutes = 0;
                ++degrees;
            }
        }
        // One full circle at most (angleOf says why); 360 00 00.0, as written
        // or as 359 59 60.0 carried, is the circle's zero.
        const bool pastZero = minutes != 0 || seconds != 0 || pastWholeSeconds;
        if (degrees > 360 || (degrees == 360 && pastZero)) {
            warnWord(block, word, "is past a full circle (360 degrees), which no circle reading is");
            return std::nullopt;
        }
        double degreesValue = static_cast<double>(degrees) + minutes / 60.0 +
                              (seconds + finer) / 3600.0;
        if (word.sign == '-') {
            degreesValue = -degreesValue;
        }
        // After the carry, so that -000 00 60.0 is as negative as -000 01
        // 00.0; and before `sixty` is set, which is for a word that is read.
        if (refusedAsNegativeZenith(block, word, degreesValue)) {
            return std::nullopt;
        }
        sixty = carried;
        noteAngularUnit(survey::AngularUnit::DegreesMinutesSeconds, block.record);
        return degreesValue * (kPi / 180.0);
    }

    // Word 51: "+0220+002" is 220 ppm and a prism constant of 2 mm, each with
    // its own sign (GET 51, and page 6: "certain data blocks ... carry more than
    // 1 value ... with a sign before each single value").
    void ppmAndPrism(Block& block, const Word& word)
    {
        const std::size_t cut = word.data.find_first_of("+-", 1);
        if (cut == std::string_view::npos || cut + 1 >= word.data.size()) {
            warnWord(block, word, "does not hold a ppm and a prism constant, each signed");
            return;
        }
        const std::optional<Number> ppm = numberOf(word.data.substr(0, cut), word.sign);
        const std::optional<Number> millimetres =
            numberOf(word.data.substr(cut + 1), word.data[cut]);
        if (!ppm || !millimetres || ppm->explicitPoint || millimetres->explicitPoint) {
            warnWord(block, word, "does not hold a ppm and a prism constant, each signed");
            return;
        }
        block.ppm = ppm->value;
        block.prismConstant = millimetres->value / 1000.0;
    }

    // The text of a code information (42-49) or remark (71-79) word. One of
    // nothing but zeros is EMPTY, not the code "0": GSI right-justifies text
    // and pads it with '0', so that is how an empty value is written - Leica's
    // Format Manager writes an unset code information word "43....+00000000"
    // (Reference Guide V1.0, Annex 2) - and read as "0" it gives every point
    // of a job that records word 71 empty on each shot one code, and one
    // feature through all of them. The code "0" would be written the same,
    // so the import says how many it read as empty.
    std::string_view codeText(const Block& block, const Word& word)
    {
        if (word.data.find_first_not_of('0') == std::string_view::npos) {
            zeroCodeWords_.addMade(block.record,
                                   [&] { return "word " + std::to_string(word.index); });
            return {};
        }
        return textOf(word.data);
    }

    void heightWord(Block& block, const Word& word, std::optional<double>& into)
    {
        // Letters where a height belongs. Some exporters write the point code
        // into a height word; see pointCode().
        bool letters = false;
        for (const char c : word.data) {
            letters = letters || (!isDigit(c) && c != '.');
        }
        if (letters) {
            block.heightText = textOf(word.data);
            block.heightTextWord = word.index;
            return;
        }
        into = lengthOf(block, word);
    }

    void decodeWord(Block& block, const Word& word, bool first)
    {
        switch (word.index) {
        case 11:
        case 41:
            if (!first) {
                warnWord(block, word, "is a block's first word, found inside a block");
                return;
            }
            block.name = textOf(word.data);
            return;
        case 12:
            block.serial = textOf(word.data);
            return;
        case 13:
            block.type = textOf(word.data);
            return;
        case 18:
            block.time18 = word.data;
            return;
        case 19:
            block.time19 = word.data;
            return;
        case 21:
            block.measurementWords = true;
            block.hz = angleOf(block, word, block.hzSixty);
            block.hzInputMode = inputMode(word);
            return;
        case 22:
            block.measurementWords = true;
            block.v = angleOf(block, word, block.vSixty);
            block.vIndex = infoAt(word, 4);
            return;
        case 31:
            block.measurementWords = true;
            block.slope = lengthOf(block, word);
            return;
        case 32:
            block.measurementWords = true;
            block.horizontal = lengthOf(block, word);
            return;
        case 33:
            block.measurementWords = true;
            block.heightDifference = lengthOf(block, word);
            return;
        case 51:
            ppmAndPrism(block, word);
            return;
        case 58:
            // GET 58: "58..16+00000020" is 2 mm; PUT 58 leaves the unit out and
            // means tenths of a millimetre.
            block.prismConstant = lengthOf(block, word, '6');
            return;
        case 59: {
            // GET 59: "59..16+02200000" is 220 ppm - the unit digit's decimals,
            // four when it is left out (PUT 59).
            char unit = unitDigit(word);
            unit = unit == '.' ? '6' : unit;
            const std::size_t decimals = (unit == '6' || unit == '7') ? 4 : unit == '8' ? 5 : 3;
            if (const std::optional<Number> value = numberIn(block, word)) {
                block.ppm =
                    value->explicitPoint ? value->value : value->value / kPowersOfTen[decimals];
            }
            return;
        }
        case 81:
            block.e = lengthOf(block, word);
            block.targetInputMode = inputMode(word);
            return;
        case 82:
            block.n = lengthOf(block, word);
            block.targetInputMode = inputMode(word);
            return;
        case 83:
            block.h = lengthOf(block, word);
            block.targetInputMode = inputMode(word);
            return;
        case 84:
            block.e0 = lengthOf(block, word);
            block.stationInputMode = inputMode(word);
            return;
        case 85:
            block.n0 = lengthOf(block, word);
            block.stationInputMode = inputMode(word);
            return;
        case 86:
            block.h0 = lengthOf(block, word);
            block.stationInputMode = inputMode(word);
            return;
        case 87:
            heightWord(block, word, block.reflectorHeight);
            return;
        case 88:
            heightWord(block, word, block.instrumentHeight);
            return;
        default:
            break;
        }
        if (word.index >= 42 && word.index <= 49) {
            block.codeInfo[static_cast<std::size_t>(word.index - 42)] = codeText(block, word);
            return;
        }
        if (word.index >= 71 && word.index <= 79) {
            // Words 71-79 are read in a point's block, as its code (71) and
            // remarks (72-79). In a code block they could be those or more
            // about the block's own code, and nothing tells which, so they
            // are counted as not read rather than guessed at.
            if (block.kind == 41) {
                remarksInCodeBlocks_.addMade(block.record,
                                             [&] { return "word " + std::to_string(word.index); });
                return;
            }
            block.remarks[static_cast<std::size_t>(word.index - 71)] = codeText(block, word);
            return;
        }
        if (word.index >= 100) {
            block.levelling = true; // said once for the file, in finish()
            return;
        }
        // Counted and reported by index: a word this reader does not use is
        // said once for the file, never dropped without a word.
        if (word.index >= 0 && word.index < static_cast<int>(unread_.size())) {
            unread_[static_cast<std::size_t>(word.index)].add(block.record);
        }
    }

    // ---- Blocks -------------------------------------------------------------------

    void readBlock(std::string_view line, std::size_t record)
    {
        Block block;
        block.record = record;
        bool first = true;
        std::size_t position = 0;
        while (true) {
            while (position < line.size() && isBlank(line[position])) {
                ++position;
            }
            if (position >= line.size()) {
                break;
            }
            std::size_t end = position;
            while (end < line.size() && !isBlank(line[end])) {
                ++end;
            }
            std::string_view token = line.substr(position, end - position);
            position = end;
            if (first && token.front() == '*') {
                block.wide = true;
                token.remove_prefix(1);
            }
            // A DOS end-of-file mark on the last line is not a word.
            if (token == "\x1a") {
                continue;
            }
            const std::optional<Word> word = parseWord(token);
            if (!word) {
                if (first) {
                    warn(record, "does not begin with a GSI word (" + shown(token) +
                                     "); the line was not read");
                    ++result_.recordsSkipped;
                    return;
                }
                warn(record, shown(token) + " is not a GSI word; the rest of the block was read");
                continue;
            }
            if (first) {
                if (word->index != 11 && word->index != 41) {
                    warn(record, "begins with word " + std::to_string(word->index) +
                                     ", where a GSI block begins with word 11 (a point) or 41 (a "
                                     "code); the block was not read");
                    ++result_.recordsSkipped;
                    return;
                }
                block.kind = word->index;
            }
            if (word->data.size() != (block.wide ? 16U : 8U)) {
                oddWidth_.add(record, token);
            }
            decodeWord(block, *word, first);
            first = false;
        }
        if (first) {
            return; // a blank line is not a record
        }
        ++result_.recordsRead;
        ++(block.wide ? wideBlocks_ : narrowBlocks_);
        if (block.kind == 41) {
            codeBlock(block);
        } else if (block.levelling) {
            // A digital level's block: its point is named, its staff readings
            // and distances are not total station shots and are not read.
            levelling_.add(block.record, block.name);
            block.horizontal.reset();
            block.heightDifference.reset();
            block.slope.reset();
            pointBlock(block);
        } else if (block.measures()) {
            shotBlock(block);
        } else if (block.stationCoordinates() || block.instrumentHeight) {
            stationBlock(block);
        } else {
            pointBlock(block);
        }
        if (block.reflectorHeight) {
            reflectorHeight_ = block.reflectorHeight;
        }
    }

    void codeBlock(const Block& block)
    {
        if (block.measurementWords) {
            warn(block.record, "a code block holds measurement words; they were not read");
        }
        // A digital level's "special code block" names the levelling method
        // ("410000+?......1" is line levelling BF: GSI ONLINE's DNA section)
        // and starts a line; it is not a point's code.
        if (!block.name.empty() && block.name.front() == '?') {
            specialCodes_.add(block.record, block.name);
            return;
        }
        CodeBlock code;
        code.code = std::string(block.name);
        for (std::size_t i = 0; i < block.codeInfo.size(); ++i) {
            code.info[i] = std::string(block.codeInfo[i]);
        }
        code.record = block.record;
        ++codeBlocks_;
        // Recorded after its point (codesRecordedAfterPoints): the point
        // before is the one. Its place in the features waited for this.
        if (codesAfterPoints_ && lastPointSlot_) {
            if (!afterPointsSaid_) {
                afterPointsSaid_ = true;
                warn(block.record,
                     "the file begins with a point block and ends with a code block, as an "
                     "instrument set to record codes after their point (<Rec Free Code: After "
                     "Point>) writes a job, so each code block was attached to the point before "
                     "it");
            }
            attachCode(*lastPointSlot_, code, codesSincePoint_++);
            return;
        }
        pendingCodes_.push_back(std::move(code));
    }

    std::size_t slotFor(std::string_view id, std::size_t record, bool wide)
    {
        if (const std::size_t found = slotIndex_.find(id); found != IdIndex::kAbsent) {
            return found;
        }
        PointSlot slot;
        slot.id = std::string(id);
        slot.record = record;
        slot.wide = wide;
        slots_.push_back(std::move(slot));
        slotIndex_.insert(id, slots_.size() - 1);
        return slots_.size() - 1;
    }

    // The input mode of a coordinate word says how the value came to be: keyed
    // in (1), computed by an on-board program (4), or measured (0, 2, 3) -
    // which for a target's coordinates means reduced from the shot. For a
    // STATION's coordinates "measured" says nothing: they are not measured at
    // the station, so it stays Unknown.
    static survey::CoordinateSource sourceOf(char mode, bool station)
    {
        switch (mode) {
        case '1':
            return survey::CoordinateSource::Entered;
        case '4':
            return survey::CoordinateSource::Calculated;
        case '0':
        case '2':
        case '3':
            return station ? survey::CoordinateSource::Unknown
                           : survey::CoordinateSource::FieldObserved;
        default:
            return survey::CoordinateSource::Unknown;
        }
    }

    void placePoint(std::size_t index, const std::optional<double>& e,
                  const std::optional<double>& n, const std::optional<double>& h, char mode,
                  bool station, const Block& block)
    {
        PointSlot& slot = slots_[index];
        if (!e || !n) {
            // A height alone cannot place a point; it is kept, not dropped.
            if (h) {
                slot.metadata.insert_or_assign("height without a position", metresText(*h));
                heightOnly_.add(block.record, slot.id);
            } else {
                partialPosition_.add(block.record, slot.id);
            }
            return;
        }
        if (!slot.positioned) {
            slot.positioned = true;
            slot.easting = *e;
            slot.northing = *n;
            slot.elevation = h;
            slot.coordinateSource = sourceOf(mode, station);
            slot.record = block.record;
            slot.wide = block.wide;
            return;
        }
        const double horizontal = std::hypot(*e - slot.easting, *n - slot.northing);
        const double vertical = (h && slot.elevation) ? std::abs(*h - *slot.elevation) : 0.0;
        if (horizontal > kSamePositionTolerance || vertical > kSamePositionTolerance) {
            warn(block.record, "point " + slot.id + " is given coordinates again, " +
                                   metresText(horizontal) + " m horizontally and " +
                                   metresText(vertical) + " m in height from those of record " +
                                   std::to_string(slot.record) + "; the first are kept");
        }
        if (!slot.elevation && h) {
            slot.elevation = h;
        }
    }

    void addCode(std::size_t index, std::string_view code, std::string_view whereFrom)
    {
        PointSlot& slot = slots_[index];
        if (slot.code.empty()) {
            slot.code = std::string(code);
        } else if (slot.code != code) {
            slot.metadata.emplace(std::string(whereFrom), std::string(code));
        }
    }

    // A point's code: word 71 in its own block; else its code block - the one
    // before it, or after it in a file recorded After Point (see leica.hpp);
    // else text the file wrote into a height word. Whichever is not the code
    // is kept in the point's metadata.
    void pointCode(std::size_t index, const Block& block)
    {
        // The point before this one has every code it will get: in a file
        // recorded After Point its code blocks came between the two.
        flushFeature();
        if (!block.remarks[0].empty()) {
            addCode(index, block.remarks[0], "remark 1");
        }
        for (std::size_t i = 1; i < block.remarks.size(); ++i) {
            if (!block.remarks[i].empty()) {
                slots_[index].metadata.insert_or_assign("remark " + std::to_string(i + 1),
                                                        std::string(block.remarks[i]));
            }
        }
        // Code information (42-49) in the point's own block rather than a
        // code block's: "one may create any kind of GSI formats" (Leica's
        // Format Manager Reference Guide V1.0, Annex 2), and here the point is
        // the one thing it can be about.
        for (std::size_t i = 0; i < block.codeInfo.size(); ++i) {
            if (!block.codeInfo[i].empty()) {
                const std::string key = "code information " + std::to_string(i + 1);
                slots_[index].metadata.insert_or_assign(key, std::string(block.codeInfo[i]));
            }
        }
        for (std::size_t c = 0; c < pendingCodes_.size(); ++c) {
            attachCode(index, pendingCodes_[c], c);
        }
        pendingCodes_.clear();
        if (!block.heightText.empty()) {
            const std::string key = "word " + std::to_string(block.heightTextWord) + " text";
            if (slots_[index].code.empty()) {
                slots_[index].code = std::string(block.heightText);
                heightTextAsCode_.add(block.record, block.heightText);
            } else {
                slots_[index].metadata.insert_or_assign(key, std::string(block.heightText));
                heightTextKept_.add(block.record, block.heightText);
            }
        }
        featureStep_ = FeatureStep{index, block.record, block.wide};
        lastPointSlot_ = index;
        codesSincePoint_ = 0;
    }

    void attachCode(std::size_t index, const CodeBlock& code, std::size_t ordinal)
    {
        PointSlot& slot = slots_[index];
        const std::string prefix =
            ordinal == 0 ? std::string("code block") : "code block " + std::to_string(ordinal + 1);
        if (slot.code.empty()) {
            slot.code = code.code;
        } else if (slot.code != code.code) {
            slot.metadata.insert_or_assign(prefix, code.code);
        }
        for (std::size_t i = 0; i < code.info.size(); ++i) {
            if (!code.info[i].empty()) {
                slot.metadata.insert_or_assign(prefix + " information " + std::to_string(i + 1),
                                               code.info[i]);
            }
        }
    }

    void flushFeature()
    {
        if (featureStep_) {
            const FeatureStep step = *featureStep_;
            featureStep_.reset();
            feature(step.slot, step.record, step.wide);
        }
    }

    // One feature per run of consecutive points with the same code: GSI has no
    // string numbers, so a change of code (or an uncoded point) ends a string.
    void feature(std::size_t index, std::size_t record, bool wide)
    {
        const PointSlot& slot = slots_[index];
        if (slot.code.empty()) {
            closeFeature();
            return;
        }
        if (run_ && run_->code == slot.code) {
            if (run_->pointIds.back() != slot.id) {
                run_->pointIds.push_back(slot.id);
            }
            return;
        }
        closeFeature();
        survey::SurveyFeature started;
        started.code = slot.code;
        started.pointIds.push_back(slot.id);
        started.source = sourceAt(record, wide);
        run_ = std::move(started);
    }

    void closeFeature()
    {
        if (run_) {
            result_.project.features.push_back(std::move(*run_));
            run_.reset();
        }
    }

    // ---- Setups ---------------------------------------------------------------------

    // A block that gives the station's coordinates or the instrument height and
    // measures nothing. It is a SETUP only if measurements follow it; one
    // followed by another such block (or by nothing) was a list of coordinates
    // - the shape of the owner's "11 + 84-86 + 88" files - and the point simply
    // keeps them.
    void stationBlock(Block& block)
    {
        discardPendingSetup();
        const std::size_t index = slotFor(block.name, block.record, block.wide);
        if (block.stationCoordinates()) {
            placePoint(index, block.e0, block.n0, block.h0, block.stationInputMode, true, block);
        } else if (block.targetCoordinates()) {
            placePoint(index, block.e, block.n, block.h, block.targetInputMode, false, block);
        }
        pointCode(index, block);
        noteSettings(block, false);
        stationBlockPpm_ = block.ppm.has_value();
        stationBlockPrism_ = block.prismConstant.has_value();
        PendingSetup setup;
        setup.slot = index;
        setup.instrumentHeight = block.instrumentHeight;
        setup.record = block.record;
        setup.wide = block.wide;
        timeOf(block, setup.time, setup.timeWithoutYear);
        pending_ = std::move(setup);
    }

    void discardPendingSetup()
    {
        if (!pending_) {
            return;
        }
        coordinateRecords_.add(pending_->record, slots_[pending_->slot].id);
        if (pending_->instrumentHeight) {
            slots_[pending_->slot].metadata.insert_or_assign(
                "instrument height", metresText(*pending_->instrumentHeight));
        }
        pending_.reset();
    }

    // A setup's id: its point's id for the first setup on the point, "ID (n)"
    // for the n-th. A point id from the file holds no blank (a blank ends a
    // GSI word) and the one id made up here, "GSI station N", no bracket, so
    // an "ID (n)" is never a point's id nor another point's setup's: a count
    // per point makes the id with no search. A search from "(2)" upwards
    // costs the k-th setup on a point k lookups - a monitoring pillar set up
    // every half hour for a year is some 17,500 setups, 150 million lookups.
    std::string stationIdFor(std::size_t slot)
    {
        PointSlot& point = slots_[slot];
        const std::size_t n = ++point.setups;
        return n == 1 ? point.id : point.id + " (" + std::to_string(n) + ")";
    }

    void openSetup(std::size_t slot, std::optional<double> instrumentHeight, std::size_t record,
                   bool wide, const survey::SurveyTimestamp& time,
                   const std::string& timeWithoutYear)
    {
        closeSetup();
        survey::SurveyStation station;
        station.setup.pointId = slots_[slot].id;
        station.setup.id = stationIdFor(slot);
        station.setup.instrumentHeight = instrumentHeight.value_or(0.0);
        station.source = sourceAt(record, wide);
        survey::InstrumentSettings& instrument = station.instrument;
        instrument.model = type_;
        instrument.serialNumber = serial_;
        // What the instrument was last set to carries over - it keeps its
        // settings from one setup to the next - until this setup's own blocks
        // state theirs (noteSettings).
        if (ppm_) {
            instrument.atmosphericPpm = ppm_;
            instrument.atmosphericPpmState = survey::CorrectionState::Applied;
        }
        if (prism_) {
            instrument.prismConstant = prism_;
            instrument.prismConstantState = survey::CorrectionState::Applied;
        }
        ppmCarried_ = ppm_.has_value() && !stationBlockPpm_;
        prismCarried_ = prism_.has_value() && !stationBlockPrism_;
        instrument.time = time;
        if (!timeWithoutYear.empty()) {
            station.metadata.emplace("time (no year recorded)", timeWithoutYear);
        }
        // Setups in one job are alike in size; starting from the last one's
        // count saves the growth copies of this one's observations.
        station.observations.reserve(lastSetupObservations_);
        result_.project.stations.push_back(std::move(station));
        active_ = result_.project.stations.size() - 1;
        activeSlot_ = slot;
        activeHeight_ = instrumentHeight;
        pointings_ = 0;
        hzInputMode_ = 0;
        vIndex_ = 0;
    }

    // What the whole setup's shots said about the instrument's own corrections,
    // written once when the setup ends.
    void closeSetup()
    {
        if (!active_) {
            return;
        }
        survey::SurveyStation& station = result_.project.stations[*active_];
        lastSetupObservations_ = station.observations.size();
        // Input mode 2 / 3 of an angle word: the horizontal (collimation and
        // tilt) correction on / off. Position 4: the automatic vertical index
        // off (0) or operating (1, 3).
        if (hzInputMode_ == 'x') {
            station.metadata.emplace("horizontal correction", "varies within the setup");
        } else if (hzInputMode_ == '2' || hzInputMode_ == '3') {
            station.metadata.emplace("horizontal correction", hzInputMode_ == '2' ? "on" : "off");
        }
        if (vIndex_ == 'x') {
            station.metadata.emplace("automatic vertical index", "varies within the setup");
        } else if (vIndex_ == '1' || vIndex_ == '3') {
            station.metadata.emplace("automatic vertical index", "on");
        } else if (vIndex_ == '0') {
            station.metadata.emplace("automatic vertical index", "off");
        }
        if (!station.instrument.atmosphericPpm) {
            ++setupsWithoutPpm_;
        }
        if (!station.instrument.prismConstant) {
            ++setupsWithoutPrism_;
        }
        if (!activeHeight_) {
            ++setupsWithoutHeight_;
        }
        active_.reset();
    }

    static void track(char& state, char value)
    {
        if (value == '.' || state == 'x') {
            return;
        }
        state = state == 0 ? value : (state == value ? state : 'x');
    }

    // Words 12, 13, 51, 58, 59 change what the instrument is set to from here
    // on; the setup that is open records them. A station block's words are for
    // the setup it opens (openSetup takes them from here), never for the one
    // before it, hence `toOpenSetup`.
    void noteSettings(const Block& block, bool toOpenSetup)
    {
        survey::SurveyStation* station =
            toOpenSetup && active_ && !pending_ ? &result_.project.stations[*active_] : nullptr;
        if (block.ppm) {
            if (station != nullptr) {
                survey::InstrumentSettings& instrument = station->instrument;
                if (!instrument.atmosphericPpm || ppmCarried_) {
                    ppmCarried_ = false;
                    instrument.atmosphericPpm = block.ppm;
                    instrument.atmosphericPpmState = survey::CorrectionState::Applied;
                } else if (*instrument.atmosphericPpm != *block.ppm) {
                    warn(block.record, "the ppm changes from " +
                                           metresText(*instrument.atmosphericPpm) + " to " +
                                           metresText(*block.ppm) + " within setup " +
                                           station->setup.id +
                                           "; the setup records the first, and the distances "
                                           "carry whichever the instrument applied");
                }
            }
            ppm_ = block.ppm;
        }
        if (block.prismConstant) {
            if (station != nullptr && (!station->instrument.prismConstant || prismCarried_)) {
                prismCarried_ = false;
                station->instrument.prismConstant = block.prismConstant;
                station->instrument.prismConstantState = survey::CorrectionState::Applied;
            }
            prism_ = block.prismConstant;
        }
        if (!block.serial.empty()) {
            serial_ = std::string(block.serial);
            if (station != nullptr && station->instrument.serialNumber.empty()) {
                station->instrument.serialNumber = serial_;
            }
        }
        if (!block.type.empty()) {
            type_ = std::string(block.type);
            if (station != nullptr && station->instrument.model.empty()) {
                station->instrument.model = type_;
            }
        }
    }

    // Word 19 is MMDDhhmm; word 18 is YYSSmmm (year, seconds, milliseconds) -
    // GET 19 and the TPS time format [YY.SS.mSmSmS]. The two-digit year is read
    // as POSIX strptime's %y does: 69-99 are 1969-1999, 00-68 are 2000-2068.
    void timeOf(const Block& block, survey::SurveyTimestamp& time, std::string& withoutYear)
    {
        if (block.time19.empty()) {
            if (!block.time18.empty()) {
                timeWords_.add(block.record);
            }
            return;
        }
        const auto field = [](std::string_view text, std::size_t from, std::size_t count) {
            int value = 0;
            for (std::size_t i = from; i < from + count; ++i) {
                if (i >= text.size() || !isDigit(text[i])) {
                    return -1;
                }
                value = value * 10 + (text[i] - '0');
            }
            return value;
        };
        const std::string_view t19 =
            block.time19.size() >= 8 ? block.time19.substr(block.time19.size() - 8) : "";
        const int month = field(t19, 0, 2);
        const int day = field(t19, 2, 2);
        const int hour = field(t19, 4, 2);
        const int minute = field(t19, 6, 2);
        if (month < 1 || month > 12 || day < 1 || day > 31 || hour < 0 || hour > 23 ||
            minute < 0 || minute > 59) {
            warn(block.record, "word 19 " + shown(block.time19) +
                                   " is not a time (MMDDhhmm); the time was not read");
            return;
        }
        const std::string_view t18 =
            block.time18.size() >= 7 ? block.time18.substr(block.time18.size() - 7) : "";
        const int year = field(t18, 0, 2);
        const int second = field(t18, 2, 2);
        const int millisecond = field(t18, 4, 3);
        if (year < 0 || second < 0 || second > 59 || millisecond < 0) {
            const auto two = [](int value) {
                return std::string(1, static_cast<char>('0' + value / 10)) +
                       static_cast<char>('0' + value % 10);
            };
            withoutYear = two(month) + "-" + two(day) + " " + two(hour) + ":" + two(minute);
            if (!block.time18.empty()) {
                warn(block.record, "word 18 " + shown(block.time18) +
                                       " is not a year and seconds (YYSSmmm); the time was read "
                                       "without them");
            }
            return;
        }
        time.year = year >= 69 ? 1900 + year : 2000 + year;
        time.month = month;
        time.day = day;
        time.hour = hour;
        time.minute = minute;
        time.second = second + millisecond / 1000.0;
    }

    void implicitSetup(const Block& block)
    {
        std::string id = "GSI station";
        for (std::size_t n = 2; slotIndex_.find(id) != IdIndex::kAbsent; ++n) {
            id = "GSI station " + std::to_string(n);
        }
        warn(block.record, "measurements begin before any station block, so the instrument's "
                           "position is not recorded; they are kept under a setup at a point "
                           "named '" + id + "' with no coordinates");
        // The index keys by view: the made-up name must outlive it.
        const std::size_t slot = slotFor(madeUpIds_.emplace_back(std::move(id)), block.record,
                                         block.wide);
        openSetup(slot, std::nullopt, block.record, block.wide, {}, {});
    }

    void shotBlock(Block& block)
    {
        if (pending_) {
            const PendingSetup setup = std::move(*pending_);
            pending_.reset();
            openSetup(setup.slot, setup.instrumentHeight, setup.record, setup.wide, setup.time,
                      setup.timeWithoutYear);
        } else if (!active_) {
            implicitSetup(block);
        }
        noteSettings(block, true);
        survey::SurveyStation& station = result_.project.stations[*active_];

        if (!block.time19.empty() || !block.time18.empty()) {
            if (!station.instrument.time.known() && pointings_ == 0) {
                std::string withoutYear;
                timeOf(block, station.instrument.time, withoutYear);
                if (!withoutYear.empty()) {
                    station.metadata.emplace("time (no year recorded)", withoutYear);
                }
            } else {
                timeWords_.add(block.record);
            }
        }
        if (block.stationCoordinates()) {
            placePoint(activeSlot_, block.e0, block.n0, block.h0, block.stationInputMode, true,
                     block);
        }
        double instrumentHeight = activeHeight_.value_or(0.0);
        if (block.instrumentHeight) {
            instrumentHeight = *block.instrumentHeight;
            if (!activeHeight_) {
                activeHeight_ = block.instrumentHeight;
                station.setup.instrumentHeight = *block.instrumentHeight;
            }
        }

        if (block.name == slots_[activeSlot_].id) {
            warn(block.record, "measures point " + slots_[activeSlot_].id +
                                   ", the point the instrument stands on; the measurement was "
                                   "not read");
            ++result_.recordsSkipped;
            --result_.recordsRead;
            return;
        }
        const std::size_t target = slotFor(block.name, block.record, block.wide);
        // GSI has no backsight word. A setup is made on the instrument as a
        // station and then an orientation shot, and the file records them in
        // that order; so a setup's FIRST shot, when it is to a point the file
        // gives coordinates, is taken as its backsight. Orienting on a point
        // of known position is sound whatever the observer called the shot; a
        // first shot to an unknown point orients nothing and is not named.
        // The coordinates may come LATER in the file - a traverse is often
        // begun on a mark keyed in only when the instrument stands on it -
        // since a reduction has the whole file; finish() settles those. They
        // may not come from this setup's own shots, which the instrument
        // computed with the orientation the backsight is to give. The import
        // says how many setups this gave.
        if (pointings_ == 0 && station.backsightPointId.empty()) {
            if (slots_[target].positioned) {
                station.backsightPointId = slots_[target].id;
                station.metadata.emplace("backsight",
                                         "the setup's first shot, to a point with coordinates "
                                         "earlier in the file (GSI marks no backsight)");
                ++backsighted_;
            } else {
                firstShots_.push_back(FirstShot{*active_, target});
            }
        }
        if (block.targetCoordinates()) {
            const bool positioned = slots_[target].positioned;
            placePoint(target, block.e, block.n, block.h, block.targetInputMode, false, block);
            if (!positioned && slots_[target].positioned) {
                slots_[target].positionedByShotOf = *active_ + 1;
            }
        }
        pointCode(target, block);
        // slotFor may have grown the vector: take the names again.
        const std::string& to = slots_[target].id;
        const std::string& from = slots_[activeSlot_].id;
        track(hzInputMode_, block.hzInputMode);
        track(vIndex_, block.vIndex);

        const double targetHeight = block.reflectorHeight ? *block.reflectorHeight
                                                           : reflectorHeight_.value_or(0.0);
        if (!block.reflectorHeight && !reflectorHeight_) {
            ++shotsWithoutReflectorHeight_;
        }
        survey::Pointing pointing{++pointings_, survey::Face::Unknown};
        std::optional<double> zenith;
        if (block.v) {
            const double reading = wrapToCircle(*block.v);
            if (reading > 0.0 && reading < kPi) {
                pointing.face = survey::Face::Left;
            } else if (reading > kPi) {
                pointing.face = survey::Face::Right;
            }
            zenith = reading > kPi ? kTwoPi - reading : reading;
            ++zenithAngles_;
        }
        // Each observation's source is the file's, at this record: copied from
        // the template straight into the observation, not through a local.
        const survey::SourceRecord& source = templateFor(block.wide);
        // Built in place: an Observation is several hundred bytes, and a job
        // is mostly observations, so a temporary moved into the vector would
        // be a second copy of most of the output.
        std::vector<survey::Observation>& observations = station.observations;
        if (block.hz) {
            ++directions_;
            auto& direction = std::get<survey::HorizontalDirectionObservation>(
                observations.emplace_back(
                    std::in_place_type<survey::HorizontalDirectionObservation>));
            direction.at = from;
            direction.to = to;
            direction.direction = wrapToCircle(*block.hz);
            direction.sigma = precision_.direction;
            direction.source = source;
            direction.source.recordNumber = block.record;
            direction.pointing = pointing;
            keptSixty(block.record, block.hzSixty);
        }
        if (zenith) {
            auto& vertical = std::get<survey::ZenithAngleObservation>(
                observations.emplace_back(std::in_place_type<survey::ZenithAngleObservation>));
            vertical.from = from;
            vertical.to = to;
            vertical.angle = *zenith;
            vertical.sigma = precision_.zenith;
            vertical.instrumentHeight = instrumentHeight;
            vertical.targetHeight = targetHeight;
            vertical.source = source;
            vertical.source.recordNumber = block.record;
            vertical.pointing = pointing;
            keptSixty(block.record, block.vSixty);
        }
        const auto distanceOf = [&](double value, survey::DistanceKind kind) {
            auto& distance = std::get<survey::DistanceObservation>(
                observations.emplace_back(std::in_place_type<survey::DistanceObservation>));
            distance.from = from;
            distance.to = to;
            distance.distance = value;
            distance.sigma = survey::distanceSigma(precision_, value);
            distance.kind = kind;
            distance.instrumentHeight = instrumentHeight;
            distance.targetHeight = targetHeight;
            distance.source = source;
            distance.source.recordNumber = block.record;
            distance.pointing = pointing;
            if (prism_) {
                distance.target.prismConstant = prism_;
                distance.target.prismConstantState = survey::CorrectionState::Applied;
                // Another constant than the setup began with: a changed prism
                // or a wrong setting, which the person has to tell apart.
                const std::optional<double>& began = station.instrument.prismConstant;
                if (began && *began != *prism_) {
                    otherPrism_.addMade(block.record, [&] {
                        return metresText(*prism_) + " m in setup " + station.setup.id +
                               ", which began with " + metresText(*began) + " m";
                    });
                }
            }
        };
        const auto usable = [&](const std::optional<double>& value, const char* what) {
            if (!value) {
                return false;
            }
            if (*value == 0.0) {
                zeroDistances_.add(block.record); // Leica's "no distance measured"
                return false;
            }
            if (*value < 0.0) {
                warn(block.record, std::string("the ") + what + " is negative (" +
                                       metresText(*value) + " m); it was not read");
                return false;
            }
            return true;
        };
        const bool slope = usable(block.slope, "slope distance");
        if (slope) {
            distanceOf(*block.slope, survey::DistanceKind::Slope);
            if (block.horizontal || block.heightDifference) {
                derivedValues_.add(block.record);
            }
        } else if (usable(block.horizontal, "horizontal distance")) {
            distanceOf(*block.horizontal, survey::DistanceKind::Horizontal);
        }
        if (block.heightDifference && !slope) {
            if (zenith) {
                derivedValues_.add(block.record);
            } else {
                auto& level = std::get<survey::LevelDifferenceObservation>(observations.emplace_back(
                    std::in_place_type<survey::LevelDifferenceObservation>));
                level.from = from;
                level.to = to;
                level.heightDifference = *block.heightDifference;
                level.length = (block.horizontal && *block.horizontal > 0.0) ? *block.horizontal
                                                                             : 0.0;
                // Both heights measured with a tape, and the angle the
                // instrument reduced with over the length of the line.
                level.sigma = std::hypot(precision_.heightMeasurement,
                                         precision_.heightMeasurement,
                                         level.length * precision_.zenith);
                level.source = source;
                level.source.recordNumber = block.record;
            }
        }
    }

    // A 60-second word whose value became an observation: counted, and its
    // record listed, for the note in finish(). Counted here, not where the
    // word is decoded, so that a word the block does not keep - in a code
    // block, or a shot at the occupied point - is not said to be read.
    void keptSixty(std::size_t record, const std::optional<SixtySeconds>& sixty)
    {
        if (sixty) {
            roundedSeconds_.addMade(record, [&] { return sixtyText(*sixty); });
            roundedSecondRecords_.add(record);
        }
    }

    void pointBlock(Block& block)
    {
        const std::size_t index = slotFor(block.name, block.record, block.wide);
        if (block.targetCoordinates()) {
            placePoint(index, block.e, block.n, block.h, block.targetInputMode, false, block);
        }
        pointCode(index, block);
        noteSettings(block, true);
        if (!block.time18.empty() || !block.time19.empty()) {
            timeWords_.add(block.record);
        }
    }

    // ---- State ----------------------------------------------------------------------

    std::string fileName_;
    survey::ObservationPrecision precision_;
    survey::SourceRecord narrowSource_;
    survey::SourceRecord wideSource_;
    ReadResult result_;
    std::size_t unlistedWarnings_ = 0;

    std::vector<PointSlot> slots_;
    IdIndex slotIndex_;
    std::deque<std::string> madeUpIds_; // ids the file does not contain (implicitSetup)
    std::vector<CodeBlock> pendingCodes_;
    std::optional<survey::SurveyFeature> run_;
    std::optional<FeatureStep> featureStep_; // the last point, not yet in a feature
    // The slot of the last block that named a point and took its codes
    // (pointCode): the point BEFORE a code block. It is not the last slot
    // made once a round of shots names its points a second time.
    std::optional<std::size_t> lastPointSlot_;
    std::size_t codesSincePoint_ = 0;
    bool codesAfterPoints_ = false; // codesRecordedAfterPoints
    bool afterPointsSaid_ = false;
    std::size_t codeBlocks_ = 0; // code blocks read as point codes

    std::optional<PendingSetup> pending_;
    std::vector<FirstShot> firstShots_; // first shots to points not yet positioned
    std::optional<std::size_t> active_; // index into project.stations
    std::size_t activeSlot_ = 0;
    std::optional<double> activeHeight_;
    std::size_t pointings_ = 0;
    std::size_t lastSetupObservations_ = 0;
    char hzInputMode_ = 0; // 0 none seen, 'x' varies, else the digit
    char vIndex_ = 0;

    // What the instrument is set to, from the last word that said so.
    std::optional<double> reflectorHeight_;
    std::optional<double> ppm_;
    std::optional<double> prism_;
    bool ppmCarried_ = false;   // the open setup's ppm came from an earlier one
    bool prismCarried_ = false; // ... and its prism constant
    bool stationBlockPpm_ = false;
    bool stationBlockPrism_ = false;
    std::string serial_;
    std::string type_;

    survey::LinearUnit linearUnit_ = survey::LinearUnit::Unknown;
    survey::AngularUnit angularUnit_ = survey::AngularUnit::Unknown;

    // By two-digit word index: a three-digit (digital level) word is counted
    // apart, as `levelling_`, before a word reaches this table.
    std::array<Tally, 100> unread_{};
    Tally explicitPoints_;
    // Sexagesimal words of 60.0 seconds read as the next minute, and their records.
    Tally roundedSeconds_;
    RecordList roundedSecondRecords_;
    Tally negativeZeniths_;     // vertical readings (word 22) below zero, refused
    Tally zeroCodeWords_;       // code information and remark words of nothing but zeros
    Tally remarksInCodeBlocks_; // remark words (71-79) in a code block, not read
    Tally otherPrism_; // distances measured with another prism constant than their setup's
    std::size_t pointedLengths_ = 0; // length words with a written decimal point
    Tally unpointedLengths_;         // ... and without one, as the specification has them
    Tally oddWidth_;
    Tally feet_;
    Tally mixedLinearUnits_;
    Tally mixedAngularUnits_;
    Tally heightTextAsCode_;
    Tally heightTextKept_;
    Tally coordinateRecords_;
    Tally zeroDistances_;
    Tally derivedValues_;
    Tally timeWords_;
    Tally heightOnly_;
    Tally partialPosition_;
    Tally levelling_;
    Tally specialCodes_; // a digital level's '?' code blocks
    std::size_t backsighted_ = 0; // setups given a backsight by their first shot
    std::size_t backsightedLater_ = 0; // ... to a point positioned later in the file
    std::size_t setupsWithoutHeight_ = 0;
    std::size_t setupsWithoutPpm_ = 0;
    std::size_t setupsWithoutPrism_ = 0;
    std::size_t shotsWithoutReflectorHeight_ = 0;
    std::size_t zenithAngles_ = 0;
    std::size_t directions_ = 0; // horizontal circle readings read
    std::size_t wideBlocks_ = 0;
    std::size_t narrowBlocks_ = 0;
};

ReadResult GsiReader::finish()
{
    discardPendingSetup();
    closeSetup();
    // A first shot to a point positioned since is its setup's backsight,
    // unless the position came from that setup's own shots (shotBlock).
    for (const FirstShot& first : firstShots_) {
        const PointSlot& slot = slots_[first.slot];
        if (!slot.positioned || slot.positionedByShotOf == first.station + 1) {
            continue;
        }
        survey::SurveyStation& station = result_.project.stations[first.station];
        station.backsightPointId = slot.id;
        station.metadata.emplace("backsight", "the setup's first shot, to a point given "
                                              "coordinates later in the file (GSI marks no "
                                              "backsight)");
        ++backsighted_;
        ++backsightedLater_;
    }
    if (!pendingCodes_.empty()) {
        // Nothing followed the last code block(s): the point before is the only
        // one they can belong to - the point of the last block that named one,
        // not the last point the file happened to name first.
        if (lastPointSlot_) {
            const std::size_t last = *lastPointSlot_;
            warn(pendingCodes_.front().record,
                 "no point follows this code block, so it is attached to the point before it, " +
                     slots_[last].id);
            for (std::size_t c = 0; c < pendingCodes_.size(); ++c) {
                attachCode(last, pendingCodes_[c], codesSincePoint_ + c);
            }
        } else {
            for (const CodeBlock& code : pendingCodes_) {
                warn(code.record, "code block " + shown(code.code) +
                                      " belongs to no point: no block before or after it names "
                                      "one");
            }
        }
        pendingCodes_.clear();
    }
    flushFeature();
    closeFeature();

    survey::SurveyProject& project = result_.project;
    std::size_t positioned = 0;
    for (const PointSlot& slot : slots_) {
        positioned += slot.positioned ? 1 : 0;
    }
    project.points.reserve(positioned);
    project.unpositionedPoints.reserve(slots_.size() - positioned);
    for (PointSlot& slot : slots_) {
        if (slot.positioned) {
            survey::SurveyPoint point;
            point.id = std::move(slot.id);
            point.northing = slot.northing;
            point.easting = slot.easting;
            point.elevation = slot.elevation;
            point.code = std::move(slot.code);
            point.metadata = std::move(slot.metadata);
            point.coordinateSource = slot.coordinateSource;
            point.source = sourceAt(slot.record, slot.wide);
            project.points.push_back(std::move(point));
        } else {
            survey::UnpositionedPoint point;
            point.id = std::move(slot.id);
            point.code = std::move(slot.code);
            point.metadata = std::move(slot.metadata);
            point.source = sourceAt(slot.record, slot.wide);
            project.unpositionedPoints.push_back(std::move(point));
        }
    }
    project.name = fileName_;
    project.source = narrowSource_;
    if (wideBlocks_ != 0) {
        project.source.formatVersion = narrowBlocks_ == 0 ? "GSI-16" : "GSI-8 and GSI-16";
    }
    project.units.linear = linearUnit_;
    project.units.angular = angularUnit_;
    project.metadata.emplace("parser version", kParserVersion);
    project.metadata.emplace("code blocks belong to",
                             codesAfterPoints_ ? "the point before them" : "the point after them");

    // ---- What the file did many times, said once each ----
    // The message is built only for a tally that has something to say: a
    // clean file reports nothing and must cost nothing here.
    const auto once = [this](const Tally& tally, const auto& message) {
        if (tally.count != 0) {
            warn(tally.firstRecord, message());
        }
    };
    once(explicitPoints_, [&] {
        return plural(explicitPoints_.count, "numeric word writes", "numeric words write") +
               " a decimal point (the first " + shown(explicitPoints_.example) +
               "), which the GSI specification places by the unit digit instead; the values "
               "were read as written";
    });
    once(roundedSeconds_, [&] {
        const bool one = roundedSeconds_.count == 1;
        return plural(roundedSeconds_.count, "angle word writes", "angle words write") +
               " 60 seconds, where a sexagesimal angle's seconds run to 59 (at " +
               roundedSecondRecords_.text() + (one ? ": " : "; the first, ") +
               roundedSeconds_.example +
               "): a writer that rounds the seconds on their own, without carrying into the "
               "minutes, writes 60 for a value within its rounding of the next minute, and 60 "
               "seconds is exactly that minute, so " +
               (one ? "it was" : "each was") +
               " read as the next minute, within the writer's rounding of what was measured";
    });
    if (unpointedLengths_.count < pointedLengths_) {
        // Both readings of such a word are guesses but one: the unit digit's
        // is the specification's, and it is the one taken. A real export has
        // been seen with two such words among some 1,500 that write a point,
        // each far from its neighbours, so the person is told where to look.
        once(unpointedLengths_, [&] {
            return plural(unpointedLengths_.count, "length word leaves",
                          "length words leave") +
                   " the decimal point to the unit digit (the first " +
                   shown(unpointedLengths_.example) + ") where the file's other " +
                   std::to_string(pointedLengths_) +
                   " write one: they were read by the unit digit, as the specification says, "
                   "but a file that mixes the two forms may have lost a point or a digit there; "
                   "check those values";
        });
    }
    once(heightTextAsCode_, [&] {
        return plural(heightTextAsCode_.count, "block has", "blocks have") +
               " text where a height belongs (the first " + shown(heightTextAsCode_.example) +
               ", in word 87 or 88, which GSI defines as a reflector or instrument height); it "
               "was read as the point's code, as some exporters write it there";
    });
    once(heightTextKept_, [&] {
        return plural(heightTextKept_.count, "block has", "blocks have") +
               " text in a height word beside a code of its own; the text is in the point's "
               "metadata";
    });
    once(zeroCodeWords_, [&] {
        return plural(zeroCodeWords_.count, "remark or code information word is",
                      "remark or code information words are") +
               " all zeros (the first, " + zeroCodeWords_.example +
               "), which is how GSI writes an empty value, padding text with '0': they were "
               "read as empty, not as the code '0', which would be written the same";
    });
    once(remarksInCodeBlocks_, [&] {
        return plural(remarksInCodeBlocks_.count, "remark word (71-79) is",
                      "remark words (71-79) are") +
               " in a code block (the first, " + remarksInCodeBlocks_.example +
               "), where nothing tells whether they are a point's code and remarks or more "
               "about the block's own code: they were not read";
    });
    once(coordinateRecords_, [&] {
        return plural(coordinateRecords_.count, "block gives", "blocks give") +
               " station coordinates (words 84-86) or an instrument height with no measurement "
               "after them (the first names point " +
               coordinateRecords_.example +
               "); they were read as the coordinates of the point they name, not as setups";
    });
    once(zeroDistances_, [&] {
        return plural(zeroDistances_.count, "block records", "blocks record") +
               " a distance of zero, Leica's way of saying none was measured; those shots have "
               "no distance";
    });
    once(otherPrism_, [&] {
        return plural(otherPrism_.count, "distance was", "distances were") +
               " measured with a prism constant other than the one " +
               (otherPrism_.count == 1 ? "its" : "their") + " setup began with (the first, " +
               otherPrism_.example +
               "): each keeps the constant the instrument applied to it, so if the prism was "
               "not changed, those distances are long or short by the difference";
    });
    once(oddWidth_, [&] {
        return plural(oddWidth_.count, "word is", "words are") +
               " not the width its block calls for (8 characters of data in GSI-8, 16 in "
               "GSI-16; the first " +
               shown(oddWidth_.example) + "); they were read as far as they go";
    });
    once(feet_, [] {
        return std::string("the file is in feet, and GSI does not say which foot: US survey "
                           "feet were assumed (1200/3937 m); if the instrument was set to "
                           "international feet every length is 2 ppm long");
    });
    once(mixedLinearUnits_, [] {
        return std::string("the file mixes length units; each value was converted from its "
                           "own, and the file is described by the first");
    });
    once(mixedAngularUnits_, [] {
        return std::string("the file mixes angle units; each value was converted from its "
                           "own, and the file is described by the first");
    });
    once(heightOnly_, [&] {
        return plural(heightOnly_.count, "point has", "points have") +
               " a height and no easting or northing (the first " + heightOnly_.example +
               "); the height is in the point's metadata and the point has no position";
    });
    once(partialPosition_, [&] {
        return plural(partialPosition_.count, "point has", "points have") +
               " an easting without a northing or the reverse (the first " +
               partialPosition_.example + "); it has no position";
    });
    once(levelling_, [&] {
        return plural(levelling_.count, "block holds", "blocks hold") +
               " a digital level's words (three-digit word indices such as 330, the staff "
               "readings; the first names point " +
               levelling_.example +
               "): this parser reads total station data, so their points are listed and the "
               "levelling is not read";
    });
    once(specialCodes_, [&] {
        return plural(specialCodes_.count, "code block is", "code blocks are") +
               " a digital level's special code block (a '?' in position 8, naming the "
               "levelling method; the first " +
               shown(specialCodes_.example) +
               "): the levelling is not read, and they were not taken as point codes";
    });
    once(timeWords_, [&] {
        return plural(timeWords_.count, "block records", "blocks record") +
               " a time the model has no place for (it keeps one per setup); those times were "
               "not carried";
    });
    for (std::size_t index = 0; index < unread_.size(); ++index) {
        const Tally& tally = unread_[index];
        once(tally, [&] {
            return "word " + std::to_string(index) + " (" + wordName(static_cast<int>(index)) +
                   ") appears in " + plural(tally.count, "block", "blocks") +
                   " and is not read by this parser";
        });
    }
    if (unlistedWarnings_ != 0) {
        result_.warnings.push_back(ReadWarning{
            fileName_, 0,
            plural(unlistedWarnings_, "further warning is", "further warnings are") +
                " not listed: the first " + std::to_string(kMaxListedWarnings) + " are"});
    }

    // ---- What the file does not carry ----
    std::vector<std::string>& lacking = result_.notCarried;
    lacking.emplace_back("GSI states no coordinate system: the coordinates are in whatever "
                         "system the instrument or controller was set to");
    const std::size_t setups = project.stations.size();
    if (setups != 0) {
        lacking.push_back(
            "GSI does not mark a backsight: " +
            (backsighted_ == 0
                 ? std::string("no setup's first shot is to a point the file gives coordinates, "
                               "other than by that setup's own shots, so no setup has one")
                 : std::to_string(backsighted_) + " of " + plural(setups, "setup", "setups") +
                       " took the point of their first shot as the backsight, because the "
                       "file gives it coordinates" +
                       (backsightedLater_ == 0
                            ? std::string()
                            : " (" + std::to_string(backsightedLater_) +
                                  " of them from coordinates later in the file)")) +
            (backsighted_ == setups ? std::string()
                                    : "; a setup without one cannot be oriented until its "
                                      "backsight is known"));
    }
    if (setupsWithoutHeight_ != 0) {
        lacking.push_back("no instrument height on " + std::to_string(setupsWithoutHeight_) +
                          " of " + plural(setups, "setup", "setups") + " (read as 0)");
    }
    if (shotsWithoutReflectorHeight_ != 0) {
        lacking.push_back("no reflector height before " +
                          plural(shotsWithoutReflectorHeight_, "shot", "shots") + " (read as 0)");
    }
    if (setupsWithoutPpm_ != 0) {
        lacking.push_back("no atmospheric ppm on " + std::to_string(setupsWithoutPpm_) + " of " +
                          plural(setups, "setup", "setups") +
                          ": whether their distances were corrected is not stated");
    }
    if (setupsWithoutPrism_ != 0) {
        lacking.push_back("no prism constant on " + std::to_string(setupsWithoutPrism_) + " of " +
                          plural(setups, "setup", "setups") +
                          ": whether their distances include one is not stated");
    }
    if (zenithAngles_ != 0 || negativeZeniths_.count != 0) {
        std::string setting = "GSI does not record the vertical angle setting: word 22 was read "
                              "as a zenith angle, the instruments' usual setting";
        if (negativeZeniths_.count != 0) {
            // Each was warned about at its record. What they suggest about
            // the file's OTHER vertical readings, which were read, is said
            // here once: a positive elevation read as a zenith is no less
            // wrong, and nothing in its word shows it.
            setting += "; " + plural(negativeZeniths_.count, "reading was", "readings were") +
                       " refused as negative, which no zenith angle is (the first at record " +
                       std::to_string(negativeZeniths_.firstRecord) +
                       "): an instrument set to read V from the horizon or in percent (GSI "
                       "ONLINE SET 44) writes negatives below the horizon, and if this one was "
                       "so set, the file's other word 22 readings are not zenith angles either "
                       "and the shots read with them are wrong";
        }
        lacking.push_back(std::move(setting));
    }
    if (directions_ != 0) {
        // GSI ONLINE's SET/CONF 171, "Direction of horizontal circle reading
        // (Hz-Angle)": 0 clockwise, 1 counterclockwise - a setting, not a word.
        lacking.emplace_back("GSI does not record the direction of the horizontal circle (an "
                             "instrument setting): word 21 was read as increasing clockwise, as "
                             "instruments are normally set; a job measured with the circle "
                             "counterclockwise comes out mirrored about each setup's backsight");
    }
    if (codeBlocks_ != 0) {
        lacking.push_back(
            "GSI does not record whether the instrument stored a code block before or after its "
            "point (its <Rec Free Code:> setting): " +
            std::string(codesAfterPoints_
                            ? "this file begins with a point block and ends with a code block, "
                              "as a job recorded After Point does, so each code block was taken "
                              "to code the point before it; had the instrument been set to "
                              "Before Point, each code belongs to the point after its block"
                            : "each code block was taken to code the point after it, as a job "
                              "recorded Before Point does; had the instrument been set to After "
                              "Point, each code belongs to the point before its block"));
    }
    if (derivedValues_.count != 0) {
        lacking.push_back(plural(derivedValues_.count, "block's", "blocks'") +
                          " instrument-computed horizontal distance or height difference beside "
                          "the measured slope distance and zenith angle: not carried, the "
                          "reduction computes its own from the measurements");
    }
    return std::move(result_);
}

// ---- The format --------------------------------------------------------------------

FormatDescriptor gsiFormat()
{
    FormatDescriptor format;
    format.id = std::string(kLeicaGsiFormatId);
    format.humanName = "Leica GSI (GSI-8, GSI-16)";
    format.manufacturer = Manufacturer::Leica;
    format.reads = {.points = true,
                    .observations = true,
                    .stations = true,
                    .features = true,
                    .coordinateSystem = false,
                    .instrumentSettings = true,
                    .gnss = false};
    format.canImport = true;
    format.canExport = false;
    format.parserVersion = kParserVersion;
    format.extensions = {"gsi"};
    return format;
}

// Does this line look like a GSI block: an optional '*', then words, the first
// of them word 11 or 41? `wide` says whether it began with '*'.
bool looksLikeBlock(std::string_view line, bool& wide, bool& standardWidth)
{
    std::size_t position = 0;
    bool first = true;
    wide = false;
    standardWidth = true;
    while (true) {
        while (position < line.size() && isBlank(line[position])) {
            ++position;
        }
        if (position >= line.size()) {
            return !first;
        }
        std::size_t end = position;
        while (end < line.size() && !isBlank(line[end])) {
            ++end;
        }
        std::string_view token = line.substr(position, end - position);
        position = end;
        if (first && token.front() == '*') {
            wide = true;
            token.remove_prefix(1);
        }
        const std::optional<Word> word = parseWord(token);
        if (!word || (first && word->index != 11 && word->index != 41)) {
            return false;
        }
        standardWidth = standardWidth && word->data.size() == (wide ? 16U : 8U);
        first = false;
    }
}

FormatSignature probeGsi(const ProbeInput& input)
{
    std::string_view rest = withoutByteOrderMark(input.bytes);
    std::size_t lines = 0;
    std::size_t blocks = 0;
    std::size_t wide = 0;
    std::size_t standard = 0;
    while (!rest.empty() && lines < 200) {
        const std::size_t end = rest.find_first_of("\r\n");
        if (end == std::string_view::npos && input.truncated) {
            break; // cut by kProbeBytes, not by the file
        }
        const std::string_view line = rest.substr(0, end);
        rest = end == std::string_view::npos ? std::string_view{} : rest.substr(end + 1);
        if (line.find_first_not_of(" \t\x1a") == std::string_view::npos) {
            continue;
        }
        ++lines;
        bool isWide = false;
        bool isStandard = false;
        if (looksLikeBlock(line, isWide, isStandard)) {
            ++blocks;
            wide += isWide ? 1 : 0;
            standard += isStandard ? 1 : 0;
        }
    }
    if (blocks == 0) {
        return ruledOut();
    }
    std::string evidence = std::to_string(blocks) + " of " + std::to_string(lines) +
                           " lines are GSI blocks beginning with word 11 or 41";
    if (blocks != lines) {
        // A GSI file is blocks and nothing else; a few stray lines is a damaged
        // one, many is something else that happens to contain such lines.
        const double share = static_cast<double>(blocks) / static_cast<double>(lines);
        return {share >= 0.9 ? 0.6 : 0.2 * share, evidence + ", the rest are not"};
    }
    double confidence = blocks >= 3 ? 0.93 : 0.8;
    evidence += wide == blocks ? ", GSI-16 (lines begin with '*')"
                : wide == 0    ? ", GSI-8"
                               : ", GSI-8 and GSI-16 mixed";
    if (standard == blocks) {
        confidence += 0.03;
        evidence += ", every word the standard width";
    }
    if (input.extension == "gsi") {
        confidence += 0.03;
        evidence += ", extension .gsi";
    }
    return {confidence, evidence};
}

Result<ReadResult> readGsi(std::string_view bytes, std::string_view fileName,
                           const ReadOptions& options)
{
    const survey::ObservationPrecision& precision = options.precision;
    if (!(precision.direction > 0.0) || !(precision.zenith > 0.0) ||
        !(precision.heightMeasurement > 0.0) ||
        !(precision.distanceConstant > 0.0 || precision.distancePpm > 0.0) ||
        !std::isfinite(precision.direction) || !std::isfinite(precision.zenith) ||
        !std::isfinite(precision.heightMeasurement) ||
        !std::isfinite(precision.distanceConstant) || !std::isfinite(precision.distancePpm)) {
        return makeError(ErrorCode::InvalidArgument,
                         "the default precision given for a GSI import must be positive: every "
                         "observation needs a standard deviation",
                         std::string(fileName));
    }
    GsiReader reader(fileName, options);
    reader.read(bytes);
    ReadResult result = reader.finish();
    if (result.recordsRead == 0) {
        return makeError(ErrorCode::ParseFailure,
                         "no GSI block could be read: a GSI file is lines of words, each "
                         "beginning with word 11 (a point) or 41 (a code)",
                         std::string(fileName));
    }
    return result;
}

const FormatRegistration kRegistration{gsiFormat(), &probeGsi, &readGsi};

} // namespace

} // namespace katana::surveyio
