// Compact RINEX (Hatanaka compression): the expansion in
// src/katana_surveyio/rinex_compact.cpp, and the observation reader on
// compact files.
//
// Fixtures in data/rinex/:
//   * rinex305_example1.crx - the specification's own example (Hatanaka 2008,
//     appendix 3: the RINEX 3.00 example of the RINEX specification,
//     compressed) as the published RNX2CRX ver.4.1.0 writes it from
//     rinex305_example1.rnx, up to the epoch at 13:14:12 (whose Doppler values
//     are not F14.3, which the tool refuses). Its lines are the ones the paper
//     prints, and the values below are worked out from them by hand;
//   * test2560.24d and the three .crx beside the .rnx of the same name - the
//     other synthetic fixtures, compressed with the same published tool. What
//     they must read as is what their plain files read as, which
//     test_rinex.cpp works out by hand.
//
// The expansion's arithmetic is invisible through the reader (it counts
// observations; it keeps no values), so this file also includes the reader's
// private header to test the expansion itself.

#include <gtest/gtest.h>

#include <algorithm>
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <random>
#include <sstream>
#include <string>
#include <vector>

#include "../../src/katana_surveyio/rinex_internal.hpp"
#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/format.hpp"
#include "katana/surveyio/reader.hpp"
#include "katana/surveyio/rinex.hpp"

using namespace katana::surveyio;
namespace survey = katana::survey;
using katana::core::ErrorCode;
using katana::core::Result;

namespace {

std::filesystem::path fixtureFolder()
{
    return std::filesystem::path(__FILE__).parent_path() / "data" / "rinex";
}

std::string fixture(const std::string& name)
{
    std::ifstream stream(fixtureFolder() / name, std::ios::binary);
    EXPECT_TRUE(stream.good()) << "missing fixture " << name;
    std::ostringstream text;
    text << stream.rdbuf();
    return text.str();
}

std::vector<std::string> linesOf(std::string_view text)
{
    std::vector<std::string> lines;
    std::size_t at = 0;
    while (at < text.size()) {
        std::size_t end = text.find('\n', at);
        if (end == std::string_view::npos) {
            end = text.size();
        }
        std::string line(text.substr(at, end - at));
        if (!line.empty() && line.back() == '\r') {
            line.pop_back();
        }
        lines.push_back(std::move(line));
        at = end + 1;
    }
    return lines;
}

std::string trimmedRight(std::string text)
{
    while (!text.empty() && text.back() == ' ') {
        text.pop_back();
    }
    return text;
}

rinex::CompactExpansion expand(const std::string& bytes, const std::string& name)
{
    Result<rinex::CompactExpansion> expanded =
        rinex::expandCompactRinex(bytes, name, kMaxSurveyFileBytes);
    EXPECT_TRUE(expanded.ok()) << (expanded.ok() ? std::string() : expanded.error().describe());
    return expanded.ok() ? std::move(*expanded) : rinex::CompactExpansion{};
}

Result<ReadResult> readRinex(std::string_view bytes, std::string_view name,
                             std::vector<SiblingFile> siblings = {})
{
    ReadOptions options;
    options.siblings = siblingsInMemory(std::move(siblings));
    return readSurvey(formatRegistry(), kRinexObservationFormatId, bytes, name, options);
}

std::string describeWarnings(const ReadResult& result)
{
    std::string text;
    for (const ReadWarning& warning : result.warnings) {
        text += describe(warning) + "\n";
    }
    return text;
}

// A header record: content in columns 1-60, label in 61-80.
std::string header(std::string content, const std::string& label)
{
    content.resize(60, ' ');
    return content + label + "\n";
}

// The compact header of a RINEX 3.04 GPS file with two observation types. 9
// lines; the first epoch line is line 10.
std::string compactV3Header()
{
    return header("3.0                 COMPACT RINEX FORMAT", "CRINEX VERS   / TYPE") +
           header("hand built                              24-Sep-26 12:00", "CRINEX PROG / DATE") +
           header("     3.04           OBSERVATION DATA    G", "RINEX VERSION / TYPE") +
           header("katana test", "PGM / RUN BY / DATE") + header("M1", "MARKER NAME") +
           header(" -4646000.0000  2553000.0000 -3534000.0000", "APPROX POSITION XYZ") +
           header("        1.0000        0.0000        0.0000", "ANTENNA: DELTA H/E/N") +
           header("G    2 C1C L1C", "SYS / # / OBS TYPES") + header("", "END OF HEADER");
}

// Section 2.2's text difference, the compressor's side: what the expansion
// undoes.
std::string textDifference(const std::string& before, const std::string& after)
{
    std::string difference;
    for (std::size_t i = 0; i < std::max(before.size(), after.size()); ++i) {
        const char old = i < before.size() ? before[i] : ' ';
        const char now = i < after.size() ? after[i] : ' ';
        difference.push_back(old == now ? ' ' : (now == ' ' ? '&' : now));
    }
    return trimmedRight(difference);
}

// A RINEX 3 epoch line as Compact RINEX 3.0 holds it: columns 1-41 and then
// the satellite list.
std::string compactEpochLine(int minute, int second, int count, const std::string& list)
{
    char line[96];
    std::snprintf(line, sizeof line, "> 2024 09 12 10 %02d%11.7f  0%3d      %s", minute,
                  static_cast<double>(second), count, list.c_str());
    return line;
}

} // namespace

