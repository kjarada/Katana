// Leica GSI-8 and GSI-16: the probe, the reader and the format's registration.
// What an import produces is described in include/katana/surveyio/leica.hpp.
//
// THE SPECIFICATION is Leica Geosystems, "GSI ONLINE for Leica TPS and DNA",
// November 2003: the data word and the word information table on pages 5-6,
// the TPS1000/1100/2000/5000 word indices in the PUT and GET tables on pages
// 35-37, the DNA (levelling) words on pages 40-42. A word is
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
#include <unordered_set>
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
constexpr const char* kParserVersion = "1.0";

constexpr double kPi = std::numbers::pi;
constexpr double kTwoPi = 2.0 * std::numbers::pi;

// Record-level warnings listed one by one before the rest are only counted. A
// damaged file can have a problem on every line, and a list of a million
// warnings is not one a person reads; the count still says how many there were.
constexpr std::size_t kMaxListedWarnings = 1000;

// Two coordinates for one point that agree to half the last digit of a
// millimetre word are the same coordinates written twice.
constexpr double kSamePositionTolerance = 0.0005;

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

struct Block {
    std::size_t record = 0;
    bool wide = false; // GSI-16
    int kind = 0;      // 11: a point's block; 41: a code block
    std::string_view name; // the point id, or the code of a code block

    std::optional<double> hz;
    std::optional<double> v;
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

    std::array<std::string_view, 9> remarks{};  // 71..79
    std::array<std::string_view, 8> codeInfo{}; // 42..49
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
};

