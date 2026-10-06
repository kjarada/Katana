#include "katana/archive12d/style_library.hpp"

#include <algorithm>
#include <array>
#include <map>
#include <string>
#include <utility>

#include "katana/archive12d/reader.hpp"
#include "lexer.hpp"
#include "text_utilities.hpp"

namespace katana::archive12d {

namespace {

using detail::Lexer;
using detail::Token;
using detail::TokenKind;
using katana::core::ErrorCode;
using katana::core::makeError;
using katana::entity::LineStyle;
using katana::entity::Stroke;
using katana::entity::StrokeOp;
using katana::entity::StrokeText;

// A damaged file can produce one warning per line, and a list nobody can read
// is not a report. Past this the rest are counted - the same rule the Archive
// reader follows, and for the same reason.
constexpr std::size_t kMaxWarnings = 100;

// How many values each command takes. An unknown command has no entry, and is
// skipped by taking what is left of its LINE - which is how these files are
// written, one command per line, so a command this does not know costs only
// itself.
struct Command {
    std::string_view keyword;
    std::size_t arity;
};

constexpr std::array<Command, 19> kCommands{{
    {"move", 2},
    {"draw", 2},
    {"arc", 3},
    {"circle", 1},
    {"dot", 1},
    {"text", 9},
    {"colour", 1},
    {"group", 1},
    {"mode", 1},
    {"length", 1},
    {"factor", 1},
    {"xorigin", 1},
    {"yorigin", 1},
    {"xorigin1", 1},
    {"yorigin1", 1},
    {"xorigin2", 1},
    {"yorigin2", 1},
    {"stretch_mode", 1},
    {"cycle_mode", 1},
}};

[[nodiscard]] const Command* commandFor(std::string_view keyword)
{
    const auto found = std::find_if(kCommands.begin(), kCommands.end(),
                                    [keyword](const Command& c) { return c.keyword == keyword; });
    return found == kCommands.end() ? nullptr : &*found;
}

class LibraryReader {
  public:
    LibraryReader(katana::entity::StyleLibrary library, std::string_view text,
                  std::string source)
        : lexer_(text), source_(std::move(source)),
          // A style library holds one kind a file and says which only by its
          // name: symbol libraries are conventionally named *symbols*.4d. The
          // same test, folded the same way, that the catalogue makes of
          // LineStyle::source - here it becomes the definition's own flag.
          symbol_(detail::lowered(source_).find("symbol") != std::string::npos)
    {
        result_.library = std::move(library);
    }

    katana::core::Result<StyleLibraryRead> run()
    {
        while (true) {
            const Token& token = lexer_.peek();
            if (token.kind == TokenKind::End) {
                break;
            }
            if (token.kind != TokenKind::Word) {
                // A stray `}` or a quoted text where a definition should
                // start. Report where, drop it, and carry on: one stray token
                // must not cost the rest of the file.
                warn("line " + std::to_string(token.line) + ": expected a definition, found " +
                     describe(token));
                lexer_.next();
                continue;
            }
            if (!readDefinition()) {
                break;
            }
        }
        if (lexer_.unterminatedQuote()) {
            return makeError(ErrorCode::InvalidArgument, "a quoted text is never closed",
                             "line " + std::to_string(lexer_.unterminatedQuoteLine()));
        }
        for (const std::string_view comment : lexer_.comments) {
            result_.comments.emplace_back(detail::trimmed(comment));
        }
        finishWarnings();
        return std::move(result_);
    }

  private:
    [[nodiscard]] static std::string describe(const Token& token)
    {
        switch (token.kind) {
        case TokenKind::Word:
        case TokenKind::Quoted:
            return "\"" + token.text() + "\"";
        case TokenKind::Open:
            return "{";
        case TokenKind::Close:
            return "}";
        case TokenKind::End:
            break;
        }
        return "the end of the file";
    }

    void warn(std::string text)
    {
        if (result_.warnings.size() < kMaxWarnings) {
            result_.warnings.push_back(std::move(text));
            return;
        }
        ++suppressed_;
    }

    void finishWarnings()
    {
        if (suppressed_ != 0) {
            result_.warnings.push_back(std::to_string(suppressed_) + " further warnings not shown");
        }
    }

    void count(std::string_view keyword, bool kept)
    {
        const auto found =
            std::find_if(result_.tally.begin(), result_.tally.end(),
                         [keyword](const ElementTally& t) { return t.keyword == keyword; });
        ElementTally& tally =
            found == result_.tally.end()
                ? result_.tally.emplace_back(ElementTally{std::string(keyword), 0, 0})
                : *found;
        ++tally.read;
        tally.imported += kept ? 1 : 0;
    }

