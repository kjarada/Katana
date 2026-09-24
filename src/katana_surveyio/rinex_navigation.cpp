// A RINEX broadcast navigation file, summarised: which systems it has orbits
// for and how many, whether it carries ionosphere parameters and the leap
// seconds. The orbits themselves are not kept - nothing in the survey model
// uses them until a positioning step does (see rinex.cpp for why there is none
// yet) - but the file is read through, so a navigation file that is not one,
// or is damaged, is said to be so.
//
// Specifications: "RINEX: The Receiver Independent Exchange Format, Version
// 2.11" (W. Gurtner, L. Estey, 2007), tables A3 and A4 (GPS navigation),
// A10 and A11 (GLONASS), A15 and A16 (GEO); "RINEX Version 3.05" (IGS/RTCM
// RINEX WG, I. Romero ed., 1 December 2020), tables A5 to A18; "RINEX Version
// 4.00" (IGS/RTCM RINEX WG, 1 December 2021), section 5.4 and tables A7 on:
// from version 4 every navigation record begins with a "> EPH", "> STO",
// "> EOP" or "> ION" line.

#include <cstdint>

#include "katana/core/text.hpp"
#include "rinex_internal.hpp"

namespace katana::surveyio::rinex {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

namespace {

// A damaged navigation file can produce a warning per line; past this many
// the rest are counted in one closing warning instead of listed.
constexpr std::size_t kMaxNavigationWarnings = 50;

bool isDigitOrBlank(char c)
{
    return (c >= '0' && c <= '9') || c == ' ';
}

// The system a RINEX 2 navigation file holds all its records for, from its
// file type: one file per system in version 2.
const char* versionTwoSystem(char fileType)
{
    switch (fileType) {
    case 'N':
        return "GPS";
    case 'G':
        return "GLONASS";
    case 'H':
        return "SBAS";
    case 'E':
    case 'L':
        return "Galileo";
    default:
        return nullptr;
    }
}

} // namespace

Result<NavigationSummary> summariseNavigation(std::string_view bytes, std::string_view fileName)
{
    const std::string name(fileName);
    if (const Packing packing = packingOf(bytes); packing != Packing::Plain) {
        return makeError(ErrorCode::FileImportFailure, packingRefusal(packing, fileName));
    }
    LineCursor cursor(bytes);
    std::string_view line;
    if (!cursor.next(line)) {
        return makeError(ErrorCode::FileImportFailure, name + " is empty");
    }
    const std::optional<VersionRecord> version = versionRecord(line);
    if (!version) {
        return makeError(ErrorCode::FileImportFailure,
                         name + " is not a RINEX file: its first line is not a RINEX VERSION / "
                                "TYPE record");
    }
    const bool versionTwo = version->major <= 2;
    const char* singleSystem = versionTwo ? versionTwoSystem(version->fileType) : nullptr;
    if (versionTwo ? singleSystem == nullptr : version->fileType != 'N') {
        return makeError(ErrorCode::FileImportFailure,
                         name + " is a RINEX " + version->versionText + " '" + version->typeText +
                             "' file, not a navigation file");
    }

    NavigationSummary summary;
    summary.fileName = name;
    summary.versionText = version->versionText;
    std::size_t suppressed = 0;
    const auto warn = [&](std::size_t record, std::string message) {
        if (summary.warnings.size() < kMaxNavigationWarnings) {
            summary.warnings.push_back(ReadWarning{name, record, std::move(message)});
        } else {
            ++suppressed;
        }
    };

    bool headerEnded = false;
    while (cursor.next(line)) {
        const std::string_view label = headerLabel(line);
        if (label == "END OF HEADER") {
            headerEnded = true;
            break;
        }
        if (label == "ION ALPHA" || label == "ION BETA" || label == "IONOSPHERIC CORR") {
            summary.ionosphereParameters = true;
        } else if (label == "LEAP SECONDS") {
            summary.leapSeconds = cleanText(headerContent(line));
        }
    }
    if (!headerEnded) {
        return makeError(ErrorCode::FileImportFailure,
                         name + " has no END OF HEADER record: the navigation file is cut short "
                                "or is not RINEX");
    }

    while (cursor.next(line)) {
        const std::size_t number = cursor.lineNumber();
        if (line.find_first_not_of(' ') == std::string_view::npos) {
            continue; // a blank line at the end of a file is common and harmless
        }
        if (version->major >= 4) {
            if (line.front() != '>') {
                continue; // an orbit line of the record above
            }
            const std::vector<std::string_view> fields = tokens(line.substr(1));
            if (fields.size() >= 2 && fields[0] == "EPH" && !fields[1].empty() &&
                systemName(fields[1].front()) != nullptr) {
                ++summary.ephemerides[systemName(fields[1].front())];
            } else if (!fields.empty() &&
                       (fields[0] == "STO" || fields[0] == "EOP" || fields[0] == "ION")) {
                ++summary.otherMessages;
                if (fields[0] == "ION") {
                    summary.ionosphereParameters = true;
                }
            } else {
                warn(number,
                     "not a RINEX 4 navigation record: '" + cleanText(line.substr(0, 40)) + "'");
            }
            continue;
        }
        if (versionTwo) {
            // Table A4: a record starts with the PRN in columns 1-2; its
            // broadcast-orbit lines start with three blanks.
            if (line.starts_with("   ")) {
                continue;
            }
            if (line.size() >= 2 && isDigitOrBlank(line[0]) && line[1] >= '0' && line[1] <= '9') {
                ++summary.ephemerides[singleSystem];
            } else {
                warn(number, "neither the start of an ephemeris nor one of its orbit lines");
            }
            continue;
        }
        // Version 3: a record starts with the satellite (A1,I2.2); its orbit
        // lines start with four blanks.
        if (line.starts_with("    ")) {
            continue;
        }
        if (line.size() >= 3 && systemName(line[0]) != nullptr && isDigitOrBlank(line[1]) &&
            line[2] >= '0' && line[2] <= '9') {
            ++summary.ephemerides[systemName(line[0])];
        } else {
            warn(number, "neither the start of an ephemeris nor one of its orbit lines");
        }
    }
    summary.lines = cursor.lineNumber();
    if (suppressed > 0) {
        summary.warnings.push_back(ReadWarning{
            name, 0, std::to_string(suppressed) + " more problems like these are not listed"});
    }
    return summary;
}

} // namespace katana::surveyio::rinex
