// Reading a customisation file, and what a session starts with
// (customisation_host.hpp).

#include "katana/cad/customisation_host.hpp"

#include <optional>
#include <system_error>
#include <utility>

#include "katana/core/path_text.hpp"

namespace katana::cad {

using katana::core::makeError;

katana::core::Result<CustomisationFile> readCustomisationFile(const std::filesystem::path& path)
{
    auto bytes = katana::core::readFileBytes(path);
    if (!bytes) {
        return bytes.error();
    }
    auto read = katana::entity::customisationFromJson(*bytes);
    if (!read) {
        // The reader says which entry and which member; which FILE is ours
        // to add, and goes first so that a list of several reads as a list.
        const katana::core::Error& refusal = read.error();
        return makeError(refusal.code, refusal.message,
                         katana::core::pathToUtf8(path) +
                             (refusal.context.empty() ? std::string{} : ": " + refusal.context));
    }
    return CustomisationFile{std::move(*read), katana::entity::customisationDigest(*bytes)};
}

namespace {

// What is installed now, as the report gives it.
void summarise(CustomisationStart& report, const Document& document, CustomisationOrigin origin)
{
    report.installed = origin;
    report.name = document.customisationState().name;
    report.definitions = document.styleLibrary().size();
    report.symbols = 0;
    document.styleLibrary().forEach([&](const katana::entity::LineStyle& definition) {
        report.symbols += definition.symbol ? 1 : 0;
    });
    report.rules = document.surveyMap().size();
    report.colours = document.customisationState().colours.size();
}

} // namespace

CustomisationStart startCustomisation(Document& document, const CustomisationHost& host)
{
    CustomisationStart report;
    const bool hasBuiltIn = host.builtIn.customisation != nullptr;
    if (!host.builtIn.problem.empty()) {
        report.problems.push_back(host.builtIn.problem);
    }
    document.setBuiltInCustomisationName(hasBuiltIn ? host.builtIn.customisation->name
                                                    : std::string());

    // The user's own first: it is what they kept. A kept file that is not
    // there is the ordinary case - nobody has kept anything - and is no
    // problem; one that is there and does not read is said, and the built-in
    // stands in for it rather than the session starting with nothing.
    if (!host.keptFile.empty()) {
        std::error_code unknown;
        const bool there = std::filesystem::exists(host.keptFile, unknown) || unknown;
        if (there) {
            const std::string fallback =
                hasBuiltIn ? "the built-in customisation is used" : "no customisation is loaded";
            auto kept = readCustomisationFile(host.keptFile);
            if (!kept) {
                report.problems.push_back("the kept customisation is not read, so " + fallback +
                                          ": " + kept.error().describe());
            } else {
                const std::optional<katana::entity::CustomisationBase> basedOn =
                    kept->customisation.basedOn;
                const auto installed = document.installCustomisation(
                    std::move(kept->customisation), CustomisationOrigin::Kept, true);
                if (installed) {
                    summarise(report, document, CustomisationOrigin::Kept);
                    if (basedOn && hasBuiltIn) {
                        // Another built-in by its name, or another edition
                        // of this one by its bytes. A host that could give
                        // no digest has no edition to differ from.
                        const bool otherName = basedOn->name != host.builtIn.customisation->name;
                        const bool otherEdition = !host.builtIn.digest.empty() &&
                                                  basedOn->digest != host.builtIn.digest;
                        report.keptFromAnotherBuiltIn = otherName || otherEdition;
                    }
                    return report;
                }
                report.problems.push_back("the kept customisation is not installed, so " +
                                          fallback + ": " + installed.error().describe());
            }
        }
    }

    if (hasBuiltIn) {
        // The one copy of it a start makes: the Document owns what it is
        // drawn with, and an editor changes that in place.
        katana::entity::Customisation builtIn = *host.builtIn.customisation;
        // Based on ITSELF, whatever its file says it was made from: a
        // built-in that was itself exported from a session names an earlier
        // one, and a copy kept from it would carry that digest - not this
        // built-in's - and be reported as made from another, at every start,
        // though nothing had changed. With no digest there is nothing to be
        // based on by.
        builtIn.basedOn.reset();
        if (!host.builtIn.digest.empty()) {
            builtIn.basedOn = katana::entity::CustomisationBase{builtIn.name, host.builtIn.digest};
        }
        const auto installed = document.installCustomisation(std::move(builtIn),
                                                             CustomisationOrigin::BuiltIn, true);
        if (!installed) {
            report.problems.push_back("the built-in customisation is not installed: " +
                                      installed.error().describe());
            return report;
        }
        summarise(report, document, CustomisationOrigin::BuiltIn);
    }
    return report;
}

} // namespace katana::cad
