#include "katana/entity/customisation.hpp"

#include <algorithm>
#include <array>
#include <bit>
#include <cmath>
#include <cstdint>
#include <limits>
#include <set>
#include <utility>

#include <nlohmann/json.hpp>

#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"
#include "katana/entity/layer_path.hpp"

namespace katana::entity {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::core::Status;
using katana::geometry::Point2;
using Json = nlohmann::json;

// docs/customisation.md is the specification this file implements: the
// members, the layout the writer follows and what may not change without a
// new version. The reader and the writer share every table of words below, so
// the two cannot come to disagree about a spelling.

namespace {

// ---- the format's words -----------------------------------------------------------
//
// Every word here is part of version 1: a word changed, and a word ADDED, is a
// new version (an older Katana would refuse the file for it).

template <typename Enum> struct Word {
    Enum value;
    std::string_view word;
};

constexpr std::array<Word<StyleUnits>, 3> kUnitsWords{{
    {StyleUnits::World, "world"},
    {StyleUnits::Paper, "paper"},
    {StyleUnits::TwoPoint, "twoPoint"},
}};

constexpr std::array<Word<SurveyBreakline>, 2> kDrawWords{{
    {SurveyBreakline::Line, "line"},
    {SurveyBreakline::Point, "point"},
}};

// What a rule SETS, in the words of entity's one table of them
// (kSurveySectionWords, survey_map.hpp). entity::toString(SurveySection) reads
// the same table for the survey code tools' text, so a file holds the word
// the window shows for the same thing.
constexpr std::array<Word<SurveySection>, kSurveySectionWords.size()> kSectionWords = [] {
    std::array<Word<SurveySection>, kSurveySectionWords.size()> table{};
    for (std::size_t i = 0; i < table.size(); ++i) {
        table[i] = {kSurveySectionWords[i].section, kSurveySectionWords[i].word};
    }
    return table;
}();

// The model keeps an attribute's type as text; the format holds only these.
constexpr std::array<std::string_view, 2> kAttributeTypes{"text", "integer"};

// A stroke is an array: its word, then `values` more. Move and draw first,
// which is what a library is made of.
struct StrokeWord {
    StrokeOp op;
    std::string_view word;
    std::size_t values;
    std::string_view takes; // the values in words, for a refusal
};

constexpr std::array<StrokeWord, 7> kStrokeWords{{
    {StrokeOp::Move, "move", 2, "x and y"},
    {StrokeOp::Draw, "draw", 2, "x and y"},
    {StrokeOp::Arc, "arc", 3, "a radius, a start angle and an end angle"},
    {StrokeOp::Circle, "circle", 1, "a radius"},
    {StrokeOp::Dot, "dot", 1, "a radius"},
    {StrokeOp::Pen, "pen", 1, "a colour name"},
    {StrokeOp::Text, "text", 1, "one object"},
}};

constexpr std::string_view kTopLevel = "top level";
constexpr std::string_view kNotACustomisation = "not a Katana customisation file";
constexpr std::string_view kNotADefinition = "not a definition in the Katana customisation format";

template <typename Enum, std::size_t N>
[[nodiscard]] std::string_view wordOf(const std::array<Word<Enum>, N>& words, Enum value)
{
    for (const Word<Enum>& entry : words) {
        if (entry.value == value) {
            return entry.word;
        }
    }
    return {}; // a value outside the enumeration: the writer refuses it
}

[[nodiscard]] const StrokeWord* strokeWordOf(StrokeOp op)
{
    const auto found = std::find_if(kStrokeWords.begin(), kStrokeWords.end(),
                                    [op](const StrokeWord& entry) { return entry.op == op; });
    return found == kStrokeWords.end() ? nullptr : &*found;
}

[[nodiscard]] const StrokeWord* strokeWordOf(std::string_view word)
{
    const auto found = std::find_if(kStrokeWords.begin(), kStrokeWords.end(),
                                    [word](const StrokeWord& entry) { return entry.word == word; });
    return found == kStrokeWords.end() ? nullptr : &*found;
}

template <typename Enum, std::size_t N>
[[nodiscard]] std::string listed(const std::array<Word<Enum>, N>& words)
{
    std::string text;
    for (const Word<Enum>& entry : words) {
        text += text.empty() ? "" : ", ";
        text += entry.word;
    }
    return text;
}

[[nodiscard]] std::string listed(const std::vector<std::string_view>& words)
{
    std::string text;
    for (const std::string_view word : words) {
        text += text.empty() ? "" : ", ";
        text += word;
    }
    return text;
}

// ---- refusals ---------------------------------------------------------------------

// Thrown inside the reader and the writer and caught at their edges, like the
// JSON library's own exceptions, so the helpers below stay plain value
// functions. Nothing thrown here leaves this file.
struct Refusal {
    ErrorCode code;
    std::string message;
    std::string context;
};

// "<where>: <what>" - every refusal names the entry it is about first.
[[noreturn]] void refuse(ErrorCode code, std::string_view where, std::string_view what,
                         std::string context = {})
{
    std::string message(where);
    message += ": ";
    message += what;
    throw Refusal{code, std::move(message), std::move(context)};
}

// A name as a message shows it: in double quotes, with what would break the
// line or the quoting escaped the way JSON escapes it. Bytes that are not
// UTF-8 are shown as U+FFFD rather than thrown on, because this is also how a
// refusal names the very text it refuses.
//
// Cut short past kLongestQuoted bytes: a refusal names the entry it is about,
// and an entry's name may be as long as the file is (a name over the bound is
// refused, but is named in that refusal too, and in every other about it).
[[nodiscard]] std::string inQuotes(std::string_view text)
{
    constexpr std::size_t kLongestQuoted = 240;
    const bool cut = text.size() > kLongestQuoted;
    std::string quoted = Json(std::string(cut ? text.substr(0, kLongestQuoted) : text))
                             .dump(-1, ' ', false, Json::error_handler_t::replace);
    if (cut) {
        quoted.insert(quoted.size() - 1, "...");
    }
    return quoted;
}

// ---- what the tree holds that JSON cannot say -------------------------------------
//
// Two things stand in the tree the reader builds for something the text held
// that a tree cannot hold as itself. Each is a BINARY value, because JSON text
// has none: nothing a file can say is mistaken for either.
//
//   * Where a definition's strokes were: a subtype, which says which entry of
//     Parsed::strokes they are (they are read as the text is parsed and never
//     become a tree).
//   * A number the reader cannot hold: no subtype, and for its bytes the
//     number as the text wrote it. Whatever reads a number refuses it by the
//     member it is in; anything else refuses it as it would any value of the
//     wrong kind, and shows it as written.

[[nodiscard]] Json strokesPlace(std::size_t which)
{
    return Json::binary(Json::binary_t::container_type{}, which);
}

[[nodiscard]] bool isStrokesPlace(const Json& value)
{
    return value.is_binary() && value.get_binary().has_subtype();
}

[[nodiscard]] Json lostNumber(std::string_view written)
{
    return Json::binary(Json::binary_t::container_type(written.begin(), written.end()));
}

[[nodiscard]] bool isLostNumber(const Json& value)
{
    return value.is_binary() && !value.get_binary().has_subtype();
}

constexpr std::string_view kTooLarge = "has a number too large to hold";
constexpr std::string_view kTooSmall =
    "has a number too small to hold: it is not zero, and would be read as 0";

// The format's bounds (customisation.hpp), in the words a refusal says them in.
static_assert(kCustomisationMostMagnitude == 1.0e9 && kCustomisationMostSweepDegrees == 720.0 &&
                  kCustomisationMostStrokes == 100000 && kCustomisationMostTextBytes == 1000 &&
                  kCustomisationMostRules == 1000000,
              "the refusals below say these numbers");
constexpr std::string_view kBeyondMagnitude =
    "larger than 1000000000 in size, the most any number of a customisation holds";
constexpr std::string_view kTooLong =
    "longer than 1000 bytes, the most a name or text of a customisation holds";
constexpr std::string_view kSweepsTooFar =
    "sweeps more than 720 degrees (two turns), the most an arc holds";
constexpr std::string_view kTooManyStrokes =
    "a definition holds at most 100000 strokes, and this is one more";
constexpr std::string_view kTooManyRules =
    "\"codes\" holds more than 1000000 rules, the most a customisation holds";

// A NaN is not within it, and is no number the file can hold either.
[[nodiscard]] bool withinMagnitude(double value)
{
    return std::abs(value) <= kCustomisationMostMagnitude;
}

[[nodiscard]] bool withinSweep(double startAngle, double endAngle)
{
    return std::abs(endAngle - startAngle) <= kCustomisationMostSweepDegrees;
}

// Whether a number's text, which the JSON library read as zero, says some
// other number: a digit that is not 0 before any exponent. ("0e-400" and
// "-0.000" are zero; "1e-400" is below the smallest double and is not.)
[[nodiscard]] bool saysMoreThanZero(std::string_view written)
{
    for (const char c : written) {
        if (c == 'e' || c == 'E') {
            break;
        }
        if (c >= '1' && c <= '9') {
            return true;
        }
    }
    return false;
}

// The most a refusal shows of a value.
constexpr std::size_t kLongestShown = 60;

// Appends `value` as a file would write it on one line, and stops once enough
// is written to be cut off. NOT the JSON library's dump(): that writes the
// whole value before any of it is cut, going down a level of the program's
// stack for every level of the value - and a value may nest as deep as its
// text is long. 100,000 "[" are a file of 200 KB, and were a stack overflow.
// This goes down a level only after writing a character, so never deeper than
// kLongestShown.
void appendShown(std::string& text, const Json& value)
{
    if (text.size() > kLongestShown) {
        return;
    }
    if (value.is_array()) {
        text += '[';
        for (auto item = value.begin(); item != value.end(); ++item) {
            if (text.size() > kLongestShown) {
                return;
            }
            text += item == value.begin() ? "" : ",";
            appendShown(text, *item);
        }
        text += ']';
    } else if (value.is_object()) {
        text += '{';
        for (auto item = value.begin(); item != value.end(); ++item) {
            if (text.size() > kLongestShown) {
                return;
            }
            text += item == value.begin() ? "" : ",";
            text += Json(item.key()).dump(-1, ' ', false, Json::error_handler_t::replace);
            text += ':';
            appendShown(text, item.value());
        }
        text += '}';
    } else if (isLostNumber(value)) {
        const Json::binary_t& written = value.get_binary();
        text.append(written.begin(), written.end());
    } else if (isStrokesPlace(value)) {
        text += "[...]"; // a list of strokes, which is not kept as it was written
    } else {
        // Text, a number, true, false or null: nothing below it to go down to.
        text += value.dump(-1, ' ', false, Json::error_handler_t::replace);
    }
}

// What a refusal shows, cut at kLongestShown bytes.
[[nodiscard]] std::string cutShort(std::string text)
{
    if (text.size() > kLongestShown) {
        // Never in the middle of a character: step back over continuation bytes.
        std::size_t cut = kLongestShown;
        while (cut > 0 && (static_cast<unsigned char>(text[cut]) & 0xC0) == 0x80) {
            --cut;
        }
        text.resize(cut);
        text += "...";
    }
    return text;
}

// A value as the file wrote it, cut short: what a refusal shows of a value of
// the wrong type.
[[nodiscard]] std::string shown(const Json& value)
{
    std::string text;
    appendShown(text, value);
    return cutShort(std::move(text));
}

[[nodiscard]] std::string counted(std::size_t count, std::string_view one, std::string_view many)
{
    std::string text = std::to_string(count);
    text += ' ';
    text += count == 1 ? one : many;
    return text;
}

// "strokes[9]", or "<entry> strokes[9]": a place in a list, counted from 0 as a
// JSON path counts. Built by appending: the obvious chain of operator+ on
// temporaries sets off a false -Wrestrict in GCC's basic_string at -O3.
[[nodiscard]] std::string placed(std::string_view entry, std::string_view list, std::size_t index)
{
    std::string label(entry);
    if (!label.empty()) {
        label += ' ';
    }
    label += list;
    label += '[';
    label += std::to_string(index);
    label += ']';
    return label;
}

// "codes[57] "WM*"": the array, the place in it and the entry's own name. All
// three, because a rule's key alone is ambiguous - several rules share one -
// and a place alone says nothing to the person who wrote the file.
[[nodiscard]] std::string labelled(std::string_view array, std::size_t index, std::string_view name)
{
    std::string label = placed({}, array, index);
    label += ' ';
    label += inQuotes(name);
    return label;
}

// The same from the file, where an entry may not be an object or may have no
// name to show.
[[nodiscard]] std::string entryLabel(std::string_view array, std::size_t index, const Json& entry,
                                     std::string_view nameMember)
{
    if (entry.is_object()) {
        if (const auto name = entry.find(nameMember); name != entry.end() && name->is_string()) {
            return labelled(array, index, name->get_ref<const std::string&>());
        }
    }
    return placed({}, array, index);
}

// The member that names an entry of a top-level array, or nothing.
[[nodiscard]] std::string_view nameMemberOf(std::string_view array)
{
    if (array == "codes") {
        return "key";
    }
    if (array == "linestyles" || array == "symbols" || array == "sources") {
        return "name";
    }
    return {};
}

[[nodiscard]] bool isDigest(std::string_view text)
{
    return text.size() == 16 && std::all_of(text.begin(), text.end(), [](char c) {
               return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f');
           });
}

// What both the reader and the writer require of a customisation outside its
// definitions and rules. One function, so that the writer cannot write a file
// the reader would then refuse - a kept customisation nobody can open again.
void checkHeader(const Customisation& customisation)
{
    if (auto status = validateCustomisationName(customisation.name); !status) {
        refuse(ErrorCode::InvalidArgument, kTopLevel, "\"name\": " + status.error().message,
               status.error().context);
    }
    for (std::size_t i = 0; i < customisation.sources.size(); ++i) {
        const std::string& name = customisation.sources[i].name;
        if (auto status = validateCustomisationName(name); !status) {
            refuse(ErrorCode::InvalidArgument, labelled("sources", i, name),
                   "\"name\": " + status.error().message, status.error().context);
        }
    }
    if (customisation.basedOn) {
        if (auto status = validateCustomisationName(customisation.basedOn->name); !status) {
            refuse(ErrorCode::InvalidArgument, "basedOn", "\"name\": " + status.error().message,
                   status.error().context);
        }
        if (!isDigest(customisation.basedOn->digest)) {
            refuse(ErrorCode::InvalidArgument, "basedOn",
                   "\"digest\" must be 16 hexadecimal digits in lower case",
                   inQuotes(customisation.basedOn->digest));
        }
    }
    if (customisation.linework) {
        if (auto status = validate(*customisation.linework); !status) {
            refuse(ErrorCode::InvalidArgument, "linework", status.error().message,
                   status.error().context);
        }
    }
}

// A definition's source is a customisation's name, or empty for one made in a
// session; it reaches a project's record like the customisation's own name.
void checkSource(const LineStyle& definition, std::string_view where)
{
    if (definition.source.empty()) {
        return;
    }
    if (auto status = validateCustomisationName(definition.source); !status) {
        refuse(ErrorCode::InvalidArgument, where, "\"from\": " + status.error().message,
               status.error().context);
    }
}

// Doubles are compared bit for bit when deciding whether a member is at its
// default, so -0.0 is not "the default 0" and is written, and reads back as
// -0.0. (operator== would call the two equal and the sign would be lost.)
[[nodiscard]] bool sameBits(double a, double b)
{
    return std::bit_cast<std::uint64_t>(a) == std::bit_cast<std::uint64_t>(b);
}

[[nodiscard]] bool samePoint(const Point2& a, const Point2& b)
{
    return sameBits(a.x, b.x) && sameBits(a.y, b.y);
}

// ---- writing ----------------------------------------------------------------------
//
// A hand-written emitter rather than the JSON library's dump(): that gives
// either one line for the whole file or one line for every number, and a
// library of 35,000 strokes is read, compared and merged by people as text.
// One stroke a line is the unit a diff shows and a person edits.

constexpr std::size_t kNoStroke = static_cast<std::size_t>(-1);

class Out {
  public:
    // Where a refusal says the trouble is: the entry being written, the member
    // of it and, inside a definition, the stroke.
    std::string where{kTopLevel};
    std::string_view member{};
    std::size_t stroke = kNoStroke;

