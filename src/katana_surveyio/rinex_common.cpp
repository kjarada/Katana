// Column, text and packing helpers shared by the RINEX observation reader and
// the navigation summary (rinex_internal.hpp says why they are shared).

#include <array>
#include <charconv>
#include <cmath>
#include <cstdint>

#include "katana/core/text.hpp"
#include "katana/core/text_encoding.hpp"
#include "rinex_internal.hpp"

namespace katana::surveyio::rinex {

std::string_view headerLabel(std::string_view line)
{
    if (line.size() <= 60) {
        return {};
    }
    std::string_view label = line.substr(60, 20);
    while (!label.empty() && (label.back() == ' ' || label.back() == '\t')) {
        label.remove_suffix(1);
    }
    return label;
}

std::optional<long long> integerField(std::string_view field)
{
    const std::string_view text = katana::core::trimmed(field);
    if (text.empty()) {
        return std::nullopt;
    }
    const std::optional<std::int64_t> value = katana::core::parseInteger(text);
    if (!value) {
        return std::nullopt;
    }
    return static_cast<long long>(*value);
}

std::optional<double> realField(std::string_view field)
{
    const std::string_view text = katana::core::trimmed(field);
    if (text.empty()) {
        return std::nullopt;
    }
    // Fortran writes double precision exponents with a D ("0.123D-04"). A
    // field is at most a few dozen characters; anything longer is not a number.
    if (text.find_first_of("Dd") != std::string_view::npos) {
        std::array<char, 64> buffer{};
        if (text.size() >= buffer.size()) {
            return std::nullopt;
        }
        for (std::size_t i = 0; i < text.size(); ++i) {
            buffer[i] = (text[i] == 'D' || text[i] == 'd') ? 'E' : text[i];
        }
        return katana::core::parseFiniteDouble(std::string_view(buffer.data(), text.size()));
    }
    return katana::core::parseFiniteDouble(text);
}

std::string cleanText(std::string_view text, bool* changed)
{
    const std::string_view kept = katana::core::trimmed(text);
    std::string clean(kept);
    bool altered = false;
    for (char& c : clean) {
        const auto byte = static_cast<unsigned char>(c);
        if (byte < 0x20 || byte == 0x7F) {
            c = ' ';
            altered = true;
        }
    }
    if (!katana::core::isValidUtf8(clean)) {
        for (char& c : clean) {
            if (static_cast<unsigned char>(c) >= 0x80) {
                c = '?';
            }
        }
        altered = true;
    }
    if (changed != nullptr && altered) {
        *changed = true;
    }
    return clean;
}

std::vector<std::string_view> tokens(std::string_view text)
{
    std::vector<std::string_view> found;
    std::size_t at = 0;
    while (at < text.size()) {
        while (at < text.size() && (text[at] == ' ' || text[at] == '\t')) {
            ++at;
        }
        const std::size_t start = at;
        while (at < text.size() && text[at] != ' ' && text[at] != '\t') {
            ++at;
        }
        if (at > start) {
            found.push_back(text.substr(start, at - start));
        }
    }
    return found;
}

const char* systemName(char letter)
{
    // RINEX 3.05 section 3.5 (satellite numbers) and Table A2 (RINEX VERSION /
    // TYPE); 'T' is the Transit system of RINEX 2.10, long gone but still a
    // letter an old file may carry.
    switch (letter) {
    case 'G':
        return "GPS";
    case 'R':
        return "GLONASS";
    case 'E':
        return "Galileo";
    case 'C':
        return "BeiDou";
    case 'J':
        return "QZSS";
    case 'I':
        return "NavIC";
    case 'S':
        return "SBAS";
    case 'T':
        return "Transit";
    default:
        return nullptr;
    }
}

std::optional<VersionRecord> versionRecord(std::string_view firstLine)
{
    if (headerLabel(firstLine) != "RINEX VERSION / TYPE") {
        return std::nullopt;
    }
    VersionRecord record;
    const std::string_view versionField = columns(firstLine, 1, 9);
    record.versionText = std::string(katana::core::trimmed(versionField));
    const std::optional<double> version = realField(versionField);
    if (!version || *version < 1.0 || *version >= 100.0) {
        return std::nullopt;
    }
    record.version = *version;
    record.major = static_cast<int>(std::floor(*version));
    record.fileType = firstLine.size() > 20 ? firstLine[20] : ' ';
    record.system = firstLine.size() > 40 ? firstLine[40] : ' ';
    record.typeText = cleanText(columns(firstLine, 21, 20));
    return record;
}

Packing packingOf(std::string_view bytes)
{
    const auto byteAt = [&](std::size_t i) {
        return i < bytes.size() ? static_cast<unsigned char>(bytes[i]) : 0U;
    };
    if (byteAt(0) == 0x1F && byteAt(1) == 0x8B) {
        return Packing::Gzip;
    }
    if (byteAt(0) == 0x1F && byteAt(1) == 0x9D) {
        return Packing::UnixCompress;
    }
    if (bytes.starts_with("BZh")) {
        return Packing::Bzip2;
    }
    if (bytes.starts_with(std::string_view("PK\x03\x04", 4))) {
        return Packing::Zip;
    }
    // Compact RINEX puts its own version record first ("1.0" or "3.0" in
    // columns 1-20, "COMPACT RINEX FORMAT" in 21-40); the RINEX header it
    // wraps follows from the third line.
    const std::size_t end = bytes.find('\n');
    std::string_view first = bytes.substr(0, end);
    if (!first.empty() && first.back() == '\r') {
        first.remove_suffix(1);
    }
    if (headerLabel(first) == "CRINEX VERS   / TYPE" ||
        first.find("COMPACT RINEX FORMAT") != std::string_view::npos) {
        return Packing::Hatanaka;
    }
    return Packing::Plain;
}

std::string packingRefusal(Packing packing, std::string_view fileName)
{
    const std::string name(fileName);
    switch (packing) {
    case Packing::Gzip:
        return name + " is gzip-compressed. Decompress it first (7-Zip, or gzip -d) and import "
                      "the RINEX file inside it.";
    case Packing::UnixCompress:
        return name + " is Unix-compressed (.Z). Decompress it first (7-Zip, or gzip -d) and "
                      "import the RINEX file inside it.";
    case Packing::Bzip2:
        return name + " is bzip2-compressed. Decompress it first (7-Zip, or bzip2 -d) and "
                      "import the RINEX file inside it.";
    case Packing::Zip:
        return name + " is a zip archive. Extract it first and import the RINEX observation "
                      "file inside it.";
    case Packing::Hatanaka:
        return name + " is Hatanaka-compressed (Compact RINEX, .crx or .YYd). Katana reads plain "
                      "RINEX: convert it with CRX2RNX (the free tool from the Geospatial "
                      "Information Authority of Japan) and import the .rnx or .YYo file it "
                      "writes.";
    case Packing::Plain:
        break;
    }
    return name + " is not compressed.";
}

namespace {

bool isDigit(char c)
{
    return c >= '0' && c <= '9';
}

bool equalsIgnoringCase(std::string_view a, std::string_view b)
{
    return katana::core::equalsIgnoringCase(a, b);
}

} // namespace

std::vector<std::string> navigationFileCandidates(std::string_view observationFileName)
{
    std::vector<std::string> names;
    const std::size_t dot = observationFileName.rfind('.');
    if (dot == std::string_view::npos || dot == 0) {
        return names;
    }
    const std::string_view stem = observationFileName.substr(0, dot);
    const std::string_view extension = observationFileName.substr(dot + 1);

    // Short names (RINEX 2, and RINEX 3 files named the old way): ssssdddf.yyo
    // beside ssssdddf.yyn (GPS), .yyg (GLONASS), .yyl (Galileo), .yyh (SBAS)
    // and .yyp (mixed, the RINEX 3 short-name convention).
    if (extension.size() == 3 && isDigit(extension[0]) && isDigit(extension[1]) &&
        (extension[2] == 'o' || extension[2] == 'O')) {
        const bool upper = extension[2] == 'O';
        for (const char kind : std::string_view("nglhp")) {
            std::string name(stem);
            name += '.';
            name += extension.substr(0, 2);
            name += upper ? static_cast<char>(kind - 'a' + 'A') : kind;
            names.push_back(std::move(name));
        }
        return names;
    }

    // Long names (RINEX 3 and 4, Table A1): SSSSMRCCC_K_YYYYDDDHHMM_PPU_FRU_xO.rnx
    // beside SSSSMRCCC_K_YYYYDDDHHMM_PPU_xN.rnx - a navigation file has no
    // observation frequency part.
    if (equalsIgnoringCase(extension, "rnx")) {
        std::vector<std::string_view> parts;
        std::size_t start = 0;
        while (true) {
            const std::size_t cut = stem.find('_', start);
            parts.push_back(stem.substr(start, cut - start));
            if (cut == std::string_view::npos) {
                break;
            }
            start = cut + 1;
        }
        if (parts.size() == 6 && parts[5].size() == 2 &&
            (parts[5][1] == 'O' || parts[5][1] == 'o')) {
            std::string base;
            for (std::size_t i = 0; i < 4; ++i) {
                base += parts[i];
                base += '_';
            }
            for (const char system : std::string_view("MGRECJIS")) {
                names.push_back(base + system + "N." + std::string(extension));
            }
            return names;
        }
    }

    // Anything else: the name converters such as RTKLIB's give a mixed
    // navigation file written beside an observation file.
    names.push_back(std::string(stem) + ".nav");
    return names;
}

} // namespace katana::surveyio::rinex
