// katana_customisation_convert --name <name> [options] [-o <output>] <file>...
//
// Converts a customisation kept in the older formats - style libraries (.4d)
// and survey code files (.mapfile) - into ONE Katana customisation file
// (docs/customisation.md, "Converting a customisation from the legacy
// formats"). The files are given in LOAD ORDER, which is their meaning: a
// later library wins a definition, an earlier survey code file wins a field.
// What each file is, is decided by looking inside it.
//
//   --name <name>                the customisation's name (required)
//   --description <text>         a line for whoever picks it from a list
//   --notice-from <file>         that file's leading // comment block becomes
//                                the notice; repeatable
//   --colours <table>            lines of `R G B <index> "name"`: the colours
//                                of the names the standard ones lack
//   --strip-leading-word <word>  taken off the front of definition names,
//                                group paths and what the rules name;
//                                repeatable
//   --remove-word <word>         taken out of rule comments; repeatable
//   -o <output>                  the file to write; without it nothing is
//                                written and the report alone is printed
//
// It prints what it did as key=value lines and exits 0, or prints why it
// could not and exits 1. Everything it does is convert.cpp, which is what the
// tests call; this file only reads the command line.
//
// A developer's tool: built with everything else, never installed, and no
// part of katana, katana_cli or katana_mcp.

#include <cstdio>
#include <exception>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "convert.hpp"

#ifdef _WIN32
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include <shellapi.h>
#endif

namespace a12 = katana::archive12d;

namespace {

constexpr const char* kUsage =
    "usage: katana_customisation_convert --name <name> [--description <text>]\n"
    "           [--notice-from <file>]... [--colours <table>]\n"
    "           [--strip-leading-word <word>]... [--remove-word <word>]...\n"
    "           [-o <output>] <style library or survey code file>...\n"
    "The files are read in the order given: a later library wins a definition,\n"
    "an earlier survey code file wins a field.\n";

// The arguments after the program's own name, as UTF-8.
//
// On Windows `argv` is in the ANSI code page: a file name outside it does not
// arrive at all, and a customisation's name or a word to strip arrives as
// bytes that are not UTF-8, which the format refuses and no definition's name
// begins with. So the command line is taken as the UTF-16 it is and
// converted. nullopt when it cannot be had, or holds an unpaired surrogate.
[[nodiscard]] std::optional<std::vector<std::string>> argumentsAsUtf8([[maybe_unused]] int argc,
                                                                      [[maybe_unused]] char* argv[])
{
#ifdef _WIN32
    int count = 0;
    wchar_t** wide = CommandLineToArgvW(GetCommandLineW(), &count);
    if (wide == nullptr) {
        return std::nullopt;
    }
    std::optional<std::vector<std::string>> arguments{std::in_place};
    for (int i = 1; i < count; ++i) {
        // The length asked for and given includes the terminating NUL, which
        // lands on the string's own.
        const int size =
            WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, wide[i], -1, nullptr, 0, nullptr, nullptr);
        if (size <= 0) {
            arguments.reset();
            break;
        }
        std::string text(static_cast<std::size_t>(size - 1), '\0');
        WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, wide[i], -1, text.data(), size, nullptr,
                            nullptr);
        arguments->push_back(std::move(text));
    }
    LocalFree(wide);
    return arguments;
#else
    return std::vector<std::string>(argv + 1, argv + argc);
#endif
}

// A path from UTF-8. Never from the bytes as they are, which a Windows build
// would read in the ANSI code page.
[[nodiscard]] std::filesystem::path pathOf(const std::string& utf8)
{
    return std::filesystem::path(std::u8string(utf8.begin(), utf8.end()));
}

int run(const std::vector<std::string>& arguments)
{
    a12::ConvertRequest request;
    bool optionsEnded = false;
    for (std::size_t i = 0; i < arguments.size(); ++i) {
        const std::string& argument = arguments[i];
        if (optionsEnded || argument.empty() || argument.front() != '-') {
            request.files.push_back(pathOf(argument));
            continue;
        }
        if (argument == "--") {
            optionsEnded = true;
            continue;
        }
        if (argument == "--help" || argument == "-h") {
            std::fputs(kUsage, stdout);
            return 0;
        }
        const bool known = argument == "--name" || argument == "--description" ||
                           argument == "--notice-from" || argument == "--colours" ||
                           argument == "--strip-leading-word" || argument == "--remove-word" ||
                           argument == "-o";
        if (!known) {
            std::fprintf(stderr, "katana_customisation_convert: %s is not an option\n%s",
                         argument.c_str(), kUsage);
            return 1;
        }
        if (i + 1 == arguments.size()) {
            std::fprintf(stderr, "katana_customisation_convert: %s is not followed by its value\n%s",
                         argument.c_str(), kUsage);
            return 1;
        }
        const std::string& value = arguments[++i];
        if (argument == "--name") {
            request.name = value;
        } else if (argument == "--description") {
            request.description = value;
        } else if (argument == "--notice-from") {
            request.noticeFrom.push_back(pathOf(value));
        } else if (argument == "--colours") {
            request.colours = pathOf(value);
        } else if (argument == "--strip-leading-word") {
            request.stripLeadingWords.push_back(value);
        } else if (argument == "--remove-word") {
            request.removeWords.push_back(value);
        } else {
            request.output = pathOf(value);
        }
    }

    const auto conversion = a12::convertLegacyFiles(request);
    if (!conversion) {
        std::fprintf(stderr, "katana_customisation_convert: %s\n",
                     conversion.error().describe().c_str());
        return 1;
    }
    std::fputs(a12::toText(conversion->report).c_str(), stdout);
    return 0;
}

} // namespace

int main(int argc, char* argv[])
{
    // Whatever goes wrong is a reason on standard error and exit code 1, the
    // one failure a caller of this program has to know: an exception let out
    // of main ends it by std::terminate, with a status that is not 1.
    try {
        const auto arguments = argumentsAsUtf8(argc, argv);
        if (!arguments) {
            std::fputs("katana_customisation_convert: the command line cannot be read as text\n",
                       stderr);
            return 1;
        }
        return run(*arguments);
    } catch (const std::exception& error) {
        std::fprintf(stderr, "katana_customisation_convert: %s\n", error.what());
    } catch (...) {
        std::fputs("katana_customisation_convert: it failed, and did not say why\n", stderr);
    }
    return 1;
}
