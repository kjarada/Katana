#pragma once

// The two environment variables a program's session reads when it starts
// (cad/customisation_host.hpp), set for one test: shared by the session's own
// tests and the MCP server's, which both start a session as katana_cli and
// katana_mcp do.

#include <cstdlib>
#include <filesystem>
#include <optional>
#include <string>

#include <gtest/gtest.h>

#include "katana/cad/customisation_host.hpp"

namespace katana::app::tests {

// An environment variable set for one test and put back as it was - set or
// not - when the test ends, so that no test depends on, or leaves a mark on,
// the environment it runs in.
class ScopedVariable {
  public:
    // nullptr: the variable is not set at all.
    ScopedVariable(const char* name, const char* value) : name_(name)
    {
        remember();
        set(value);
    }
    // A path as the value, EXACTLY: on Windows as its own wide text. A narrow
    // value there is the ANSI code page's, which cannot spell every name a
    // folder may have - and such a name is what some tests are about.
    ScopedVariable(const char* name, const std::filesystem::path& value) : name_(name)
    {
        remember();
#if defined(_WIN32)
        std::wstring wideName; // the name is ASCII
        for (const char* ch = name_; *ch != '\0'; ++ch) {
            wideName += static_cast<wchar_t>(*ch);
        }
        EXPECT_EQ(_wputenv_s(wideName.c_str(), value.c_str()), 0);
#else
        EXPECT_EQ(setenv(name_, value.c_str(), 1), 0);
#endif
    }
    ~ScopedVariable() { set(before_ ? before_->c_str() : nullptr); }
    ScopedVariable(const ScopedVariable&) = delete;
    ScopedVariable& operator=(const ScopedVariable&) = delete;

  private:
    void remember()
    {
        if (const char* before = std::getenv(name_)) {
            before_ = before;
        }
    }

    void set(const char* value) const
    {
#if defined(_WIN32)
        // An empty value removes the variable.
        EXPECT_EQ(_putenv_s(name_, value == nullptr ? "" : value), 0);
#else
        if (value == nullptr) {
            EXPECT_EQ(unsetenv(name_), 0);
        } else {
            EXPECT_EQ(setenv(name_, value, 1), 0);
        }
#endif
    }

    const char* name_;
    std::optional<std::string> before_;
};

// Both variables, set for one test: the built-in of the run, and the kept
// file. nullptr leaves that one unset.
struct StartEnvironment {
    ScopedVariable builtIn;
    ScopedVariable kept;
    StartEnvironment(const char* builtInValue, const char* keptValue)
        : builtIn(katana::cad::kBuiltInCustomisationVariable, builtInValue),
          kept(katana::cad::kKeptCustomisationVariable, keptValue)
    {
    }
};

// Two kanji, a hyphen and two Hebrew letters - U+65E5 U+672C, U+002D, U+05E2
// U+05D1 - in UTF-8, each worked out from its code point: E6 97 A5, E6 9C AC,
// 2D, D7 A2, D7 91. Two scripts because no ANSI code page holds both: on any
// Windows machine whose code page is not UTF-8 itself, the C runtime's narrow
// getenv has a '?' for some of them, and a file under a folder of this name
// cannot be named through it.
inline const std::string kTwoScripts = "\xE6\x97\xA5\xE6\x9C\xAC-\xD7\xA2\xD7\x91";

} // namespace katana::app::tests
