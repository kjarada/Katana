// Reading a customisation file, and what a session starts with
// (customisation_host.hpp).

#include "katana/cad/customisation_host.hpp"

#include <algorithm>
#include <optional>
#include <span>
#include <system_error>
#include <utility>

#include "katana/cad/style_catalogue.hpp"
#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"

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
    report.symbols = symbolDefinitionCount(document);
    report.rules = document.surveyMap().size();
    report.colours = document.customisationState().colours.size();
}

} // namespace

katana::core::Status installBuiltInCustomisation(Document& document,
                                                 const BuiltInCustomisation& builtIn, bool kept)
{
    if (builtIn.customisation == nullptr) {
        return makeError(katana::core::ErrorCode::InvalidState,
                         "there is no built-in customisation to install");
    }
    // The one copy of it an install makes: the Document owns what it is
    // drawn with, and an editor changes that in place.
    katana::entity::Customisation copy = *builtIn.customisation;
    // Based on ITSELF, whatever its file says it was made from: a built-in
    // that was itself exported from a session names an earlier one, and a
    // copy kept from it would carry that digest - not this built-in's - and
    // be reported as made from another, at every start, though nothing had
    // changed. With no digest there is nothing to be based on by.
    copy.basedOn.reset();
    if (!builtIn.digest.empty()) {
        copy.basedOn = katana::entity::CustomisationBase{copy.name, builtIn.digest};
    }
    return document.installCustomisation(std::move(copy), CustomisationOrigin::BuiltIn, kept);
}

const std::vector<BuiltInEntry>& builtInsOf(const CustomisationHost& host)
{
    return host.builtIns ? *host.builtIns : builtIns();
}

std::vector<BuiltInChoice> builtInChoices(const Document& document,
                                          std::span<const BuiltInEntry> entries)
{
    const CustomisationState& state = document.customisationState();
    std::vector<BuiltInChoice> choices;
    choices.reserve(entries.size());
    for (const BuiltInEntry& entry : entries) {
        BuiltInChoice choice;
        choice.name = entry.name;
        choice.description = entry.description;
        choice.source = entry.source;
        choice.problem = entry.builtIn.problem;
        if (entry.builtIn.customisation != nullptr) {
            const katana::entity::Customisation& customisation = *entry.builtIn.customisation;
            choice.definitions = customisation.library.size();
            choice.codes = customisation.map.keys().size();
            choice.rules = customisation.map.size();
        }
        choice.isDefault = !state.builtIn.empty() && entry.name == state.builtIn;
        choice.isActive = state.origin == CustomisationOrigin::BuiltIn && entry.name == state.name &&
                          entry.builtIn.customisation != nullptr;
        choices.push_back(std::move(choice));
    }
    return choices;
}

namespace {

// Whether a kept customisation that says it is `basedOn` was made from
// something the program does not have. The run's own built-in is the common
// case and is judged first: by its name, and by its bytes (a host that could
// give no digest has no edition to differ from). Another built-in the program
// offers is not "another": the user chose it (CUSTOMISE BUILTIN, then KEEP),
// and a copy of it kept says so in the same two words. It is another when the
// program has no such built-in, or has another edition of it.
bool madeFromAnotherBuiltIn(const katana::entity::CustomisationBase& basedOn,
                            const CustomisationHost& host)
{
    const BuiltInCustomisation* ours = &host.builtIn;
    // The list is looked through only when the name is not the run's: it may
    // be parsed for it, and a start that finds its own needs none of that.
    if (basedOn.name != host.builtIn.customisation->name) {
        const std::vector<BuiltInEntry>& offered = builtInsOf(host);
        const auto found = std::ranges::find_if(offered, [&](const BuiltInEntry& entry) {
            return entry.name == basedOn.name && entry.builtIn.customisation != nullptr;
        });
        if (found == offered.end()) {
            return true;
        }
        ours = &found->builtIn;
    }
    return !ours->digest.empty() && basedOn.digest != ours->digest;
}

// The choice startCustomisation makes, and its report.
CustomisationStart chooseAndInstall(Document& document, const CustomisationHost& host)
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
                        report.keptFromAnotherBuiltIn = madeFromAnotherBuiltIn(*basedOn, host);
                    }
                    return report;
                }
                report.problems.push_back("the kept customisation is not installed, so " +
                                          fallback + ": " + installed.error().describe());
            }
        }
    }

    if (hasBuiltIn) {
        const auto installed = installBuiltInCustomisation(document, host.builtIn, true);
        if (!installed) {
            report.problems.push_back("the built-in customisation is not installed: " +
                                      installed.error().describe());
            return report;
        }
        summarise(report, document, CustomisationOrigin::BuiltIn);
    }
    return report;
}

} // namespace

CustomisationStart startCustomisation(Document& document, const CustomisationHost& host)
{
    CustomisationStart report = chooseAndInstall(document, host);
    // Left on the Document as well as handed back. The front end says it
    // once, where its errors go; whoever asks what the session holds later -
    // CUSTOMISE JSON, a client of katana_mcp, which is never shown that
    // stream - is then still told that the customisation it kept is not the
    // one it got.
    document.setCustomisationStart(report.problems, report.keptFromAnotherBuiltIn);
    return report;
}

} // namespace katana::cad
