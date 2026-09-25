#pragma once

// Values written as words of a command line, the inverse of
// CommandInterpreter::tokenize: what a dialog that builds a verb line
// (CLAUDE.md section 1; docs/desktop.md, "One executor") puts on it, so the
// verb reads back exactly the value the dialog showed.
//
// The tokenizer splits on blanks, groups quoted words and removes the
// quotes, and has no escape: a value holding a double quote cannot be said on
// a line at all. It is REFUSED here, naming it, rather than written and cut
// short at the quote - the silent failure of a line that runs but sets
// something other than what was asked.

#include <string>
#include <string_view>

#include "katana/core/error.hpp"

namespace katana::cad::annotation {

// `text` as one word: bare when it can be, double-quoted when it is empty or
// holds a blank. InvalidArgument for a double quote or a line break.
[[nodiscard]] katana::core::Result<std::string> commandWord(std::string_view text);

// An annotation text or template as one word of an annotation verb (TEXT,
// TEXTEDIT, LEADER text=, LABELSTYLE text=), whose values read "\n" - a
// backslash and an n - as a line break: each line break written so. Refused
// as commandWord refuses, and for a backslash already followed by an n,
// which the verb would read as a break the text does not have.
[[nodiscard]] katana::core::Result<std::string> annotationTextWord(std::string_view text);

// The other direction: a value as a field of a key=value REPLY record, which
// an agent reads rather than types - bare when it can be, else quoted with \",
// \\ and \n escaped (an empty value, or one holding a blank, a quote, a
// backslash, a line break or an '='). Every annotation verb's reply writes
// its values so.
[[nodiscard]] std::string recordValue(std::string_view value);

} // namespace katana::cad::annotation
