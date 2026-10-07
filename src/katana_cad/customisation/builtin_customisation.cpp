// The built-in customisation: the one compiled into the program, and the seam
// that says what "the built-in" is for one run (customisation_host.hpp).

#include "katana/cad/customisation_host.hpp"

#include <string>
#include <string_view>
#include <utility>

#include "katana/core/path_text.hpp"
#include "katana/core/text.hpp"

namespace katana::cad {

namespace {

// The built-in's bytes, embedded by the compiler (#embed). The file is named
// by KATANA_BUILTIN_CUSTOMISATION_FILE and found through --embed-dir, both
// given to THIS file alone by src/katana_cad/CMakeLists.txt, and only when the
// file exists: a build whose path names no file has none, and compiles the
// other branch. The dependency file names the JSON, so editing it rebuilds this
// file. #embed is C++26; the pragma keeps a C++23 build (KATANA_CXX_STANDARD)
// from failing -Werror on it.
#if defined(KATANA_BUILTIN_CUSTOMISATION_FILE)
#pragma GCC diagnostic push
#if defined(__clang__)
// clang files #embed in C++ under the C23 extensions.
#pragma clang diagnostic ignored "-Wc23-extensions"
#else
#pragma GCC diagnostic ignored "-Wc++26-extensions"
#endif
constexpr unsigned char kCompiledIn[] = {
// An empty file is no customisation, and no array either: one zero byte
// stands for it, which the reader refuses in so many words.
#embed KATANA_BUILTIN_CUSTOMISATION_FILE if_empty(0)
};
#pragma GCC diagnostic pop

// A failure is reported, never thrown: a program whose built-in is damaged
// still starts, and says so.
[[nodiscard]] BuiltInCustomisation parseCompiledIn()
{
    const std::string_view bytes(reinterpret_cast<const char*>(kCompiledIn), sizeof kCompiledIn);
    BuiltInCustomisation builtIn;
    auto read = katana::entity::customisationFromJson(bytes);
    if (!read) {
        builtIn.problem = "the customisation compiled into this program is not read: " +
                          read.error().describe();
        return builtIn;
    }
    builtIn.customisation =
        std::make_shared<const katana::entity::Customisation>(std::move(*read));
    builtIn.digest = katana::entity::customisationDigest(bytes);
    return builtIn;
}
#else
// A build with no built-in: none, and no problem.
[[nodiscard]] BuiltInCustomisation parseCompiledIn() { return {}; }
#endif

} // namespace

const BuiltInCustomisation& compiledInCustomisation()
{
    // Parsed at the first call and never again: it is eight hundred
    // definitions, and a session that never asks pays nothing.
    static const BuiltInCustomisation compiledIn = parseCompiledIn();
    return compiledIn;
}

BuiltInCustomisation builtInCustomisation()
{
    // THE one place the seam is read. What it means is the function below's.
    // Through core, never getenv: on Windows that gives the ANSI code page's
    // bytes, in which a file named outside the code page is a file named
    // with '?' (core/path_text.hpp).
    return builtInCustomisationFor(katana::core::environmentVariable(kBuiltInCustomisationVariable),
                                   compiledInCustomisation());
}

BuiltInCustomisation builtInCustomisationFor(std::string_view seam,
                                             const BuiltInCustomisation& compiledIn)
{
    if (seam.empty()) {
        return compiledIn;
    }
    if (katana::core::lowered(seam) == "none") {
        return {};
    }
    // As text, through core's one conversion: on Windows the seam's value
    // arrives as UTF-8, but elsewhere, and from any other caller, the text
    // may be narrow bytes that are not - and a path built straight from
    // those THROWS at the first of them (path_text.hpp), out of a function
    // that promises to report and never throw.
    const std::filesystem::path path = katana::core::pathFromUtf8(seam);
    auto file = readCustomisationFile(path);
    if (!file) {
        BuiltInCustomisation none;
        none.problem = std::string("the file ") + kBuiltInCustomisationVariable +
                       " names as the built-in customisation is not read: " +
                       file.error().describe();
        return none;
    }
    BuiltInCustomisation builtIn;
    builtIn.customisation =
        std::make_shared<const katana::entity::Customisation>(std::move(file->customisation));
    builtIn.digest = std::move(file->digest);
    return builtIn;
}

} // namespace katana::cad