    void reserve(std::size_t bytes) { text_.reserve(bytes); }
    void raw(std::string_view piece) { text_ += piece; }

    // Text goes through the JSON library, which is the one place that knows
    // what a JSON string must escape; it would throw on bytes that are not
    // UTF-8, so those are refused here, where the entry can be named.
    void text(std::string_view value)
    {
        if (!isValidUtf8(value)) {
            refuse(ErrorCode::InvalidArgument, place(),
                   subject() + " is not valid UTF-8, and a customisation file is UTF-8 text");
        }
        if (value.size() > kCustomisationMostTextBytes) {
            refuse(ErrorCode::InvalidArgument, place(),
                   subject() + " is " + std::string(kTooLong));
        }
        text_ += Json(std::string(value)).dump();
    }

    void real(double value)
    {
        if (!std::isfinite(value)) {
            refuse(ErrorCode::InvalidArgument, place(),
                   subject() + " is not a finite number (an infinity or NaN), which the "
                               "format cannot hold");
        }
        if (!withinMagnitude(value)) {
            refuse(ErrorCode::InvalidArgument, place(),
                   subject() + " is " + std::string(kBeyondMagnitude));
        }
        // The shortest text that reads back as the same double is "-0" for
        // negative zero - which a JSON reader takes for the INTEGER 0, losing
        // the sign. "-0.0" is a real and keeps it.
        if (value == 0.0 && std::signbit(value)) {
            text_ += "-0.0";
            return;
        }
        text_ += katana::core::formatExactReal(value);
    }

    void whole(std::int64_t value) { text_ += std::to_string(value); }
    void flag(bool value) { text_ += value ? "true" : "false"; }

    // A word of the format: written in quotes as it stands, since every one is
    // plain ASCII letters. An empty word is a value outside its enumeration.
    void word(std::string_view value)
    {
        if (value.empty()) {
            refuse(ErrorCode::InvalidArgument, place(),
                   subject() + " holds a value outside its enumeration");
        }
        text_ += '"';
        text_ += value;
        text_ += '"';
    }

    [[nodiscard]] std::string place() const
    {
        return stroke == kNoStroke ? where : placed(where, "strokes", stroke);
    }

    [[nodiscard]] std::string take() { return std::move(text_); }

  private:
    [[nodiscard]] std::string subject() const
    {
        return member.empty() ? std::string("a value") : inQuotes(member);
    }

    std::string text_{};
};

// One object written on one line: {"a": 1, "b": "x"}. A member at its default
// is left out by the helpers that take a fallback; key() is for the members
// that are always written and for values the caller writes itself.
class Object {
  public:
    explicit Object(Out& out) : out_(out) { out_.raw("{"); }

    void key(std::string_view name)
    {
        out_.raw(first_ ? "\"" : ", \"");
        first_ = false;
        out_.raw(name);
        out_.raw("\": ");
        out_.member = name;
    }

    void text(std::string_view name, std::string_view value, std::string_view fallback = {})
    {
        if (value != fallback) {
            key(name);
            out_.text(value);
        }
    }

    void real(std::string_view name, double value, double fallback)
    {
        if (!sameBits(value, fallback)) {
            key(name);
            out_.real(value);
        }
    }

    void whole(std::string_view name, int value, int fallback)
    {
        if (value != fallback) {
            key(name);
            out_.whole(value);
        }
    }

    void flag(std::string_view name, bool value, bool fallback)
    {
        if (value != fallback) {
            key(name);
            out_.flag(value);
        }
    }

    void close() { out_.raw("}"); }

  private:
    Out& out_;
    bool first_ = true;
};

// Names a part of an entry while it is being written (`codes[3] "AC*" symbol`)
// and puts the entry's own name back afterwards.
class Within {
  public:
    Within(Out& out, std::string_view part) : out_(out), size_(out.where.size())
    {
        out_.where += ' ';
        out_.where += part;
    }
    ~Within() { out_.where.resize(size_); }
    Within(const Within&) = delete;
    Within& operator=(const Within&) = delete;