// ---- The expansion -----------------------------------------------------------------

TEST(CompactRinexExpansion, TheSpecificationsExampleExpandsToTheValuesItsDifferencesWorkOutTo)
{
    const rinex::CompactExpansion expanded =
        expand(fixture("rinex305_example1.crx"), "rinex305_example1.crx");
    const std::vector<std::string> lines = linesOf(expanded.text);
    // The RINEX header is lines 3-35 of the compact file and 1-33 of the
    // expansion; epoch 1 is expanded line 34 and its five satellites 35-39.
    ASSERT_GE(lines.size(), 47U);
    EXPECT_EQ(lines[0].substr(0, 9), "     3.05");
    EXPECT_EQ(lines[32].substr(60), "END OF HEADER");
    // Epoch 1 is given whole ("3&-123456789012": order 3, -0.123456789012 s).
    EXPECT_EQ(lines[33], "> 2006 03 24 13 10 36.0000000  0  5       -.123456789012");
    // G06 "3&23629347915 3&300 3&-353 3&23629347158 3&24158 &&&8&4&&&&":
    // flags "&&&8&4&&&&" are LLI/SSI of C1C "  ", L1W " 8", L2W " 4".
    EXPECT_EQ(lines[34],
              "G06  23629347.915            .300 8         -.353 4  23629347.158          24.158");
    // Epoch 2: the epoch line's difference changes the seconds 36 -> 54, the
    // count 5 -> 7 and the list G06G09G12E11S20 -> G06G09G12R21R22E11S20; the
    // clock's first difference -198 gives -123456789012 - 198 = -123456789210.
    EXPECT_EQ(lines[39], "> 2006 03 24 13 10 54.0000000  0  7       -.123456789210");
    // G06 "-10252465 -53875932 -41981022 -10252150 1076", first differences
    // in thousandths:
    //   C1C 23629347.915 - 10252.465 = 23619095.450
    //   L1W       0.300 - 53875.932 =   -53875.632
    //   L2W      -0.353 - 41981.022 =   -41981.375
    //   C1W 23629347.158 - 10252.150 = 23619095.008
    //   S2W      24.158 +     1.076 =       25.234
    // and no flag text: the flags stay " 8" and " 4".
    EXPECT_EQ(lines[40],
              "G06  23619095.450      -53875.632 8    -41981.375 4  23619095.008          25.234");
    // G09 "-5458981 -28687907 -22354177 -5469191 4108      7": the flag text
    // after the fifth separator is "     7", so position 6 (L2W's SSI) becomes
    // 7 and the rest stay ("   9 6" -> "   9 7").
    //   C1C 20891534.648 - 5458.981 = 20886075.667
    //   L1W       -0.120 - 28687.907 = -28688.027
    //   L2W       -0.358 - 22354.177 = -22354.535
    //   C1W 20891545.292 - 5469.191 = 20886076.101
    //   S2W       38.123 + 4.108     =     42.231
    EXPECT_EQ(lines[41],
              "G09  20886075.667      -28688.027 9    -22354.535 7  20886076.101          42.231");
    // R21 and R22 are new, so start afresh: "3&21345678576 3&12345567 &&&5".
    EXPECT_EQ(lines[43], "R21  21345678.576       12345.567 5");
    // E11 "65431799 48861408  5": 0.324 + 65431.799 = 65432.123 and
    // 0.178 + 48861.408 = 48861.586; flags "&8&7" (" 8 7") with " 5" over
    // them give " 5 7".
    EXPECT_EQ(lines[45], "E11     65432.123 5     48861.586 7");
    // S20 "0 0": unchanged.
    EXPECT_EQ(lines[46], "S20  38137559.506      335849.135 9");
    // The event (flag 2) and its two records as they were.
    EXPECT_EQ(lines[47], "> 2006 03 24 13 11 12.0000000  2  2");
    EXPECT_EQ(lines[48].substr(0, 42), "       *** FROM NOW ON KINEMATIC DATA! ***");

    // Every line maps back to the compact file: compact line 36 is epoch 1,
    // 37 its clock, 38-42 its satellites, 43 epoch 2, 44 its clock, 45 G06.
    EXPECT_EQ(expanded.lines.compactLine(1), 3U);
    EXPECT_EQ(expanded.lines.compactLine(33), 35U);
    EXPECT_EQ(expanded.lines.compactLine(34), 36U);
    EXPECT_EQ(expanded.lines.compactLine(35), 38U);
    EXPECT_EQ(expanded.lines.compactLine(40), 43U);
    EXPECT_EQ(expanded.lines.compactLine(41), 45U);
    EXPECT_EQ(expanded.lines.compactLine(48), 52U) << "the event line";
    EXPECT_EQ(expanded.compactLines, 75U);
    EXPECT_EQ(expanded.linesSkipped, 0U);
    EXPECT_TRUE(expanded.warnings.empty());
    EXPECT_EQ(expanded.description,
              "Compact RINEX 3.0, written by RNX2CRX ver.4.1.0 on 24-Sep-26 05:16");
}