    // Skips to the end of the block this is inside, so a definition that
    // cannot be understood costs only itself.
    void skipBlock(std::size_t depthOutside)
    {
        while (lexer_.depth() > depthOutside) {
            const Token token = lexer_.next();
            if (token.kind == TokenKind::End) {
                return;
            }
        }
    }

    // True to carry on reading the file, false when it has ended.
    bool readDefinition()
    {
        const Token kindToken = lexer_.next();
        const std::string kindWord = kindToken.text();
        const auto units = katana::entity::parseStyleUnits(kindWord);

        const Token nameToken = lexer_.next();
        if (nameToken.kind != TokenKind::Quoted) {
            warn("line " + std::to_string(kindToken.line) + ": `" + kindWord +
                 "` is not followed by a name in quotes");
            count(kindWord, false);
            return nameToken.kind != TokenKind::End;
        }
        const std::size_t depthOutside = lexer_.depth();
        const Token open = lexer_.next();
        if (open.kind != TokenKind::Open) {
            warn("line " + std::to_string(kindToken.line) + ": definition \"" + nameToken.text() +
                 "\" has no `{`");
            count(kindWord, false);
            return open.kind != TokenKind::End;
        }
        if (!units) {
            warn("line " + std::to_string(kindToken.line) + ": \"" + nameToken.text() +
                 "\" is a `" + kindWord + "`, which is not a kind of style this reads");
            count(kindWord, false);
            skipBlock(depthOutside);
            return true;
        }

        LineStyle style;
        style.name = nameToken.text();
        style.units = *units;
        style.source = source_;
        style.symbol = symbol_;
        lexer_.collectComments = false; // the header is read; the rest is licence text
        readBody(style, depthOutside);

        if (auto status = katana::entity::validate(style); !status) {
            warn("definition \"" + style.name + "\" skipped: " + status.error().describe());
            count(kindWord, false);
            return true;
        }
        auto replaced = katana::entity::addOrReplace(result_.library, std::move(style));
        if (!replaced) {
            warn("definition \"" + nameToken.text() +
                 "\" skipped: " + replaced.error().describe());
            count(kindWord, false);
            return true;
        }
        result_.replaced += *replaced ? 1 : 0;
        count(kindWord, true);
        return true;
    }

    void readBody(LineStyle& style, std::size_t depthOutside)
    {
        while (lexer_.depth() > depthOutside) {
            const Token token = lexer_.next();
            if (token.kind == TokenKind::End || token.kind == TokenKind::Close) {
                return;
            }
            if (token.kind != TokenKind::Word) {
                warn("line " + std::to_string(token.line) + ": definition \"" + style.name +
                     "\" has " + describe(token) + " where a command should be");
                continue;
            }
            const std::string keyword = token.text();
            const Command* command = commandFor(keyword);
            std::vector<Token> arguments;
            if (command != nullptr) {
                for (std::size_t i = 0; i < command->arity; ++i) {
                    if (!lexer_.peek().isValue()) {
                        break;
                    }
                    arguments.push_back(lexer_.next());
                }
                if (arguments.size() != command->arity) {
                    warn("line " + std::to_string(token.line) + ": `" + keyword + "` in \"" +
                         style.name + "\" wants " + std::to_string(command->arity) +
                         " values and has " + std::to_string(arguments.size()));
                    count(keyword, false);
                    continue;
                }
            } else {
                // Unknown: take what is left of the line, so the command is
                // dropped whole rather than leaving its values to be read as
                // commands of their own.
                while (lexer_.peek().isValue() && lexer_.peek().line == token.line) {
                    arguments.push_back(lexer_.next());
                }
                warn("line " + std::to_string(token.line) + ": `" + keyword + "` in \"" +
                     style.name + "\" is not a command this reads");
                count(keyword, false);
                continue;
            }
            count(keyword, apply(style, keyword, arguments, token.line));
        }
    }

    // A value as a number, warning where it is not one.
    [[nodiscard]] std::optional<double> real(const Token& token, std::string_view keyword,
                                             const LineStyle& style)
    {
        const auto value = parseReal(token.raw);
        if (!value) {
            warn("line " + std::to_string(token.line) + ": `" + std::string(keyword) + "` in \"" +
                 style.name + "\" has \"" + token.text() + "\" where a number should be");
        }
        return value;
    }

