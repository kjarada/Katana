// Trimble Survey Controller DC (.dc): recognised, and REFUSED with a way out.
//
// Why there is no reader. The DC format is Trimble's own. Its records are one
// per line, each opening with a two-character record type ("00", "10", "77",
// "D9", "E0", ...) and a two-character derivation code ("NM", "F1", ...), the
// same shape as the Sokkia SDR format it grew out of but with Trimble's own
// record numbers and fields. Trimble has not published the record layouts:
// the Survey Controller Reference Manual (version 7.7, part number
// 32968-70-ENG, January 2001, section "File formats") says only that
// "information about the .dc file format is available from the Trimble web
// site ... contact your local Trimble dealer", and no such document is
// available there now. What is public are third parties' partial notes - a
// list of record names without layouts (Traverse PC's "Trimble DC10.70"
// help page) and seven example lines (the Novapoint wiki's "Trimble DC file
// format" page) - which is not enough to know what a field means, only where
// it might be.
//
// A reader built from example lines would be a guess at which 16-character
// field is the zenith angle and which the slope distance, and at what a
// one-character flag between them says. A guess that is wrong reads a
// plausible survey that is wrong, which is worse than no survey. So this
// format is detected - so the person is not told "unrecognised file" - and
// refused with what to do instead: Trimble Access and Trimble Business Center
// both export the same job as JobXML, whose schema IS published and which
// trimble_jobxml.cpp reads.
//
// The refusal lists the record types the file holds, counted, so a person or
// a support request can see what the file contained.
//
// A Sokkia SDR file (a header "00", a derivation code, then "SDR") is NOT
// claimed: a controller can write SDR33 into a file named .dc, and
// sokkia_sdr.cpp reads it. The derivation code is not only NM - Sokkia's
// SDR33 layout allows ED as well - so this probe steps aside for any.

#include <cstddef>
#include <map>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/format.hpp"

namespace katana::surveyio {
namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

constexpr std::string_view kFormatId = "trimble-dc";

bool isRecordTypeChar(char c)
{
    return (c >= '0' && c <= '9') || (c >= 'A' && c <= 'F');
}

bool isDerivationChar(char c)
{
    return (c >= '0' && c <= '9') || (c >= 'A' && c <= 'Z');
}

// Two hexadecimal-looking characters and two upper-case ones: the shape every
// DC record opens with (the Novapoint notes' "00NM", "D9F1", "E0NM").
bool hasRecordShape(std::string_view line)
{
    return line.size() >= 4 && isRecordTypeChar(line[0]) && isRecordTypeChar(line[1]) &&
           isDerivationChar(line[2]) && isDerivationChar(line[3]);
}

FormatSignature probeDc(const ProbeInput& input)
{
    const bool dcExtension = input.extension == "dc";
    if (!looksLikeText(input.bytes)) {
        return ruledOut();
    }
    const std::vector<std::string_view> lines = probeLines(input, 40);
    if (lines.empty()) {
        return ruledOut();
    }
    const std::string_view header = lines.front();
    if (header.starts_with("00") && header.size() >= 7 && header.substr(4).starts_with("SDR")) {
        return ruledOut(); // Sokkia SDR, even when named .dc
    }
    std::size_t shaped = 0;
    std::size_t nonEmpty = 0;
    for (const std::string_view line : lines) {
        if (line.empty()) {
            continue;
        }
        ++nonEmpty;
        if (hasRecordShape(line)) {
            ++shaped;
        }
    }
    const bool allShaped = nonEmpty > 0 && shaped == nonEmpty;
    const bool controllerHeader =
        header.starts_with("00NM") && header.substr(4).starts_with("SC V");
    if (controllerHeader && allShaped) {
        return {dcExtension ? 0.97 : 0.92,
                dcExtension ? "extension .dc, header 00NM naming Survey Controller, every record "
                              "opens with a record type and derivation code"
                            : "header 00NM naming Survey Controller, every record opens with a "
                              "record type and derivation code"};
    }
    if (header.starts_with("00NM") && allShaped && dcExtension) {
        return {0.8, "extension .dc, header record 00NM, every record opens with a record type"};
    }
    if (dcExtension && allShaped) {
        return {0.5, "extension .dc and every record opens with a record type, but no 00NM header"};
    }
    if (dcExtension) {
        return {0.1, "extension .dc only"};
    }
    return ruledOut();
}

Result<ReadResult> readDc(std::string_view bytes, std::string_view fileName, const ReadOptions&)
{
    // Count record types, so the refusal says what the file holds.
    std::map<std::string, std::size_t> types;
    std::size_t lines = 0;
    std::size_t start = 0;
    while (start < bytes.size()) {
        std::size_t end = bytes.find_first_of("\r\n", start);
        if (end == std::string_view::npos) {
            end = bytes.size();
        }
        const std::string_view line = bytes.substr(start, end - start);
        if (!line.empty()) {
            ++lines;
            ++types[hasRecordShape(line) ? std::string(line.substr(0, 2)) : std::string("??")];
        }
        start = end + 1;
    }
    std::string context = std::to_string(lines) + " record(s):";
    std::size_t listed = 0;
    for (const auto& [type, count] : types) {
        if (++listed > 48) {
            context += " ...";
            break;
        }
        context += " " + type + " x" + std::to_string(count);
    }
    return makeError(ErrorCode::Unsupported,
                     std::string(fileName) +
                         " is a Trimble Survey Controller DC file. Trimble has not published the "
                         "layout of its records, so Katana does not guess at them. Export the job "
                         "as JobXML (.jxl) from Trimble Access or Trimble Business Center and "
                         "import that file instead.",
                     std::move(context));
}

FormatDescriptor descriptor()
{
    FormatDescriptor format;
    format.id = std::string(kFormatId);
    format.humanName = "Trimble Survey Controller DC (recognised only: export JobXML)";
    format.manufacturer = Manufacturer::Trimble;
    // Nothing: the reader refuses every file, and says so before an import
    // is attempted through this empty list.
    format.reads = {};
    // The registry has no "recognised but refused" state: a format must import
    // or export to be registered, and one that imports must have a reader to
    // refuse through. So canImport is true and the reader is the refusal.
    format.canImport = true;
    format.canExport = false;
    format.parserVersion = "1.0";
    format.extensions = {"dc"};
    return format;
}

const FormatRegistration kRegistration{descriptor(), &probeDc, &readDc};

} // namespace
} // namespace katana::surveyio
