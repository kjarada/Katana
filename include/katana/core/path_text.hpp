#pragma once

// A path as the text a person typed, and a file as the bytes it holds.
//
// Text in Katana is UTF-8 (core/text_encoding.hpp), and a path arrives as
// text: typed on a command line, read from a script, named by a settings
// file or an environment variable. What narrow bytes handed to
// std::filesystem::path MEAN is the implementation's to say, and on Windows
// the two Katana meets do not agree: the C runtime opens a narrow name in the
// ANSI code page, and the standard library it is built with (libstdc++;
// probed with GCC 16.2 on 2026-10-06) converts one as UTF-8 and THROWS at the
// first byte that is not. So a name is not handed to it as it comes: "café"
// from an ANSI console would end the program. These are the one place the
// conversion is made, in each direction.
// It was a function private to the sheet verbs, with a second copy in the
// utility verbs, before the customisation needed a third.

#include <filesystem>
#include <string>
#include <string_view>

#include "katana/core/error.hpp"

namespace katana::core {

// The path `text` names. No file is touched.
//
// UTF-8 is read as UTF-8, through char8_t, which every implementation reads
// so. Bytes that are NOT UTF-8 are taken as the narrow name they are - the
// file the C runtime would open if handed them, which on Windows is the name
// read in the ANSI code page: katana_cli's arguments and a script saved from
// an ANSI editor arrive that way ("café" in the ANSI code page is not
// UTF-8). It never throws. std::filesystem::path constructed from such bytes
// does, here, and nothing up a verb's call chain catches it.
[[nodiscard]] std::filesystem::path pathFromUtf8(std::string_view text);

// The path as UTF-8 text, for a message or a reply.
[[nodiscard]] std::string pathToUtf8(const std::filesystem::path& path);

// The value of the environment variable `name` as text; empty when it is not
// set, and when it is set to nothing - one case to every reader here, as it
// is to Windows' own `set NAME=`, which removes the variable. `name` is
// ASCII: a variable of Katana's own.
//
// On Windows the value is read WIDE and handed back as UTF-8. std::getenv
// gives the ANSI code page's bytes there, and by the time it has them a
// character the code page has no byte for is already a '?': a kept
// customisation under a folder named in Japanese, on a machine set up for
// English, was a file under "??" - not there, so not read, with nothing said
// - and a KEEP then failed to write it. Elsewhere the bytes are the
// variable's own. Either way pathFromUtf8 makes the path of them, so a file
// a variable names is asked for through here and never through getenv.
[[nodiscard]] std::string environmentVariable(const char* name);

// Every byte of the file, undecoded - what a reader that decodes for itself
// (core::decodeText) and a digest both want. NotFound when the file cannot be
// opened (it is not there, is a directory, or may not be read) and
// FileImportFailure when reading it fails part way; the context is the path.
[[nodiscard]] Result<std::string> readFileBytes(const std::filesystem::path& path);

} // namespace katana::core