TEST(CompactRinexExpansion, EveryCompactFixtureExpandsToThePlainFileItWasCompressedFrom)
{
    // The published compressor's output, expanded here, gives back the plain
    // file line for line - trailing blanks aside, which Compact RINEX never
    // keeps (section 3), and a zero before a decimal point, which it cannot.
    const std::vector<std::pair<std::string, std::string>> pairs{
        {"test2560.24d", "test2560.24o"},
        {"TEST00AUS_R_20242561000_01H_30S_MO.crx", "TEST00AUS_R_20242561000_01H_30S_MO.rnx"},
        {"ROVR00AUS_R_20242561100_01H_01S_GO.crx", "ROVR00AUS_R_20242561100_01H_01S_GO.rnx"},
        {"BASE00AUS_R_20242561200_01H_30S_MO.crx", "BASE00AUS_R_20242561200_01H_30S_MO.rnx"},
        {"rinex305_example1.crx", "rinex305_example1.rnx"},
    };
    for (const auto& [compact, plain] : pairs) {
        const std::vector<std::string> expanded = linesOf(expand(fixture(compact), compact).text);
        std::vector<std::string> original = linesOf(fixture(plain));
        if (compact == "rinex305_example1.crx") {
            original.resize(69); // the example up to the epoch the tool refuses (line 70)
        }
        ASSERT_EQ(expanded.size(), original.size()) << compact;
        for (std::size_t i = 0; i < original.size(); ++i) {
            std::string want = trimmedRight(original[i]);
            std::string got = expanded[i];
            // " 0.000123456789" and "  .000123456789" are the same clock.
            for (std::string* line : {&want, &got}) {
                for (std::size_t at = line->find(" 0."); at != std::string::npos;
                     at = line->find(" 0.", at + 1)) {
                    (*line)[at + 1] = ' ';
                }
                for (std::size_t at = line->find("-0."); at != std::string::npos;
                     at = line->find("-0.", at + 1)) {
                    (*line)[at] = ' ';
                    (*line)[at + 1] = '-';
                }
            }
            EXPECT_EQ(got, want) << compact << " line " << i + 1;
        }
    }
}