  private:
    Out& out_;
    std::size_t size_;
};

void writePoint(Out& out, const Point2& point)
{
    out.raw("[");
    out.real(point.x);
    out.raw(", ");
    out.real(point.y);
    out.raw("]");
}

void writeLines(Out& out, const std::vector<std::string>& lines)
{
    out.raw("[");
    for (std::size_t i = 0; i < lines.size(); ++i) {
        out.raw(i == 0 ? "" : ", ");
        out.text(lines[i]);
    }
    out.raw("]");
}

// What the writer refuses of a definition's strokes, so that "lossless" is
// literally true: a stroke carrying a member its kind does not use (the file
// has nowhere to put it), and texts that are not exactly the text strokes in
// order (a text no stroke places, or one two strokes share, has no form
// either). entity::validate allows both; they are not errors in a model, only
// things this file cannot say.
void checkStrokes(const LineStyle& definition, std::string_view where)
{
    static const Stroke d;
    if (definition.strokes.size() > kCustomisationMostStrokes) {
        refuse(ErrorCode::InvalidArgument, where,
               "the definition holds " + counted(definition.strokes.size(), "stroke", "strokes") +
                   ", and a definition holds at most 100000");
    }
    std::size_t texts = 0;
    for (std::size_t i = 0; i < definition.strokes.size(); ++i) {
        const Stroke& stroke = definition.strokes[i];
        const auto here = [&] { return placed(where, "strokes", i); };
        const StrokeWord* kind = strokeWordOf(stroke.op);
        if (kind == nullptr) {
            refuse(ErrorCode::InvalidArgument, here(), "the stroke is of no kind the format knows");
        }
        const bool usesPoint = stroke.op == StrokeOp::Move || stroke.op == StrokeOp::Draw;
        const bool usesRadius = stroke.op == StrokeOp::Arc || stroke.op == StrokeOp::Circle ||
                                stroke.op == StrokeOp::Dot;
        const bool usesAngles = stroke.op == StrokeOp::Arc;
        std::string_view stray;
        if (!usesPoint && !samePoint(stroke.point, d.point)) {
            stray = "a point";
        } else if (!usesRadius && !sameBits(stroke.radius, d.radius)) {
            stray = "a radius";
        } else if (!usesAngles && (!sameBits(stroke.startAngle, d.startAngle) ||
                                   !sameBits(stroke.endAngle, d.endAngle))) {
            stray = "an angle";
        } else if (stroke.op != StrokeOp::Text && stroke.text != d.text) {
            stray = "a text";
        } else if (stroke.op != StrokeOp::Pen && !stroke.pen.empty()) {
            stray = "a pen";
        }
        if (!stray.empty()) {
            refuse(ErrorCode::InvalidArgument, here(),
                   "this " + std::string(kind->word) + " stroke carries " + std::string(stray) +
                       ", which it does not use and the format cannot hold");
        }
        if (stroke.op == StrokeOp::Arc && !withinSweep(stroke.startAngle, stroke.endAngle)) {
            refuse(ErrorCode::InvalidArgument, here(), "this arc " + std::string(kSweepsTooFar));
        }
        if (stroke.op == StrokeOp::Text) {
            if (stroke.text != texts) {
                refuse(ErrorCode::InvalidArgument, here(),
                       "the texts of this definition are not its text strokes in order: this "
                       "is text stroke " +
                           std::to_string(texts) + " and names text " +
                           (stroke.text == Stroke::kNoText ? std::string("none")
                                                           : std::to_string(stroke.text)));
            }
            ++texts;
        }
    }
    if (texts != definition.texts.size()) {
        refuse(ErrorCode::InvalidArgument, where,
               "the definition holds " + counted(definition.texts.size(), "text", "texts") +
                   " and " + counted(texts, "text stroke", "text strokes") +
                   ": a text no stroke places cannot be written");
    }
}

void writeStroke(Out& out, const LineStyle& definition, const Stroke& stroke)
{
    static const StrokeText d;
    const StrokeWord* kind = strokeWordOf(stroke.op); // checkStrokes found it
    out.member = kind->word;
    out.raw("[\"");
    out.raw(kind->word);
    out.raw("\", ");
    switch (stroke.op) {
    case StrokeOp::Move:
    case StrokeOp::Draw:
        out.real(stroke.point.x);
        out.raw(", ");
        out.real(stroke.point.y);
        break;
    case StrokeOp::Arc:
        out.real(stroke.radius);
        out.raw(", ");
        out.real(stroke.startAngle);
        out.raw(", ");
        out.real(stroke.endAngle);
        break;
    case StrokeOp::Circle:
    case StrokeOp::Dot:
        out.real(stroke.radius);
        break;
    case StrokeOp::Pen:
        out.text(stroke.pen);
        break;
    case StrokeOp::Text: {
        const StrokeText& text = definition.texts[stroke.text]; // in step: checkStrokes
        Object object(out);
        object.text("text", text.text, d.text);
        object.real("angle", text.angle, d.angle);
        object.real("height", text.height, d.height);
        object.text("justify", text.justify, d.justify);
        object.text("font", text.font, d.font);
        object.real("widthFactor", text.widthFactor, d.widthFactor);
        if (!sameBits(text.unnamed[0], d.unnamed[0]) || !sameBits(text.unnamed[1], d.unnamed[1]) ||
            !sameBits(text.unnamed[2], d.unnamed[2])) {
            object.key("extra");
            out.raw("[");
            out.real(text.unnamed[0]);
            out.raw(", ");
            out.real(text.unnamed[1]);
            out.raw(", ");
            out.real(text.unnamed[2]);
            out.raw("]");
        }
        object.close();
        break;
    }
    }
    out.raw("]");
}

// One definition: its members on one line and then its strokes one a line,
// each indented two further than `indent`. `out.where` names it already.
void writeDefinition(Out& out, const LineStyle& definition, std::string_view customisation,
                     std::string_view indent)
{
    static const LineStyle d;
    // A library holds only what validate accepted; a definition handed over
    // on its own (definitionToJson) may not have been asked.
    if (auto status = validate(definition); !status) {
        refuse(ErrorCode::InvalidArgument, out.where, status.error().message);
    }
    checkSource(definition, out.where);
    checkStrokes(definition, out.where);

    Object object(out);
    object.key("name");
    out.text(definition.name);
    object.text("group", definition.group, d.group);
    if (definition.units != d.units) {
        object.key("units");
        out.word(wordOf(kUnitsWords, definition.units));
    }
    object.flag("atVertices", definition.atVertices, d.atVertices);
    object.real("length", definition.length, d.length);
    object.real("factor", definition.factor, d.factor);
    if (!samePoint(definition.origin, d.origin)) {
        object.key("origin");
        writePoint(out, definition.origin);
    }
    if (!samePoint(definition.anchor1, d.anchor1) || !samePoint(definition.anchor2, d.anchor2)) {
        object.key("anchors");
        out.raw("[");
        writePoint(out, definition.anchor1);
        out.raw(", ");
        writePoint(out, definition.anchor2);
        out.raw("]");
    }
    object.whole("stretchMode", definition.stretchMode, d.stretchMode);
    object.whole("cycleMode", definition.cycleMode, d.cycleMode);
    // Written only when it differs from the name it sits under; an absent
    // "from" reads as that name. So a source of "" under a named customisation
    // IS written, as "", and stays "made in a session".
    if (definition.source != customisation) {
        object.key("from");
        out.text(definition.source);
    }
    if (!definition.strokes.empty()) {
        object.key("strokes");
        out.raw("[");
        for (std::size_t i = 0; i < definition.strokes.size(); ++i) {
            out.raw(i == 0 ? "\n" : ",\n");
            out.raw(indent);
            out.raw("  ");
            out.stroke = i;
            writeStroke(out, definition, definition.strokes[i]);
        }
        out.stroke = kNoStroke;
        out.raw("\n");
        out.raw(indent);
        out.raw("]");
    }
    object.close();
}

void writeAttributes(Out& out, std::string_view name,
                     const std::vector<SurveyAttribute>& attributes)
{
    out.raw("[");
    for (std::size_t i = 0; i < attributes.size(); ++i) {
        const SurveyAttribute& attribute = attributes[i];
        const Within within(out, placed({}, name, i));
        out.raw(i == 0 ? "" : ", ");
        if (std::find(kAttributeTypes.begin(), kAttributeTypes.end(), attribute.type) ==
            kAttributeTypes.end()) {
            refuse(ErrorCode::InvalidArgument, out.where,
                   "\"type\" is " + inQuotes(attribute.type) +
                       ", and an attribute is \"text\" or \"integer\"");
        }
        Object object(out);
        object.key("type");
        out.word(attribute.type);
        object.key("name");
        out.text(attribute.name);
        object.text("value", attribute.value);
        object.close();
    }
    out.raw("]");
}

void writePipe(Out& out, std::string_view name, const SurveyPipe& pipe)
{
    static const SurveyPipe d;
    const Within within(out, name);
    Object object(out);
    object.text("justify", pipe.justify, d.justify);
    object.text("shape", pipe.shape, d.shape);
    object.text("size1", pipe.size1, d.size1);
    object.text("size2", pipe.size2, d.size2);
    object.flag("active", pipe.active, d.active);
    object.close();
}

// One rule on one line. An optional that is PRESENT is written even when
// everything in it is at its default ("text": {}): present and absent are two
// states of the model - SurveyMap::lookup takes a present sub-object whole, so
// an empty one still stops a less specific rule's from showing through.
void writeRule(Out& out, const SurveyRule& rule)
{
    static const SurveyRule d;
    Object object(out);
    object.key("key");
    out.text(rule.key);
    object.key("sets");
    out.word(wordOf(kSectionWords, rule.section));
    object.text("layer", rule.model, d.model);
    object.text("colour", rule.colour, d.colour);
    if (rule.breakline) {
        object.key("draw");
        out.word(wordOf(kDrawWords, *rule.breakline));
    }
    object.text("linestyle", rule.linestyle, d.linestyle);
    object.text("weight", rule.weight, d.weight);
    object.text("group", rule.group, d.group);
    object.text("comment", rule.comment, d.comment);
    if (rule.tinable) {
        object.key("surface");
        out.flag(*rule.tinable);
    }
    if (rule.hide) {
        object.key("hide");
        out.flag(*rule.hide);
    }
    if (rule.symbol) {
        static const SurveySymbol s;
        object.key("symbol");
        const Within within(out, "symbol");
        Object part(out);
        part.text("name", rule.symbol->style, s.style);
        part.text("colour", rule.symbol->colour, s.colour);
        part.real("size", rule.symbol->size, s.size);
        part.real("rotation", rule.symbol->rotation, s.rotation);
        part.real("offset", rule.symbol->offset, s.offset);
        part.real("raise", rule.symbol->raise, s.raise);
        part.close();
    }
    if (rule.textStyle) {
        static const SurveyTextStyle t;
        const SurveyTextStyle& style = *rule.textStyle;
        object.key("text");
        const Within within(out, "text");
        Object part(out);
        part.text("style", style.textstyle, t.textstyle);
        part.text("colour", style.colour, t.colour);
        part.text("units", style.type, t.type);
        part.real("size", style.size, t.size);
        part.text("justifyX", style.justifyX, t.justifyX);
        part.text("justifyY", style.justifyY, t.justifyY);
        part.real("offset", style.offset, t.offset);
        part.real("raise", style.raise, t.raise);
        part.real("angle", style.angle, t.angle);
        part.real("slant", style.slant, t.slant);
        part.real("widthFactor", style.widthFactor, t.widthFactor);
        part.flag("underline", style.underline, t.underline);
        part.flag("strikeout", style.strikeout, t.strikeout);
        part.flag("italic", style.italic, t.italic);
        part.text("weight", style.weight, t.weight);
        part.close();
    }
    if (rule.pipe) {
        object.key("pipe");
        writePipe(out, "pipe", *rule.pipe);
    }
    if (rule.vertexPipe) {
        object.key("vertexPipe");
        writePipe(out, "vertexPipe", *rule.vertexPipe);
    }
    if (rule.segmentPipe) {
        object.key("segmentPipe");
        writePipe(out, "segmentPipe", *rule.segmentPipe);
    }
    if (!rule.attributes.empty()) {
        object.key("attributes");
        writeAttributes(out, "attributes", rule.attributes);
    }
    if (!rule.vertexAttributes.empty()) {
        object.key("vertexAttributes");
        writeAttributes(out, "vertexAttributes", rule.vertexAttributes);
    }
    if (!rule.segmentAttributes.empty()) {
        object.key("segmentAttributes");
        writeAttributes(out, "segmentAttributes", rule.segmentAttributes);
    }
    object.close();
}

// The definitions a write holds, by the array each goes in, in name order.
struct Selection {
    std::vector<const LineStyle*> linestyles{};
    std::vector<const LineStyle*> symbols{};
};

[[nodiscard]] Selection selected(const StyleLibrary& library,
                                 const CustomisationWriteOptions& options)
{
    const std::set<std::string_view> only(options.only.begin(), options.only.end());
    for (const std::string_view name : only) {
        const LineStyle* definition = library.find(name);
        if (definition == nullptr) {
            throw Refusal{ErrorCode::NotFound, "the customisation has no definition of this name",
                          inQuotes(name)};
        }
        if (!(definition->symbol ? options.symbols : options.linestyles)) {
            throw Refusal{
                ErrorCode::InvalidArgument,
                std::string("a definition asked for by name is a ") +
                    (definition->symbol ? "symbol, and symbols" : "linestyle, and linestyles") +
                    " are not being written",
                inQuotes(name)};
        }
    }
    Selection selection;
    library.forEach([&](const LineStyle& definition) { // in name order
        if (!only.empty() && !only.contains(definition.name)) {
            return;
        }
        if (definition.symbol ? options.symbols : options.linestyles) {
            (definition.symbol ? selection.symbols : selection.linestyles).push_back(&definition);
        }
    });
    return selection;
}

// The layout, as docs/customisation.md states it: two blanks a level; the
// top-level members one a line; the entries of the six lists one a line; a
// definition's strokes one a line; everything else on its entry's line.
[[nodiscard]] std::string written(const Customisation& customisation,
                                  const CustomisationWriteOptions& options)
{
    checkHeader(customisation);
    const Selection selection = selected(customisation.library, options);

    Out out;
    // About what a line of each kind takes; only to save regrowing the text.
    std::size_t strokes = 0;
    customisation.library.forEach([&](const LineStyle& d) { strokes += d.strokes.size(); });
    out.reserve(1024 + 32 * strokes + 160 * customisation.map.size());

    bool firstMember = true;
    const auto member = [&](std::string_view name) {
        out.raw(firstMember ? "\n  \"" : ",\n  \"");
        firstMember = false;
        out.raw(name);
        out.raw("\": ");
        out.member = name;
    };
    // The entries of a list, one a line.
    const auto entry = [&](std::size_t index) { out.raw(index == 0 ? "\n    " : ",\n    "); };

    out.raw("{");
    member("format");
    out.text(kCustomisationFormat);
    member("version");
    out.whole(kCustomisationVersion);
    member("name");
    out.text(customisation.name);
    if (!customisation.description.empty()) {
        member("description");
        out.text(customisation.description);
    }
    if (!customisation.notice.empty()) {
        member("notice");
        out.raw("[");
        for (std::size_t i = 0; i < customisation.notice.size(); ++i) {
            entry(i);
            out.text(customisation.notice[i]);
        }
        out.raw("\n  ]");
    }
    if (!customisation.sources.empty()) {
        static const CustomisationSourceNote d;
        member("sources");
        out.raw("[");
        for (std::size_t i = 0; i < customisation.sources.size(); ++i) {
            const CustomisationSourceNote& source = customisation.sources[i];
            out.where = labelled("sources", i, source.name);
            entry(i);
            Object object(out);
            object.key("name");
            out.text(source.name);
            object.flag("definitions", source.definitions, d.definitions);
            object.flag("rules", source.rules, d.rules);
            if (!source.notice.empty()) {
                object.key("notice");
                writeLines(out, source.notice);
            }
            object.close();
        }
        out.raw("\n  ]");
        out.where = kTopLevel;
    }
    if (customisation.basedOn) {
        member("basedOn");
        out.where = "basedOn";
        Object object(out);
        object.key("name");
        out.text(customisation.basedOn->name);
        object.key("digest");
        out.text(customisation.basedOn->digest);
        object.close();
        out.where = kTopLevel;
    }
    if (!customisation.colours.empty()) {
        member("colours");
        out.raw("{");
        std::size_t index = 0;
        for (const ColourTable::Entry& colour : customisation.colours.entries()) {
            entry(index++);
            out.text(colour.name);
            out.raw(": ");
            out.text(colour.colour.toHex());
        }
        out.raw("\n  }");
    }
    // Every member of these two is written, default or not: they are settings,
    // and a file Katana wrote should not come to mean something else because a
    // default changed under it.
    if (customisation.linework) {
        member("linework");
        out.where = "linework";
        Object object(out);
        for (const LineworkCodeMember& code : lineworkCodeMembers()) {
            object.key(code.name);
            out.text((*customisation.linework).*code.spelling);
        }
        object.close();
        out.where = kTopLevel;
    }
    if (customisation.automation) {
        member("automation");
        Object object(out);
        object.key("codesOnSurveyImport");
        out.flag(customisation.automation->codesOnSurveyImport);
        object.key("lineworkOnSurveyImport");
        out.flag(customisation.automation->lineworkOnSurveyImport);
        object.close();
    }
    const auto definitions = [&](std::string_view array,
                                 const std::vector<const LineStyle*>& list) {
        if (list.empty()) {
            return;
        }
        member(array);
        out.raw("[");
        for (std::size_t i = 0; i < list.size(); ++i) {
            out.where = labelled(array, i, list[i]->name);
            entry(i);
            writeDefinition(out, *list[i], customisation.name, "    ");
        }
        out.raw("\n  ]");
        out.where = kTopLevel;
    };
    definitions("linestyles", selection.linestyles);
    definitions("symbols", selection.symbols);
    if (options.codes && !customisation.map.empty()) {
        member("codes");
        out.raw("[");
        const std::vector<SurveyRule>& rules = customisation.map.rules();
        for (std::size_t i = 0; i < rules.size(); ++i) {
            out.where = labelled("codes", i, rules[i].key);
            entry(i);
            writeRule(out, rules[i]);
        }
        out.raw("\n  ]");
        out.where = kTopLevel;
    }
    out.raw("\n}\n");
    return out.take();
}

// ---- reading ----------------------------------------------------------------------

// One step from the root of a text to a place in it: a member, or a place in a
// list.
struct Step {
    std::string key{};
    std::size_t index = 0;
    bool indexed = false;
};

// A member given twice in one object, and where the object is.
struct Duplicate {
    std::vector<Step> path{};
    std::string member{};
};

// A number past the largest a double holds, at which the JSON library stops
// reading: where it is, and what the text wrote.
struct TooLarge {
    // To the object the number is in - itself a member, or in a list under
    // one - or, inside a stroke, to the stroke.
    std::vector<Step> path{};
    std::string member{}; // of that object; empty in a stroke
    bool stroke = false;
    std::string written{}; // "1e400"
    std::string at{};      // "line 2, column 45": of its last character
};

// The strokes of one definition, read while the text is parsed rather than
// from a tree of it. A library is 35,000 strokes, and a tree holds each as a
// list of its own with its word as text of its own - most of what a tree of
// the whole file weighs, built only to be read once and thrown away.
struct StrokesRead {
    std::vector<Stroke> strokes{};
    // What each text stroke holds, with the stroke's place. Read when the
    // definition is (readDefinition), where the entry it is in can be named.
    std::vector<std::pair<std::size_t, Json>> texts{};
    // The first stroke that is not one: its place, and what is wrong with it.
    struct Problem {
        std::size_t index = 0;
        std::string what{};
        std::string context{};
    };
    std::optional<Problem> problem{};
};

struct Parsed {
    Json root{};
    std::optional<Duplicate> duplicate{};
    // By the number the tree holds where a definition's "strokes" would be.
    std::vector<StrokesRead> strokes{};
    // Set when the reading stopped at such a number: `root` is then only
    // what the text held BEFORE it.
    std::optional<TooLarge> tooLarge{};
    // Set when the reading stopped at the rule past the most a file holds
    // (kCustomisationMostRules): `root` is then only what the text held before
    // it, as above.
    bool tooManyRules = false;
};

// Which text is being read: it decides where a list of strokes can be.
enum class TextKind { File, Definition };

// Builds the tree of a text from the JSON library's parse events (its SAX
// interface), for two things the library's own tree does not give:
//
// * A member given twice is SEEN. The library keeps the last of two equal
//   keys and says nothing, so a second "codes" list pasted at the end of a
//   file would replace 1,624 rules silently. Here the second is noticed as it
//   is put in, which is the only moment the first can still be seen.
// * A definition's strokes never become a tree (StrokesRead).
class TreeBuilder {
  public:
    explicit TreeBuilder(TextKind kind) : kind_(kind) {}