struct CodeBlock {
    std::string code;
    std::array<std::string, 8> info;
    std::size_t record = 0;
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
        warn(block.record, "word " + std::to_string(word.index) + " " + shown(word.data) + " " +
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

    // An angle word in radians, as read: not wrapped, not turned into a zenith.
    std::optional<double> angleOf(const Block& block, const Word& word)
    {
        const char unit = unitDigit(word);
        if (unit == '4') {
            return sexagesimal(block, word);
        }
        double toRadians = 0.0;
        std::size_t decimals = 5;
        survey::AngularUnit declared = survey::AngularUnit::Unknown;
        switch (unit) {
        case '2':
            toRadians = kPi / 200.0;
            declared = survey::AngularUnit::Gons;
            break;
        case '3':
            toRadians = kPi / 180.0;
            declared = survey::AngularUnit::DecimalDegrees;
            break;
        case '5':
            toRadians = kTwoPi / 6400.0;
            decimals = 4;
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
        noteAngularUnit(declared, block.record);
        const double inUnit =
            value->explicitPoint ? value->value : value->value / kPowersOfTen[decimals];
        return inUnit * toRadians;
    }

    // DDDMMSSs: degrees, minutes, seconds and tenths - DDD.MMSSs with the point
    // unwritten. A file that writes the point is read the same way around it.
    std::optional<double> sexagesimal(const Block& block, const Word& word)
    {
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
        const int minutes = digitsOf(fraction, 0, 2);
        const int seconds = digitsOf(fraction, 2, 2);
        // Tenths and anything finer, as the decimal part of the seconds.
        double finer = 0.0;
        double scale = 0.1;
        for (std::size_t i = 4; i < fraction.size(); ++i, scale /= 10.0) {
            if (!isDigit(fraction[i])) {
                warnWord(block, word, "is not a sexagesimal angle");
                return std::nullopt;
            }
            finer += (fraction[i] - '0') * scale;
        }
        if (minutes < 0 || seconds < 0 || minutes >= 60 || seconds >= 60) {
            warnWord(block, word, "is not a sexagesimal angle: minutes and seconds run to 59");
            return std::nullopt;
        }
        noteAngularUnit(survey::AngularUnit::DegreesMinutesSeconds, block.record);
        double degreesValue = static_cast<double>(degrees) + minutes / 60.0 +
                              (seconds + finer) / 3600.0;
        if (word.sign == '-') {
            degreesValue = -degreesValue;
        }
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
            block.hz = angleOf(block, word);
            block.hzInputMode = inputMode(word);
            return;
        case 22:
            block.measurementWords = true;
            block.v = angleOf(block, word);
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
            block.codeInfo[static_cast<std::size_t>(word.index - 42)] = textOf(word.data);
            return;
        }
        if (word.index >= 71 && word.index <= 79) {
            block.remarks[static_cast<std::size_t>(word.index - 71)] = textOf(word.data);
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
        CodeBlock code;
        code.code = std::string(block.name);
        for (std::size_t i = 0; i < block.codeInfo.size(); ++i) {
            code.info[i] = std::string(block.codeInfo[i]);
        }
        code.record = block.record;
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

    // A point's code: word 71 in its own block; else the code block before it
    // (see leica.hpp); else text the file wrote into a height word. Whichever
    // is not the code is kept in the point's metadata.
    void pointCode(std::size_t index, const Block& block)
    {
        if (!block.remarks[0].empty()) {
            addCode(index, block.remarks[0], "remark 1");
        }
        for (std::size_t i = 1; i < block.remarks.size(); ++i) {
            if (!block.remarks[i].empty()) {
                slots_[index].metadata.insert_or_assign("remark " + std::to_string(i + 1),
                                                        std::string(block.remarks[i]));
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
        feature(index, block);
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

    // One feature per run of consecutive points with the same code: GSI has no
    // string numbers, so a change of code (or an uncoded point) ends a string.
    void feature(std::size_t index, const Block& block)
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
        started.source = sourceAt(block.record, block.wide);
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

    std::string uniqueStationId(const std::string& pointId)
    {
        std::string id = pointId;
        for (std::size_t n = 2; stationIds_.contains(id); ++n) {
            id = pointId + " (" + std::to_string(n) + ")";
        }
        stationIds_.insert(id);
        return id;
    }

    void openSetup(std::size_t slot, std::optional<double> instrumentHeight, std::size_t record,
                   bool wide, const survey::SurveyTimestamp& time,
                   const std::string& timeWithoutYear)
    {
        closeSetup();
        survey::SurveyStation station;
        station.setup.pointId = slots_[slot].id;
        station.setup.id = uniqueStationId(station.setup.pointId);
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

        survey::SurveyTimestamp time;
        std::string withoutYear;
        if (!block.time19.empty() || !block.time18.empty()) {
            if (!station.instrument.time.known() && pointings_ == 0) {
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
        // had already given coordinates, is taken as its backsight. Orienting
        // on a point of known position is sound whatever the observer called
        // the shot; a first shot to an unknown point orients nothing and is
        // not named. The import says how many setups this gave (finish()).
        if (pointings_ == 0 && slots_[target].positioned && station.backsightPointId.empty()) {
            station.backsightPointId = slots_[target].id;
            station.metadata.emplace("backsight",
                                     "the setup's first shot, to a point with coordinates "
                                     "earlier in the file (GSI marks no backsight)");
            ++backsighted_;
        }
        if (block.targetCoordinates()) {
            placePoint(target, block.e, block.n, block.h, block.targetInputMode, false, block);
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
        const survey::SourceRecord source = sourceAt(block.record, block.wide);
        // Built in place: an Observation is several hundred bytes, and a job
        // is mostly observations, so a temporary moved into the vector would
        // be a second copy of most of the output.
        std::vector<survey::Observation>& observations = station.observations;
        if (block.hz) {
            auto& direction = std::get<survey::HorizontalDirectionObservation>(
                observations.emplace_back(
                    std::in_place_type<survey::HorizontalDirectionObservation>));
            direction.at = from;
            direction.to = to;
            direction.direction = wrapToCircle(*block.hz);
            direction.sigma = precision_.direction;
            direction.source = source;
            direction.pointing = pointing;
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
            vertical.pointing = pointing;
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
            distance.pointing = pointing;
            if (prism_) {
                distance.target.prismConstant = prism_;
                distance.target.prismConstantState = survey::CorrectionState::Applied;
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
            }
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
    std::unordered_set<std::string> stationIds_;
    std::vector<CodeBlock> pendingCodes_;
    std::optional<survey::SurveyFeature> run_;

    std::optional<PendingSetup> pending_;
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

    std::array<Tally, 1000> unread_{};
    Tally explicitPoints_;
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
    std::size_t backsighted_ = 0; // setups given a backsight by their first shot
    std::size_t setupsWithoutHeight_ = 0;
    std::size_t setupsWithoutPpm_ = 0;
    std::size_t setupsWithoutPrism_ = 0;
    std::size_t shotsWithoutReflectorHeight_ = 0;
    std::size_t zenithAngles_ = 0;
    std::size_t wideBlocks_ = 0;
    std::size_t narrowBlocks_ = 0;
};

ReadResult GsiReader::finish()
{
    discardPendingSetup();
    closeSetup();
    if (!pendingCodes_.empty()) {
        // Nothing followed the last code block(s): the point before is the only
        // one they can belong to.
        if (!slots_.empty()) {
            const std::size_t last = slots_.size() - 1;
            warn(pendingCodes_.front().record,
                 "no point follows this code block, so it is attached to the point before it, " +
                     slots_[last].id);
            for (std::size_t c = 0; c < pendingCodes_.size(); ++c) {
                attachCode(last, pendingCodes_[c], c);
            }
        } else {
            for (const CodeBlock& code : pendingCodes_) {
                warn(code.record, "code block " + shown(code.code) +
                                      " belongs to no point: the file names none");
            }
        }
        pendingCodes_.clear();
    }
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
    project.metadata.emplace("code blocks belong to", "the point after them");

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
                 ? std::string("no setup's first shot is to a point with coordinates earlier in "
                               "the file, so no setup has one")
                 : std::to_string(backsighted_) + " of " + plural(setups, "setup", "setups") +
                       " took the point of their first shot as the backsight, because the "
                       "file had already given it coordinates") +
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
    if (zenithAngles_ != 0) {
        lacking.emplace_back("GSI does not record the vertical angle setting: word 22 was read "
                             "as a zenith angle, the instruments' usual setting");
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