TEST(CompactRinexExpansion, AThirdOrderSeriesIsRecoveredByEquationThreeAsItRampsUp)
{
    // One satellite, C1C only: "3&1000" starts a series of order 3 at 1.000;
    // each later field is the difference of the order the series has reached:
    //   epoch 2: 1st difference 10          -> Y1 = 10,           Y = 1010
    //   epoch 3: 2nd difference  1          -> Y1 = 10 + 1 = 11,  Y = 1021
    //   epoch 4: 3rd difference  0 -> Y2 = 1, Y1 = 12,            Y = 1033
    //   epoch 5: 3rd difference  2 -> Y2 = 3, Y1 = 15,            Y = 1048
    // (appendix 1, equation 3). L1C is blank throughout.
    // Epochs 30 s apart; each epoch line is given as its difference from the
    // one before (section 2.2), each has no clock offset (an empty line).
    const std::vector<std::string> epochs{
        compactEpochLine(0, 0, 1, "G01"), compactEpochLine(0, 30, 1, "G01"),
        compactEpochLine(1, 0, 1, "G01"), compactEpochLine(1, 30, 1, "G01"),
        compactEpochLine(2, 0, 1, "G01")};
    const std::vector<std::string> fields{"3&1000", "10", "1", "0", "2"};
    std::string bytes = compactV3Header();
    for (std::size_t i = 0; i < epochs.size(); ++i) {
        bytes += (i == 0 ? epochs[0] : textDifference(epochs[i - 1], epochs[i])) + "\n\n" +
                 fields[i] + "\n";
    }
    const rinex::CompactExpansion expanded = expand(bytes, "ramp.crx");
    const std::vector<std::string> lines = linesOf(expanded.text);
    ASSERT_EQ(lines.size(), 7U + 10U) << expanded.text;
    EXPECT_EQ(lines[7], "> 2024 09 12 10 00  0.0000000  0  1");
    EXPECT_EQ(lines[8], "G01         1.000");
    EXPECT_EQ(lines[9], "> 2024 09 12 10 00 30.0000000  0  1");
    EXPECT_EQ(lines[10], "G01         1.010");
    EXPECT_EQ(lines[11], "> 2024 09 12 10 01  0.0000000  0  1");
    EXPECT_EQ(lines[12], "G01         1.021");
    EXPECT_EQ(lines[14], "G01         1.033");
    EXPECT_EQ(lines[15], "> 2024 09 12 10 02  0.0000000  0  1");
    EXPECT_EQ(lines[16], "G01         1.048");
    EXPECT_TRUE(expanded.warnings.empty());
}

TEST(CompactRinexExpansion, DamageIsWarnedOnceWithTheLinesItSpoilsAndExpansionResumesAtAFreshEpoch)
{
    // Lines 1-9 the header. 10-12: epoch 1, G01 starts both series. 13: an
    // optional record ('&'), skipped. 14-16: epoch 2 names G02, which was not
    // in epoch 1, yet gives it differences - nothing to apply them to (line
    // 16). 17-19: epoch 3, a difference of the spoiled epoch line. 20-22: a
    // fresh epoch, expanded again.
    const std::string first = compactEpochLine(0, 0, 1, "G01");
    const std::string second = compactEpochLine(0, 30, 1, "G02");
    const std::string third = compactEpochLine(1, 0, 1, "G02");
    const std::string bytes = compactV3Header() + first + "\n\n3&1000 3&2000\n" + "&reserved\n" +
                              textDifference(first, second) + "\n\n5 5\n" +
                              textDifference(second, third) + "\n\n5 5\n" +
                              compactEpochLine(1, 30, 1, "G03") + "\n\n3&4000 3&5000\n";
    const rinex::CompactExpansion expanded = expand(bytes, "damaged.crx");
    ASSERT_EQ(expanded.warnings.size(), 1U);
    EXPECT_EQ(expanded.warnings[0].record, 16U) << "the line the damage was found at";
    EXPECT_NE(expanded.warnings[0].message.find("no earlier value"), std::string::npos)
        << expanded.warnings[0].message;
    EXPECT_NE(expanded.warnings[0].message.find("Lines 14 to 19 are left out"), std::string::npos)
        << expanded.warnings[0].message;
    EXPECT_EQ(expanded.linesSkipped, 6U);
    EXPECT_EQ(expanded.optionalLines, 1U);
    // The RINEX header is 7 lines; then epoch 1 and G01, epoch 4 and G03.
    const std::vector<std::string> lines = linesOf(expanded.text);
    ASSERT_EQ(lines.size(), 7U + 4U) << expanded.text;
    EXPECT_EQ(lines[8], "G01         1.000           2.000");
    EXPECT_EQ(lines[9], "> 2024 09 12 10 01 30.0000000  0  1");
    EXPECT_EQ(lines[10], "G03         4.000           5.000");
    EXPECT_EQ(expanded.lines.compactLine(10), 20U);
    EXPECT_EQ(expanded.lines.compactLine(11), 22U);
}