    // ---- what the parser calls ---------------------------------------------------
    bool null() { return scalar(Json(nullptr)); }
    bool boolean(bool value) { return scalar(Json(value)); }
    bool number_integer(Json::number_integer_t value)
    {
        return number(Json(value), static_cast<double>(value));
    }
    bool number_unsigned(Json::number_unsigned_t value)
    {
        return number(Json(value), static_cast<double>(value));
    }
    bool number_float(Json::number_float_t value, const Json::string_t& written)
    {
        // A number below the smallest a double holds comes from the library
        // as 0, with nothing said. But 0 is not what the file wrote, and here
        // 0 has meanings of its own - a length "not said", a size "the
        // definition's own" - so it is kept as what it is, a number this
        // cannot hold, and refused where it is read.
        if (value == 0.0 && saysMoreThanZero(written)) {
            return scalar(lostNumber(written));
        }
        return number(Json(value), value);
    }
    bool string(Json::string_t& value)
    {
        // A stroke's first value is its kind, one of seven words: known by
        // looking at it, with no copy made.
        if (inStroke() && values_.empty()) {
            if (const StrokeWord* word = strokeWordOf(value)) {
                child();
                values_.push_back(StrokeValue{StrokeValue::Kind::Word, word, 0.0, Json()});
                return true;
            }
        }
        return scalar(Json(value));
    }
    bool binary(Json::binary_t& /*value*/) { return false; } // JSON text holds none
    bool start_object(std::size_t /*size*/) { return begin(true); }
    bool key(Json::string_t& value)
    {
        open_.back().key = value;
        return true;
    }
    bool end_object() { return end(); }
    bool start_array(std::size_t /*size*/) { return begin(false); }
    bool end_array() { return end(); }
    bool parse_error(std::size_t position, const std::string& token, const Json::exception& error)
    {
        reason_ = error.what();
        // The one thing the library refuses that IS JSON: a number past the
        // largest double ("number overflow", its error 406). Its place is
        // kept, because what follows it will never be read.
        constexpr int kNumberOverflow = 406;
        if (error.id == kNumberOverflow) {
            tooLarge_ = here();
            tooLarge_->written = token;
            stoppedAt_ = position;
        }
        return false;
    }

    // ---- what was built ----------------------------------------------------------
    // Why the text is not JSON, after a parse that failed.
    [[nodiscard]] const std::string& reason() const { return reason_; }
    // Whether it failed at a number too large to hold, and how far into the
    // text that number ends.
    [[nodiscard]] bool stoppedAtANumber() const { return tooLarge_.has_value(); }
    // Whether it stopped at one rule too many.
    [[nodiscard]] bool stoppedAtTooManyRules() const { return tooManyRules_; }
    [[nodiscard]] std::size_t stoppedAt() const { return stoppedAt_; }
    [[nodiscard]] Parsed take()
    {
        return Parsed{std::move(root_), std::move(duplicate_), std::move(strokes_),
                      std::move(tooLarge_), tooManyRules_};
    }

  private:
    enum class Role {
        Tree,    // a list or an object being built into a tree
        Strokes, // a definition's list of strokes
        Stroke,  // one stroke of it
    };
    struct Frame {
        Role role = Role::Tree;
        Json* node = nullptr;     // Tree: where it is being built
        bool object = false;      // an object rather than a list
        bool aside = false;       // Tree: the root of a tree built aside from the text's own
        std::string key{};        // an object: the member whose value comes next
        std::size_t children = 0; // a list: its values so far
    };
    // One value of the stroke being read. `written` is the value as the text
    // gave it - except for a known kind word, which is kept as the word alone.
    struct StrokeValue {
        enum class Kind { Word, Number, Other };
        Kind kind = Kind::Other;
        const StrokeWord* word = nullptr;
        double number = 0.0;
        Json written{};
    };

    [[nodiscard]] bool inStroke() const
    {
        return !open_.empty() && open_.back().role == Role::Stroke;
    }

    // A value begins: in a list, it is the next one.
    void child()
    {
        if (!open_.empty() && !open_.back().object) {
            ++open_.back().children;
        }
    }

    // Where the innermost of the first `count` open containers is: each of
    // them but the root, by where it sits in its parent - and no further than
    // a stroke. A stroke is the smallest place a refusal names
    // (`symbols[0] "TEST Valve" strokes[9]`); what is wrong inside one, in a
    // text stroke's object, is said of that stroke.
    [[nodiscard]] std::vector<Step> path(std::size_t count) const
    {
        std::vector<Step> steps;
        for (std::size_t i = 0; i + 1 < count; ++i) {
            const Frame& parent = open_[i];
            steps.push_back(parent.object ? Step{parent.key, 0, false}
                                          : Step{{}, parent.children - 1, true});
            if (open_[i + 1].role == Role::Stroke) {
                break;
            }
        }
        return steps;
    }

    // Where the value now being read is, as a refusal names a place: the
    // object it is a member of - or, when it is in a list, the object that
    // list is a member of - with that member; or the stroke it is a value of.
    [[nodiscard]] TooLarge here() const
    {
        TooLarge place;
        for (std::size_t i = open_.size(); i-- > 0;) {
            if (open_[i].object) {
                place.member = open_[i].key;
                place.path = path(i + 1);
                return place;
            }
            if (open_[i].role == Role::Stroke) {
                place.stroke = true;
                place.path = path(i + 1);
                return place;
            }
        }
        // In no object at all: lists alone, or the text is the value itself.
        place.path = path(open_.size());
        return place;
    }

