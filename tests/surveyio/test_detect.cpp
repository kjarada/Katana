#include <gtest/gtest.h>

#include <filesystem>
#include <fstream>
#include <initializer_list>
#include <limits>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>

#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/format.hpp"

using namespace katana::surveyio;
using katana::core::ErrorCode;
using katana::core::Status;

// The framework, not any one format. No parser is registered from this file into
// the process-wide registry: every test that needs formats builds its own
// FormatRegistry, so the suite says the same thing whichever parsers happen to be
// linked into the binary beside it.

namespace {

constexpr double kNaN = std::numeric_limits<double>::quiet_NaN();

// Byte strings built from numbers rather than written as string literals: a hex
// escape swallows every following hex digit, so "\xFF" followed by "face" is not
// the five bytes it looks like.
std::string bytesOf(std::initializer_list<int> values)
{
    std::string bytes;
    bytes.reserve(values.size());
    for (const int value : values) {
        bytes.push_back(static_cast<char>(value));
    }
    return bytes;
}

FormatDescriptor descriptorFor(std::string id)
{
    FormatDescriptor format;
    format.id = std::move(id);
    format.humanName = "Test format " + format.id;
    format.manufacturer = Manufacturer::Generic;
    format.reads.points = true;
    format.canImport = true;
    format.parserVersion = "1.0";
    return format;
}

FormatSignature neverThisFormat(const ProbeInput&)
{
    return ruledOut();
}

FormatSignature veryConfident(const ProbeInput&)
{
    return {0.95, "a signature nothing else has"};
}

FormatSignature equallyConfident(const ProbeInput&)
{
    return {0.95, "the very same signature"};
}

FormatSignature nearlyAsConfident(const ProbeInput&)
{
    return {0.85, "most of that signature"};
}

FormatSignature clearlyBehind(const ProbeInput&)
{
    return {0.75, "a weaker signature"};
}

FormatSignature halfConfident(const ProbeInput&)
{
    return {0.5, "the extension matches and nothing else does"};
}

FormatSignature slightlyLessThanHalf(const ProbeInput&)
{
    return {0.4, "a record length that could be a coincidence"};
}

FormatSignature impossiblyConfident(const ProbeInput&)
{
    return {1.5, "a bug in this probe"};
}

FormatSignature confidenceIsNotANumber(const ProbeInput&)
{
    return {kNaN, "another bug in this probe"};
}

// Uses the shared helpers, so that what a real parser would do is exercised too.
FormatSignature magicProbe(const ProbeInput& input)
{
    if (input.extension != "magic") {
        return ruledOut();
    }
    const std::vector<std::string_view> lines = probeLines(input, 1);
    if (lines.empty() || lines.front() != "MAGIC") {
        return ruledOut();
    }
    return {0.9, "extension .magic and a MAGIC first line"};
}

std::filesystem::path writeTemporaryFile(const std::string& name, std::string_view bytes)
{
    const std::filesystem::path path = std::filesystem::path(::testing::TempDir()) / name;
    std::ofstream out(path, std::ios::binary | std::ios::trunc);
    out.write(bytes.data(), static_cast<std::streamsize>(bytes.size()));
    out.close();
    return path;
}

} // namespace

// ---- descriptors ---------------------------------------------------------------

TEST(SurveyFormatDescriptor, AFormatDescribesWhatItCanActuallyDoRatherThanItsVendor)
{
    FormatDescriptor format = descriptorFor("leica-gsi16");
    format.humanName = "Leica GSI-16";
    format.manufacturer = Manufacturer::Leica;
    format.parserVersion = "1.0";

    // descriptorFor leaves a format that imports and does not export, which is
    // what a new parser is: the descriptor says so rather than implying both.
    EXPECT_EQ(describeFormat(format), "Leica GSI-16, import: yes, export: no, parser: 1.0");
    format.canImport = false;
    format.canExport = true;
    EXPECT_EQ(describeFormat(format), "Leica GSI-16, import: no, export: yes, parser: 1.0");
    EXPECT_EQ(std::string(toString(Manufacturer::Leica)), "Leica");
    EXPECT_EQ(std::string(toString(Manufacturer::OpenStandard)), "Open standard");
}

// ---- registration --------------------------------------------------------------