TEST(CompactRinexExpansion, CompactHeadersThatCannotBeExpandedAreErrorsThatSayWhy)
{
    const auto error = [](const std::string& bytes) {
        const Result<rinex::CompactExpansion> expanded =
            rinex::expandCompactRinex(bytes, "x.crx", kMaxSurveyFileBytes);
        EXPECT_FALSE(expanded.ok());
        return expanded.ok() ? katana::core::Error{} : expanded.error();
    };
    std::string later = compactV3Header();
    later.replace(0, 3, "4.0");
    const katana::core::Error version = error(later);
    EXPECT_EQ(version.code, ErrorCode::Unsupported);
    EXPECT_NE(version.message.find("version 4.0"), std::string::npos) << version.message;

    std::string mismatched = compactV3Header();
    mismatched.replace(0, 3, "1.0");
    const katana::core::Error mismatch = error(mismatched);
    EXPECT_EQ(mismatch.code, ErrorCode::FileImportFailure);
    EXPECT_NE(mismatch.message.find("says RINEX 3.04"), std::string::npos) << mismatch.message;

    std::string unended = compactV3Header();
    unended.resize(unended.find("END OF HEADER") - 60);
    EXPECT_NE(error(unended).message.find("no END OF HEADER"), std::string::npos);

    // The RINEX header expands to 7 lines of 60 columns, a label and a line
    // end: 420 + 121 + 7 = 548 bytes; the one epoch adds 36 + 18. A cap of 560
    // lets the header through and stops at the epoch.
    const std::string tiny =
        compactV3Header() + "> 2024 09 12 10 00  0.0000000  0  1      G01\n\n3&1000\n";
    const Result<rinex::CompactExpansion> expanded =
        rinex::expandCompactRinex(tiny, "x.crx", 560);
    ASSERT_FALSE(expanded.ok()) << "a cap on the expanded size";
    EXPECT_NE(expanded.error().message.find("expands to more than"), std::string::npos);
}

// ---- The reader on compact files ------------------------------------------------------

TEST(CompactRinex, EveryCompactFixtureIsIdentifiedAsRinexObservationWithItsCompactHeader)
{
    for (const std::string name :
         {"test2560.24d", "TEST00AUS_R_20242561000_01H_30S_MO.crx",
          "ROVR00AUS_R_20242561100_01H_01S_GO.crx", "BASE00AUS_R_20242561200_01H_30S_MO.crx",
          "rinex305_example1.crx"}) {
        const std::string bytes = fixture(name);
        const Detection detection = detectFormat(probeOf(bytes, name));
        ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified)
            << name << ": " << detection.summary();
        EXPECT_EQ(detection.format()->id, kRinexObservationFormatId) << name;
        const std::string& evidence = detection.candidates().front().evidence;
        EXPECT_NE(evidence.find("CRINEX VERS / TYPE"), std::string::npos) << evidence;
        EXPECT_NE(evidence.find("observation data"), std::string::npos) << evidence;
    }
}

