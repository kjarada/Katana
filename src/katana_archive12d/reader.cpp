#include "katana/archive12d/reader.hpp"

#include <algorithm>
#include <cctype>
#include <charconv>
#include <cmath>
#include <limits>
#include <system_error>
#include <string>
#include <unordered_map>
#include <unordered_set>
#include <utility>

#include "katana/archive12d/text_encoding.hpp"
#include "katana/entity/entity.hpp"
#include "lexer.hpp"
#include "text_utilities.hpp"

namespace katana::archive12d {

namespace {

using detail::Lexer;
using detail::Token;
using detail::TokenKind;
using detail::lowered;
using katana::core::Error;
using katana::core::ErrorCode;
using katana::core::makeError;

// A null inside a block of numbers, while it is still a block of numbers.
// parseReal never yields a NaN - it refuses anything not finite - so the value
// cannot collide with one that was read.
constexpr double kNull = std::numeric_limits<double>::quiet_NaN();

// Warnings kept verbatim. A damaged 60 MB file can produce one per line, and
// a list nobody can read is not a report; past this the rest are counted.
constexpr std::size_t kMaxWarnings = 100;

// The record layouts of manual 1.5.18, as the tags of the tagged form in the
// order the compact form lists the same values.
const std::vector<std::string_view>& lasFieldOrder(int pointFormat)
{
    static const std::vector<std::string_view> p0 = {"x",  "y",  "z",  "i",  "rn", "rc",
                                                     "sd", "fe", "cl", "sr", "ud", "id"};
    static const std::vector<std::string_view> p1 = {"x",  "y",  "z",  "i",  "rn", "rc", "sd",
                                                     "fe", "cl", "sr", "ud", "id", "t"};
    static const std::vector<std::string_view> p2 = {"x",  "y",  "z",  "i",  "rn", "rc", "sd",
                                                     "fe", "cl", "sr", "ud", "id", "c"};
    static const std::vector<std::string_view> p3 = {"x",  "y",  "z",  "i",  "rn", "rc", "sd",
                                                     "fe", "cl", "sr", "ud", "id", "t",  "c"};
    static const std::vector<std::string_view> p6 = {"x",  "y",  "z",  "i",  "rn",
                                                     "rc", "cf", "sc", "sd", "fe",
                                                     "cl", "ud", "sr", "id", "t"};
    static const std::vector<std::string_view> p7 = {"x",  "y",  "z",  "i",  "rn", "rc",
                                                     "cf", "sc", "sd", "fe", "cl", "ud",
                                                     "sr", "id", "t",  "c"};
    static const std::vector<std::string_view> p8 = {"x",  "y",  "z",  "i",  "rn", "rc",
                                                     "cf", "sc", "sd", "fe", "cl", "ud",
                                                     "sr", "id", "t",  "c",  "ir"};
    switch (pointFormat) {
    case 1:
    case 4: // p1 plus wave data, which the manual marks "not yet implemented"
        return p1;
    case 2:
        return p2;
    case 3:
    case 5:
        return p3;
    case 6:
    case 9:
        return p6;
    case 7:
        return p7;
    case 8:
    case 10:
        return p8;
    default:
        return p0;
    }
}

// True when `value` is a whole number within T; the field then takes it.
template <typename T> bool clampTo(double value, T& out)
{
    const double low = static_cast<double>(std::numeric_limits<T>::lowest());
    const double high = static_cast<double>(std::numeric_limits<T>::max());
    const bool exact = value == std::round(value) && value >= low && value <= high;
    out = static_cast<T>(std::clamp(std::round(value), low, high));
    return exact;
}

// Returns false when the value was not one the field can hold, so that the
// caller can count it: a LAS field read as 0 because it was "abc" is a value
// lost, and the manual gives every one of them a range.
bool setLasField(LasPoint& point, std::string_view tag, const std::string& value)
{
    if (tag == "c") {
        // "64 bit integer" (manual 1.5.18): more than a double holds exactly,
        // and more than a SIGNED 64-bit integer holds - a LAS colour packs
        // three 16-bit channels, so its top bit is set whenever red is bright.
        std::uint64_t colour = 0;
        const auto begin = value.data();
        const auto end = begin + value.size();
        const auto unsignedResult = std::from_chars(begin, end, colour);
        if (unsignedResult.ec == std::errc{} && unsignedResult.ptr == end) {
            point.colour = colour;
            return true;
        }
        // A file that wrote the same bits as a signed number.
        if (const auto integer = detail::parseInteger(value)) {
            point.colour = static_cast<std::uint64_t>(*integer);
            return true;
        }
        return false;
    }
    const auto real = parseReal(value);
    if (!real) {
        return false;
    }
    if (tag == "x") {
        point.x = *real;
    } else if (tag == "y") {
        point.y = *real;
    } else if (tag == "z") {
        point.z = *real;
    } else if (tag == "t") {
        point.gpsTime = *real;
    } else if (tag == "i") {
        return clampTo(*real, point.intensity);
    } else if (tag == "rn") {
        return clampTo(*real, point.returnNumber);
    } else if (tag == "rc") {
        return clampTo(*real, point.returnCount);
    } else if (tag == "cf") {
        return clampTo(*real, point.classificationFlags);
    } else if (tag == "sc") {
        return clampTo(*real, point.scannerChannel);
    } else if (tag == "sd") {
        return clampTo(*real, point.scanDirection);
    } else if (tag == "fe") {
        return clampTo(*real, point.flightLineEdge);
    } else if (tag == "cl") {
        return clampTo(*real, point.classification);
    } else if (tag == "sr") {
        return clampTo(*real, point.scanAngle);
    } else if (tag == "ud") {
        return clampTo(*real, point.userData);
    } else if (tag == "id") {
        return clampTo(*real, point.pointSourceId);
    } else if (tag == "ir") {
        return clampTo(*real, point.nearInfrared);
    }
    return true; // a tag the format does not have: nothing to hold it
}

class Reader {
  public:
    Reader(std::string_view text, const ReadOptions& options) : lexer_(text), options_(options) {}

    katana::core::Result<Archive> run();

  private:
    // ---- token plumbing ------------------------------------------------------
    //
    // Failure is sticky: once fail() has been called every token is End, every
    // loop over a block's contents ends, and run() reports the FIRST failure.
    // That keeps a recursive-descent reader free of both exceptions and a
    // Result on every one of its sixty functions.

    [[nodiscard]] bool failed() const { return error_.has_value(); }

    void fail(std::uint32_t line, std::string message)
    {
        if (!error_) {
            error_ = makeError(ErrorCode::ParseFailure, std::move(message),
                               "line " + std::to_string(line));
        }
    }

    void warn(std::string message)
    {
        if (archive_.warnings.size() < kMaxWarnings) {
            archive_.warnings.push_back(std::move(message));
        } else {
            ++suppressedWarnings_;
        }
    }

    [[nodiscard]] const Token& peek()
    {
        if (failed()) {
            return end_;
        }
        return lexer_.peek();
    }

    Token next()
    {
        if (failed()) {
            return end_;
        }
        Token token = lexer_.next();
        if (token.kind == TokenKind::Open && lexer_.depth() > options_.maxDepth) {
            fail(token.line, "blocks are nested more than " + std::to_string(options_.maxDepth) +
                                 " deep");
            return end_;
        }
        return token;
    }

    // True at the `}` that ends the current block - and at the end of the
    // text and after a failure, so that `while (!atBlockEnd())` always ends.
    [[nodiscard]] bool atBlockEnd()
    {
        const TokenKind kind = peek().kind;
        return kind == TokenKind::Close || kind == TokenKind::End;
    }

    bool expectOpen(std::string_view what)
    {
        const Token token = next();
        if (token.kind != TokenKind::Open) {
            fail(token.line, "expected '{' after " + std::string(what));
            return false;
        }
        return true;
    }

    void expectClose(std::string_view what)
    {
        const Token token = next();
        if (token.kind != TokenKind::Close && !failed()) {
            fail(token.line, "the file ends inside " + std::string(what) + ": a '}' is missing");
        }
    }

    // The value after a keyword. A brace or the end of the text is a failure,
    // because "a value must always exist" (manual 1.4).
    std::string nextValue(std::string_view keyword)
    {
        const Token token = next();
        if (!token.isValue()) {
            fail(token.line, "'" + std::string(keyword) + "' has no value");
            return {};
        }
        return token.text();
    }

    // A colour name. 12d's standard colours include two-word names - "dark
    // green", "light grey" - which 12d Model itself writes in quotes but which
    // hand-made files write bare, so `dark` or `light` takes the next word
    // with it unless that word is the next keyword.
    std::string nextColour(std::string_view keyword)
    {
        const Token token = next();
        if (!token.isValue()) {
            fail(token.line, "'" + std::string(keyword) + "' has no value");
            return {};
        }
        std::string colour = token.text();
        if (token.kind == TokenKind::Word) {
            const bool qualifier = detail::equalsIgnoringCase(colour, "dark") ||
                                   detail::equalsIgnoringCase(colour, "light");
            if (qualifier && peek().kind == TokenKind::Word && !isKnownKeyword(peek().raw)) {
                colour += ' ';
                colour += next().text();
            }
        }
        return colour;
    }

    // nullopt is a NULL, which is a value; a token that is not a number at
    // all is a failure.
    std::optional<double> nextNullableReal(std::string_view keyword)
    {
        const Token token = next();
        if (!token.isValue()) {
            fail(token.line, "'" + std::string(keyword) + "' has no value");
            return std::nullopt;
        }
        if (token.kind == TokenKind::Word && detail::equalsIgnoringCase(token.raw, "null")) {
            return std::nullopt;
        }
        const auto value = parseReal(token.raw);
        if (!value) {
            fail(token.line, "'" + std::string(keyword) + "' needs a number, not '" +
                                 std::string(token.raw) + "'");
            return std::nullopt;
        }
        return value;
    }

    double nextReal(std::string_view keyword)
    {
        const std::uint32_t line = peek().line;
        const auto value = nextNullableReal(keyword);
        if (!value && !failed()) {
            fail(line, "'" + std::string(keyword) + "' cannot be null");
        }
        return value.value_or(0.0);
    }

    bool nextBoolean(std::string_view keyword)
    {
        const Token token = next();
        const auto value = token.isValue() ? detail::parseBoolean(token.raw) : std::nullopt;
        if (!value) {
            fail(token.line, "'" + std::string(keyword) + "' needs true or false");
            return false;
        }
        return *value;
    }

    // A height as read: only the `null` KEYWORD is no height yet. The null
    // VALUE is applied afterwards, by applyNull, once the element is whole -
    // because a string's own `null_value` may come after its data (string
    // commands are order-free, manual 1.4.6), and until the block is closed
    // the value in force is not known.
    [[nodiscard]] static std::optional<double> heightOf(double value)
    {
        if (std::isnan(value)) {
            return std::nullopt;
        }
        return value;
    }

    static void applyNull(std::optional<double>& z, double nullValue)
    {
        if (z && *z == nullValue) {
            z.reset();
        }
    }

    static void applyNull(std::vector<Vertex>& vertices, double nullValue)
    {
        for (Vertex& vertex : vertices) {
            applyNull(vertex.z, nullValue);
        }
    }

