// entity::StyleLibrary -> `.4d` text. The inverse of style_library.cpp, and
// tested as one: every field the reader keeps is written, so a library read,
// written and read again is the library it was.

#include "style_library.hpp"

#include <algorithm>
#include <charconv>
#include <string>
#include <system_error>

#include "customisation_words.hpp"

namespace katana::archive12d {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::entity::LineStyle;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::entity::StyleUnits;

// The shortest PLAIN decimal that reads back as exactly `value`. Fixed rather
// than general (core::formatExactReal) because general writes 0.00001 as
// "1e-05", and neither 12d's own files nor anything available here say 12d
// reads an exponent. to_chars is locale-independent, as a writer must be.
[[nodiscard]] std::string number(double value)
{
    // A double's fixed form is at most 309 integer digits and 1,074 decimal
    // ones, and the shortest round-trip form is far shorter than that.
    char buffer[1100];
    const auto written =
        std::to_chars(buffer, buffer + sizeof buffer, value, std::chars_format::fixed);
    if (written.ec != std::errc{}) {
        return "0"; // unreachable for a finite value; validate() refused the rest
    }
    return {buffer, written.ptr};
}

// A quoted text, with the only two escapes the format has (manual 1.1).
[[nodiscard]] std::string quoted(std::string_view text)
{
    std::string out = "\"";
    for (const char ch : text) {
        if (ch == '"' || ch == '\\') {
            out += '\\';
        }
        out += ch;
    }
    out += '"';
    return out;
}

void line(std::string& out, std::string_view text)
{
    out += "  ";
    out += text;
    out += '\n';
}

void writeStroke(std::string& out, const LineStyle& style, const Stroke& stroke)
{
    switch (stroke.op) {
    case StrokeOp::Move:
    case StrokeOp::Draw:
        line(out, std::string(stroke.op == StrokeOp::Move ? "move " : "draw ") +
                      number(stroke.point.x) + " " + number(stroke.point.y));
        return;
    case StrokeOp::Arc:
        // The radius keeps its sign: 12d draws a negative radius on the other
        // side, and the reader keeps it as written.
        line(out, "arc " + number(stroke.radius) + " " + number(stroke.startAngle) + " " +
                      number(stroke.endAngle));
        return;
    case StrokeOp::Circle:
        line(out, "circle " + number(stroke.radius));
        return;
    case StrokeOp::Dot:
        line(out, "dot " + number(stroke.radius));
        return;
    case StrokeOp::Pen:
        line(out, "colour " + quoted(stroke.pen));
        return;
    case StrokeOp::Text: {
        // validate() has already checked the index.
        const auto& text = style.texts[stroke.text];
        line(out, "text " + quoted(text.text) + " " + number(text.angle) + " " +
                      number(text.height) + " " + quoted(text.justify) + " " + quoted(text.font) +
                      " " + number(text.widthFactor) + " " + number(text.unnamed[0]) + " " +
                      number(text.unnamed[1]) + " " + number(text.unnamed[2]));
        return;
    }
    }
}

void writeDefinition(std::string& out, const LineStyle& style)
{
    out += detail::styleKindWord(style.units);
    out += ' ';
    out += quoted(style.name);
    out += " {\n";
    // Only what differs from what the reader assumes when a command is
    // absent, so a definition reads as it was written by hand - except a
    // twoptstyle's anchors and modes, which say how it is stretched and which
    // 12d's own files always carry.
    const bool twoPoint = style.units == StyleUnits::TwoPoint;
    if (!style.group.empty()) {
        line(out, "group " + quoted(style.group));
    }
    if (style.atVertices) {
        line(out, "mode vertex");
    }
    if (style.length != 0.0) {
        line(out, "length " + number(style.length));
    }
    if (style.factor != 1.0) {
        line(out, "factor " + number(style.factor));
    }
    if (style.origin.x != 0.0) {
        line(out, "xorigin " + number(style.origin.x));
    }
    if (style.origin.y != 0.0) {
        line(out, "yorigin " + number(style.origin.y));
    }
    if (twoPoint || style.anchor1.x != 0.0 || style.anchor1.y != 0.0 || style.anchor2.x != 0.0 ||
        style.anchor2.y != 0.0) {
        line(out, "xorigin1 " + number(style.anchor1.x));
        line(out, "yorigin1 " + number(style.anchor1.y));
        line(out, "xorigin2 " + number(style.anchor2.x));
        line(out, "yorigin2 " + number(style.anchor2.y));
    }
    if (twoPoint || style.stretchMode != 0) {
        line(out, "stretch_mode " + std::to_string(style.stretchMode));
    }
    if (twoPoint || style.cycleMode != 0) {
        line(out, "cycle_mode " + std::to_string(style.cycleMode));
    }
    if (!style.strokes.empty()) {
        out += '\n';
    }
    for (const Stroke& stroke : style.strokes) {
        writeStroke(out, style, stroke);
    }
    out += "}\n";
}

} // namespace

katana::core::Result<std::string> writeStyleLibrary(const katana::entity::StyleLibrary& library,
                                                    const StyleLibraryWriteOptions& options)
{
    for (const std::string& name : options.names) {
        if (!library.contains(name)) {
            return makeError(ErrorCode::NotFound, "the library has no definition of this name",
                             name);
        }
    }
    std::string out;
    for (const std::string& comment : options.comments) {
        if (comment.find_first_of("\r\n") != std::string::npos) {
            return makeError(ErrorCode::InvalidArgument,
                             "a comment line holds a line break, and what follows it would be read "
                             "as definitions",
                             comment);
        }
        out += comment.empty() ? "//\n" : "// " + comment + "\n";
    }
    if (!options.comments.empty()) {
        out += '\n';
    }

    const auto wanted = [&options](const std::string& name) {
        return options.names.empty() ||
               std::find(options.names.begin(), options.names.end(), name) != options.names.end();
    };
    katana::core::Status failed;
    bool first = true;
    library.forEach([&](const LineStyle& style) {
        if (!failed || !wanted(style.name)) {
            return;
        }
        // A definition the reader would skip is one this would lose, so it is
        // refused here, where the person exporting can still be told.
        if (auto status = katana::entity::validate(style); !status) {
            failed = status;
            return;
        }
        if (!first) {
            out += '\n';
        }
        first = false;
        writeDefinition(out, style);
    });
    if (!failed) {
        return failed.error();
    }
    return out;
}

} // namespace katana::archive12d