TEST(CompactRinex, EachCompactFixtureReadsAsThePlainFileItCompressesWithWarningsAtItsOwnLines)
{
    const std::vector<std::pair<std::string, std::string>> pairs{
        {"test2560.24d", "test2560.24o"},
        {"TEST00AUS_R_20242561000_01H_30S_MO.crx", "TEST00AUS_R_20242561000_01H_30S_MO.rnx"},
        {"ROVR00AUS_R_20242561100_01H_01S_GO.crx", "ROVR00AUS_R_20242561100_01H_01S_GO.rnx"},
        {"BASE00AUS_R_20242561200_01H_30S_MO.crx", "BASE00AUS_R_20242561200_01H_30S_MO.rnx"},
    };
    for (const auto& [compactName, plainName] : pairs) {
        const std::string compactBytes = fixture(compactName);
        const Result<ReadResult> compact = readRinex(compactBytes, compactName);
        const Result<ReadResult> plain = readRinex(fixture(plainName), plainName);
        ASSERT_TRUE(compact.ok()) << compact.error().describe();
        ASSERT_TRUE(plain.ok()) << plain.error().describe();
        const survey::SurveyProject& a = compact->project;
        const survey::SurveyProject& b = plain->project;
        ASSERT_EQ(a.gnssSessions.size(), b.gnssSessions.size()) << compactName;
        for (std::size_t i = 0; i < a.gnssSessions.size(); ++i) {
            const survey::GnssSession& x = a.gnssSessions[i];
            const survey::GnssSession& y = b.gnssSessions[i];
            EXPECT_EQ(x.markerName, y.markerName) << compactName;
            EXPECT_EQ(x.epochCount, y.epochCount) << compactName;
            EXPECT_EQ(x.firstEpoch, y.firstEpoch) << compactName;
            EXPECT_EQ(x.lastEpoch, y.lastEpoch) << compactName;
            EXPECT_EQ(x.intervalSeconds, y.intervalSeconds) << compactName;
            EXPECT_EQ(x.satellitesPerSystem, y.satellitesPerSystem) << compactName;
            EXPECT_EQ(x.antenna, y.antenna) << compactName;
            EXPECT_EQ(x.receiverType, y.receiverType) << compactName;
            EXPECT_EQ(x.approximatePosition, y.approximatePosition) << compactName;
            EXPECT_EQ(x.source.fileName, compactName);
        }
        EXPECT_EQ(a.observations.size(), b.observations.size()) << compactName;
        EXPECT_EQ(a.unpositionedPoints.size(), b.unpositionedPoints.size()) << compactName;
        std::map<std::string, std::string> metadata = a.metadata;
        EXPECT_NE(metadata["compression"].find("Compact RINEX"), std::string::npos);
        metadata.erase("compression");
        std::map<std::string, std::string> plainMetadata = b.metadata;
        // Neither was read with its navigation file; what was looked for is
        // named after each file.
        metadata.erase("navigation files");
        plainMetadata.erase("navigation files");
        EXPECT_EQ(metadata, plainMetadata) << compactName;
        EXPECT_EQ(compact->notCarried, plain->notCarried) << compactName;
        // The same warnings, each at the compact file's own line.
        ASSERT_EQ(compact->warnings.size(), plain->warnings.size())
            << compactName << "\n"
            << describeWarnings(*compact);
        const std::vector<std::string> compactLines = linesOf(compactBytes);
        for (std::size_t w = 0; w < compact->warnings.size(); ++w) {
            EXPECT_EQ(compact->warnings[w].message, plain->warnings[w].message) << compactName;
            const std::size_t record = compact->warnings[w].record;
            ASSERT_LE(record, compactLines.size()) << compactName;
        }
        // Every compact line read, none skipped.
        EXPECT_EQ(compact->recordsRead, compactLines.size()) << compactName;
        EXPECT_EQ(compact->recordsSkipped, 0U) << compactName;
    }
}

TEST(CompactRinex, TheExamplesKinematicWarningNamesTheCompactLineOfItsEvent)
{
    // In the plain example the flag 2 event is line 48; in the compact one it
    // is line 52 (35 header lines; epoch 1 = 1 + clock + 5 = 36-42; epoch 2 =
    // 1 + clock + 7 = 43-51).
    const Result<ReadResult> read = readRinex(fixture("rinex305_example1.crx"), "rinex305_example1.crx");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    const auto kinematic = std::find_if(
        read->warnings.begin(), read->warnings.end(),
        [](const ReadWarning& w) { return w.message.find("antenna moving") != std::string::npos; });
    ASSERT_NE(kinematic, read->warnings.end()) << describeWarnings(*read);
    EXPECT_EQ(kinematic->record, 52U);
    EXPECT_EQ(read->project.gnssSessions.size(), 2U);
    EXPECT_EQ(read->project.gnssSessions[0].epochCount, 2U);
    EXPECT_EQ(read->project.gnssSessions[1].markerName, "A 9081");
    // The session's own record is its MARKER NAME: plain line 57, and in the
    // compact file 57 + 2 header lines + the clock lines of the three epochs
    // before it = 62.
    EXPECT_EQ(read->project.gnssSessions[1].source.recordNumber, 62U);
}