    // The null value in force for an element: its own `null_value` when it
    // has one (kept in its extras, so that it is written back), else the
    // file's current one.
    [[nodiscard]] double nullValueFor(const FieldList& extras) const
    {
        return extras.real("null_value").value_or(archive_.nullValue);
    }

    // ---- blocks --------------------------------------------------------------

    [[nodiscard]] std::string pathWith(std::string_view leaf) const
    {
        std::string path;
        for (const std::string_view part : path_) {
            path += part;
            path += '/';
        }
        path += leaf;
        return path;
    }

    // The name is BORROWED, not copied: every block opens one of these, and
    // the names are keyword literals or a `what` the caller already holds. The
    // rvalue-string overload is deleted so that a caller cannot hand it a
    // temporary whose bytes die at the semicolon.
    struct Scope {
        Scope(Reader& reader, std::string_view name) : reader_(reader)
        {
            reader_.path_.push_back(name);
        }
        // An exact match, so that a keyword literal is not ambiguous between
        // the two below.
        Scope(Reader& reader, const char* name) : Scope(reader, std::string_view(name)) {}
        Scope(Reader& reader, std::string&& name) = delete;
        ~Scope() { reader_.path_.pop_back(); }
        Scope(const Scope&) = delete;
        Scope& operator=(const Scope&) = delete;

      private:
        Reader& reader_;
    };

    // The next token is the `{` of a block this reader has no member for.
    void skipUnknownBlock(std::string_view keyword)
    {
        ++archive_.unrecognised[pathWith(keyword)];
        const Token open = next();
        if (open.kind != TokenKind::Open) {
            fail(open.line, "expected '{' after " + std::string(keyword));
            return;
        }
        const std::size_t target = lexer_.depth() - 1;
        while (!failed()) {
            const Token token = next();
            if (token.kind == TokenKind::End) {
                fail(open.line, "the block '" + std::string(keyword) + "' is never closed");
                return;
            }
            if (token.kind == TokenKind::Close && lexer_.depth() == target) {
                return;
            }
        }
    }

    // `{ v v v ... }` of numbers, nulls as kNull.
    bool readNumberBlock(std::string_view what, std::vector<double>& out)
    {
        if (!expectOpen(what)) {
            return false;
        }
        while (!atBlockEnd()) {
            const Token token = next();
            if (token.kind == TokenKind::Word && detail::equalsIgnoringCase(token.raw, "null")) {
                out.push_back(kNull);
            } else if (const auto value = parseReal(token.raw);
                       value && token.kind == TokenKind::Word) {
                out.push_back(*value);
            } else {
                fail(token.line, "'" + std::string(token.raw) + "' in " + std::string(what) +
                                     " is not a number");
                return false;
            }
            if (out.size() > options_.maxValuesPerBlock) {
                fail(token.line, std::string(what) + " holds more values than the limit of " +
                                     std::to_string(options_.maxValuesPerBlock));
                return false;
            }
        }
        expectClose(what);
        return !failed();
    }

    bool readIntegerBlock(std::string_view what, std::vector<std::int64_t>& out)
    {
        if (!expectOpen(what)) {
            return false;
        }
        readIntegersUntilClose(what, out);
        expectClose(what);
        return !failed();
    }

    void readIntegersUntilClose(std::string_view what, std::vector<std::int64_t>& out)
    {
        while (!atBlockEnd()) {
            const Token token = next();
            const auto value =
                token.kind == TokenKind::Word ? detail::parseInteger(token.raw) : std::nullopt;
            if (!value) {
                fail(token.line, "'" + std::string(token.raw) + "' in " + std::string(what) +
                                     " is not a whole number");
                return;
            }
            out.push_back(*value);
            if (out.size() > options_.maxValuesPerBlock) {
                fail(token.line, std::string(what) + " holds more values than the limit of " +
                                     std::to_string(options_.maxValuesPerBlock));
                return;
            }
        }
    }

    // `{ word "text" word ... }` as their values.
    bool readTextBlock(std::string_view what, std::vector<std::string>& out)
    {
        if (!expectOpen(what)) {
            return false;
        }
        while (!atBlockEnd()) {
            out.push_back(next().text());
            if (out.size() > options_.maxValuesPerBlock) {
                fail(peek().line, std::string(what) + " holds more values than the limit");
                return false;
            }
        }
        expectClose(what);
        return !failed();
    }

    bool readBooleanBlock(std::string_view what, std::vector<bool>& out)
    {
        std::vector<std::string> words;
        const std::uint32_t line = peek().line;
        if (!readTextBlock(what, words)) {
            return false;
        }
        for (const std::string& word : words) {
            const auto value = detail::parseBoolean(word);
            if (!value) {
                fail(line, "'" + word + "' in " + std::string(what) + " is not a true/false flag");
                return false;
            }
            out.push_back(*value);
        }
        return true;
    }

    // Rows of `columns` numbers as vertices: x y [z]. A value count that does
    // not divide into rows means the rows cannot be told apart, and guessing
    // would shift every coordinate after the mistake.
    bool readVertexRows(std::string_view what, std::size_t columns, std::vector<Vertex>& vertices,
                        std::vector<double>* extraColumns = nullptr)
    {
        const std::uint32_t line = peek().line;
        std::vector<double> values;
        if (!readNumberBlock(what, values)) {
            return false;
        }
        if (values.size() % columns != 0) {
            fail(line, std::string(what) + " holds " + std::to_string(values.size()) +
                           " values, which is not a whole number of rows of " +
                           std::to_string(columns));
            return false;
        }
        vertices.reserve(vertices.size() + values.size() / columns);
        for (std::size_t row = 0; row < values.size(); row += columns) {
            if (std::isnan(values[row]) || std::isnan(values[row + 1])) {
                fail(line, std::string(what) + " has a null x or y; only a height can be null");
                return false;
            }
            Vertex vertex;
            vertex.x = values[row];
            vertex.y = values[row + 1];
            if (columns >= 3) {
                vertex.z = heightOf(values[row + 2]);
            }
            vertices.push_back(vertex);
            for (std::size_t extra = 3; extra < columns && extraColumns != nullptr; ++extra) {
                extraColumns->push_back(values[row + extra]);
            }
        }
        return true;
    }

    // The superseded `x y z radius bulge` rows of a polyline, a drainage
    // string and an old super string: geometry of the segment LEAVING each
    // vertex rides on the vertex.
    bool readPolylineRows(std::string_view what, std::vector<Vertex>& vertices,
                          std::vector<Segment>& segments)
    {
        std::vector<double> extras;
        if (!readVertexRows(what, 5, vertices, &extras)) {
            return false;
        }
        segments.clear();
        bool anyArc = false;
        for (std::size_t i = 0; i + 1 < extras.size(); i += 2) {
            Segment segment;
            const double radius = std::isnan(extras[i]) ? 0.0 : extras[i];
            if (radius != 0.0) {
                segment.kind = SegmentKind::Arc;
                segment.radius = radius;
                segment.major = !std::isnan(extras[i + 1]) && extras[i + 1] != 0.0;
                anyArc = true;
            }
            segments.push_back(std::move(segment));
        }
        if (!anyArc) {
            segments.clear(); // "empty means every segment is straight"
        }
        return true;
    }

    void readAttributes(AttributeList& out)
    {
        if (!expectOpen("attributes")) {
            return;
        }
        readAttributeBody(out);
        expectClose("attributes");
    }

    void readAttributeBody(AttributeList& out)
    {
        detail::CaseBuffer types;
        while (!atBlockEnd()) {
            const Token typeToken = next();
            const std::string_view type = types.lower(typeToken.raw);
            if (!typeToken.isWord()) {
                warn("line " + std::to_string(typeToken.line) +
                     ": text where an attribute type was expected; ignored");
                continue;
            }
            if (type == "group") {
                Attribute group;
                AttributeList members;
                if (peek().kind != TokenKind::Open) {
                    group.name = nextValue("group"); // `group "name" { ... }`
                }
                if (!expectOpen("group")) {
                    return;
                }
                detail::CaseBuffer keys;
                while (!atBlockEnd()) {
                    const Token key = next();
                    const std::string_view word = keys.lower(key.raw);
                    if (key.isWord() && word == "name") {
                        group.name = nextValue("name");
                    } else if (key.isWord() && word == "attributes") {
                        readAttributes(members);
                    } else if (peek().kind == TokenKind::Open) {
                        skipUnknownBlock(word);
                    } else if (key.isWord()) {
                        (void)next(); // an undocumented scalar of the group itself
                    }
                }
                expectClose("group");
                group.value = std::move(members);
                out.push_back(std::move(group));
                continue;
            }
            if (peek().kind == TokenKind::Open) {
                skipUnknownBlock(type);
                continue;
            }

            Attribute attribute;
            attribute.name = nextValue(type);
            const Token valueToken = next();
            if (!valueToken.isValue()) {
                fail(valueToken.line, "attribute '" + attribute.name + "' has no value");
                return;
            }
            const bool isNull = valueToken.kind == TokenKind::Word &&
                                detail::equalsIgnoringCase(valueToken.raw, "null");
            if (type == "integer" || type == "uid") {
                const auto value = detail::parseInteger(valueToken.raw);
                if (!value) {
                    if (!isNull) {
                        warn("line " + std::to_string(valueToken.line) + ": integer attribute '" +
                             attribute.name + "' has the value '" + std::string(valueToken.raw) +
                             "'; ignored");
                    }
                    ++nullAttributes_;
                    continue;
                }
                attribute.value = *value;
            } else if (type == "real") {
                const auto value = parseReal(valueToken.raw);
                if (!value) {
                    if (!isNull) {
                        warn("line " + std::to_string(valueToken.line) + ": real attribute '" +
                             attribute.name + "' has the value '" + std::string(valueToken.raw) +
                             "'; ignored");
                    }
                    ++nullAttributes_;
                    continue;
                }
                attribute.value = *value;
            } else {
                attribute.value = valueToken.text();
            }
            if (type != "integer" && type != "real" && type != "text") {
                attribute.declaredType = type;
            }
            out.push_back(std::move(attribute));
        }
    }

    // A block of `key value` pairs, with an optional `attributes` block and -
    // for a property control - a `data` block of its own.
    void readFieldBlock(std::string_view what, FieldList& fields,
                        AttributeList* attributes = nullptr, DrainageRecord* record = nullptr)
    {
        if (!expectOpen(what)) {
            return;
        }
        const Scope scope(*this, what);
        while (!atBlockEnd()) {
            const Token key = next();
            if (!key.isWord()) {
                warn("line " + std::to_string(key.line) + ": text where a keyword was expected in " +
                     std::string(what) + "; ignored");
                continue;
            }
            const std::string word = lowered(key.raw);
            if (peek().kind == TokenKind::Open) {
                if (word == "attributes" && attributes != nullptr) {
                    readAttributes(*attributes);
                } else if (word == "data" && record != nullptr) {
                    readPolylineRows("data", record->vertices, record->segments);
                } else {
                    skipUnknownBlock(word);
                }
                continue;
            }
            if (atBlockEnd()) {
                fields.add(word, {}); // a keyword with nothing after it
                break;
            }
            if (word.ends_with("colour")) {
                const bool wasQuoted = peek().kind == TokenKind::Quoted;
                fields.add(word, nextColour(word), wasQuoted);
            } else {
                const Token value = next();
                fields.add(word, value.text(), value.kind == TokenKind::Quoted);
            }
        }
        expectClose(what);
    }