    // Puts a value where the innermost open list or object wants it, and says
    // where it now is. A list's values cannot move while one of them is still
    // open, so the address is good for as long as the frame that keeps it.
    Json* place(Json value)
    {
        if (open_.empty()) {
            root_ = std::move(value);
            return &root_;
        }
        Frame& parent = open_.back();
        if (!parent.object) {
            return &parent.node->emplace_back(std::move(value));
        }
        if (const auto existing = parent.node->find(parent.key); existing != parent.node->end()) {
            if (!duplicate_) {
                duplicate_ = Duplicate{path(open_.size()), parent.key};
            }
            *existing = std::move(value); // refused later; the text is read to its end first
            return &*existing;
        }
        return &*parent.node->emplace(parent.key, std::move(value)).first;
    }

    bool scalar(Json value)
    {
        child();
        if (open_.empty() || open_.back().role == Role::Tree) {
            place(std::move(value));
        } else if (open_.back().role == Role::Stroke) {
            values_.push_back(
                StrokeValue{StrokeValue::Kind::Other, nullptr, 0.0, std::move(value)});
        } else {
            notAStroke(value); // a bare value where a stroke should be
        }
        return true;
    }

    bool number(Json written, double value)
    {
        if (!inStroke()) {
            return scalar(std::move(written));
        }
        child();
        values_.push_back(
            StrokeValue{StrokeValue::Kind::Number, nullptr, value, std::move(written)});
        return true;
    }

    bool begin(bool object)
    {
        child();
        if (!open_.empty() && open_.back().role != Role::Tree) {
            if (open_.back().role == Role::Strokes && !object) {
                open_.push_back(Frame{Role::Stroke, nullptr, false, false, {}, 0});
                values_.clear();
                return true;
            }
            // An object where a stroke should be, or an object or a list
            // inside a stroke (a text's object is one): built aside, and
            // handed over whole when it closes.
            aside_ = object ? Json::object() : Json::array();
            open_.push_back(Frame{Role::Tree, &aside_, object, true, {}, 0});
            return true;
        }
        if (!object && beginsStrokes()) {
            // The tree holds, where the strokes would be, which entry of
            // strokes_ they are.
            place(strokesPlace(strokes_.size()));
            strokes_.emplace_back();
            open_.push_back(Frame{Role::Strokes, nullptr, false, false, {}, 0});
            return true;
        }
        if (object && isRuleOfAFile() && ++rules_ > kCustomisationMostRules) {
            // Stops the parse here: the rules past the bound are never held,
            // so a file of millions costs no more than the bound does.
            tooManyRules_ = true;
            return false;
        }
        Json* node = place(object ? Json::object() : Json::array());
        open_.push_back(Frame{Role::Tree, node, object, false, {}, 0});
        return true;
    }

    bool end()
    {
        const Role role = open_.back().role;
        const bool aside = open_.back().aside;
        open_.pop_back();
        if (role == Role::Stroke) {
            finishStroke();
        } else if (role == Role::Tree && aside) {
            if (open_.back().role == Role::Stroke) {
                values_.push_back(
                    StrokeValue{StrokeValue::Kind::Other, nullptr, 0.0, std::move(aside_)});
            } else {
                notAStroke(aside_);
            }
        }
        return true;
    }

    // An object that is an entry of a file's "codes" list: a rule.
    [[nodiscard]] bool isRuleOfAFile() const
    {
        return kind_ == TextKind::File && open_.size() == 2 && open_[0].object &&
               open_[0].key == "codes" && !open_[1].object;
    }

    // In a file, the "strokes" of an entry of "linestyles" or "symbols"; in a
    // definition's own text, its "strokes". Anywhere else a member of that
    // name is built into the tree like any other, and refused there.
    [[nodiscard]] bool beginsStrokes() const
    {
        if (kind_ == TextKind::Definition) {
            return open_.size() == 1 && open_[0].object && open_[0].key == "strokes";
        }
        return open_.size() == 3 && open_[0].object &&
               (open_[0].key == "linestyles" || open_[0].key == "symbols") && !open_[1].object &&
               open_[2].object && open_[2].key == "strokes";
    }

    // The stroke being read as the text wrote it, for a refusal to show:
    // written straight from its values, which are never copied to do it (a
    // copy of a value goes as deep into the stack as the value is nested).
    [[nodiscard]] std::string shownStroke() const
    {
        std::string text = "[";
        for (std::size_t i = 0; i < values_.size() && text.size() <= kLongestShown; ++i) {
            text += i == 0 ? "" : ",";
            if (values_[i].kind == StrokeValue::Kind::Word) {
                text += inQuotes(values_[i].word->word);
            } else {
                appendShown(text, values_[i].written);
            }
        }
        text += ']';
        return cutShort(std::move(text));
    }

    // The first problem of a definition's strokes is the one a refusal names;
    // the list frame is innermost when this is called.
    void problem(std::string what, std::string context)
    {
        StrokesRead& read = strokes_.back();
        if (!read.problem) {
            read.problem = StrokesRead::Problem{open_.back().children - 1, std::move(what),
                                                std::move(context)};
        }
    }

    void notAStroke(const Json& value)
    {
        problem("a stroke is a list that begins with its kind, such as [\"move\", 0, 0]",
                shown(value));
    }

    // One stroke's values are in: the stroke they make, or what is wrong.
    void finishStroke()
    {
        StrokesRead& read = strokes_.back();
        if (read.problem) {
            return; // nothing after the first problem is kept
        }
        if (read.strokes.size() >= kCustomisationMostStrokes) {
            problem(std::string(kTooManyStrokes), {});
            return;
        }
        if (values_.empty() ||
            (values_[0].kind != StrokeValue::Kind::Word && !values_[0].written.is_string())) {
            problem("a stroke is a list that begins with its kind, such as [\"move\", 0, 0]",
                    shownStroke());
            return;
        }
        if (values_[0].kind != StrokeValue::Kind::Word) {
            std::string known;
            for (const StrokeWord& entry : kStrokeWords) {
                known += known.empty() ? "" : ", ";
                known += entry.word;
            }
            std::string what = inQuotes(values_[0].written.get_ref<const std::string&>());
            what += " is not a kind of stroke, which is one of ";
            what += known;
            problem(std::move(what), {});
            return;
        }
        const StrokeWord& kind = *values_[0].word;
        // Built only for a refusal: there are tens of thousands of strokes.
        const auto takes = [&kind] {
            std::string words = inQuotes(kind.word);
            words += " takes ";
            words += kind.takes;
            return words;
        };
        const auto refused = [&](std::string_view how) {
            std::string what = takes();
            what += how;
            problem(std::move(what), shownStroke());
        };
        if (values_.size() != kind.values + 1) {
            std::string how = " (";
            how += std::to_string(kind.values);
            how += " after the word) and has ";
            how += std::to_string(values_.size() - 1);
            refused(how);
            return;
        }
        const auto numbers = [this] {
            return std::all_of(values_.begin() + 1, values_.end(), [](const StrokeValue& value) {
                return value.kind == StrokeValue::Kind::Number;
            });
        };

        Stroke stroke;
        stroke.op = kind.op;
        switch (kind.op) {
        case StrokeOp::Move:
        case StrokeOp::Draw:
        case StrokeOp::Arc:
        case StrokeOp::Circle:
        case StrokeOp::Dot:
            if (!numbers()) {
                // A number too small to hold is among the values as what it
                // is (number_float), and is said to be that.
                const bool lost =
                    std::any_of(values_.begin() + 1, values_.end(), [](const StrokeValue& value) {
                        return isLostNumber(value.written);
                    });
                if (lost) {
                    std::string what = "this stroke ";
                    what += kTooSmall;
                    problem(std::move(what), shownStroke());
                } else {
                    refused(", each a number");
                }
                return;
            }
            for (std::size_t i = 1; i < values_.size(); ++i) {
                if (!withinMagnitude(values_[i].number)) {
                    std::string what = inQuotes(kind.word);
                    what += " has a number ";
                    what += kBeyondMagnitude;
                    problem(std::move(what), shownStroke());
                    return;
                }
            }
            if (kind.op == StrokeOp::Arc && !withinSweep(values_[2].number, values_[3].number)) {
                std::string what = inQuotes(kind.word);
                what += ' ';
                what += kSweepsTooFar;
                problem(std::move(what), shownStroke());
                return;
            }
            if (kind.op == StrokeOp::Move || kind.op == StrokeOp::Draw) {
                stroke.point = Point2(values_[1].number, values_[2].number);
            } else {
                stroke.radius = values_[1].number;
                if (kind.op == StrokeOp::Arc) {
                    stroke.startAngle = values_[2].number;
                    stroke.endAngle = values_[3].number;
                }
            }
            break;
        case StrokeOp::Pen:
            if (!values_[1].written.is_string()) {
                refused(", as text in double quotes");
                return;
            }
            if (values_[1].written.get_ref<const std::string&>().size() >
                kCustomisationMostTextBytes) {
                std::string what = inQuotes(kind.word);
                what += " is ";
                what += kTooLong;
                problem(std::move(what), shownStroke());
                return;
            }
            stroke.pen = values_[1].written.get<std::string>();
            break;
        case StrokeOp::Text:
            // Kept beside the strokes in the model and inside the stroke in
            // the file, where a person looks for it: so the i-th text stroke
            // names text i, the only arrangement the writer accepts back.
            stroke.text = read.texts.size();
            read.texts.emplace_back(open_.back().children - 1, std::move(values_[1].written));
            break;
        }
        read.strokes.push_back(std::move(stroke));
    }