TEST(CompactRinex, TheNavigationFileBesideACompactFileIsFoundByItsPlainName)
{
    // BASE..._MO.crx -> BASE..._MN.rnx (navigation files are never compact),
    // test2560.24d -> test2560.24n.
    const std::string longName = "BASE00AUS_R_20242561200_01H_30S_MO.crx";
    const std::string nav = "BASE00AUS_R_20242561200_01H_MN.rnx";
    const Result<ReadResult> base =
        readRinex(fixture(longName), longName, {SiblingFile{nav, fixture(nav)}});
    ASSERT_TRUE(base.ok()) << base.error().describe();
    EXPECT_NE(base->project.metadata.at("navigation files").find(nav), std::string::npos)
        << base->project.metadata.at("navigation files");

    const Result<ReadResult> shortName = readRinex(
        fixture("test2560.24d"), "test2560.24d", {SiblingFile{"test2560.24n", fixture("test2560.24n")}});
    ASSERT_TRUE(shortName.ok()) << shortName.error().describe();
    EXPECT_NE(shortName->project.metadata.at("navigation files").find("test2560.24n"),
              std::string::npos);
}

TEST(CompactRinex, DamageInACompactFileIsAWarningAtItsLineAndTheLinesAfterItAreSkipped)
{
    // test2560.24d: epoch 2 is its epoch line (36), clock line (37) and five
    // satellites (38-42), then an event at line 43. G02's line (39) replaced
    // by a word spoils the epoch; nothing after it can be expanded until the
    // event restarts every series.
    std::vector<std::string> lines = linesOf(fixture("test2560.24d"));
    ASSERT_EQ(lines[38], "0 0 0 0 0 0");
    lines[38] = "zero";
    std::string bytes;
    for (const std::string& line : lines) {
        bytes += line + "\n";
    }
    const Result<ReadResult> read = readRinex(bytes, "test2560.24d");
    ASSERT_TRUE(read.ok()) << read.error().describe();
    ASSERT_FALSE(read->warnings.empty());
    EXPECT_EQ(read->warnings[0].record, 39U) << describeWarnings(*read);
    EXPECT_NE(read->warnings[0].message.find("'zero' is not a number"), std::string::npos)
        << read->warnings[0].message;
    EXPECT_NE(read->warnings[0].message.find("Lines 36 to 42 are left out"), std::string::npos)
        << read->warnings[0].message;
    EXPECT_EQ(read->recordsSkipped, 7U);
    EXPECT_EQ(read->recordsRead, lines.size() - 7U);
}

// ---- Throughput -------------------------------------------------------------------------

