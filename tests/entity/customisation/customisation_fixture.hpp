#pragma once

// What the tests of the Katana customisation format share: the smallest file
// that is one, and reading and writing with the failure shown.

#include <gtest/gtest.h>

#include <string>
#include <string_view>

#include "katana/entity/customisation.hpp"

namespace katana::testing {

// A customisation holding only what every one must - format, version, name -
// and then `members`, written as they would be in the file.
inline std::string customisationWith(std::string_view members)
{
    std::string text = R"({"format": "katana-customisation", "version": 1, "name": "T")";
    if (!members.empty()) {
        text += ", ";
        text += members;
    }
    text += "}";
    return text;
}

// The customisation `text` holds; a failure to read is reported and gives an
// empty one, so the test goes on to say what else is wrong.
inline katana::entity::Customisation readCustomisation(std::string_view text)
{
    auto read = katana::entity::customisationFromJson(text);
    EXPECT_TRUE(read.ok()) << (read.ok() ? std::string{} : read.error().describe()) << "\n" << text;
    return read.ok() ? std::move(*read) : katana::entity::Customisation{};
}

// Why `text` is refused; reading it is the failure here.
inline katana::core::Error refusalOf(std::string_view text)
{
    const auto read = katana::entity::customisationFromJson(text);
    EXPECT_FALSE(read.ok()) << "read without complaint:\n" << text;
    return read.ok() ? katana::core::Error{} : read.error();
}

inline std::string writeCustomisation(const katana::entity::Customisation& customisation,
                                      const katana::entity::CustomisationWriteOptions& options = {})
{
    auto written = katana::entity::customisationToJson(customisation, options);
    EXPECT_TRUE(written.ok()) << (written.ok() ? std::string{} : written.error().describe());
    return written.ok() ? std::move(*written) : std::string{};
}

// A refusal names the entry first, then says what is wrong with which member:
// `codes[1] "WM*": unknown member "linesytle"`.
inline void expectNames(const katana::core::Error& error, std::string_view entry,
                        std::string_view member)
{
    const std::string start = std::string(entry) + ": ";
    EXPECT_EQ(error.message.compare(0, start.size(), start), 0)
        << "the refusal does not begin with the entry " << entry << ": " << error.describe();
    const std::string quoted = "\"" + std::string(member) + "\"";
    EXPECT_NE(error.message.find(quoted, start.size()), std::string::npos)
        << "the refusal does not name " << quoted << ": " << error.describe();
}

} // namespace katana::testing