    TextKind kind_;
    Json root_{};
    Json aside_{};
    std::vector<Frame> open_{};
    std::vector<StrokeValue> values_{}; // of the stroke being read
    std::vector<StrokesRead> strokes_{};
    std::optional<Duplicate> duplicate_{};
    std::optional<TooLarge> tooLarge_{};
    std::size_t stoppedAt_ = 0; // bytes of the text read when it stopped
    std::size_t rules_ = 0;     // the objects of the file's "codes" list so far
    bool tooManyRules_ = false;
    std::string reason_{};
};

// "line 2, column 45" for the last of the first `read` bytes of `text`: where
// the JSON library stands when it has read that far, counted as it counts for
// its own messages - lines from 1, and the column of the last byte read.
[[nodiscard]] std::string lineAndColumn(std::string_view text, std::size_t read)
{
    const std::string_view before = text.substr(0, std::min(read, text.size()));
    const auto line = 1 + std::count(before.begin(), before.end(), '\n');
    const std::size_t lineBreak = before.rfind('\n');
    const std::size_t column =
        before.size() - (lineBreak == std::string_view::npos ? 0 : lineBreak + 1);
    return "line " + std::to_string(line) + ", column " + std::to_string(column);
}

// The bytes of a text as JSON. Anything that is not JSON is `notThis` - the
// one message a caller's own kind of text gets - with the reason, and for
// malformed JSON the line and column, as the context. The one exception comes
// back in what is returned, for the caller to refuse: a number too large to
// hold (Parsed::tooLarge), where what the text is depends on how far it had
// been read.
[[nodiscard]] Parsed parseJson(std::string_view bytes, std::string_view notThis, TextKind kind)
{
    // Decoded first: the JSON library steps over a UTF-8 byte order mark and
    // nothing else, and an editor on Windows saves UTF-16 readily.
    auto decoded = katana::core::decodeText(bytes);
    if (!decoded) {
        // A byte order mark over bytes that are not what it promises - a
        // file cut short, or damaged. Still bytes that are neither UTF-8 nor
        // UTF-16, and so the same message as those below, with the decoder's
        // account of what is wrong beside it.
        throw Refusal{ErrorCode::ParseFailure, std::string(notThis), decoded.error().message};
    }
    // Bytes that are neither UTF-8 nor UTF-16 are decoded as Windows-1252 for
    // the formats that never said what they are. This one does say: it is
    // JSON, and JSON is UTF-8. A guess at a code page would put a wrong
    // character into a name silently, and a name here is an identity.
    if (decoded->encoding == katana::core::TextEncoding::Windows1252) {
        throw Refusal{ErrorCode::ParseFailure, std::string(notThis),
                      "the bytes are not UTF-8 text"};
    }
    TreeBuilder builder(kind);
    if (!Json::sax_parse(decoded->text, &builder)) {
        if (builder.stoppedAtTooManyRules()) {
            return builder.take();
        }
        if (builder.stoppedAtANumber()) {
            const std::string at = lineAndColumn(decoded->text, builder.stoppedAt());
            Parsed text = builder.take();
            text.tooLarge->at = at;
            return text;
        }
        // "[json.exception.parse_error.101] parse error at line 3, column 7: ...":
        // the bracketed id is the library's and says nothing to a person.
        std::string reason = builder.reason();
        if (const std::size_t id = reason.find("] ");
            reason.starts_with("[") && id != std::string::npos) {
            reason.erase(0, id + 2);
        }
        constexpr std::size_t kLongest = 240;
        if (reason.size() > kLongest) {
            reason.resize(kLongest);
            reason += "...";
        }
        throw Refusal{ErrorCode::ParseFailure, std::string(notThis), std::move(reason)};
    }
    return builder.take();
}

// Where something noticed while the text was parsed is - a member given
// twice, a number too large to hold - in the words every other refusal uses.
// `base` names the root when it is an entry itself (a definition read on its
// own).
[[nodiscard]] std::string placeOf(const Json& root, const std::vector<Step>& path,
                                  std::string_view base)
{
    std::string label(base);
    if (path.empty()) {
        return label.empty() ? std::string(kTopLevel) : label;
    }
    const Json* at = &root;
    for (std::size_t i = 0; i < path.size(); ++i) {
        const Step& step = path[i];
        if (!step.indexed) {
            label += label.empty() ? "" : " ";
            label += step.key;
            at = at != nullptr && at->is_object() && at->contains(step.key) ? &(*at)[step.key]
                                                                            : nullptr;
            continue;
        }
        label += '[';
        label += std::to_string(step.index);
        label += ']';
        at = at != nullptr && at->is_array() && step.index < at->size() ? &(*at)[step.index]
                                                                        : nullptr;
        // An entry of one of the file's lists is shown with its own name.
        if (base.empty() && i == 1 && !path[0].indexed && at != nullptr && at->is_object()) {
            const std::string_view nameMember = nameMemberOf(path[0].key);
            if (const auto name = at->find(nameMember);
                !nameMember.empty() && name != at->end() && name->is_string()) {
                label += " " + inQuotes(name->get_ref<const std::string&>());
            }
        }
    }
    return label;
}

void refuseDuplicate(const Parsed& text, std::string_view base)
{
    if (text.duplicate) {
        refuse(ErrorCode::ParseFailure, placeOf(text.root, text.duplicate->path, base),
               "member " + inQuotes(text.duplicate->member) +
                   " is given twice, and only one of them could be kept");
    }
}

// What a refusal shows beside a number too large to hold: the number, and
// where in the text it is.
[[nodiscard]] std::string writtenAt(const TooLarge& number)
{
    return number.written + " at " + number.at;
}

// Refuses the number too large to hold that the reading of `text` stopped at,
// by the entry and the member it is in, as a value of the wrong kind is
// refused.
[[noreturn]] void refuseTooLarge(const Parsed& text, std::string_view base)
{
    const TooLarge& number = *text.tooLarge;
    std::string what = number.stroke ? std::string("this stroke") : inQuotes(number.member);
    what += ' ';
    what += kTooLarge;
    refuse(ErrorCode::ParseFailure, placeOf(text.root, number.path, base), what,
           writtenAt(number));
}

// One object of the file being read. Every member is taken through here by
// name, and that is what makes a member KNOWN: finish() refuses whatever the
// object holds that nothing asked for. There is no separate list of allowed
// names to fall out of step with the code that reads them.
class Members {
  public:
    Members(const Json& json, std::string label) : json_(json), label_(std::move(label))
    {
        asked_.reserve(kMostMembers);
        if (!json.is_object()) {
            refuse(ErrorCode::ParseFailure, label_, "must be an object, written {...}",
                   shown(json));
        }
    }

    // The entry this object is, as a refusal names it.
    [[nodiscard]] const std::string& label() const { return label_; }

    // The member, or nullptr when the object does not have it.
    [[nodiscard]] const Json* find(std::string_view name)
    {
        asked_.push_back(name);
        const auto found = json_.find(name);
        if (found == json_.end()) {
            return nullptr;
        }
        ++found_;
        return &*found;
    }

    [[nodiscard]] const Json& required(std::string_view name)
    {
        const Json* value = find(name);
        if (value == nullptr) {
            refuse(ErrorCode::ParseFailure, label_, "has no " + inQuotes(name) + ", which it must");
        }
        return *value;
    }

    [[nodiscard]] std::string textOf(std::string_view name, const Json& value) const
    {
        if (!value.is_string()) {
            wrong(name, "must be text, in double quotes", value);
        }
        const std::string& text = value.get_ref<const std::string&>();
        if (text.size() > kCustomisationMostTextBytes) {
            wrong(name, "is " + std::string(kTooLong), value);
        }
        return text;
    }

    [[nodiscard]] std::string text(std::string_view name, const std::string& fallback = {})
    {
        const Json* value = find(name);
        return value == nullptr ? fallback : textOf(name, *value);
    }

    [[nodiscard]] std::string requiredText(std::string_view name)
    {
        return textOf(name, required(name));
    }

    [[nodiscard]] double real(std::string_view name, double fallback)
    {
        const Json* value = find(name);
        if (value == nullptr) {
            return fallback;
        }
        if (isLostNumber(*value)) {
            wrong(name, kTooSmall, *value);
        }
        if (!value->is_number()) {
            wrong(name, "must be a number", *value);
        }
        if (!withinMagnitude(value->get<double>())) {
            wrong(name, "is " + std::string(kBeyondMagnitude), *value);
        }
        return value->get<double>();
    }

    // A whole number: 2.7 would otherwise be cut to 2 and `true` read as 1,
    // each some other value than the file gave.
    [[nodiscard]] int whole(std::string_view name, int fallback)
    {
        const Json* value = find(name);
        if (value == nullptr) {
            return fallback;
        }
        if (!value->is_number_integer()) {
            wrong(name, "must be a whole number", *value);
        }
        const bool fits = value->is_number_unsigned()
                              ? value->get<std::uint64_t>() <=
                                    static_cast<std::uint64_t>(std::numeric_limits<int>::max())
                              : value->get<std::int64_t>() >= std::numeric_limits<int>::min();
        if (!fits) {
            wrong(name, "is a whole number too large to hold", *value);
        }
        return static_cast<int>(value->get<std::int64_t>());
    }

    [[nodiscard]] std::optional<bool> optionalFlag(std::string_view name)
    {
        const Json* value = find(name);
        if (value == nullptr) {
            return std::nullopt;
        }
        if (!value->is_boolean()) {
            wrong(name, "must be true or false", *value);
        }
        return value->get<bool>();
    }

    [[nodiscard]] bool flag(std::string_view name, bool fallback)
    {
        return optionalFlag(name).value_or(fallback);
    }

    template <typename Enum, std::size_t N>
    [[nodiscard]] std::optional<Enum> optionalWord(std::string_view name,
                                                   const std::array<Word<Enum>, N>& words)
    {
        const Json* value = find(name);
        if (value == nullptr) {
            return std::nullopt;
        }
        if (value->is_string()) {
            const std::string& given = value->get_ref<const std::string&>();
            for (const Word<Enum>& entry : words) {
                if (entry.word == given) {
                    return entry.value;
                }
            }
        }
        refuse(ErrorCode::ParseFailure, label_,
               inQuotes(name) + " is " + shown(*value) + ", which is not one of " + listed(words));
    }

    template <typename Enum, std::size_t N>
    [[nodiscard]] Enum word(std::string_view name, const std::array<Word<Enum>, N>& words,
                            Enum fallback)
    {
        return optionalWord(name, words).value_or(fallback);
    }

    [[noreturn]] void wrong(std::string_view name, std::string_view rule, const Json& value) const
    {
        refuse(ErrorCode::ParseFailure, label_, inQuotes(name) + " " + std::string(rule),
               shown(value));
    }

    // After every member has been asked for: anything left is unknown. It is
    // refused rather than skipped because a customisation is edited by hand,
    // and "linesytle" skipped is a rule that silently draws nothing - and is
    // no longer in the file once Katana has written it back.
    void finish() const
    {
        if (found_ == json_.size()) {
            return;
        }
        for (auto item = json_.begin(); item != json_.end(); ++item) {
            if (std::find(asked_.begin(), asked_.end(), std::string_view(item.key())) ==
                asked_.end()) {
                refuse(ErrorCode::ParseFailure, label_, "unknown member " + inQuotes(item.key()),
                       "the members here are " + listed(asked_));
            }
        }
    }

  private:
    // A rule has the most members, nineteen; room for them at once saves the
    // list growing five times for every rule read.
    static constexpr std::size_t kMostMembers = 24;