    bool apply(LineStyle& style, const std::string& keyword, const std::vector<Token>& arguments,
               std::uint32_t line)
    {
        const auto number = [&](std::size_t i) { return real(arguments[i], keyword, style); };

        if (keyword == "group") {
            style.group = arguments[0].text();
            return true;
        }
        if (keyword == "mode") {
            const std::string mode = detail::lowered(arguments[0].raw);
            if (mode != "vertex") {
                // The only mode either library uses. Anything else changes
                // where the strokes go, so it is reported rather than ignored.
                warn("line " + std::to_string(line) + ": \"" + style.name + "\" is `mode " + mode +
                     "`, which is not a mode this reads");
                return false;
            }
            style.atVertices = true;
            return true;
        }
        if (keyword == "length" || keyword == "factor" || keyword == "xorigin" ||
            keyword == "yorigin" || keyword == "xorigin1" || keyword == "yorigin1" ||
            keyword == "xorigin2" || keyword == "yorigin2") {
            const auto value = number(0);
            if (!value) {
                return false;
            }
            if (keyword == "length") {
                style.length = *value;
            } else if (keyword == "factor") {
                style.factor = *value;
            } else if (keyword == "xorigin") {
                style.origin.x = *value;
            } else if (keyword == "yorigin") {
                style.origin.y = *value;
            } else if (keyword == "xorigin1") {
                style.anchor1.x = *value;
            } else if (keyword == "yorigin1") {
                style.anchor1.y = *value;
            } else if (keyword == "xorigin2") {
                style.anchor2.x = *value;
            } else {
                style.anchor2.y = *value;
            }
            return true;
        }
        if (keyword == "stretch_mode" || keyword == "cycle_mode") {
            const auto value = detail::parseInteger(arguments[0].raw);
            if (!value) {
                warn("line " + std::to_string(line) + ": `" + keyword + "` in \"" + style.name +
                     "\" has \"" + arguments[0].text() + "\" where a whole number should be");
                return false;
            }
            (keyword == "stretch_mode" ? style.stretchMode : style.cycleMode) =
                static_cast<int>(*value);
            return true;
        }
        if (keyword == "move" || keyword == "draw") {
            const auto x = number(0);
            const auto y = number(1);
            if (!x || !y) {
                return false;
            }
            style.strokes.push_back(
                Stroke{keyword == "move" ? StrokeOp::Move : StrokeOp::Draw, {*x, *y}});
            return true;
        }
        if (keyword == "circle" || keyword == "dot") {
            const auto radius = number(0);
            if (!radius) {
                return false;
            }
            Stroke stroke;
            stroke.op = keyword == "circle" ? StrokeOp::Circle : StrokeOp::Dot;
            stroke.radius = *radius;
            style.strokes.push_back(stroke);
            return true;
        }
        if (keyword == "arc") {
            const auto radius = number(0);
            const auto start = number(1);
            const auto end = number(2);
            if (!radius || !start || !end) {
                return false;
            }
            Stroke stroke;
            stroke.op = StrokeOp::Arc;
            stroke.radius = *radius;
            stroke.startAngle = *start;
            stroke.endAngle = *end;
            style.strokes.push_back(stroke);
            return true;
        }
        if (keyword == "colour") {
            Stroke stroke;
            stroke.op = StrokeOp::Pen;
            stroke.pen = arguments[0].text();
            style.strokes.push_back(stroke);
            return true;
        }
        if (keyword == "text") {
            StrokeText text;
            text.text = arguments[0].text();
            const auto angle = number(1);
            const auto height = number(2);
            const auto widthFactor = number(5);
            if (!angle || !height || !widthFactor) {
                return false;
            }
            text.angle = *angle;
            text.height = *height;
            text.justify = arguments[3].text();
            text.font = arguments[4].text();
            text.widthFactor = *widthFactor;
            for (std::size_t i = 0; i < text.unnamed.size(); ++i) {
                const auto value = number(6 + i);
                if (!value) {
                    return false;
                }
                text.unnamed[i] = *value;
            }
            Stroke stroke;
            stroke.op = StrokeOp::Text;
            stroke.text = style.texts.size();
            style.texts.push_back(std::move(text));
            style.strokes.push_back(stroke);
            return true;
        }
        return false;
    }

    Lexer lexer_;
    std::string source_;
    bool symbol_ = false; // the file's name says it is a symbol library
    StyleLibraryRead result_{};
    std::size_t suppressed_ = 0;
};

} // namespace

std::string sourceFileName(std::string_view source)
{
    // Both separators, whatever the platform: a name recorded on Windows is
    // read on Linux, and std::filesystem there would keep "C:\x\y.4d" whole.
    const std::size_t slash = source.find_last_of("/\\");
    return std::string(slash == std::string_view::npos ? source : source.substr(slash + 1));
}

katana::core::Result<StyleLibraryRead> readStyleLibrary(std::string_view text,
                                                        std::string_view sourceName)
{
    return readStyleLibraryInto({}, text, sourceName);
}

katana::core::Result<StyleLibraryRead> readStyleLibraryInto(katana::entity::StyleLibrary library,
                                                            std::string_view text,
                                                            std::string_view sourceName)
{
    LibraryReader reader(std::move(library), text, sourceFileName(sourceName));
    return reader.run();
}

} // namespace katana::archive12d