namespace {

// A synthetic Compact RINEX 3.0 file at 1 Hz: 12 GPS satellites with eight
// observation types, each a third-order series whose third differences
// alternate 1, -1 - about 1.6 kB of RINEX an epoch from about 350 bytes.
std::string syntheticCompact(std::size_t epochs)
{
    std::string text =
        header("3.0                 COMPACT RINEX FORMAT", "CRINEX VERS   / TYPE") +
        header("katana test                             24-Sep-26 12:00", "CRINEX PROG / DATE") +
        header("     3.04           OBSERVATION DATA    G", "RINEX VERSION / TYPE") +
        header("katana test", "PGM / RUN BY / DATE") + header("BIG", "MARKER NAME") +
        header(" -4646000.0000  2553000.0000 -3534000.0000", "APPROX POSITION XYZ") +
        header("        1.0000        0.0000        0.0000", "ANTENNA: DELTA H/E/N") +
        header("G    8 C1C L1C D1C S1C C2W L2W D2W S2W", "SYS / # / OBS TYPES") +
        header("     1.000", "INTERVAL") +
        header("  2024     9    12     0     0    0.0000000     GPS", "TIME OF FIRST OBS") +
        header("", "END OF HEADER");
    std::string list;
    for (int n = 1; n <= 12; ++n) {
        list += (n < 10 ? "G0" : "G") + std::to_string(n);
    }
    text.reserve(epochs * 400 + text.size());
    std::string previous;
    char line[96];
    for (std::size_t e = 0; e < epochs; ++e) {
        std::snprintf(line, sizeof line, "> 2024 09 12 %02zu %02zu%11.7f  0 12      %s",
                      (e / 3600) % 24, (e / 60) % 60, static_cast<double>(e % 60), list.c_str());
        const std::string epoch = line;
        text += e == 0 ? epoch : textDifference(previous, epoch);
        text += "\n\n"; // and no clock offset
        previous = epoch;
        for (int s = 0; s < 12; ++s) {
            if (e == 0) {
                text += "3&21000000123 3&110000001500 3&-2000250 3&44500 3&21000004123 "
                        "3&85000005500 3&-1500250 3&40250\n";
            } else if (e == 1) {
                text += "1000 1000 -10 0 1000 1000 -10 0\n";
            } else {
                text += e % 2 == 0 ? "1 1 1 0 1 1 1 0\n" : "-1 -1 -1 0 -1 -1 -1 0\n";
            }
        }
    }
    return text;
}

} // namespace

TEST(CompactRinexThroughput, ALargeCompactFileIsExpandedAndCountedAtSpeed)
{
    // 30 000 epochs (8 h 20 min at 1 Hz) is about 11 MB compact, 50 MB
    // expanded. KATANA_RINEX_THROUGHPUT_MB scales the expanded size.
    std::size_t megabytes = 50;
    if (const char* wanted = std::getenv("KATANA_RINEX_THROUGHPUT_MB")) {
        megabytes = static_cast<std::size_t>(std::max(1, std::atoi(wanted)));
    }
    const std::size_t epochs = std::min<std::size_t>(megabytes * 600, 86'400);
    const std::string bytes = syntheticCompact(epochs);
    const auto start = std::chrono::steady_clock::now();
    const Result<ReadResult> read = readRinex(bytes, "BIG00AUS_R_20242560000_01D_01S_MO.crx");
    const auto elapsed = std::chrono::steady_clock::now() - start;
    ASSERT_TRUE(read.ok()) << read.error().describe();
    EXPECT_TRUE(read->warnings.empty()) << describeWarnings(*read);
    const survey::GnssSession& session = read->project.gnssSessions.at(0);
    EXPECT_EQ(session.epochCount, epochs);
    EXPECT_EQ(session.satellitesPerSystem.at("GPS"), 12U);
    EXPECT_EQ(read->project.metadata.at("satellite records"), std::to_string(epochs * 12));
    EXPECT_EQ(read->recordsSkipped, 0U);
    const double seconds = std::max(std::chrono::duration<double>(elapsed).count(), 1e-9);
    const double compactRate = static_cast<double>(bytes.size()) / 1e6 / seconds;
    // The expansion alone, to say where the time goes.
    const auto expandStart = std::chrono::steady_clock::now();
    const Result<rinex::CompactExpansion> expanded =
        rinex::expandCompactRinex(bytes, "big.crx", kMaxSurveyFileBytes);
    const double expandSeconds = std::max(
        std::chrono::duration<double>(std::chrono::steady_clock::now() - expandStart).count(),
        1e-9);
    ASSERT_TRUE(expanded.ok());
    const double expandedRate = static_cast<double>(expanded->text.size()) / 1e6 / seconds;
    std::cout << "[ CRINEX   ] read " << static_cast<double>(bytes.size()) / 1e6
              << " MB compact (" << static_cast<double>(expanded->text.size()) / 1e6
              << " MB of RINEX), " << epochs << " epochs in " << seconds << " s: " << compactRate
              << " MB/s compact, " << expandedRate << " MB/s of RINEX; the expansion alone "
              << expandSeconds << " s\n";
    RecordProperty("compact_megabytes_per_second", std::to_string(compactRate));
}