    // `name_data { properties { ... } properties { ... } }`
    void readPropertiesList(std::string_view what, std::vector<FieldList>& out)
    {
        if (!expectOpen(what)) {
            return;
        }
        const Scope scope(*this, what);
        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            if (peek().kind != TokenKind::Open) {
                continue; // stray word between records
            }
            if (word == "properties") {
                FieldList properties;
                readFieldBlock("properties", properties);
                out.push_back(std::move(properties));
            } else {
                skipUnknownBlock(word);
            }
        }
        expectClose(what);
    }

    void readAttributesList(std::string_view what, std::vector<AttributeList>& out)
    {
        if (!expectOpen(what)) {
            return;
        }
        const Scope scope(*this, what);
        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            if (peek().kind != TokenKind::Open) {
                continue;
            }
            if (word == "attributes") {
                AttributeList attributes;
                readAttributes(attributes);
                out.push_back(std::move(attributes));
            } else {
                skipUnknownBlock(word);
            }
        }
        expectClose(what);
    }

    void readGeometryData(std::vector<Segment>& segments)
    {
        if (!expectOpen("geometry_data")) {
            return;
        }
        const Scope scope(*this, "geometry_data");
        segments.clear();
        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            if (peek().kind != TokenKind::Open) {
                continue;
            }
            Segment segment;
            FieldList fields;
            if (word == "straight") {
                readFieldBlock(word, fields);
            } else if (word == "arc") {
                readFieldBlock(word, fields);
                segment.kind = SegmentKind::Arc;
                segment.radius = fields.real("radius").value_or(0.0);
                segment.major = fields.boolean("major").value_or(false);
                if (segment.radius == 0.0) {
                    segment.kind = SegmentKind::Straight; // an arc of no radius is a line
                }
            } else if (word == "spiral" || word == "curve" || word == "parabola") {
                readFieldBlock(word, fields);
                segment.kind = word == "spiral"  ? SegmentKind::Spiral
                               : word == "curve" ? SegmentKind::Curve
                                                 : SegmentKind::Parabola;
                segment.parameters = std::move(fields);
            } else {
                // A kind of segment the manual does not have. It still
                // occupies a place in the list - dropping it would shift every
                // later segment onto the wrong pair of vertices - so it is
                // kept as a straight and said to be one.
                warn("line " + std::to_string(key.line) + ": segment of unknown kind '" + word +
                     "' is drawn as a straight");
                skipUnknownBlock(word);
            }
            segments.push_back(std::move(segment));
        }
        expectClose("geometry_data");
    }

    // radius_data + major_data -> segments, once both have been read.
    static void applyRadii(const std::vector<double>& radii, const std::vector<bool>& majors,
                           std::vector<Segment>& segments)
    {
        if (!segments.empty() || radii.empty()) {
            return; // geometry_data, when present, says more and wins
        }
        bool anyArc = false;
        for (std::size_t i = 0; i < radii.size(); ++i) {
            Segment segment;
            const double radius = std::isnan(radii[i]) ? 0.0 : radii[i];
            if (radius != 0.0) {
                segment.kind = SegmentKind::Arc;
                segment.radius = radius;
                segment.major = i < majors.size() && majors[i];
                anyArc = true;
            }
            segments.push_back(std::move(segment));
        }
        if (!anyArc) {
            segments.clear();
        }
    }

    // ---- strings -------------------------------------------------------------

    [[nodiscard]] StringHeader newHeader() const
    {
        StringHeader header;
        header.model = model_;
        header.colour = colour_;
        header.style = style_;
        header.breakline = breakline_;
        return header;
    }

    // The fields every string has. True when `key` was one and was consumed.
    bool readHeaderField(StringHeader& header, std::string_view key)
    {
        if (key == "name") {
            header.name = nextValue(key);
        } else if (key == "model") {
            header.model = nextValue(key); // registered by addElement
        } else if (key == "colour" || key == "color") {
            header.colour = nextColour(key);
        } else if (key == "style") {
            header.style = nextValue(key);
        } else if (key == "chainage") {
            header.chainage = nextNullableReal(key).value_or(0.0);
        } else if (key == "breakline") {
            header.breakline = readBreakline();
        } else if (key == "attributes" && peek().kind == TokenKind::Open) {
            readAttributes(header.attributes);
        } else {
            return false;
        }
        return true;
    }

    Breakline readBreakline()
    {
        const Token token = next();
        if (detail::equalsIgnoringCase(token.raw, "point")) {
            return Breakline::Point;
        }
        if (!detail::equalsIgnoringCase(token.raw, "line")) {
            warn("line " + std::to_string(token.line) + ": breakline '" + std::string(token.raw) +
                 "' is neither point nor line; read as line");
        }
        return Breakline::Line;
    }

    // Anything a string reader has no case for: a block is counted, a scalar
    // is kept. `documented` says the key is one the manual gives this element,
    // and so ALWAYS has a value ("a value must always exist", 1.4): the guard
    // below, which stops an undocumented flag from swallowing the next keyword,
    // must not fire on `text Pipe` or `title_1 Scale`, which are legal - the
    // manual only requires quotes around text that is not alphanumeric.
    void readUnknown(const Token& key, std::string_view word, FieldList& extras,
                     bool documented = false)
    {
        if (!key.isWord()) {
            warn("line " + std::to_string(key.line) + ": stray text \"" + key.text() + "\" in " +
                 pathWith("") + " ignored");
            return;
        }
        if (peek().kind == TokenKind::Open) {
            skipUnknownBlock(word);
            return;
        }
        if (atBlockEnd()) {
            extras.add(std::string(word), {});
            return;
        }
        // A keyword this reader knows cannot be the VALUE of one it does not:
        // the unknown word is a flag with no value, and swallowing the next
        // keyword as its value would lose a real field.
        if (!documented && peek().kind == TokenKind::Word && isKnownKeyword(peek().raw)) {
            extras.add(std::string(word), {});
            return;
        }
        if (word.ends_with("colour")) {
            const bool wasQuoted = peek().kind == TokenKind::Quoted;
            extras.add(std::string(word), nextColour(word), wasQuoted);
            return;
        }
        const Token value = next();
        extras.add(std::string(word), value.text(), value.kind == TokenKind::Quoted);
    }

    void readVertexString(StringKind kind)
    {
        VertexString string;
        string.kind = kind;
        string.header = newHeader();
        // Named before the Scope so that the name outlives the borrow.
        const std::string label = "string " + std::string(toString(kind));
        const Scope scope(*this, label);

        std::vector<double> radii;
        std::vector<bool> majors;
        FieldList fourDAnnotation;

        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            if (key.isWord() && readHeaderField(string.header, word)) {
                continue;
            }
            const bool block = peek().kind == TokenKind::Open;
            if (!key.isWord()) {
                readUnknown(key, word, string.header.extras);
            } else if (word == "closed") {
                string.closed = nextBoolean(word);
            } else if (word == "z" && !block) {
                string.constantZ = heightOf(nextNullableReal(word).value_or(kNull));
            } else if (word == "data_2d" && block) {
                readVertexRows(word, 2, string.vertices);
            } else if (word == "data_3d" && block) {
                readVertexRows(word, 3, string.vertices);
            } else if (word == "data" && block) {
                readLegacyData(string);
            } else if (word == "radius_data" && block) {
                readNumberBlock(word, radii);
            } else if (word == "major_data" && block) {
                readBooleanBlock(word, majors);
            } else if (word == "geometry_data" && block) {
                readGeometryData(string.segments);
            } else if (word == "colour_data" && block) {
                readTextBlock(word, string.segmentColours);
            } else if (word == "point_data" && block) {
                readTextBlock(word, string.pointIds);
            } else if ((word == "diameter_value" || word == "diameter") && !block) {
                string.diameter = nextNullableReal(word);
            } else if (word == "diameter_data" && block) {
                readNumberBlock(word, string.diameters);
            } else if (word == "pipe_value" && block) {
                // What 12d Model 15 writes where the manual says diameter_value.
                FieldList fields;
                readFieldBlock(word, fields);
                string.diameter = fields.real("diameter");
                keepOthers(fields, {"diameter"}, "pipe_value.", string.header.extras);
            } else if (word == "pipe_data" && block) {
                std::vector<FieldList> records;
                readPropertiesList(word, records);
                for (const FieldList& record : records) {
                    string.diameters.push_back(record.real("diameter").value_or(0.0));
                }
            } else if (word == "culvert_value" && block) {
                FieldList fields;
                readFieldBlock(word, fields);
                string.culvert = std::array<double, 2>{fields.real("width").value_or(0.0),
                                                       fields.real("height").value_or(0.0)};
            } else if (word == "culvert_data" && block) {
                std::vector<FieldList> records;
                readPropertiesList(word, records);
                for (const FieldList& record : records) {
                    string.culverts.push_back({record.real("width").value_or(0.0),
                                               record.real("height").value_or(0.0)});
                }
            } else if ((word == "justify" || word == "pipe_justify") && !block &&
                       kind != StringKind::FourD) {
                string.justify = lowered(nextValue(word));
            } else if (word == "vertex_tinable_value" && !block) {
                string.vertexTinableValue = nextBoolean(word);
            } else if (word == "segment_tinable_value" && !block) {
                string.segmentTinableValue = nextBoolean(word);
            } else if (word == "vertex_visible_value" && !block) {
                string.vertexVisibleValue = nextBoolean(word);
            } else if (word == "segment_visible_value" && !block) {
                string.segmentVisibleValue = nextBoolean(word);
            } else if (word == "vertex_tinable_data" && block) {
                readBooleanBlock(word, string.vertexTinable);
            } else if (word == "segment_tinable_data" && block) {
                readBooleanBlock(word, string.segmentTinable);
            } else if (word == "vertex_visible_data" && block) {
                readBooleanBlock(word, string.vertexVisible);
            } else if (word == "segment_visible_data" && block) {
                readBooleanBlock(word, string.segmentVisible);
            } else if (word == "vertex_text_value" && !block) {
                string.vertexTextValue = nextValue(word);
            } else if (word == "segment_text_value" && !block) {
                string.segmentTextValue = nextValue(word);
            } else if (word == "vertex_text_data" && block) {
                readTextBlock(word, string.vertexText);
            } else if (word == "segment_text_data" && block) {
                readTextBlock(word, string.segmentText);
            } else if (word == "vertex_annotate_value" && block) {
                string.vertexAnnotation.emplace();
                readFieldBlock(word, *string.vertexAnnotation);
            } else if (word == "segment_annotate_value" && block) {
                string.segmentAnnotation.emplace();
                readFieldBlock(word, *string.segmentAnnotation);
            } else if (word == "vertex_annotate_data" && block) {
                readPropertiesList(word, string.vertexAnnotations);
            } else if (word == "segment_annotate_data" && block) {
                readPropertiesList(word, string.segmentAnnotations);
            } else if (word == "symbol_value" && block) {
                string.symbol.emplace();
                readFieldBlock(word, *string.symbol);
            } else if (word == "symbol_data" && block) {
                readPropertiesList(word, string.symbols);
            } else if (word == "vertex_attribute_data" && block) {
                readAttributesList(word, string.vertexAttributes);
            } else if (word == "segment_attribute_data" && block) {
                readAttributesList(word, string.segmentAttributes);
            } else if (word == "interval" && block) {
                string.interval.emplace();
                readFieldBlock(word, *string.interval);
            } else if (kind == StringKind::FourD && !block && isTextAnnotationKey(word)) {
                const Token value = next();
                fourDAnnotation.add(std::string(word), value.text(),
                                    value.kind == TokenKind::Quoted);
            } else {
                // The face string's hatching (manual 1.5.4), and the fields
                // every string may carry: documented, so they take a value.
                static const std::vector<std::string_view> documented = {
                    "hatch_angle", "hatch_distance", "hatch_colour", "edge_colour",
                    "fill_mode",   "edge_mode",      "null_value",   "weight",
                    "time_created", "time_updated",  "interval"};
                readUnknown(key, word, string.header.extras,
                            std::find(documented.begin(), documented.end(), word) !=
                                documented.end());
            }
        }
        expectClose("string " + std::string(toString(kind)));
        if (failed()) {
            return;
        }

        const double nullValue = nullValueFor(string.header.extras);
        applyNull(string.vertices, nullValue);
        applyNull(string.constantZ, nullValue);
        applyRadii(radii, majors, string.segments);
        if (!fourDAnnotation.empty()) {
            string.vertexAnnotation = std::move(fourDAnnotation);
        }
        checkCounts(string);
        addElement(std::move(string));
    }

    // The `data` block of the string types that have one, whose columns
    // depend on the type (manual 1.5.11-1.5.15, 1.5.2, 1.5.5, 1.5.8.3).
    void readLegacyData(VertexString& string)
    {
        switch (string.kind) {
        case StringKind::TwoD:
            readVertexRows("data", 2, string.vertices);
            return;
        case StringKind::ThreeD:
        case StringKind::Pipe:
        case StringKind::Face:
            readVertexRows("data", 3, string.vertices);
            return;
        case StringKind::Interface: {
            std::vector<double> modes;
            if (readVertexRows("data", 4, string.vertices, &modes)) {
                std::size_t bad = 0;
                for (const double mode : modes) {
                    // -1 cut, 0 on the surface, 1 fill (manual 1.5.6). Anything
                    // else is not a mode, and a cast would make one up.
                    const bool valid = mode == -1.0 || mode == 0.0 || mode == 1.0;
                    bad += valid ? 0 : 1;
                    string.interfaceModes.push_back(valid ? static_cast<int>(mode) : 0);
                }
                if (bad != 0) {
                    warn("string \"" + string.header.name + "\": " + std::to_string(bad) +
                         " interface modes are not -1, 0 or 1 and were read as 0");
                }
            }
            return;
        }
        case StringKind::FourD: {
            // x y z text - the one data block that is not all numbers.
            const std::uint32_t line = peek().line;
            std::vector<std::string> tokens;
            if (!readTextBlock("data", tokens)) {
                return;
            }
            if (tokens.size() % 4 != 0) {
                fail(line, "the data of a 4d string holds " + std::to_string(tokens.size()) +
                               " values, which is not a whole number of x y z text rows");
                return;
            }
            for (std::size_t row = 0; row < tokens.size(); row += 4) {
                const auto x = parseReal(tokens[row]);
                const auto y = parseReal(tokens[row + 1]);
                const bool nullZ = detail::equalsIgnoringCase(tokens[row + 2], "null");
                const auto z = parseReal(tokens[row + 2]);
                if (!x || !y || (!z && !nullZ)) {
                    fail(line, "a row of the 4d string's data does not start with x y z");
                    return;
                }
                string.vertices.push_back(Vertex{*x, *y, heightOf(z.value_or(kNull))});
                string.vertexText.push_back(tokens[row + 3]);
            }
            return;
        }
        case StringKind::Super:
        case StringKind::Polyline:
            readPolylineRows("data", string.vertices, string.segments);
            return;
        }
    }

    [[nodiscard]] static bool isTextAnnotationKey(std::string_view word)
    {
        static const std::vector<std::string_view> keys = {
            "angle",     "offset",     "raise",    "worldsize", "papersize", "screensize",
            "textstyle", "slant",      "xfactor",  "x_factor",  "justify",   "text_colour",
            "whiteout",  "border_style"};
        return std::find(keys.begin(), keys.end(), word) != keys.end();
    }

    // Moves what a small block held BESIDES its modelled members into extras,
    // so that `pipe_value { diameter 0.3 thickness 0.02 }` does not lose the
    // thickness for having been understood.
    static void keepOthers(const FieldList& fields, std::initializer_list<std::string_view> known,
                           std::string_view prefix, FieldList& extras)
    {
        for (const Field& field : fields.fields()) {
            if (std::find(known.begin(), known.end(), field.key) == known.end()) {
                extras.add(std::string(prefix) + field.key, field.value, field.quoted);
            }
        }
    }

    void checkCounts(const VertexString& string)
    {
        const std::size_t vertices = string.vertices.size();
        const std::size_t segments = string.segmentCount();
        const auto perVertex = [&](std::size_t count, const char* what) {
            if (count != 0 && count != vertices) {
                warn("string \"" + string.header.name + "\": " + what + " has " +
                     std::to_string(count) + " entries for " + std::to_string(vertices) +
                     " vertices");
            }
        };
        // n entries are allowed for the n-1 segments of an open string.
        const auto perSegment = [&](std::size_t count, const char* what) {
            if (count != 0 && count != segments && count != vertices) {
                warn("string \"" + string.header.name + "\": " + what + " has " +
                     std::to_string(count) + " entries for " + std::to_string(segments) +
                     " segments");
            }
        };
        perVertex(string.pointIds.size(), "point_data");
        perVertex(string.vertexText.size(), "vertex_text_data");
        perVertex(string.vertexTinable.size(), "vertex_tinable_data");
        perVertex(string.vertexVisible.size(), "vertex_visible_data");
        perVertex(string.vertexAttributes.size(), "vertex_attribute_data");
        perSegment(string.segments.size(), "the segment geometry");
        perSegment(string.segmentColours.size(), "colour_data");
        perSegment(string.segmentText.size(), "segment_text_data");
        perSegment(string.diameters.size(), "the pipe diameters");
        perSegment(string.culverts.size(), "culvert_data");
        // The manual (1.5.8.4.4) says a string cannot have both pipe diameters
        // and culvert dimensions. 12d Model 15 writes both on every culvert
        // string it exports, so that is not worth a warning; both are kept.
    }

    // Scalars of a string whose geometry is all keyword fields. `documented`
    // lists the keys the manual gives the element, every one of which takes a
    // value; anything else is an unknown and gets the flag-or-value guess.
    void readScalarString(std::string_view what, StringHeader& header, FieldList& fields,
                          const std::vector<std::string_view>& documented)
    {
        const Scope scope(*this, what);
        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            if (key.isWord() && readHeaderField(header, word)) {
                continue;
            }
            const bool known =
                std::find(documented.begin(), documented.end(), word) != documented.end();
            readUnknown(key, word, fields, known);
        }
        expectClose(what);
    }

    // A vertex from three named fields, or nothing when x or y is missing or
    // not a number: a point placed at the origin for want of a coordinate is
    // a point nobody wrote, and in a projected job it is 6 000 km away.
    [[nodiscard]] static std::optional<Vertex> takeVertex(FieldList& fields, std::string_view x,
                                                          std::string_view y, std::string_view z)
    {
        const auto ex = fields.real(x);
        const auto ny = fields.real(y);
        if (!ex || !ny) {
            return std::nullopt;
        }
        Vertex vertex;
        vertex.x = *ex;
        vertex.y = *ny;
        const auto height = fields.real(z);
        vertex.z = height ? heightOf(*height) : std::nullopt;
        fields.take(x);
        fields.take(y);
        fields.take(z);
        return vertex;
    }

    // Manual 1.4.6: "if there is not enough recognised information to define
    // the string, the string is ignored" - said, not silent.
    void incomplete(const StringHeader& header, std::string_view what, std::string_view missing)
    {
        warn(std::string(what) + " \"" + header.name + "\" in model \"" + header.model +
             "\" ignored: " + std::string(missing) + " is missing or not a number");
    }

    static const std::vector<std::string_view>& arcKeys()
    {
        static const std::vector<std::string_view> keys = {
            "interval", "radius", "xcentre", "ycentre", "zcentre", "xstart", "ystart",
            "zstart",   "xend",   "yend",    "zend",    "weight",  "time_created",
            "time_updated", "null_value"};
        return keys;
    }

    void readArcString()
    {
        ArcString arc;
        arc.header = newHeader();
        FieldList fields;
        readScalarString("string arc", arc.header, fields, arcKeys());
        if (failed()) {
            return;
        }
        arc.radius = fields.real("radius").value_or(0.0);
        fields.take("radius");
        const auto centre = takeVertex(fields, "xcentre", "ycentre", "zcentre");
        const auto start = takeVertex(fields, "xstart", "ystart", "zstart");
        const auto end = takeVertex(fields, "xend", "yend", "zend");
        if (!centre || !start || !end) {
            incomplete(arc.header, "string arc",
                       !centre ? "its centre" : !start ? "its start" : "its end");
            return;
        }
        arc.centre = *centre;
        arc.start = *start;
        arc.end = *end;
        arc.header.extras = std::move(fields);
        const double nullValue = nullValueFor(arc.header.extras);
        applyNull(arc.centre.z, nullValue);
        applyNull(arc.start.z, nullValue);
        applyNull(arc.end.z, nullValue);
        addElement(std::move(arc));
    }

    void readCircleString(bool feature)
    {
        CircleString circle;
        circle.feature = feature;
        circle.header = newHeader();
        FieldList fields;
        const char* what = feature ? "string feature" : "string circle";
        readScalarString(what, circle.header, fields, arcKeys());
        if (failed()) {
            return;
        }
        const auto radius = fields.real("radius");
        fields.take("radius");
        const auto centre = takeVertex(fields, "xcentre", "ycentre", "zcentre");
        if (!centre || !radius) {
            incomplete(circle.header, what, !centre ? "its centre" : "its radius");
            return;
        }
        circle.radius = *radius;
        circle.centre = *centre;
        circle.header.extras = std::move(fields);
        applyNull(circle.centre.z, nullValueFor(circle.header.extras));
        addElement(std::move(circle));
    }

    void readTextString()
    {
        TextString text;
        text.header = newHeader();
        FieldList fields;
        static const std::vector<std::string_view> keys = {
            "x",        "y",     "z",         "text",      "angle",       "offset",
            "raise",    "textstyle", "slant", "xfactor",   "x_factor",    "worldsize",
            "papersize", "screensize", "justify", "text_colour", "whiteout", "border_style",
            "weight",   "time_created", "time_updated", "null_value"};
        readScalarString("string text", text.header, fields, keys);
        if (failed()) {
            return;
        }
        if (const auto value = fields.take("text")) {
            text.text = value->value;
        }
        const auto position = takeVertex(fields, "x", "y", "z");
        if (!position) {
            incomplete(text.header, "string text", "its position");
            return;
        }
        text.position = *position;
        for (const std::string_view key : {"weight", "time_created", "time_updated", "null_value"}) {
            if (const auto field = fields.take(key)) {
                text.header.extras.add(field->key, field->value, field->quoted);
            }
        }
        text.annotation = std::move(fields);
        applyNull(text.position.z, nullValueFor(text.header.extras));
        addElement(std::move(text));
    }

    // `text "label" x y [z]` on one line: not in the manual, but written by
    // other tools and read by 12d-compatible viewers.
    void readShortText(std::uint32_t line)
    {
        TextString text;
        text.header = newHeader();
        text.text = nextValue("text");
        std::vector<double> numbers;
        while (numbers.size() < 3 && peek().kind == TokenKind::Word) {
            const bool isNull = detail::equalsIgnoringCase(peek().raw, "null");
            const auto value = parseReal(peek().raw);
            if (!value && !isNull) {
                break;
            }
            numbers.push_back(value.value_or(kNull));
            (void)next();
        }
        if (numbers.size() < 2 || std::isnan(numbers[0]) || std::isnan(numbers[1])) {
            fail(line, "'text \"" + text.text + "\"' must be followed by x and y");
            return;
        }
        text.position = Vertex{numbers[0], numbers[1],
                               numbers.size() > 2 ? heightOf(numbers[2]) : std::nullopt};
        applyNull(text.position.z, archive_.nullValue);
        addElement(std::move(text));
    }

    void readPlotFrame()
    {
        PlotFrame frame;
        frame.header = newHeader();
        static const std::vector<std::string_view> keys = {
            "title_file",  "border",       "viewport",     "user_title_file", "use_title_file",
            "title_1",     "title_2",      "plot_file",    "text_size",       "sheet_code",
            "width",       "height",       "scale",        "rotation",        "xorigin",
            "yorigin",     "left_margin",  "right_margin", "top_margin",      "bottom_margin",
            "plotter",     "textstyle",    "plotter_type", "plotter_mode",    "plotter_names",
            "weight",      "time_created", "time_updated"};
        readScalarString("string plot_frame", frame.header, frame.fields, keys);
        if (!failed()) {
            addElement(std::move(frame));
        }
    }

    void readDrainageString()
    {
        DrainageString drainage;
        drainage.header = newHeader();
        const Scope scope(*this, "string drainage");
        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            if (key.isWord() && readHeaderField(drainage.header, word)) {
                continue;
            }
            const bool block = peek().kind == TokenKind::Open;
            if (!key.isWord()) {
                readUnknown(key, word, drainage.header.extras);
            } else if (word == "outfall" && !block) {
                drainage.outfall = nextNullableReal(word);
            } else if (word == "flow_direction" && !block) {
                const std::uint32_t line = key.line;
                const double direction = nextReal(word);
                if (direction == 0.0 || direction == 1.0) {
                    drainage.flowDirection = static_cast<int>(direction);
                } else if (!failed()) {
                    warn("line " + std::to_string(line) + ": flow_direction must be 0 or 1; '" +
                         detail::formatExactReal(direction) + "' ignored");
                }
            } else if (word == "data" && block) {
                readPolylineRows("data", drainage.vertices, drainage.segments);
            } else if ((word == "pit" || word == "pit_v2") && block) {
                drainage.pitsAreVersion2 = drainage.pitsAreVersion2 || word == "pit_v2";
                DrainageRecord record;
                readFieldBlock(word, record.fields, &record.attributes);
                drainage.pits.push_back(std::move(record));
            } else if (word == "pipe" && block) {
                DrainageRecord record;
                readFieldBlock(word, record.fields, &record.attributes);
                drainage.pipes.push_back(std::move(record));
            } else if (word == "property_control" && block) {
                DrainageRecord record;
                readFieldBlock(word, record.fields, &record.attributes, &record);
                drainage.propertyControls.push_back(std::move(record));
            } else if (word == "house_connection" && block) {
                DrainageRecord record;
                readFieldBlock(word, record.fields, &record.attributes);
                drainage.houseConnections.push_back(std::move(record));
            } else {
                readUnknown(key, word, drainage.header.extras);
            }
        }
        expectClose("string drainage");
        if (!failed()) {
            const double nullValue = nullValueFor(drainage.header.extras);
            applyNull(drainage.vertices, nullValue);
            for (DrainageRecord& control : drainage.propertyControls) {
                applyNull(control.vertices, nullValue);
            }
            addElement(std::move(drainage));
        }
    }

    // ---- alignments ----------------------------------------------------------

    void readParts(std::string_view what, std::vector<AlignmentPart>& parts)
    {
        if (!expectOpen(what)) {
            return;
        }
        const Scope scope(*this, what);
        while (!atBlockEnd()) {
            const Token key = next();
            if (peek().kind != TokenKind::Open) {
                continue; // nothing in a parts block is a bare scalar
            }
            AlignmentPart part;
            part.kind = lowered(key.raw);
            readFieldBlock(part.kind, part.fields, &part.attributes);
            parts.push_back(std::move(part));
        }
        expectClose(what);
    }

    void readSolvedGeometry(std::string_view what, SolvedGeometry& geometry)
    {
        const bool vertical = what == "vertical_data";
        if (!expectOpen(what)) {
            return;
        }
        const Scope scope(*this, what);
        std::vector<double> radii;
        std::vector<bool> majors;
        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            const bool block = peek().kind == TokenKind::Open;
            if (!key.isWord()) {
                continue;
            }
            if (word == "data_2d" && block) {
                readVertexRows(word, 2, geometry.vertices);
            } else if (word == "geometry_data" && block) {
                readGeometryData(geometry.segments);
            } else if (word == "radius_data" && block) {
                readNumberBlock(word, radii);
            } else if (word == "major_data" && block) {
                readBooleanBlock(word, majors);
            } else if (word == "closed" && !block) {
                geometry.closed = nextBoolean(word);
            } else if (word == "interval" && block) {
                geometry.interval.emplace();
                readFieldBlock(word, *geometry.interval);
            } else if (word == "colour_data" && block) {
                readTextBlock(word, geometry.segmentColours);
            } else {
                readUnknown(key, word, geometry.header);
            }
        }
        expectClose(what);
        applyRadii(radii, majors, geometry.segments);
        if (vertical) {
            // "major ... is ignored since only minor arcs are used" (manual
            // 1.5.9.2.3b): a grade line cannot double back in chainage.
            for (Segment& segment : geometry.segments) {
                segment.major = false;
            }
        }
    }

    void readSuperAlignment()
    {
        SuperAlignment alignment;
        alignment.header = newHeader();
        const Scope scope(*this, "string super_alignment");
        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            if (key.isWord() && readHeaderField(alignment.header, word)) {
                continue;
            }
            const bool block = peek().kind == TokenKind::Open;
            if (!key.isWord()) {
                readUnknown(key, word, alignment.header.extras);
            } else if (word == "closed" && !block) {
                alignment.closed = nextBoolean(word);
            } else if (word == "spiral_type" && !block) {
                alignment.spiralType = nextValue(word);
            } else if (word == "valid_horizontal" && !block) {
                alignment.validHorizontal = nextBoolean(word);
            } else if (word == "valid_vertical" && !block) {
                alignment.validVertical = nextBoolean(word);
            } else if (word == "pipe_value" && block) {
                // Where a super alignment that is a pipeline keeps its size.
                FieldList fields;
                readFieldBlock(word, fields);
                alignment.diameter = fields.real("diameter");
            } else if (word == "length" && !block) {
                alignment.pipeLength = nextNullableReal(word);
            } else if (word == "horizontal_parts" && block) {
                readParts(word, alignment.horizontalParts);
            } else if (word == "vertical_parts" && block) {
                readParts(word, alignment.verticalParts);
            } else if (word == "horizontal_data" && block) {
                alignment.horizontalData.emplace();
                readSolvedGeometry(word, *alignment.horizontalData);
            } else if (word == "vertical_data" && block) {
                alignment.verticalData.emplace();
                readSolvedGeometry(word, *alignment.verticalData);
            } else {
                readUnknown(key, word, alignment.header.extras);
            }
        }
        expectClose("string super_alignment");
        if (!failed()) {
            addElement(std::move(alignment));
        }
    }

    // The superseded alignment and pipeline strings. Their hipdata and vipdata
    // are the IP method written as rows instead of blocks, and are read into
    // the same parts, numbered in hundreds as the manual numbers them.
    void readLegacyAlignment(AlignmentSource source)
    {
        SuperAlignment alignment;
        alignment.source = source;
        alignment.header = newHeader();
        const std::string what =
            source == AlignmentSource::Pipeline ? "string pipeline" : "string alignment";
        const Scope scope(*this, what);
        std::int64_t partId = 0;

        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            if (key.isWord() && readHeaderField(alignment.header, word)) {
                continue;
            }
            const bool block = peek().kind == TokenKind::Open;
            if (!key.isWord()) {
                readUnknown(key, word, alignment.header.extras);
            } else if (word == "spiral_type" && !block) {
                alignment.spiralType = nextValue(word);
            } else if (word == "diameter" && !block && source == AlignmentSource::Pipeline) {
                alignment.diameter = nextNullableReal(word);
            } else if (word == "length" && !block && source == AlignmentSource::Pipeline) {
                alignment.pipeLength = nextNullableReal(word);
            } else if (word == "hipdata" && block) {
                readHipData(alignment.horizontalParts, partId);
            } else if (word == "vipdata" && block) {
                readVipData(alignment.verticalParts, partId);
            } else {
                readUnknown(key, word, alignment.header.extras);
            }
        }
        expectClose(what);
        if (!failed()) {
            addElement(std::move(alignment));
        }
    }

    // x y radius [spil1 length] [spil2 length]
    void readHipData(std::vector<AlignmentPart>& parts, std::int64_t& partId)
    {
        const std::uint32_t line = peek().line;
        std::vector<std::string> tokens;
        if (!readTextBlock("hipdata", tokens)) {
            return;
        }
        for (std::size_t i = 0; i < tokens.size();) {
            const auto x = parseReal(tokens[i]);
            const auto y = i + 1 < tokens.size() ? parseReal(tokens[i + 1]) : std::nullopt;
            const auto radius = i + 2 < tokens.size() ? parseReal(tokens[i + 2]) : std::nullopt;
            if (!x || !y || !radius) {
                fail(line, "a row of hipdata is not 'x y radius'");
                return;
            }
            i += 3;
            double in = 0.0;
            double out = 0.0;
            while (i + 1 < tokens.size()) {
                const std::string tag = lowered(tokens[i]);
                if (tag != "spil1" && tag != "spil2") {
                    break;
                }
                const auto length = parseReal(tokens[i + 1]);
                if (!length) {
                    fail(line, "'" + tag + "' in hipdata needs a length");
                    return;
                }
                (tag == "spil1" ? in : out) = *length;
                i += 2;
            }
            AlignmentPart part;
            part.kind = (in != 0.0 || out != 0.0) ? "spiral" : *radius != 0.0 ? "arc" : "ip";
            part.fields.setInteger("id", partId += 100);
            if (part.kind != "ip") {
                part.fields.setReal("r", *radius);
            }
            if (part.kind == "spiral") {
                part.fields.setReal("l1", in);
                part.fields.setReal("l2", out);
            }
            part.fields.setReal("x", *x);
            part.fields.setReal("y", *y);
            parts.push_back(std::move(part));
        }
    }

    // chainage height length [parabola]   |   chainage height radius circle
    void readVipData(std::vector<AlignmentPart>& parts, std::int64_t& partId)
    {
        const std::uint32_t line = peek().line;
        std::vector<std::string> tokens;
        if (!readTextBlock("vipdata", tokens)) {
            return;
        }
        for (std::size_t i = 0; i < tokens.size();) {
            const auto chainage = parseReal(tokens[i]);
            const auto height = i + 1 < tokens.size() ? parseReal(tokens[i + 1]) : std::nullopt;
            const auto size = i + 2 < tokens.size() ? parseReal(tokens[i + 2]) : std::nullopt;
            if (!chainage || !height || !size) {
                fail(line, "a row of vipdata is not 'chainage height length'");
                return;
            }
            i += 3;
            bool circle = false;
            if (i < tokens.size()) {
                const std::string tag = lowered(tokens[i]);
                if (tag == "parabola" || tag == "circle") {
                    circle = tag == "circle";
                    ++i;
                }
            }
            AlignmentPart part;
            part.kind = *size == 0.0 ? "ip" : circle ? "arc" : "length";
            part.fields.setInteger("id", partId += 100);
            if (part.kind == "arc") {
                part.fields.setReal("r", *size);
            } else if (part.kind == "length") {
                part.fields.setReal("l", *size);
            }
            part.fields.setReal("x", *chainage);
            part.fields.setReal("y", *height);
            parts.push_back(std::move(part));
        }
    }

    // ---- point clouds ---------------------------------------------------------

    void readLasCloud()
    {
        LasCloud cloud;
        cloud.header = newHeader();
        const Scope scope(*this, "string las_cloud_data");
        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            if (key.isWord() && readHeaderField(cloud.header, word)) {
                continue;
            }
            const bool block = peek().kind == TokenKind::Open;
            if (key.isWord() && (word == "data" || word == "ref_data") && block) {
                readLasData(word, cloud);
            } else {
                readUnknown(key, word, cloud.header.extras);
            }
        }
        expectClose("string las_cloud_data");
        if (!failed()) {
            if (badLasFields_ != 0) {
                warn("point cloud \"" + cloud.header.name + "\": " +
                     std::to_string(badLasFields_) +
                     " point fields were not numbers or were outside the range the manual "
                     "gives them, and were read as 0 or clamped");
                badLasFields_ = 0;
            }
            addElement(std::move(cloud));
        }
    }

    void readLasData(std::string_view what, LasCloud& cloud)
    {
        if (!expectOpen(what)) {
            return;
        }
        const Scope scope(*this, what);
        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            const bool block = peek().kind == TokenKind::Open;
            if (!key.isWord()) {
                continue;
            }
            if (word == "categories" && block) {
                readBooleanBlock(word, cloud.categories);
            } else if (word == "range" && block) {
                readFieldBlock(word, cloud.range);
            } else if (word == "format" && !block) {
                cloud.format = lowered(nextValue(word));
                cloud.pointFormat = pointFormatOf(cloud.format);
            } else if (word == "file_name" && !block) {
                cloud.referenceFile = nextValue(word);
            } else if (block && (word.starts_with("points_v") ||
                                 word.starts_with("compact_points_v"))) {
                const bool compact = word.starts_with("compact_");
                const std::string format(word.substr(compact ? 15 : 7));
                if (cloud.format.empty()) {
                    cloud.format = format;
                    cloud.pointFormat = pointFormatOf(format);
                } else if (cloud.format != format) {
                    warn("line " + std::to_string(key.line) + ": point cloud declares format " +
                         cloud.format + " but holds a " + std::string(word) + " block");
                }
                readLasPoints(word, compact, pointFormatOf(format), cloud.points);
            } else {
                readUnknown(key, word, cloud.header.extras);
            }
        }
        expectClose(what);
    }

    // "v12_p3" -> 3
    [[nodiscard]] static int pointFormatOf(const std::string& format)
    {
        const auto marker = format.rfind("_p");
        if (marker == std::string::npos) {
            return 0;
        }
        const auto value = detail::parseInteger(std::string_view(format).substr(marker + 2));
        return value && *value >= 0 && *value <= 10 ? static_cast<int>(*value) : 0;
    }

    void readLasPoints(std::string_view what, bool compact, int pointFormat,
                       std::vector<LasPoint>& points)
    {
        if (!expectOpen(what)) {
            return;
        }
        const Scope scope(*this, what);
        const auto& order = lasFieldOrder(pointFormat);
        detail::CaseBuffer tags;
        while (!atBlockEnd()) {
            const Token key = next();
            if (peek().kind != TokenKind::Open) {
                continue;
            }
            if (!detail::equalsIgnoringCase(key.raw, "p")) {
                skipUnknownBlock(lowered(key.raw));
                continue;
            }
            const std::uint32_t line = key.line;
            std::vector<std::string> tokens;
            if (!readTextBlock("p", tokens)) {
                return;
            }
            LasPoint point;
            if (compact) {
                if (tokens.size() < 3) {
                    fail(line, "a compact point record needs at least x y z");
                    return;
                }
                for (std::size_t i = 0; i < tokens.size() && i < order.size(); ++i) {
                    badLasFields_ += setLasField(point, order[i], tokens[i]) ? 0 : 1;
                }
            } else {
                for (std::size_t i = 0; i + 1 < tokens.size(); i += 2) {
                    badLasFields_ +=
                        setLasField(point, tags.lower(tokens[i]), tokens[i + 1]) ? 0 : 1;
                }
            }
            points.push_back(point);
            if (points.size() > options_.maxValuesPerBlock) {
                fail(line, "the point cloud holds more points than the limit");
                return;
            }
        }
        expectClose(what);
    }

    // ---- tins ------------------------------------------------------------------

    void readTin(bool full)
    {
        Tin tin;
        tin.full = full;
        tin.colour = colour_;
        const std::string what = full ? "full_tin" : "tin";
        const std::uint32_t startLine = peek().line;
        if (!expectOpen(what)) {
            return;
        }
        const Scope scope(*this, what);
        std::vector<std::int64_t> triangles;
        std::vector<std::int64_t> neighbours;
        std::vector<std::int64_t> nulling;

        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            const bool block = peek().kind == TokenKind::Open;
            if (!key.isWord()) {
                readUnknown(key, word, tin.extras);
            } else if (word == "name" && !block) {
                tin.name = nextValue(word);
            } else if ((word == "colour" || word == "color") && !block) {
                tin.colour = nextColour(word);
            } else if (word == "attributes" && block) {
                readAttributes(tin.attributes);
            } else if (word == "points" && block) {
                readVertexRows(word, 3, tin.points);
            } else if (word == "triangles" && block) {
                readTriangles(triangles, neighbours);
            } else if (word == "neighbours" && block) {
                readIntegerBlock(word, neighbours);
            } else if (word == "nulling" && block) {
                readIntegerBlock(word, nulling);
            } else if (word == "colours" && block) {
                readTextBlock(word, tin.colours);
            } else if (word == "input" && block) {
                readTinInput(tin);
            } else {
                if (word == "transformation" && block) {
                    warn("tin \"" + tin.name +
                         "\" carries a transformation block, which the manual does not define; "
                         "its points are read as written");
                }
                readUnknown(key, word, tin.extras);
            }
        }
        expectClose(what);
        if (failed()) {
            return;
        }

        // Manual 1.4.7: points and triangles are MANDATORY in both forms;
        // neighbours and nulling in a full_tin. Absent, they default - to no
        // surface, or to every triangle visible - and a default that quiet
        // would be a mistake hidden (PLAN.MD section 36).
        const auto mandatory = [&](bool present, const char* block) {
            if (!present) {
                warn(what + " \"" + tin.name + "\" has no " + block +
                     " block, which the manual makes mandatory");
            }
        };
        mandatory(!tin.points.empty(), "points");
        mandatory(!triangles.empty(), "triangles");
        if (full) {
            mandatory(!neighbours.empty(), "neighbours");
            mandatory(!nulling.empty(), "nulling");
        }
        applyNull(tin.points, archive_.nullValue);
        if (!buildTriangles(what, startLine, tin.points.size(), triangles, tin.triangles)) {
            return;
        }
        if (!neighbours.empty()) {
            if (neighbours.size() != triangles.size()) {
                warn(what + " \"" + tin.name + "\": neighbours holds " +
                     std::to_string(neighbours.size() / 3) + " rows for " +
                     std::to_string(tin.triangles.size()) + " triangles; ignored");
            } else {
                for (std::size_t i = 0; i + 2 < neighbours.size(); i += 3) {
                    std::array<std::uint32_t, 3> row{};
                    for (std::size_t k = 0; k < 3; ++k) {
                        const std::int64_t value = neighbours[i + k];
                        row[k] = value <= 0 || static_cast<std::size_t>(value) > tin.triangles.size()
                                     ? Tin::kNoNeighbour
                                     : static_cast<std::uint32_t>(value - 1);
                    }
                    tin.neighbours.push_back(row);
                }
            }
        }
        if (!nulling.empty()) {
            if (nulling.size() != tin.triangles.size()) {
                warn(what + " \"" + tin.name + "\": nulling holds " +
                     std::to_string(nulling.size()) + " flags for " +
                     std::to_string(tin.triangles.size()) +
                     " triangles; triangles without a flag are taken as visible");
            }
            tin.visible.reserve(nulling.size());
            for (const std::int64_t flag : nulling) {
                tin.visible.push_back(flag == 2);
            }
            tin.visible.resize(tin.triangles.size(), true);
        }
        if (tin.name.empty()) {
            warn(what + " at line " + std::to_string(startLine) +
                 " has no name, which the manual makes mandatory");
        }
        if (!tin.colours.empty() && tin.colours.size() != tin.triangles.size()) {
            warn(what + " \"" + tin.name + "\": colours holds " +
                 std::to_string(tin.colours.size()) + " entries for " +
                 std::to_string(tin.triangles.size()) + " triangles");
        }
        addElement(std::move(tin));
    }

    // `triangles { 1 2 3 ... }` as the manual has it, or with the indices in
    // a `vertices { }` block of their own as some writers produce.
    void readTriangles(std::vector<std::int64_t>& triangles, std::vector<std::int64_t>& neighbours)
    {
        if (!expectOpen("triangles")) {
            return;
        }
        const Scope scope(*this, "triangles");
        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            if (peek().kind == TokenKind::Word && detail::parseInteger(peek().raw)) {
                readIntegersUntilClose("triangles", triangles);
                continue;
            }
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            if (peek().kind != TokenKind::Open) {
                fail(key.line, "'" + std::string(key.raw) + "' in triangles is not a point number");
                return;
            }
            if (word == "vertices") {
                readIntegerBlock(word, triangles);
            } else if (word == "neighbours") {
                readIntegerBlock(word, neighbours);
            } else {
                skipUnknownBlock(word);
            }
        }
        expectClose("triangles");
    }

    // One-based indices in rows of three -> zero-based triangles. An index
    // outside the points is a failure: the triangle cannot be drawn, and one
    // bad index usually means every later one is wrong too.
    bool buildTriangles(std::string_view what, std::uint32_t line, std::size_t pointCount,
                        const std::vector<std::int64_t>& indices,
                        std::vector<std::array<std::uint32_t, 3>>& out)
    {
        if (indices.size() % 3 != 0) {
            fail(line, std::string(what) + " lists " + std::to_string(indices.size()) +
                           " point numbers, which is not a whole number of triangles");
            return false;
        }
        // The manual numbers points from 1. A file with a 0 in it was written
        // by something that counts from 0, and says so by containing one.
        const bool zeroBased =
            std::find(indices.begin(), indices.end(), std::int64_t{0}) != indices.end();
        if (zeroBased) {
            warn(std::string(what) + " at line " + std::to_string(line) +
                 " numbers its points from 0, not from 1 as the manual requires; read as such");
        }
        out.reserve(indices.size() / 3);
        for (std::size_t i = 0; i < indices.size(); i += 3) {
            std::array<std::uint32_t, 3> triangle{};
            for (std::size_t k = 0; k < 3; ++k) {
                const std::int64_t index = indices[i + k] - (zeroBased ? 0 : 1);
                if (index < 0 || static_cast<std::size_t>(index) >= pointCount) {
                    fail(line, std::string(what) + ": triangle " + std::to_string(i / 3 + 1) +
                                   " names point " + std::to_string(indices[i + k]) +
                                   " but there are " + std::to_string(pointCount) + " points");
                    return false;
                }
                triangle[k] = static_cast<std::uint32_t>(index);
            }
            out.push_back(triangle);
        }
        return true;
    }

    void readTinInput(Tin& tin)
    {
        if (!expectOpen("input")) {
            return;
        }
        const Scope scope(*this, "input");
        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            if (word == "models" && peek().kind == TokenKind::Open) {
                readTextBlock(word, tin.inputModels);
            } else {
                readUnknown(key, word, tin.input);
            }
        }
        expectClose("input");
    }

    void readSuperTin()
    {
        SuperTin superTin;
        superTin.colour = colour_;
        if (!expectOpen("super_tin")) {
            return;
        }
        const Scope scope(*this, "super_tin");
        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            const bool block = peek().kind == TokenKind::Open;
            if (key.isWord() && word == "name" && !block) {
                superTin.name = nextValue(word);
            } else if (key.isWord() && (word == "colour" || word == "color") && !block) {
                superTin.colour = nextColour(word);
            } else if (key.isWord() && word == "attributes" && block) {
                readAttributes(superTin.attributes);
            } else if (key.isWord() && word == "tins" && block) {
                readTextBlock(word, superTin.tins);
            } else {
                readUnknown(key, word, superTin.extras);
            }
        }
        expectClose("super_tin");
        if (!failed()) {
            // Manual 1.4.8: the name and the tins block are mandatory.
            if (superTin.name.empty()) {
                warn("super_tin has no name, which the manual makes mandatory");
            }
            if (superTin.tins.empty()) {
                warn("super_tin \"" + superTin.name +
                     "\" has no tins block, which the manual makes mandatory");
            }
            addElement(std::move(superTin));
        }
    }

    // ---- trimeshes -------------------------------------------------------------

    void readPrimitive3d()
    {
        Trimesh mesh;
        mesh.model = model_;
        mesh.colour = colour_;
        const std::uint32_t startLine = peek().line;
        if (!expectOpen("primitive_3d")) {
            return;
        }
        const Scope scope(*this, "primitive_3d");
        bool sawMesh = false;
        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            const bool block = peek().kind == TokenKind::Open;
            if (key.isWord() && word == "name" && !block) {
                mesh.name = nextValue(word);
            } else if (key.isWord() && word == "model" && !block) {
                mesh.model = nextValue(word); // registered by addElement
            } else if (key.isWord() && (word == "colour" || word == "color") && !block) {
                mesh.colour = nextColour(word);
            } else if (key.isWord() && word == "attributes" && block) {
                readAttributes(mesh.attributes);
            } else if (key.isWord() && word == "trimesh_3d" && block) {
                sawMesh = true;
                readTrimesh(mesh, startLine);
            } else {
                if (word == "transformation" && block) {
                    warn("trimesh \"" + mesh.name +
                         "\" carries a transformation block, which the manual does not define; "
                         "its vertices are read as written");
                }
                readUnknown(key, word, mesh.extras);
            }
        }
        expectClose("primitive_3d");
        if (failed()) {
            return;
        }
        if (!sawMesh) {
            warn("primitive_3d at line " + std::to_string(startLine) + " holds no trimesh_3d");
        }
        applyNull(mesh.vertices, archive_.nullValue);
        addElement(std::move(mesh));
    }

    void readTrimesh(Trimesh& mesh, std::uint32_t startLine)
    {
        if (!expectOpen("trimesh_3d")) {
            return;
        }
        const Scope scope(*this, "trimesh_3d");
        std::vector<std::int64_t> faces;
        std::vector<std::int64_t> edges;
        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            const bool block = peek().kind == TokenKind::Open;
            if (!key.isWord()) {
                continue;
            }
            if (word == "vertices" && block) {
                readVertexRows(word, 3, mesh.vertices);
            } else if (word == "faces" && block) {
                readIntegerBlock(word, faces);
            } else if (word == "edges" && block) {
                readIntegerBlock(word, edges);
            } else if (word == "info" && block) {
                readFieldBlock(word, mesh.info);
            } else if (word == "blend" && !block) {
                mesh.blend = nextNullableReal(word);
            } else if (word == "vertex_infos" && block) {
                readInfos(word, mesh.vertexInfos);
            } else if (word == "edge_infos" && block) {
                readInfos(word, mesh.edgeInfos);
            } else if (word == "face_infos" && block) {
                readInfos(word, mesh.faceInfos);
            } else if (word == "vertex_flags" && block) {
                readIndexBlock(word, mesh.vertexFlags);
            } else if (word == "edge_flags" && block) {
                readIndexBlock(word, mesh.edgeFlags);
            } else if (word == "face_flags" && block) {
                readIndexBlock(word, mesh.faceFlags);
            } else {
                readUnknown(key, word, mesh.extras);
            }
        }
        expectClose("trimesh_3d");
        if (failed()) {
            return;
        }
        // Manual 1.4.9: vertices and faces are MANDATORY.
        if (mesh.vertices.empty()) {
            warn("trimesh \"" + mesh.name + "\" has no vertices block, which the manual makes mandatory");
        }
        if (faces.empty()) {
            warn("trimesh \"" + mesh.name + "\" has no faces block, which the manual makes mandatory");
        }
        buildTriangles("trimesh_3d", startLine, mesh.vertices.size(), faces, mesh.faces);
        // A flag is a ONE-based index into its info table, 0 meaning no info:
        // the manual's example (1.4.9) has two infos and the flags 2 0 1 2 0,
        // which reads no other way, and 12d Model's own exports agree.
        const auto checkFlags = [&](const std::vector<std::uint32_t>& flags,
                                    const std::vector<TrimeshInfo>& infos, const char* what) {
            const bool outOfRange = std::any_of(flags.begin(), flags.end(), [&](std::uint32_t f) {
                return f > infos.size();
            });
            if (outOfRange) {
                warn("trimesh \"" + mesh.name + "\": " + what +
                     " names an info that does not exist");
            }
        };
        checkFlags(mesh.vertexFlags, mesh.vertexInfos, "vertex_flags");
        checkFlags(mesh.edgeFlags, mesh.edgeInfos, "edge_flags");
        checkFlags(mesh.faceFlags, mesh.faceInfos, "face_flags");
        if (edges.size() % 2 == 0) {
            for (std::size_t i = 0; i + 1 < edges.size(); i += 2) {
                // Edges are "for checking only" (manual 1.4.9): a bad one is
                // dropped, not fatal.
                const std::int64_t a = edges[i] - 1;
                const std::int64_t b = edges[i + 1] - 1;
                const auto count = static_cast<std::int64_t>(mesh.vertices.size());
                if (a >= 0 && b >= 0 && a < count && b < count) {
                    mesh.edges.push_back(
                        {static_cast<std::uint32_t>(a), static_cast<std::uint32_t>(b)});
                }
            }
        }
    }

    // flag key colour name, four to a row
    void readInfos(std::string_view what, std::vector<TrimeshInfo>& out)
    {
        const std::uint32_t line = peek().line;
        std::vector<std::string> tokens;
        if (!readTextBlock(what, tokens)) {
            return;
        }
        if (tokens.size() % 4 != 0) {
            warn("line " + std::to_string(line) + ": " + what +
                 " is not rows of 'flag key colour name'; ignored");
            return;
        }
        for (std::size_t i = 0; i < tokens.size(); i += 4) {
            TrimeshInfo info;
            info.flag = detail::parseInteger(tokens[i]).value_or(0);
            info.key = detail::parseInteger(tokens[i + 1]).value_or(0);
            info.colour = tokens[i + 2];
            info.name = tokens[i + 3];
            out.push_back(std::move(info));
        }
    }

    void readIndexBlock(std::string_view what, std::vector<std::uint32_t>& out)
    {
        std::vector<std::int64_t> values;
        if (!readIntegerBlock(what, values)) {
            return;
        }
        out.reserve(values.size());
        for (const std::int64_t value : values) {
            out.push_back(value < 0 ? 0u : static_cast<std::uint32_t>(value));
        }
    }

    // ---- top level ---------------------------------------------------------------

    // Registers a model and returns the ONE spelling it is known by. Manual
    // 1.1: leading and trailing spaces are ignored and case is stored but not
    // compared, so " Fred " and "FRED" are the model "Fred" if that came first
    // - and the importer, which keys layers on the string, must see one name.
    const std::string& noteModel(std::string_view name)
    {
        const std::string_view trimmed = detail::trimmed(name);
        // Looked up rather than scanned: every element asks, so the scan this
        // replaces was O(elements x models) - 20 million comparisons on an
        // archive of 100 000 elements over 400 models. The map hashes and
        // compares WITHOUT case, so it answers the manual's question directly
        // and needs no folded copy of the name to ask it.
        if (const auto known = modelIndex_.find(trimmed); known != modelIndex_.end()) {
            return archive_.modelNames[known->second];
        }
        archive_.modelNames.emplace_back(trimmed);
        modelIndex_.emplace(archive_.modelNames.back(), archive_.modelNames.size() - 1);
        return archive_.modelNames.back();
    }

    // Every element goes in through here, so that the model it lands in is
    // registered when it is USED - not before its own `model` field, which
    // the manual puts inside the string, has been read. Otherwise a file
    // whose strings all name their model would also gain the default "data".
    template <typename T> void addElement(T element)
    {
        if constexpr (requires { element.header.model; }) {
            element.header.model = noteModel(element.header.model);
        } else if constexpr (requires { element.model; }) {
            element.model = noteModel(element.model);
        }
        archive_.elements.emplace_back(std::move(element));
    }

    void readModelBlock()
    {
        ModelRecord record;
        if (!expectOpen("model")) {
            return;
        }
        const Scope scope(*this, "model");
        detail::CaseBuffer keys;
        while (!atBlockEnd()) {
            const Token key = next();
            const std::string_view word = keys.lower(key.raw);
            const bool block = peek().kind == TokenKind::Open;
            if (key.isWord() && word == "name" && !block) {
                record.name = nextValue(word);
            } else if (key.isWord() && word == "attributes" && block) {
                readAttributes(record.attributes);
            } else {
                readUnknown(key, word, record.extras);
            }
        }
        expectClose("model");
        if (failed()) {
            return;
        }
        if (!record.name.empty()) {
            model_ = noteModel(record.name);
            record.name = model_;
        }
        archive_.models.push_back(std::move(record));
    }

    void readString(std::uint32_t line)
    {
        const std::string type = lowered(nextValue("string"));
        if (failed()) {
            return;
        }
        if (peek().kind != TokenKind::Open) {
            fail(line, "expected '{' after 'string " + type + "'");
            return;
        }
        lexer_.collectComments = false;

        static const std::vector<std::pair<std::string_view, StringKind>> vertexKinds = {
            {"super", StringKind::Super},       {"2d", StringKind::TwoD},
            {"3d", StringKind::ThreeD},         {"4d", StringKind::FourD},
            {"pipe", StringKind::Pipe},         {"polyline", StringKind::Polyline},
            {"face", StringKind::Face},         {"interface", StringKind::Interface}};
        for (const auto& [name, kind] : vertexKinds) {
            if (type == name) {
                (void)next(); // the '{'
                readVertexString(kind);
                return;
            }
        }
        if (type == "arc" || type == "circle" || type == "feature" || type == "text" ||
            type == "plot_frame" || type == "drainage" || type == "super_alignment" ||
            type == "alignment" || type == "pipeline" || type == "las_cloud_data") {
            (void)next(); // the '{'
        }
        if (type == "arc") {
            readArcString();
        } else if (type == "circle" || type == "feature") {
            readCircleString(type == "feature");
        } else if (type == "text") {
            readTextString();
        } else if (type == "plot_frame") {
            readPlotFrame();
        } else if (type == "drainage") {
            readDrainageString();
        } else if (type == "super_alignment") {
            readSuperAlignment();
        } else if (type == "alignment") {
            readLegacyAlignment(AlignmentSource::Alignment);
        } else if (type == "pipeline") {
            readLegacyAlignment(AlignmentSource::Pipeline);
        } else if (type == "las_cloud_data") {
            readLasCloud();
        } else {
            skipUnknownBlock("string " + type);
        }
    }

    void readTopLevel()
    {
        while (!failed()) {
            const Token token = next();
            if (token.kind == TokenKind::End) {
                break;
            }
            if (token.kind == TokenKind::Close) {
                fail(token.line, "a '}' closes nothing");
                break;
            }
            if (token.kind == TokenKind::Open) {
                // Put back in spirit: count it as a block with no keyword.
                ++archive_.unrecognised["(no keyword)"];
                skipRestOfBlock(token.line);
                continue;
            }
            if (token.kind == TokenKind::Quoted) {
                warn("line " + std::to_string(token.line) + ": stray text \"" + token.text() +
                     "\" between elements ignored");
                continue;
            }

            const std::string word = lowered(token.raw);
            const bool block = peek().kind == TokenKind::Open;
            if (word == "model") {
                if (block) {
                    readModelBlock();
                } else {
                    model_ = noteModel(nextValue(word));
                }
            } else if ((word == "colour" || word == "color") && !block) {
                colour_ = nextColour(word);
            } else if (word == "style" && !block) {
                style_ = nextValue(word);
            } else if (word == "breakline" && !block) {
                breakline_ = readBreakline();
            } else if (word == "null" && !block) {
                // `null -999` sets the value; `null null`, which 12d Model
                // writes when it is using the keyword, changes nothing.
                const Token value = next();
                if (const auto number = parseReal(value.raw); number && value.isWord()) {
                    archive_.nullValue = *number;
                }
            } else if (word == "string") {
                readString(token.line);
            } else if (word == "tin" && block) {
                lexer_.collectComments = false;
                readTin(false);
            } else if (word == "full_tin" && block) {
                lexer_.collectComments = false;
                readTin(true);
            } else if (word == "super_tin" && block) {
                lexer_.collectComments = false;
                readSuperTin();
            } else if (word == "primitive_3d" && block) {
                lexer_.collectComments = false;
                readPrimitive3d();
            } else if (word == "project_attributes" && block) {
                readAttributes(archive_.projectAttributes);
            } else if (word == "text" && peek().kind == TokenKind::Quoted) {
                readShortText(token.line);
            } else if (block) {
                skipUnknownBlock(word);
            } else {
                warn("line " + std::to_string(token.line) + ": '" + std::string(token.raw) +
                     "' is not a 12da command; ignored");
            }

            if (archive_.elements.size() > options_.maxElements) {
                fail(token.line, "the file holds more elements than the limit of " +
                                     std::to_string(options_.maxElements));
            }
        }
    }

    // After a `{` has already been consumed.
    void skipRestOfBlock(std::uint32_t openLine)
    {
        const std::size_t target = lexer_.depth() - 1;
        while (!failed()) {
            const Token token = next();
            if (token.kind == TokenKind::End) {
                fail(openLine, "a '{' is never closed");
                return;
            }
            if (token.kind == TokenKind::Close && lexer_.depth() == target) {
                return;
            }
        }
    }

    // 12d Model's export settings, written as `// key value` comments.
    void readHeaderSettings()
    {
        for (const std::string_view comment : lexer_.comments) {
            const std::string_view body = detail::trimmed(comment);
            const std::size_t space = body.find_first_of(" \t");
            if (space == std::string_view::npos) {
                continue;
            }
            const std::string_view key = body.substr(0, space);
            const bool isSetting =
                key == "archive_version" || key == "decimal_places" || key == "null" ||
                key == "null_keyword" || key == "do_null" || key == "dereference" ||
                key == "project" || key == "model_attributes_mode" || key.starts_with("output_");
            if (!isSetting) {
                continue;
            }
            std::string_view value = detail::trimmed(body.substr(space));
            const bool isQuoted = value.size() >= 2 && value.front() == '"' && value.back() == '"';
            if (isQuoted) {
                value = value.substr(1, value.size() - 2);
            }
            archive_.headerSettings.push_back(Field{std::string(key), std::string(value), isQuoted});
        }
    }

    // Every keyword some reader above has a case for. Used to tell a colour's
    // second word from the next keyword, and an unknown flag from an unknown
    // key and its value.
    // `word` is the token AS WRITTEN: the set folds case itself, so asking it
    // costs no folded copy.
    [[nodiscard]] static bool isKnownKeyword(std::string_view word)
    {
        // A set, not a list: this is asked once per colour and once per
        // undocumented key, and the linear scan it replaces walked all 140
        // entries for every miss - 14 million string comparisons on a
        // 100 000-element archive.
        static const std::unordered_set<std::string_view, detail::CaseFoldedHash,
                                        detail::CaseFoldedEqual>
            keywords = {
            "name", "model", "colour", "color", "style", "chainage", "breakline", "attributes",
            "closed", "z", "data", "data_2d", "data_3d", "radius_data", "major_data",
            "geometry_data", "colour_data", "point_data", "diameter", "diameter_value",
            "diameter_data", "pipe_value", "pipe_data", "culvert_value", "culvert_data",
            "justify", "pipe_justify", "interval", "weight", "time_created", "time_updated",
            "vertex_tinable_value", "vertex_tinable_data", "segment_tinable_value",
            "segment_tinable_data", "vertex_visible_value", "vertex_visible_data",
            "segment_visible_value", "segment_visible_data", "vertex_text_value",
            "vertex_text_data", "segment_text_value", "segment_text_data",
            "vertex_annotate_value", "vertex_annotate_data", "segment_annotate_value",
            "segment_annotate_data", "symbol_value", "symbol_data", "vertex_attribute_data",
            "segment_attribute_data", "radius", "xcentre", "ycentre", "zcentre", "xstart",
            "ystart", "zstart", "xend", "yend", "zend", "x", "y", "text", "angle", "offset",
            "raise", "textstyle", "slant", "xfactor", "x_factor", "worldsize", "papersize",
            "screensize", "outfall", "flow_direction", "pit", "pit_v2", "pipe",
            "property_control", "house_connection", "spiral_type", "valid_horizontal",
            "valid_vertical", "horizontal_parts", "horizontal_data", "vertical_parts",
            "vertical_data", "hipdata", "vipdata", "draw_mode", "length", "points", "triangles",
            "neighbours", "nulling", "colours", "input", "tins", "trimesh_3d", "vertices", "faces",
            "edges", "info", "blend", "size", "rotation", "width", "height", "scale", "xorigin",
            "yorigin", "border", "format", "range", "categories", "file_name",
            // and the words that begin an element or a command, so that
            // `colour dark` at the end of a header does not eat the next one
            "string", "tin", "full_tin", "super_tin", "primitive_3d", "null", "null_value",
            "project_attributes"};
        return keywords.contains(word);
    }

    Lexer lexer_;
    ReadOptions options_;
    Archive archive_;
    // Model names -> their place in archive_.modelNames. The key is owned,
    // NOT a view into modelNames: that vector grows, and a short name moved
    // by the growth moves its bytes with it.
    std::unordered_map<std::string, std::size_t, detail::CaseFoldedHash, detail::CaseFoldedEqual>
        modelIndex_;
    std::optional<Error> error_;
    Token end_{};
    std::vector<std::string_view> path_;
    std::size_t suppressedWarnings_ = 0;
    std::size_t nullAttributes_ = 0;
    std::size_t badLasFields_ = 0;

    // The commands in force (manual 1.4): defaults as the manual gives them.
    std::string model_ = "data";
    std::string colour_ = "red";
    std::string style_ = "1";
    Breakline breakline_ = Breakline::Point;
};

