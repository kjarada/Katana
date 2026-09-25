#pragma once

// A name as the command line reads it, for a dialog that builds the verb line
// a person would type (command_runner.hpp): double-quoted when it holds a
// blank - or is empty - which the interpreter's tokenizer would otherwise
// split or drop. A double quote cannot be carried at all, since the
// tokenizer's quoted words have no escape, so a name holding one is refused
// naming `what` rather than cut short at the quote.

#include <algorithm>

#include <QString>

#include "katana/core/error.hpp"

namespace katana::qt {

[[nodiscard]] inline katana::core::Result<QString> commandWord(const QString& text,
                                                               const QString& what)
{
    if (text.contains('"')) {
        return katana::core::makeError(katana::core::ErrorCode::InvalidArgument,
                                       what.toStdString() + " holds a double quote, which a "
                                                            "command line cannot carry",
                                       text.toStdString());
    }
    const bool blank = text.isEmpty() ||
                       std::any_of(text.begin(), text.end(), [](QChar c) { return c.isSpace(); });
    return blank ? "\"" + text + "\"" : text;
}

} // namespace katana::qt
