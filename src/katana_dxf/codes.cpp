#include "katana/dxf/codes.hpp"

#include <array>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <limits>

namespace katana::dxf {

using katana::entity::Color;

namespace {

Color rgb(int r, int g, int b)
{
    return Color{static_cast<std::uint8_t>(r), static_cast<std::uint8_t>(g),
                 static_cast<std::uint8_t>(b), 255};
}

// Indexed colours 10-249: hue (index - 10) / 10 in steps of 15 degrees, then
// within the hue five brightnesses, each full and then a third saturated.
Color computedHue(int index)
{
    static constexpr std::array<double, 5> kValues{255.0, 189.0, 129.0, 104.0, 79.0};
    const int offset = index - 10;
    const int hue = offset / 10;
    const int step = offset % 10;
    const double value = kValues[static_cast<std::size_t>(step / 2)];
    const double saturation = step % 2 == 0 ? 1.0 : 1.0 / 3.0;
    // The hue's pure components, 0 to 1, by the usual six sectors of 60
    // degrees; 15 degrees is a quarter of a sector, so f is exact.
    const int sector = hue / 4;
    const double f = static_cast<double>(hue % 4) / 4.0;
    double r = 0.0;
    double g = 0.0;
    double b = 0.0;
    switch (sector) {
    case 0: r = 1.0; g = f; b = 0.0; break;
    case 1: r = 1.0 - f; g = 1.0; b = 0.0; break;
    case 2: r = 0.0; g = 1.0; b = f; break;
    case 3: r = 0.0; g = 1.0 - f; b = 1.0; break;
    case 4: r = f; g = 0.0; b = 1.0; break;
    default: r = 1.0; g = 0.0; b = 1.0 - f; break;
    }
    const auto channel = [&](double pure) {
        return static_cast<int>(std::lround(value * (1.0 - saturation * (1.0 - pure))));
    };
    return rgb(channel(r), channel(g), channel(b));
}

const std::array<Color, 256>& table()
{
    static const std::array<Color, 256> colours = [] {
        std::array<Color, 256> out{};
        static constexpr std::array<std::array<int, 3>, 10> kNamed{{{0, 0, 0},
                                                                    {255, 0, 0},
                                                                    {255, 255, 0},
                                                                    {0, 255, 0},
                                                                    {0, 255, 255},
                                                                    {0, 0, 255},
                                                                    {255, 0, 255},
                                                                    {255, 255, 255},
                                                                    {128, 128, 128},
                                                                    {192, 192, 192}}};
        static constexpr std::array<int, 6> kGreys{51, 80, 105, 130, 190, 255};
        for (int i = 1; i < 256; ++i) {
            if (i <= 9) {
                const auto& named = kNamed[static_cast<std::size_t>(i)];
                out[static_cast<std::size_t>(i)] = rgb(named[0], named[1], named[2]);
            } else if (i >= 250) {
                const int grey = kGreys[static_cast<std::size_t>(i - 250)];
                out[static_cast<std::size_t>(i)] = rgb(grey, grey, grey);
            } else {
                out[static_cast<std::size_t>(i)] = computedHue(i);
            }
        }
        return out;
    }();
    return colours;
}

void appendUtf8(std::string& out, char32_t cp)
{
    if (cp >= 0xD800 && cp <= 0xDFFF) {
        cp = 0xFFFD; // a surrogate half is not a character
    }
    if (cp < 0x80) {
        out.push_back(static_cast<char>(cp));
    } else if (cp < 0x800) {
        out.push_back(static_cast<char>(0xC0 | (cp >> 6)));
        out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
    } else if (cp < 0x10000) {
        out.push_back(static_cast<char>(0xE0 | (cp >> 12)));
        out.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
    } else {
        out.push_back(static_cast<char>(0xF0 | (cp >> 18)));
        out.push_back(static_cast<char>(0x80 | ((cp >> 12) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
        out.push_back(static_cast<char>(0x80 | (cp & 0x3F)));
    }
}

int hexDigit(char c)
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

// \U+XXXX at `i` (the backslash): the characters consumed, 0 when it is not
// one, and the character appended.
std::size_t unicodeEscape(std::string_view raw, std::size_t i, std::string& out)
{
    if (i + 7 > raw.size()) {
        return 0;
    }
    if ((raw[i + 1] != 'U' && raw[i + 1] != 'u') || raw[i + 2] != '+') {
        return 0;
    }
    char32_t cp = 0;
    for (std::size_t k = 3; k < 7; ++k) {
        const int digit = hexDigit(raw[i + k]);
        if (digit < 0) {
            return 0;
        }
        cp = cp * 16 + static_cast<char32_t>(digit);
    }
    appendUtf8(out, cp);
    return 7;
}

// %%x at `i`: the characters consumed, 0 when it is not a code.
std::size_t percentCode(std::string_view raw, std::size_t i, std::string& out)
{
    if (i + 2 >= raw.size() || raw[i] != '%' || raw[i + 1] != '%') {
        return 0;
    }
    const char code = raw[i + 2];
    switch (code) {
    case 'd':
    case 'D':
        appendUtf8(out, U'°');
        return 3;
    case 'p':
    case 'P':
        appendUtf8(out, U'±');
        return 3;
    case 'c':
    case 'C':
        appendUtf8(out, U'∅');
        return 3;
    case '%':
        out.push_back('%');
        return 3;
    case 'u':
    case 'U':
    case 'o':
    case 'O':
    case 'k':
    case 'K':
        return 3; // a toggle Katana text has no way to show
    default:
        break;
    }
    if (code >= '0' && code <= '9') {
        std::size_t end = i + 2;
        unsigned value = 0;
        while (end < raw.size() && end < i + 5 && raw[end] >= '0' && raw[end] <= '9') {
            value = value * 10 + static_cast<unsigned>(raw[end] - '0');
            ++end;
        }
        if (value > 0 && value < 256) {
            appendUtf8(out, static_cast<char32_t>(value));
        }
        return end - i;
    }
    return 0;
}

} // namespace

std::optional<Color> indexedColour(int index)
{
    if (index < 1 || index > 255) {
        return std::nullopt;
    }
    return table()[static_cast<std::size_t>(index)];
}

int nearestIndexedColour(const Color& colour)
{
    if (colour.r == 0 && colour.g == 0 && colour.b == 0) {
        return kColourForeground;
    }
    const auto& colours = table();
    int best = kColourForeground;
    long bestDistance = std::numeric_limits<long>::max();
    for (int i = 1; i < 256; ++i) {
        const Color& candidate = colours[static_cast<std::size_t>(i)];
        const long dr = static_cast<long>(candidate.r) - colour.r;
        const long dg = static_cast<long>(candidate.g) - colour.g;
        const long db = static_cast<long>(candidate.b) - colour.b;
        const long distance = dr * dr + dg * dg + db * db;
        if (distance < bestDistance) { // strict: the lower index wins a tie
            bestDistance = distance;
            best = i;
            if (distance == 0) {
                break;
            }
        }
    }
    return best;
}

Color trueColour(long value)
{
    return rgb(static_cast<int>((value >> 16) & 0xFF), static_cast<int>((value >> 8) & 0xFF),
               static_cast<int>(value & 0xFF));
}

int nearestLineweight(double millimetres)
{
    static constexpr std::array<int, 24> kWeights{0,  5,  9,  13, 15, 18,  20,  25,
                                                  30, 35, 40, 50, 53, 60,  70,  80,
                                                  90, 100, 106, 120, 140, 158, 200, 211};
    if (!std::isfinite(millimetres) || millimetres <= 0.0) {
        return 0;
    }
    const double hundredths = millimetres * 100.0;
    int best = kWeights.front();
    double bestDistance = std::numeric_limits<double>::infinity();
    for (const int weight : kWeights) {
        const double distance = std::abs(hundredths - static_cast<double>(weight));
        if (distance < bestDistance) {
            bestDistance = distance;
            best = weight;
        }
    }
    return best;
}

std::string plainText(std::string_view raw)
{
    std::string out;
    out.reserve(raw.size());
    for (std::size_t i = 0; i < raw.size();) {
        const char c = raw[i];
        if (c == '%') {
            if (const std::size_t used = percentCode(raw, i, out); used != 0) {
                i += used;
                continue;
            }
        } else if (c == '\\') {
            if (const std::size_t used = unicodeEscape(raw, i, out); used != 0) {
                i += used;
                continue;
            }
        }
        out.push_back(c);
        ++i;
    }
    return out;
}

std::string plainMText(std::string_view raw)
{
    std::string out;
    out.reserve(raw.size());
    // Skips past the ';' that ends a code's argument; to the end if none does.
    const auto pastSemicolon = [&](std::size_t from) {
        const std::size_t end = raw.find(';', from);
        return end == std::string_view::npos ? raw.size() : end + 1;
    };
    for (std::size_t i = 0; i < raw.size();) {
        const char c = raw[i];
        if (c == '{' || c == '}') {
            ++i;
            continue;
        }
        if (c == '%') {
            if (const std::size_t used = percentCode(raw, i, out); used != 0) {
                i += used;
                continue;
            }
            out.push_back(c);
            ++i;
            continue;
        }
        if (c == '^' && i + 1 < raw.size()) {
            const char next = raw[i + 1];
            if (next == 'J') {
                out.push_back('\n');
                i += 2;
                continue;
            }
            if (next == 'I') {
                out.push_back(' ');
                i += 2;
                continue;
            }
            if (next == 'M') {
                i += 2;
                continue;
            }
            if (next == ' ') {
                out.push_back('^');
                i += 2;
                continue;
            }
        }
        if (c != '\\' || i + 1 >= raw.size()) {
            out.push_back(c);
            ++i;
            continue;
        }
        const char code = raw[i + 1];
        switch (code) {
        case 'P':
        case 'N':
            out.push_back('\n');
            i += 2;
            break;
        case '~':
            appendUtf8(out, U' ');
            i += 2;
            break;
        case '\\':
        case '{':
        case '}':
            out.push_back(code);
            i += 2;
            break;
        case 'L':
        case 'l':
        case 'O':
        case 'o':
        case 'K':
        case 'k':
        case 'X':
            i += 2;
            break;
        case 'S': {
            const std::size_t end = pastSemicolon(i + 2);
            const std::size_t stop = end > i + 2 && end <= raw.size() && raw[end - 1] == ';'
                                         ? end - 1
                                         : end;
            for (std::size_t k = i + 2; k < stop; ++k) {
                const char s = raw[k];
                if (s == '^' || s == '#') {
                    out.push_back('/');
                } else if (s == '\\' && k + 1 < stop) {
                    out.push_back(raw[++k]);
                } else {
                    out.push_back(s);
                }
            }
            i = end;
            break;
        }
        case 'U':
        case 'u':
            if (const std::size_t used = unicodeEscape(raw, i, out); used != 0) {
                i += used;
            } else {
                i += 2;
            }
            break;
        case 'M':
        case 'm':
            // \M+nXXXX: a double-byte character in a named code page. Rare,
            // and not decodable without the page; dropped rather than shown
            // as its code.
            i = std::min(raw.size(), i + 8);
            break;
        case 'f':
        case 'F':
        case 'H':
        case 'h':
        case 'C':
        case 'c':
        case 'W':
        case 'w':
        case 'T':
        case 't':
        case 'Q':
        case 'q':
        case 'A':
        case 'a':
        case 'p':
            i = pastSemicolon(i + 2);
            break;
        default:
            // An escape this does not know: the character without the
            // backslash is the likelier reading than both.
            out.push_back(code);
            i += 2;
            break;
        }
    }
    return out;
}

std::string encodeText(std::string_view utf8)
{
    std::string out;
    out.reserve(utf8.size() + 8);
    static constexpr char kHex[] = "0123456789ABCDEF";
    for (std::size_t i = 0; i < utf8.size();) {
        const auto byte = static_cast<unsigned char>(utf8[i]);
        char32_t cp = 0;
        std::size_t length = 1;
        if (byte < 0x80) {
            cp = byte;
        } else if ((byte & 0xE0) == 0xC0 && i + 1 < utf8.size()) {
            cp = static_cast<char32_t>(((byte & 0x1F) << 6) |
                                       (static_cast<unsigned char>(utf8[i + 1]) & 0x3F));
            length = 2;
        } else if ((byte & 0xF0) == 0xE0 && i + 2 < utf8.size()) {
            cp = static_cast<char32_t>(((byte & 0x0F) << 12) |
                                       ((static_cast<unsigned char>(utf8[i + 1]) & 0x3F) << 6) |
                                       (static_cast<unsigned char>(utf8[i + 2]) & 0x3F));
            length = 3;
        } else if ((byte & 0xF8) == 0xF0 && i + 3 < utf8.size()) {
            cp = 0x10000; // beyond the basic plane: no R2000 escape reaches it
            length = 4;
        } else {
            cp = U'?';
        }
        i += length;
        if (cp < 0x20 || cp == 0x7F) {
            out.push_back(' ');
        } else if (cp == U'%') {
            // A percent sign another follows would start a code on reading.
            const bool another = i < utf8.size() && utf8[i] == '%';
            out += another ? "%%%" : "%";
        } else if (cp < 0x80) {
            out.push_back(static_cast<char>(cp));
        } else if (cp == U'°') {
            out += "%%d";
        } else if (cp == U'±') {
            out += "%%p";
        } else if (cp == U'∅') {
            out += "%%c";
        } else if (cp < 0x10000) {
            out += "\\U+";
            out.push_back(kHex[(cp >> 12) & 0xF]);
            out.push_back(kHex[(cp >> 8) & 0xF]);
            out.push_back(kHex[(cp >> 4) & 0xF]);
            out.push_back(kHex[cp & 0xF]);
        } else {
            out.push_back('?');
        }
    }
    return out;
}

} // namespace katana::dxf
