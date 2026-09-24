// Leica DBX jobs: recognised and refused, with the export to make instead.
//
// DBX is the job database of Leica's field software (System 1200, Viva,
// Captivate): a folder holding an index file, <job>.xcf, and data files
// <job>.x01, <job>.x02 ... Leica publishes no specification of it. The
// open-source Total Open Station project records it as "a binary, undocumented
// file format" (github.com/totalopenstation/totalopenstation issue 38, which is
// also where the .xcf index / .x01, .x02 file set is described), and Leica's
// own route out of a job is an export: GSI, LandXML, or Leica's XML (HeXML).
//
// So nothing here reads a DBX value. A reader that guessed at an undocumented
// binary layout would produce coordinates that look right and are not, which
// is worse than no reader. What it does is recognise the job - its files by
// name, confirmed through the sibling lookup, since a job is several files -
// and say plainly what to export and import instead. It is registered WITH a
// reader, one that always refuses, so that the import wizard (which offers the
// column layout page to any format without a reader) gives this sentence rather
// than a page of columns for a binary file.

#include <string>
#include <string_view>

#include "katana/surveyio/detect.hpp"
#include "katana/surveyio/format.hpp"
#include "katana/surveyio/leica.hpp"

namespace katana::surveyio {

namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

constexpr const char* kParserVersion = "1.0";

constexpr std::string_view kRefusal =
    "Leica DBX is Leica's internal job database and has no published format; export the job "
    "from Captivate/Infinity as GSI-16 or LandXML (or HeXML) and import that.";

bool isDigit(char c)
{
    return c >= '0' && c <= '9';
}

// "x01" .. "x99": a DBX job's data files.
bool isDataExtension(std::string_view extension)
{
    return extension.size() == 3 && extension[0] == 'x' && isDigit(extension[1]) &&
           isDigit(extension[2]) && extension != "x00";
}

bool startsWith(std::string_view text, std::string_view prefix)
{
    return text.substr(0, prefix.size()) == prefix;
}

// Binary content: a NUL, or a large share of bytes that no text file holds.
bool looksBinary(std::string_view bytes)
{
    std::size_t control = 0;
    const std::string_view sample = bytes.substr(0, 4096);
    for (const char c : sample) {
        const auto byte = static_cast<unsigned char>(c);
        if (byte == 0) {
            return true;
        }
        if (byte < 0x09 || (byte > 0x0D && byte < 0x20)) {
            ++control;
        }
    }
    return !sample.empty() && control * 10 > sample.size();
}

FormatDescriptor dbxFormat()
{
    FormatDescriptor format;
    format.id = std::string(kLeicaDbxFormatId);
    // The name says what happens, because describeFormat() will say "import:
    // yes" - the registry lets only an importable format carry the reader
    // that gives the refusal.
    format.humanName = "Leica DBX job (recognised; export GSI or LandXML to import)";
    format.manufacturer = Manufacturer::Leica;
    format.reads = {}; // nothing: see the file comment
    format.canImport = true;
    format.canExport = false;
    format.parserVersion = kParserVersion;
    format.extensions = {"xcf"};
    return format;
}

FormatSignature probeDbx(const ProbeInput& input)
{
    const bool index = input.extension == "xcf";
    const bool data = isDataExtension(input.extension);
    if (!index && !data) {
        return ruledOut();
    }
    // GIMP's image format shares the extension and names itself.
    if (startsWith(input.bytes, "gimp xcf")) {
        return ruledOut();
    }
    if (!looksBinary(input.bytes)) {
        // A text file called .xcf / .x01 is not a DBX job file, but the name
        // alone is some evidence.
        return {0.3, "extension ." + input.extension + ", but the content is text, which a DBX "
                                                        "job file is not"};
    }
    if (index) {
        return {0.9, "extension .xcf, binary, not a GIMP image: the index file of a Leica DBX job"};
    }
    return {0.75,
            "extension ." + input.extension + ", binary: a data file of a Leica DBX job"};
}

// The job's other files, by name: the index names the job, the data files
// share its stem. Only whether they are there is looked at.
Result<ReadResult> readDbx(std::string_view bytes, std::string_view fileName,
                           const ReadOptions& options)
{
    (void)bytes;
    const std::size_t dot = fileName.find_last_of('.');
    const std::string stem(fileName.substr(0, dot));
    std::string found;
    if (options.siblings && dot != std::string_view::npos) {
        const bool isIndex = fileName.size() - dot == 4 &&
                             (fileName[dot + 1] == 'x' || fileName[dot + 1] == 'X') &&
                             (fileName[dot + 2] == 'c' || fileName[dot + 2] == 'C');
        const std::string other = stem + (isIndex ? ".x01" : ".xcf");
        if (options.siblings(other)) {
            found = std::string(fileName) + " with " + other + " beside it";
        }
    }
    std::string message = found.empty()
                              ? std::string(fileName) + " looks like a file of a Leica DBX job. "
                              : "This is a Leica DBX job (" + found + "). ";
    message += kRefusal;
    return makeError(ErrorCode::Unsupported, std::move(message), std::string(fileName));
}

const FormatRegistration kRegistration{dbxFormat(), &probeDbx, &readDbx};

} // namespace

} // namespace katana::surveyio