TEST(SurveyFormatRegistry, ADescriptorTheUserInterfaceCouldNotShowIsRejected)
{
    FormatRegistry registry;

    EXPECT_EQ(registry.add(descriptorFor(""), &neverThisFormat).error().code,
              ErrorCode::InvalidArgument);
    // The id reaches saved settings and every SourceRecord, so it is a slug.
    EXPECT_EQ(registry.add(descriptorFor("Leica GSI"), &neverThisFormat).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(registry.add(descriptorFor("Leica-Gsi16"), &neverThisFormat).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(registry.add(descriptorFor("16-gsi"), &neverThisFormat).error().code,
              ErrorCode::InvalidArgument);
    EXPECT_EQ(registry.add(descriptorFor("gsi_16"), &neverThisFormat).error().code,
              ErrorCode::InvalidArgument);

    EXPECT_EQ(registry.add(descriptorFor("good-id"), nullptr).error().code,
              ErrorCode::InvalidArgument);

    FormatDescriptor unnamed = descriptorFor("good-id");
    unnamed.humanName.clear();
    EXPECT_EQ(registry.add(unnamed, &neverThisFormat).error().code, ErrorCode::InvalidArgument);

    FormatDescriptor unversioned = descriptorFor("good-id");
    unversioned.parserVersion.clear();
    EXPECT_EQ(registry.add(unversioned, &neverThisFormat).error().code,
              ErrorCode::InvalidArgument);

    FormatDescriptor useless = descriptorFor("good-id");
    useless.canImport = false;
    useless.canExport = false;
    EXPECT_EQ(registry.add(useless, &neverThisFormat).error().code, ErrorCode::InvalidArgument);

    FormatDescriptor dotted = descriptorFor("good-id");
    dotted.extensions = {".gsi"};
    EXPECT_EQ(registry.add(dotted, &neverThisFormat).error().code, ErrorCode::InvalidArgument);
    dotted.extensions = {"GSI"};
    EXPECT_EQ(registry.add(dotted, &neverThisFormat).error().code, ErrorCode::InvalidArgument);

    EXPECT_TRUE(registry.empty()); // nothing above was half-added
}

TEST(SurveyFormatRegistry, TwoFormatsCannotAnswerToOneId)
{
    FormatRegistry registry;
    ASSERT_TRUE(registry.add(descriptorFor("leica-gsi16"), &veryConfident).ok());

    const Status second = registry.add(descriptorFor("leica-gsi16"), &halfConfident);
    ASSERT_FALSE(second.ok());
    EXPECT_EQ(second.error().code, ErrorCode::AlreadyExists);
    EXPECT_EQ(registry.size(), 1u);
    EXPECT_TRUE(registry.find("leica-gsi16").ok());
    EXPECT_EQ(registry.find("no-such-format").error().code, ErrorCode::NotFound);
}

TEST(SurveyFormatRegistry, FormatsComeBackInIdOrderWhateverOrderTheyWereRegisteredIn)
{
    FormatRegistry registry;
    ASSERT_TRUE(registry.add(descriptorFor("zulu"), &neverThisFormat).ok());
    ASSERT_TRUE(registry.add(descriptorFor("alpha"), &neverThisFormat).ok());
    ASSERT_TRUE(registry.add(descriptorFor("mike"), &neverThisFormat).ok());

    const std::vector<FormatDescriptor> formats = registry.formats();
    ASSERT_EQ(formats.size(), 3u);
    EXPECT_EQ(formats[0].id, "alpha");
    EXPECT_EQ(formats[1].id, "mike");
    EXPECT_EQ(formats[2].id, "zulu");
}

TEST(SurveyFormatRegistry, ARejectedRegistrationIsLoudRatherThanAMissingFormat)
{
    // A parser registers at static initialisation, where there is no caller to
    // hand a Status to. A rejected registration must not leave the program
    // running with one format quietly absent.
    FormatDescriptor bad = descriptorFor("Not A Slug");
    EXPECT_THROW((FormatRegistration{bad, &neverThisFormat}), std::logic_error);
    EXPECT_FALSE(formatRegistry().contains("Not A Slug"));
}

TEST(SurveyFormatRegistry, EveryFormatThisProgramRegisteredSatisfiesTheContract)
{
    // Whatever parsers are linked into this binary, each must have a descriptor
    // the registry would accept and a distinct id. Re-adding them to a fresh
    // registry runs exactly those checks, so a parser that registers a bad
    // descriptor fails here rather than at some customer's import.
    FormatRegistry fresh;
    for (const FormatDescriptor& format : formatRegistry().formats()) {
        const Status status = fresh.add(format, &neverThisFormat);
        EXPECT_TRUE(status.ok()) << describeFormat(format) << ": "
                                 << (status.ok() ? std::string{} : status.error().describe());
        EXPECT_FALSE(describeFormat(format).empty());
    }
    EXPECT_EQ(fresh.size(), formatRegistry().size());
}

// ---- detection -----------------------------------------------------------------

TEST(SurveyFormatDetection, AnEmptyFileIsReportedEmptyRatherThanUnrecognised)
{
    FormatRegistry registry;
    ASSERT_TRUE(registry.add(descriptorFor("always"), &veryConfident).ok());

    const Detection detection = detectFormat(probeOf("", "job.gsi"), registry);
    EXPECT_EQ(detection.outcome(), DetectionOutcome::Empty);
    EXPECT_TRUE(detection.candidates().empty());
    EXPECT_FALSE(detection.format().ok()); // and no format can be had from it
}

TEST(SurveyFormatDetection, WithNothingRegisteredNoFileIsIdentified)
{
    const FormatRegistry registry;
    const Detection detection = detectFormat(probeOf("some bytes", "job.gsi"), registry);
    EXPECT_EQ(detection.outcome(), DetectionOutcome::Uncertain);
    EXPECT_FALSE(detection.format().ok());
    EXPECT_FALSE(detection.summary().empty());
}

TEST(SurveyFormatDetection, AConfidentSoleCandidateIsIdentifiedAndSaysWhy)
{
    FormatRegistry registry;
    ASSERT_TRUE(registry.add(descriptorFor("leica-gsi16"), &veryConfident).ok());
    ASSERT_TRUE(registry.add(descriptorFor("trimble-dc"), &neverThisFormat).ok());

    const Detection detection = detectFormat(probeOf("some bytes", "job.gsi"), registry);
    ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified);

    const katana::core::Result<FormatDescriptor> format = detection.format();
    ASSERT_TRUE(format.ok());
    EXPECT_EQ(format.value().id, "leica-gsi16");

    // A format that ruled itself out is not a candidate at all.
    ASSERT_EQ(detection.candidates().size(), 1u);
    EXPECT_EQ(detection.candidates().front().evidence, "a signature nothing else has");
    EXPECT_NE(detection.summary().find("a signature nothing else has"), std::string::npos);
}

TEST(SurveyFormatDetection, TwoFormatsThatBothFitAreAmbiguousRatherThanAGuess)
{
    FormatRegistry registry;
    ASSERT_TRUE(registry.add(descriptorFor("alpha"), &veryConfident).ok());
    ASSERT_TRUE(registry.add(descriptorFor("bravo"), &nearlyAsConfident).ok());

    // 0.95 and 0.85: both past the bar, 0.10 apart, which is inside the margin.
    const Detection detection = detectFormat(probeOf("some bytes", "job.dat"), registry);
    EXPECT_EQ(detection.outcome(), DetectionOutcome::Ambiguous);
    EXPECT_EQ(detection.candidates().size(), 2u);

    const katana::core::Result<FormatDescriptor> format = detection.format();
    ASSERT_FALSE(format.ok());
    EXPECT_EQ(format.error().code, ErrorCode::NotFound);
    // The error carries what was close, so a caller that only propagates it
    // still reports something a person can act on.
    EXPECT_NE(format.error().context.find("alpha"), std::string::npos);
    EXPECT_NE(format.error().context.find("bravo"), std::string::npos);
}

TEST(SurveyFormatDetection, AWinnerClearOfTheRunnerUpByMoreThanTheMarginIsIdentified)
{
    FormatRegistry registry;
    ASSERT_TRUE(registry.add(descriptorFor("alpha"), &veryConfident).ok());
    ASSERT_TRUE(registry.add(descriptorFor("bravo"), &clearlyBehind).ok());

    // 0.95 - 0.75 = 0.20, past kIdentificationMargin of 0.15.
    const Detection detection = detectFormat(probeOf("some bytes", "job.dat"), registry);
    ASSERT_EQ(detection.outcome(), DetectionOutcome::Identified);
    ASSERT_TRUE(detection.format().ok());
    EXPECT_EQ(detection.format().value().id, "alpha");
    EXPECT_EQ(detection.candidates().size(), 2u); // the runner-up is still reported
}

TEST(SurveyFormatDetection, ACandidateBelowTheBarIsListedAsTheClosestButNotChosen)
{
    FormatRegistry registry;
    ASSERT_TRUE(registry.add(descriptorFor("alpha"), &halfConfident).ok());
    ASSERT_TRUE(registry.add(descriptorFor("bravo"), &slightlyLessThanHalf).ok());

    const Detection detection = detectFormat(probeOf("some bytes", "job.dat"), registry);
    EXPECT_EQ(detection.outcome(), DetectionOutcome::Uncertain);
    ASSERT_EQ(detection.candidates().size(), 2u);
    EXPECT_EQ(detection.candidates()[0].formatId, "alpha");
    EXPECT_EQ(detection.candidates()[1].formatId, "bravo");
    EXPECT_FALSE(detection.format().ok());
    EXPECT_NE(detection.summary().find("alpha"), std::string::npos);
}

TEST(SurveyFormatDetection, EqualConfidencesAreRankedByIdSoTheAnswerNeverDependsOnLinkOrder)
{
    FormatRegistry registered;
    ASSERT_TRUE(registered.add(descriptorFor("zulu"), &veryConfident).ok());
    ASSERT_TRUE(registered.add(descriptorFor("alpha"), &equallyConfident).ok());

    FormatRegistry reversed;
    ASSERT_TRUE(reversed.add(descriptorFor("alpha"), &equallyConfident).ok());
    ASSERT_TRUE(reversed.add(descriptorFor("zulu"), &veryConfident).ok());

    const Detection first = detectFormat(probeOf("some bytes", "job.dat"), registered);
    const Detection second = detectFormat(probeOf("some bytes", "job.dat"), reversed);
    ASSERT_EQ(first.candidates().size(), 2u);
    EXPECT_EQ(first.candidates()[0].formatId, "alpha");
    EXPECT_EQ(first.candidates(), second.candidates());
}

TEST(SurveyFormatDetection, AProbeReportingAnImpossibleConfidenceIsIgnoredAndSaidSo)
{
    FormatRegistry registry;
    ASSERT_TRUE(registry.add(descriptorFor("broken"), &impossiblyConfident).ok());
    ASSERT_TRUE(registry.add(descriptorFor("nonsense"), &confidenceIsNotANumber).ok());
    ASSERT_TRUE(registry.add(descriptorFor("sound"), &veryConfident).ok());

    const Detection detection = detectFormat(probeOf("some bytes", "job.dat"), registry);
    // Not clamped into range: a clamp would turn a broken probe into a confident
    // one and run the wrong parser.
    ASSERT_EQ(detection.candidates().size(), 1u);
    EXPECT_EQ(detection.candidates().front().formatId, "sound");
    EXPECT_EQ(detection.outcome(), DetectionOutcome::Identified);
    EXPECT_NE(detection.summary().find("broken"), std::string::npos);
    EXPECT_NE(detection.summary().find("nonsense"), std::string::npos);
}

TEST(SurveyFormatDetection, BinaryGarbageThatNoProbeClaimsIsUncertainRatherThanTheFirstFormat)
{
    FormatRegistry registry;
    ASSERT_TRUE(registry.add(descriptorFor("magic"), &magicProbe).ok());

    const std::string garbage = bytesOf({0x00, 0x01, 0xFF, 0xFE, 0x7F, 0x00, 0x13, 0x00});
    const Detection detection = detectFormat(probeOf(garbage, "job.magic"), registry);
    EXPECT_EQ(detection.outcome(), DetectionOutcome::Uncertain);
    EXPECT_TRUE(detection.candidates().empty());
    EXPECT_FALSE(detection.format().ok());
}

TEST(SurveyFormatDetection, TheDefaultOverloadAnswersFromTheProcessWideRegistry)
{
    // Which parsers are linked into this binary is not this test's business; the
    // invariant is. A format comes back exactly when one was identified.
    const std::string bytes = "110001+00000001 81..00+00012345\r\n";
    const Detection detection = detectFormat(probeOf(bytes, "job.gsi"));
    EXPECT_EQ(detection.format().ok(), detection.outcome() == DetectionOutcome::Identified);
    EXPECT_FALSE(detection.summary().empty());
}

// ---- detecting a file on disk ---------------------------------------------------

TEST(SurveyFormatDetectionOfFiles, AFileThatIsNotThereFailsRatherThanReportingUncertain)
{
    const FormatRegistry registry;
    const std::filesystem::path missing =
        std::filesystem::path(::testing::TempDir()) / "katana_no_such_survey_file.gsi";
    std::filesystem::remove(missing);

    const katana::core::Result<Detection> detection = detectFile(missing, registry);
    ASSERT_FALSE(detection.ok());
    EXPECT_EQ(detection.error().code, ErrorCode::FileImportFailure);
}

TEST(SurveyFormatDetectionOfFiles, AnEmptyFileIsNotAFailureItIsAnEmptyDetection)
{
    const FormatRegistry registry;
    const std::filesystem::path path = writeTemporaryFile("katana_empty_survey_file.gsi", "");

    const katana::core::Result<Detection> detection = detectFile(path, registry);
    ASSERT_TRUE(detection.ok());
    EXPECT_EQ(detection.value().outcome(), DetectionOutcome::Empty);
    std::filesystem::remove(path);
}

TEST(SurveyFormatDetectionOfFiles, AFileIsProbedWithItsOwnNameAndItsOwnBytes)
{
    FormatRegistry registry;
    ASSERT_TRUE(registry.add(descriptorFor("magic"), &magicProbe).ok());
    const std::filesystem::path path =
        writeTemporaryFile("katana_survey_probe.magic", "MAGIC\r\n1 2 3\r\n");

    const katana::core::Result<Detection> detection = detectFile(path, registry);
    ASSERT_TRUE(detection.ok());
    ASSERT_EQ(detection.value().outcome(), DetectionOutcome::Identified);
    EXPECT_EQ(detection.value().format().value().id, "magic");

    // The same bytes under a name the probe does not accept are not that format.
    const std::filesystem::path wrongName =
        writeTemporaryFile("katana_survey_probe.txt", "MAGIC\r\n1 2 3\r\n");
    const katana::core::Result<Detection> byName = detectFile(wrongName, registry);
    ASSERT_TRUE(byName.ok());
    EXPECT_EQ(byName.value().outcome(), DetectionOutcome::Uncertain);

    std::filesystem::remove(path);
    std::filesystem::remove(wrongName);
}

// ---- helpers the probes share ---------------------------------------------------

TEST(SurveyProbeInput, OnlyTheNameOfWhateverTheCallerSuppliedReachesAProbe)
{
    const std::string bytes = "x";
    // A probe is never handed something it could open, whatever the caller passes.
    EXPECT_EQ(probeOf(bytes, "..\\..\\secret\\job.GSI").fileName, "job.GSI");
    EXPECT_EQ(probeOf(bytes, "/etc/passwd").fileName, "passwd");

    // The extension is lower-cased once, here, so no two probes disagree on it.
    EXPECT_EQ(probeOf(bytes, "..\\..\\secret\\job.GSI").extension, "gsi");
    EXPECT_EQ(probeOf(bytes, "/etc/passwd").extension, "");
    EXPECT_EQ(probeOf(bytes, "job.").extension, "");
    // A leading dot makes a hidden file, not an extension.
    EXPECT_EQ(probeOf(bytes, ".gsi").extension, "");
    EXPECT_EQ(probeOf(bytes, "archive.tar.GZ").extension, "gz");
}

TEST(SurveyProbeHelpers, AByteOrderMarkIsSteppedOverWhicheverOneItIs)
{
    EXPECT_EQ(withoutByteOrderMark(bytesOf({0xEF, 0xBB, 0xBF, 'x'})), "x");
    EXPECT_EQ(withoutByteOrderMark(bytesOf({0xFF, 0xFE, 'x'})), "x");
    EXPECT_EQ(withoutByteOrderMark(bytesOf({0xFE, 0xFF, 'x'})), "x");
    // UTF-32LE begins with the UTF-16LE mark, so the longer mark is tested first;
    // otherwise two NUL bytes would be left at the front of every UTF-32 file.
    EXPECT_EQ(withoutByteOrderMark(bytesOf({0xFF, 0xFE, 0x00, 0x00, 'x'})), "x");
    EXPECT_EQ(withoutByteOrderMark(bytesOf({0x00, 0x00, 0xFE, 0xFF, 'x'})), "x");

    EXPECT_EQ(withoutByteOrderMark("no mark here"), "no mark here");
    EXPECT_EQ(withoutByteOrderMark(""), "");
    // Two bytes that merely start like a mark are not one.
    EXPECT_EQ(withoutByteOrderMark(bytesOf({0xEF, 'x'})).size(), 2u);
}

TEST(SurveyProbeHelpers, EveryKindOfLineEndingEndsALine)
{
    const std::string mixed = "alpha\r\nbravo\ncharlie\rdelta";
    const std::vector<std::string_view> lines = probeLines(probeOf(mixed, "job.txt"), 10);
    ASSERT_EQ(lines.size(), 4u);
    EXPECT_EQ(lines[0], "alpha");
    EXPECT_EQ(lines[1], "bravo");
    EXPECT_EQ(lines[2], "charlie");
    EXPECT_EQ(lines[3], "delta");

    EXPECT_EQ(probeLines(probeOf(mixed, "job.txt"), 2).size(), 2u);
    EXPECT_TRUE(probeLines(probeOf(mixed, "job.txt"), 0).empty());
    EXPECT_TRUE(probeLines(probeOf("", "job.txt"), 5).empty());

    // A blank line is a line: a probe counting records must see it.
    const std::vector<std::string_view> blank =
        probeLines(probeOf("alpha\n\nbravo\n", "job.txt"), 10);
    ASSERT_EQ(blank.size(), 3u);
    EXPECT_EQ(blank[1], "");

    // The mark belongs to the file, not to the first line.
    const std::string withMark = bytesOf({0xEF, 0xBB, 0xBF}) + "header\nbody\n";
    EXPECT_EQ(probeLines(probeOf(withMark, "job.txt"), 1).front(), "header");
}

TEST(SurveyProbeHelpers, ATruncatedProbeDropsItsLastPartialLine)
{
    // Whether the last line is a record or the place kProbeBytes fell is not
    // decidable from the bytes, so a probe must not be shown it.
    const std::string bytes = "first\nsecond\nthir";
    EXPECT_EQ(probeLines(probeOf(bytes, "job.txt", true), 10).size(), 2u);
    EXPECT_EQ(probeLines(probeOf(bytes, "job.txt", false), 10).size(), 3u);

    // A line that DID end is kept either way.
    const std::string terminated = "first\nsecond\n";
    EXPECT_EQ(probeLines(probeOf(terminated, "job.txt", true), 10).size(), 2u);
}

TEST(SurveyProbeHelpers, TextIsToldFromBinaryByItsControlBytes)
{
    EXPECT_TRUE(looksLikeText("110001+00000001 81..00+00012345\r\n"));
    EXPECT_TRUE(looksLikeText("one\ttwo\r\n"));

    // Nothing to judge is not evidence of text.
    EXPECT_FALSE(looksLikeText(""));
    // A NUL settles it: no format read here is UTF-16.
    EXPECT_FALSE(looksLikeText(bytesOf({'a', 'b', 0x00, 'c'})));
    EXPECT_FALSE(looksLikeText(bytesOf({0x01, 0x02, 0x03, 0x04})));

    // One control byte in two hundred is ordinary in instrument output.
    std::string mostlyText(200, 'a');
    mostlyText[100] = '\x0C'; // form feed
    EXPECT_TRUE(looksLikeText(mostlyText));
    // Three in two hundred is past the one-in-a-hundred policy.
    mostlyText[101] = '\x0C';
    mostlyText[102] = '\x0C';
    EXPECT_FALSE(looksLikeText(mostlyText));
}