    const Json& json_;
    std::string label_;
    std::vector<std::string_view> asked_{};
    std::size_t found_ = 0;
};

[[nodiscard]] Point2 pointOf(const Json& json, std::string_view where, std::string_view member)
{
    if (json.is_array() && json.size() == 2 && (isLostNumber(json[0]) || isLostNumber(json[1]))) {
        refuse(ErrorCode::ParseFailure, where, inQuotes(member) + " " + std::string(kTooSmall),
               shown(json));
    }
    if (!json.is_array() || json.size() != 2 || !json[0].is_number() || !json[1].is_number()) {
        refuse(ErrorCode::ParseFailure, where,
               inQuotes(member) + " must be a point, written [x, y]", shown(json));
    }
    if (!withinMagnitude(json[0].get<double>()) || !withinMagnitude(json[1].get<double>())) {
        refuse(ErrorCode::ParseFailure, where, inQuotes(member) + " is " + std::string(kBeyondMagnitude),
               shown(json));
    }
    return Point2(json[0].get<double>(), json[1].get<double>());
}

[[nodiscard]] std::vector<std::string> linesOf(const Json& json, std::string_view where,
                                               std::string_view member)
{
    const auto bad = [&] {
        refuse(ErrorCode::ParseFailure, where,
               inQuotes(member) + " must be lines of text, written [\"...\", \"...\"]",
               shown(json));
    };
    if (!json.is_array()) {
        bad();
    }
    std::vector<std::string> lines;
    lines.reserve(json.size());
    for (const Json& line : json) {
        if (!line.is_string()) {
            bad();
        }
        if (line.get_ref<const std::string&>().size() > kCustomisationMostTextBytes) {
            refuse(ErrorCode::ParseFailure, where,
                   inQuotes(member) + " holds a line that is " + std::string(kTooLong),
                   shown(line));
        }
        lines.push_back(line.get<std::string>());
    }
    return lines;
}

// A list of the file: an array, or the refusal that says so.
[[nodiscard]] const Json& listOf(const Json& json, std::string_view where, std::string_view member)
{
    if (!json.is_array()) {
        refuse(ErrorCode::ParseFailure, where, inQuotes(member) + " must be a list, written [...]",
               shown(json));
    }
    return json;
}

[[nodiscard]] StrokeText readStrokeText(const Json& json, const std::string& where)
{
    const StrokeText d;
    Members members(json, where);
    StrokeText text;
    text.text = members.text("text", d.text);
    text.angle = members.real("angle", d.angle);
    text.height = members.real("height", d.height);
    text.justify = members.text("justify", d.justify);
    text.font = members.text("font", d.font);
    text.widthFactor = members.real("widthFactor", d.widthFactor);
    if (const Json* extra = members.find("extra")) {
        const bool three = extra->is_array() && extra->size() == text.unnamed.size();
        if (three && std::any_of(extra->begin(), extra->end(), isLostNumber)) {
            members.wrong("extra", kTooSmall, *extra);
        }
        if (!three || !std::all_of(extra->begin(), extra->end(),
                                   [](const Json& value) { return value.is_number(); })) {
            members.wrong("extra", "must be three numbers, written [a, b, c]", *extra);
        }
        for (std::size_t i = 0; i < text.unnamed.size(); ++i) {
            text.unnamed[i] = (*extra)[i].get<double>();
            if (!withinMagnitude(text.unnamed[i])) {
                members.wrong("extra", "has a number " + std::string(kBeyondMagnitude), *extra);
            }
        }
    }
    members.finish();
    return text;
}

// One definition. `symbol` is the array it sits in and `customisation` the
// name it sits under, which is its source unless it says "from"; `strokesRead`
// is where the parse left every definition's strokes.
[[nodiscard]] LineStyle readDefinition(const Json& json, const std::string& where, bool symbol,
                                       std::string_view customisation,
                                       std::vector<StrokesRead>& strokesRead)
{
    const LineStyle d;
    Members members(json, where);
    LineStyle definition;
    definition.name = members.requiredText("name");
    definition.group = members.text("group", d.group);
    definition.units = members.word("units", kUnitsWords, d.units);
    definition.atVertices = members.flag("atVertices", d.atVertices);
    definition.length = members.real("length", d.length);
    definition.factor = members.real("factor", d.factor);
    if (const Json* origin = members.find("origin")) {
        definition.origin = pointOf(*origin, where, "origin");
    }
    if (const Json* anchors = members.find("anchors")) {
        if (!anchors->is_array() || anchors->size() != 2) {
            members.wrong("anchors", "must be two points, written [[x, y], [x, y]]", *anchors);
        }
        definition.anchor1 = pointOf((*anchors)[0], where, "anchors");
        definition.anchor2 = pointOf((*anchors)[1], where, "anchors");
    }
    definition.stretchMode = members.whole("stretchMode", d.stretchMode);
    definition.cycleMode = members.whole("cycleMode", d.cycleMode);
    definition.symbol = symbol;
    if (const Json* from = members.find("from")) {
        definition.source = members.textOf("from", *from);
    } else {
        definition.source = std::string(customisation);
    }
    const Json* strokes = members.find("strokes");
    members.finish();
    if (strokes != nullptr) {
        // A list of strokes was read as the parse went by, and the tree holds
        // only which one it was; anything else here is not a list.
        if (!isStrokesPlace(*strokes)) {
            members.wrong("strokes", "must be a list, written [...]", *strokes);
        }
        StrokesRead& read = strokesRead.at(strokes->get_binary().subtype());
        // In the order of the strokes, so that the first thing wrong is the
        // one reported: the texts before the first stroke that is not one,
        // then that stroke.
        definition.texts.reserve(read.texts.size());
        for (const auto& [index, object] : read.texts) {
            if (read.problem && index > read.problem->index) {
                break;
            }
            definition.texts.push_back(readStrokeText(object, placed(where, "strokes", index)));
        }
        if (read.problem) {
            refuse(ErrorCode::ParseFailure, placed(where, "strokes", read.problem->index),
                   read.problem->what, read.problem->context);
        }
        definition.strokes = std::move(read.strokes);
    }
    checkSource(definition, where);
    return definition;
}

[[nodiscard]] std::vector<SurveyAttribute>
readAttributes(const Json& json, const std::string& where, std::string_view member)
{
    std::vector<SurveyAttribute> attributes;
    std::size_t index = 0;
    for (const Json& item : listOf(json, where, member)) {
        Members members(item, placed(where, member, index++));
        SurveyAttribute attribute;
        // Text in the model, and only these two words here: a type this did
        // not know would be an attribute nothing can set.
        const Json& type = members.required("type");
        if (!type.is_string() ||
            std::find(kAttributeTypes.begin(), kAttributeTypes.end(),
                      type.get_ref<const std::string&>()) == kAttributeTypes.end()) {
            refuse(ErrorCode::ParseFailure, members.label(),
                   "\"type\" is " + shown(type) + ", which is not one of text, integer");
        }
        attribute.type = type.get<std::string>();
        attribute.name = members.requiredText("name");
        attribute.value = members.text("value");
        members.finish();
        attributes.push_back(std::move(attribute));
    }
    return attributes;
}

[[nodiscard]] SurveyPipe readPipe(const Json& json, const std::string& where)
{
    const SurveyPipe d;
    Members members(json, where);
    SurveyPipe pipe;
    pipe.justify = members.text("justify", d.justify);
    pipe.shape = members.text("shape", d.shape);
    pipe.size1 = members.text("size1", d.size1);
    pipe.size2 = members.text("size2", d.size2);
    pipe.active = members.flag("active", d.active);
    members.finish();
    return pipe;
}

[[nodiscard]] SurveyRule readRule(const Json& json, const std::string& where)
{
    const SurveyRule d;
    Members members(json, where);
    SurveyRule rule;
    rule.key = members.requiredText("key");
    // Required, with no default: a rule's identity in a merge is what it sets
    // and its key, so a symbol rule read as the default would replace that
    // key's layer and colour rule.
    const auto sets = members.optionalWord("sets", kSectionWords);
    if (!sets) {
        refuse(ErrorCode::ParseFailure, where, "has no \"sets\", which it must",
               "it is one of " + listed(kSectionWords));
    }
    rule.section = *sets;
    rule.model = members.text("layer", d.model);
    rule.colour = members.text("colour", d.colour);
    rule.breakline = members.optionalWord("draw", kDrawWords);
    rule.linestyle = members.text("linestyle", d.linestyle);
    rule.weight = members.text("weight", d.weight);
    rule.group = members.text("group", d.group);
    rule.comment = members.text("comment", d.comment);
    rule.tinable = members.optionalFlag("surface");
    rule.hide = members.optionalFlag("hide");
    if (const Json* symbol = members.find("symbol")) {
        const SurveySymbol s;
        Members part(*symbol, where + " symbol");
        SurveySymbol value;
        value.style = part.text("name", s.style);
        value.colour = part.text("colour", s.colour);
        value.size = part.real("size", s.size);
        value.rotation = part.real("rotation", s.rotation);
        value.offset = part.real("offset", s.offset);
        value.raise = part.real("raise", s.raise);
        part.finish();
        rule.symbol = std::move(value);
    }
    if (const Json* text = members.find("text")) {
        const SurveyTextStyle t;
        Members part(*text, where + " text");
        SurveyTextStyle value;
        value.textstyle = part.text("style", t.textstyle);
        value.colour = part.text("colour", t.colour);
        value.type = part.text("units", t.type);
        value.size = part.real("size", t.size);
        value.justifyX = part.text("justifyX", t.justifyX);
        value.justifyY = part.text("justifyY", t.justifyY);
        value.offset = part.real("offset", t.offset);
        value.raise = part.real("raise", t.raise);
        value.angle = part.real("angle", t.angle);
        value.slant = part.real("slant", t.slant);
        value.widthFactor = part.real("widthFactor", t.widthFactor);
        value.underline = part.flag("underline", t.underline);
        value.strikeout = part.flag("strikeout", t.strikeout);
        value.italic = part.flag("italic", t.italic);
        value.weight = part.text("weight", t.weight);
        part.finish();
        rule.textStyle = std::move(value);
    }
    if (const Json* pipe = members.find("pipe")) {
        rule.pipe = readPipe(*pipe, where + " pipe");
    }
    if (const Json* pipe = members.find("vertexPipe")) {
        rule.vertexPipe = readPipe(*pipe, where + " vertexPipe");
    }
    if (const Json* pipe = members.find("segmentPipe")) {
        rule.segmentPipe = readPipe(*pipe, where + " segmentPipe");
    }
    if (const Json* list = members.find("attributes")) {
        rule.attributes = readAttributes(*list, where, "attributes");
    }
    if (const Json* list = members.find("vertexAttributes")) {
        rule.vertexAttributes = readAttributes(*list, where, "vertexAttributes");
    }
    if (const Json* list = members.find("segmentAttributes")) {
        rule.segmentAttributes = readAttributes(*list, where, "segmentAttributes");
    }
    members.finish();
    // entity::validate would refuse this too, in the model's word for it; the
    // file's word is "layer".
    if (!rule.model.empty()) {
        if (auto status = validateLayerPath(rule.model); !status) {
            refuse(ErrorCode::InvalidArgument, where,
                   "\"layer\" is not a layer path: " + status.error().message,
                   inQuotes(rule.model));
        }
    }
    return rule;
}

[[nodiscard]] bool saysThisFormat(const Json& format)
{
    return format.is_string() && format.get_ref<const std::string&>() == kCustomisationFormat;
}

// A version past the newest this build reads.
[[nodiscard]] bool isNewer(const Json& version)
{
    return version.is_number_unsigned() &&
           version.get<std::uint64_t>() > static_cast<std::uint64_t>(kCustomisationVersion);
}

[[noreturn]] void refuseNewer(const Json& version)
{
    throw Refusal{ErrorCode::Unsupported,
                  "the customisation was written by a newer version of Katana",
                  "version " + shown(version) + "; this build reads version " +
                      std::to_string(kCustomisationVersion)};
}

// The root is a customisation of a version this build reads, or the refusal
// that says which it is not. Before anything else is looked at: a newer file
// may hold members this build would call unknown, and what it must be told is
// that the file is newer.
void requireCustomisation(const Json& root)
{
    if (!root.is_object()) {
        throw Refusal{ErrorCode::ParseFailure, std::string(kNotACustomisation),
                      "the text is JSON, but not an object"};
    }
    const auto format = root.find("format");
    if (format == root.end()) {
        throw Refusal{ErrorCode::ParseFailure, std::string(kNotACustomisation),
                      "it has no \"format\""};
    }
    if (!saysThisFormat(*format)) {
        throw Refusal{ErrorCode::ParseFailure, std::string(kNotACustomisation),
                      "its \"format\" is " + shown(*format)};
    }
    const auto version = root.find("version");
    if (version == root.end()) {
        refuse(ErrorCode::ParseFailure, kTopLevel, "has no \"version\", which it must");
    }
    if (!version->is_number_integer()) {
        refuse(ErrorCode::ParseFailure, kTopLevel, "\"version\" must be a whole number",
               shown(*version));
    }
    if (!version->is_number_unsigned() || version->get<std::uint64_t>() == 0) {
        refuse(ErrorCode::ParseFailure, kTopLevel,
               "\"version\" is " + shown(*version) + ", and the first version is 1");
    }
    if (isNewer(*version)) {
        refuseNewer(*version);
    }
}

// What a file is told whose reading stopped at a number too large to hold.
// Everything else the reader refuses is refused with the whole text read, in
// the order docs/customisation.md gives: is it this format, is it newer, then
// what is wrong with it. Here there is only what the text held BEFORE the
// number - the JSON library cannot go on past one - so that order is kept as
// far as the text had got. A file that had said its "format" is refused by
// the entry and the member, as for any value of the wrong kind (and one that
// had said it is newer is told that); a file that had not is not known to be
// a customisation at all, and gets the message every other text gets, with
// the number and its place beside it. The writer puts "format" and "version"
// first, so a file it wrote, or one edited from such a file, is the first
// case.
[[noreturn]] void refuseTooLargeInFile(const Parsed& file)
{
    const Json& root = file.root;
    const auto said = [&root](std::string_view member) -> const Json* {
        if (!root.is_object()) {
            return nullptr;
        }
        const auto found = root.find(member);
        return found == root.end() ? nullptr : &*found;
    };
    const Json* format = said("format");
    if (format == nullptr || !saysThisFormat(*format)) {
        throw Refusal{ErrorCode::ParseFailure, std::string(kNotACustomisation),
                      "it " + std::string(kTooLarge) + ", " + writtenAt(*file.tooLarge)};
    }
    if (const Json* version = said("version"); version != nullptr && isNewer(*version)) {
        refuseNewer(*version);
    }
    refuseTooLarge(file, {});
}

[[nodiscard]] Customisation readCustomisation(Parsed& file)
{
    const Json& root = file.root;
    const std::string top(kTopLevel);
    Members members(root, top);
    (void)members.find("format"); // read by requireCustomisation
    (void)members.find("version");

    Customisation customisation;
    customisation.name = members.requiredText("name");
    customisation.description = members.text("description");
    if (const Json* notice = members.find("notice")) {
        customisation.notice = linesOf(*notice, top, "notice");
    }
    if (const Json* sources = members.find("sources")) {
        const CustomisationSourceNote d;
        std::size_t index = 0;
        for (const Json& item : listOf(*sources, top, "sources")) {
            Members part(item, entryLabel("sources", index++, item, "name"));
            CustomisationSourceNote source;
            source.name = part.requiredText("name");
            source.definitions = part.flag("definitions", d.definitions);
            source.rules = part.flag("rules", d.rules);
            if (const Json* notice = part.find("notice")) {
                source.notice = linesOf(*notice, part.label(), "notice");
            }
            part.finish();
            customisation.sources.push_back(std::move(source));
        }
    }
    if (const Json* based = members.find("basedOn")) {
        Members part(*based, "basedOn");
        CustomisationBase base;
        base.name = part.requiredText("name");
        base.digest = part.requiredText("digest");
        part.finish();
        customisation.basedOn = std::move(base);
    }
    const Json* colours = members.find("colours");
    if (const Json* linework = members.find("linework")) {
        Members part(*linework, "linework");
        LineworkCodes codes;
        for (const LineworkCodeMember& code : lineworkCodeMembers()) {
            codes.*code.spelling = part.text(code.name, codes.*code.spelling);
        }
        part.finish();
        customisation.linework = std::move(codes);
    }
    if (const Json* automation = members.find("automation")) {
        const CustomisationAutomation d;
        Members part(*automation, "automation");
        CustomisationAutomation value;
        value.codesOnSurveyImport = part.flag("codesOnSurveyImport", d.codesOnSurveyImport);
        value.lineworkOnSurveyImport =
            part.flag("lineworkOnSurveyImport", d.lineworkOnSurveyImport);
        part.finish();
        customisation.automation = value;
    }
    const Json* linestyles = members.find("linestyles");
    const Json* symbols = members.find("symbols");
    const Json* codes = members.find("codes");
    // Before the lists are gone through: a top-level member spelled wrongly
    // ("symbol" for "symbols") is the mistake to report, not whatever its
    // absence makes of the rest.
    members.finish();
    checkHeader(customisation);

    if (colours != nullptr) {
        if (!colours->is_object()) {
            refuse(ErrorCode::ParseFailure, top,
                   "\"colours\" must be an object of names and colours, written {\"name\": "
                   "\"#RRGGBB\"}",
                   shown(*colours));
        }
        for (auto item = colours->begin(); item != colours->end(); ++item) {
            if (item.key().size() > kCustomisationMostTextBytes) {
                refuse(ErrorCode::ParseFailure, "colours",
                       "a colour's name is " + std::string(kTooLong), shown(Json(item.key())));
            }
            const std::string where = "colours " + inQuotes(item.key());
            if (!item->is_string()) {
                refuse(ErrorCode::ParseFailure, where,
                       "the colour must be text, written \"#RRGGBB\"", shown(*item));
            }
            const auto colour = Color::fromHex(item->get_ref<const std::string&>());
            if (!colour) {
                refuse(ErrorCode::ParseFailure, where,
                       "the colour is " + shown(*item) +
                           ", which is not #RRGGBB or #RRGGBBAA in hexadecimal");
            }
            if (auto status = customisation.colours.add(item.key(), *colour); !status) {
                refuse(ErrorCode::InvalidArgument, where, status.error().message,
                       status.error().context);
            }
        }
    }

    const auto definitions = [&](const Json* json, std::string_view array, bool symbol) {
        if (json == nullptr) {
            return;
        }
        std::size_t index = 0;
        for (const Json& item : listOf(*json, top, array)) {
            const std::string where = entryLabel(array, index++, item, "name");
            LineStyle definition =
                readDefinition(item, where, symbol, customisation.name, file.strokes);
            // One library holds both lists, so a name is taken once across
            // the two - said here because NamedTable would call it
            // AlreadyExists and not say where the other one is.
            if (const LineStyle* other = customisation.library.find(definition.name)) {
                refuse(ErrorCode::InvalidArgument, where,
                       std::string("the file holds two definitions of this name; the other is "
                                   "in \"") +
                           (other->symbol ? "symbols" : "linestyles") + "\"");
            }
            if (auto status = customisation.library.add(std::move(definition)); !status) {
                refuse(ErrorCode::InvalidArgument, where, status.error().message);
            }
        }
    };
    definitions(linestyles, "linestyles", false);
    definitions(symbols, "symbols", true);

    if (codes != nullptr) {
        std::size_t index = 0;
        for (const Json& item : listOf(*codes, top, "codes")) {
            const std::string where = entryLabel("codes", index++, item, "key");
            SurveyRule rule = readRule(item, where);
            if (auto status = customisation.map.add(std::move(rule)); !status) {
                refuse(ErrorCode::InvalidArgument, where, status.error().message);
            }
        }
    }
    return customisation;
}

// Every refusal comes back as an Error; nothing is thrown past this.
template <typename T, typename Work> [[nodiscard]] Result<T> guarded(Work&& work)
{
    try {
        return work();
    } catch (const Refusal& refusal) {
        return makeError(refusal.code, refusal.message, refusal.context);
    } catch (const Json::exception& error) {
        // Every value is asked its type before it is taken and every text is
        // checked before it is written, so the library has nothing left to
        // object to; if it does, it is this file that is wrong.
        return makeError(ErrorCode::Internal,
                         "the customisation could not be read or written as JSON", error.what());
    }
}

[[nodiscard]] bool sameDefinitions(const StyleLibrary& a, const StyleLibrary& b)
{
    if (a.size() != b.size()) {
        return false;
    }
    bool same = true;
    a.forEach([&](const LineStyle& definition) {
        if (same) {
            const LineStyle* other = b.find(definition.name);
            same = other != nullptr && *other == definition;
        }
    });
    return same;
}

} // namespace

Status validateCustomisationName(std::string_view name)
{
    if (name.empty()) {
        return makeError(ErrorCode::InvalidArgument, "a customisation name cannot be empty");
    }
    if (!isValidUtf8(name)) {
        return makeError(ErrorCode::InvalidArgument, "a customisation name must be valid UTF-8");
    }
    if (name.find_first_of("\n\r/\\") != std::string_view::npos) {
        return makeError(ErrorCode::InvalidArgument,
                         "a customisation name cannot hold a line break, '/' or '\\': a project "
                         "records it as one line, and it is not a path",
                         inQuotes(name));
    }
    // Those three are what the project's store refuses. These two it would
    // take, and are refused because a name is an IDENTITY - a project is
    // matched to its customisation by it - and must be what a person sees:
    // the same rule a layer's name is held to (validateLayerPath).
    if (std::any_of(name.begin(), name.end(), [](char c) {
            const auto byte = static_cast<unsigned char>(c);
            return byte < 0x20 || byte == 0x7F;
        })) {
        return makeError(ErrorCode::InvalidArgument,
                         "a customisation name cannot hold a control character: a tab, an "
                         "escape and the like cannot be seen where the name is shown",
                         inQuotes(name));
    }
    if (name.front() == ' ' || name.back() == ' ') {
        return makeError(ErrorCode::InvalidArgument,
                         "a customisation name cannot begin or end with a blank: it would look "
                         "like the name without it, and be another",
                         inQuotes(name));
    }
    return {};
}

bool operator==(const Customisation& a, const Customisation& b)
{
    return a.name == b.name && a.description == b.description && a.notice == b.notice &&
           a.sources == b.sources && a.basedOn == b.basedOn && a.colours == b.colours &&
           a.linework == b.linework && a.automation == b.automation &&
           sameDefinitions(a.library, b.library) && a.map == b.map;
}

Result<Customisation> customisationFromJson(std::string_view text)
{
    return guarded<Customisation>([&] {
        Parsed file = parseJson(text, kNotACustomisation, TextKind::File);
        if (file.tooLarge) {
            refuseTooLargeInFile(file);
        }
        requireCustomisation(file.root);
        if (file.tooManyRules) {
            // What the text said before the rule that was one too many: its
            // "format" and "version" came first in any file Katana wrote, and
            // are asked as for any other, so a file of another kind or a newer
            // one is told that.
            refuse(ErrorCode::ParseFailure, kTopLevel, kTooManyRules);
        }
        refuseDuplicate(file, {});
        return readCustomisation(file);
    });
}

Result<std::string> customisationToJson(const Customisation& customisation,
                                        const CustomisationWriteOptions& options)
{
    return guarded<std::string>([&] { return written(customisation, options); });
}

Result<std::string> definitionToJson(const LineStyle& definition, std::string_view customisation)
{
    return guarded<std::string>([&] {
        Out out;
        out.where = "definition " + inQuotes(definition.name);
        writeDefinition(out, definition, customisation, "");
        return out.take();
    });
}

Result<LineStyle> definitionFromJson(std::string_view text, bool symbol,
                                     std::string_view customisation)
{
    return guarded<LineStyle>([&] {
        Parsed file = parseJson(text, kNotADefinition, TextKind::Definition);
        if (!file.root.is_object()) {
            throw Refusal{ErrorCode::ParseFailure, std::string(kNotADefinition),
                          file.tooLarge
                              ? "it " + std::string(kTooLarge) + ", " + writtenAt(*file.tooLarge)
                              : std::string("the text is JSON, but not an object")};
        }
        std::string where = "definition";
        if (const auto name = file.root.find("name");
            name != file.root.end() && name->is_string()) {
            where += " " + inQuotes(name->get_ref<const std::string&>());
        }
        if (file.tooLarge) {
            refuseTooLarge(file, where);
        }
        refuseDuplicate(file, where);
        LineStyle definition =
            readDefinition(file.root, where, symbol, customisation, file.strokes);
        // In a file the library does this as the definition is added to it.
        if (auto status = validate(definition); !status) {
            refuse(ErrorCode::InvalidArgument, where, status.error().message);
        }
        return definition;
    });
}

std::string customisationDigest(std::string_view bytes)
{
    // FNV-1a, 64-bit (Fowler, Noll and Vo): the hash a customisation's earlier
    // names are already known by in the cad layer, restated because entity may
    // not see cad. Checked against the algorithm's published vectors.
    std::uint64_t hash = 0xcbf29ce484222325ULL; // the offset basis
    for (const char byte : bytes) {
        hash ^= static_cast<unsigned char>(byte);
        hash *= 0x100000001b3ULL; // the prime
    }
    constexpr std::string_view kDigits = "0123456789abcdef";
    std::string digest(16, '0');
    for (std::size_t i = digest.size(); i-- > 0;) {
        digest[i] = kDigits[hash & 0xF];
        hash >>= 4;
    }
    return digest;
}

} // namespace katana::entity
