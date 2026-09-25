#pragma once

// Edit > Select by ID: the ids that LIST, INFO, AREA and a refusal print,
// turned back into a selection and found in the drawing (docs/desktop.md,
// "Select by ID").
//
// A front end over the interpreter's SELECT id id ...: it builds that line
// and hands it to the window's one executor (command_runner.hpp), so the
// log shows what was run and a refusal - an id that does not exist, or is on
// a hidden or locked layer - is SELECT's own. Non-modal and kept, so several
// lookups are made without reopening it.
//
// Object names: selectByIdDialog, selectByIdIds (the ids, separated by
// commas or blanks, #12 as well as 12), selectByIdAdd (add to the current
// selection), selectByIdZoom (frame what was selected; on by default),
// selectByIdSelect, selectByIdStatus (what SELECT answered) and
// selectByIdClose.

#include <functional>
#include <vector>

#include <QDialog>

#include "command_runner.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"

class QCheckBox;
class QLabel;
class QLineEdit;

namespace katana::qt {

// The ids in `text` - separated by commas or blanks, each a positive whole
// number, written 12 or #12 - in the order given, each once. InvalidArgument
// naming the first word that is not an id, or saying there is none.
[[nodiscard]] katana::core::Result<std::vector<katana::entity::EntityId>>
parseEntityIds(const QString& text);

// The SELECT line for `ids`: with `add`, the ones in `current` first and
// then the new ones, each once, as SELECT sets the selection to exactly what
// it names.
[[nodiscard]] QString selectByIdLine(const std::vector<katana::entity::EntityId>& ids,
                                     const std::vector<katana::entity::EntityId>& current,
                                     bool add);

struct SelectByIdContext {
    const katana::cad::Document* document = nullptr;
    CommandRunner run;
    // After SELECT succeeded: frame the selection when `zoom`, and show its
    // properties. The window's; a test's own.
    std::function<void(bool zoom)> onSelected;
};

class SelectByIdDialog final : public QDialog {
  public:
    explicit SelectByIdDialog(SelectByIdContext context, QWidget* parent = nullptr);

    // What Select does: parse, run SELECT, report. False when nothing was
    // selected - the ids did not parse or SELECT refused them - with the
    // reason in the status line.
    bool selectIds();

  private:
    SelectByIdContext context_;
    QLineEdit* ids_ = nullptr;
    QCheckBox* add_ = nullptr;
    QCheckBox* zoom_ = nullptr;
    QLabel* status_ = nullptr;
};

} // namespace katana::qt
