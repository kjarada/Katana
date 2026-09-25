#pragma once

// A name as the command line reads it, for a dialog that builds the verb line
// a person would type (command_runner.hpp): double-quoted when it holds a
// blank - or is empty - which the interpreter's tokenizer would otherwise
// split or drop. A double quote or a line break cannot be carried at all,
// since the tokenizer's quoted words have no escape, so a name holding one is
// refused naming `what` rather than cut short at the quote.
//
// The rule itself is katana_cad's (cad::annotation::commandWord), the one the
// annotation dialogs use: this only carries a QString to it and names the
// field in the refusal, so every dialog writes a word the same way.
// annotationTextWord is the same for an annotation's text, whose line breaks
// the line carries as a backslash and an n (cad::annotation::annotationTextWord).

#include <QString>

#include "katana/cad/annotation/command_words.hpp"
#include "katana/core/error.hpp"
#include "katana/core/text.hpp"

namespace katana::qt {

namespace detail {

// `word` for `text` as a QString, or its refusal with `what` named first.
[[nodiscard]] inline katana::core::Result<QString>
namedWord(const katana::core::Result<std::string>& word, const QString& text, const QString& what)
{
    if (!word) {
        return katana::core::makeError(word.error().code,
                                       what.toStdString() + ": " + word.error().message,
                                       text.toStdString());
    }
    return QString::fromStdString(*word);
}

} // namespace detail

[[nodiscard]] inline katana::core::Result<QString> commandWord(const QString& text,
                                                               const QString& what)
{
    return detail::namedWord(katana::cad::annotation::commandWord(text.toStdString()), text,
                             what);
}

// A number as a dialog writes it into its line: exactly, the shortest text
// that reads back as the same double (core::formatExactReal), so a value
// written back unedited changes nothing.
[[nodiscard]] inline QString exactNumber(double value)
{
    return QString::fromStdString(katana::core::formatExactReal(value));
}

[[nodiscard]] inline katana::core::Result<QString> annotationTextWord(const QString& text,
                                                                      const QString& what)
{
    return detail::namedWord(katana::cad::annotation::annotationTextWord(text.toStdString()),
                             text, what);
}

} // namespace katana::qt