katana::core::Result<Archive> Reader::run()
{
    readTopLevel();
    if (lexer_.unterminatedQuote()) {
        // Whatever else was reported - usually a missing '}' - is a symptom:
        // the text swallowed the rest of the file, braces included.
        error_.reset();
        fail(lexer_.unterminatedQuoteLine(), "a text in double quotes is never closed");
    }
    if (failed()) {
        return *error_;
    }
    readHeaderSettings();
    if (nullAttributes_ != 0) {
        warn(std::to_string(nullAttributes_) +
             " attributes had a null or unreadable value and were not read");
    }
    if (suppressedWarnings_ != 0) {
        archive_.warnings.push_back(std::to_string(suppressedWarnings_) +
                                    " further warnings were not listed");
    }
    return std::move(archive_);
}

} // namespace

namespace {

// The reader proper. `text` must already be known to be UTF-8.
katana::core::Result<Archive> readChecked(std::string_view text, const ReadOptions& options)
{
    Reader reader(text, options);
    return reader.run();
}

} // namespace

katana::core::Result<Archive> readArchive(std::string_view text, const ReadOptions& options)
{
    // Every name and text in the archive ends up in the entity model, which
    // refuses bytes that are not UTF-8 - and refuses them for the whole import
    // at once. Better to say so here, about the file.
    if (!katana::entity::isValidUtf8(text)) {
        return makeError(ErrorCode::ParseFailure,
                         "the text is not valid UTF-8; decode the file with decodeText first");
    }
    return readChecked(text, options);
}

katana::core::Result<Archive> readArchiveBytes(std::string_view bytes, const ReadOptions& options)
{
    auto decoded = decodeText(bytes);
    if (!decoded) {
        return decoded.error();
    }
    // decodeText has already established what readArchive would check, so
    // going through it would walk all 58 MB a second time for nothing.
    auto archive = decoded->validatedUtf8 ? readChecked(decoded->text, options)
                                          : readArchive(decoded->text, options);
    if (archive && decoded->guessed) {
        archive->warnings.insert(archive->warnings.begin(),
                                 std::string("the file has no byte order mark; read as ") +
                                     toString(decoded->encoding));
    }
    return archive;
}

} // namespace katana::archive12d
