#pragma once

// Annotate > Edit Text... (docs/annotation.md, "In the window"): the one text
// selected in the drawing, every field of it in a form - its words over
// several lines, style, height on paper, model height, justification,
// rotation and position.
//
// Non-modal and following the selection: with no text, or more than one
// thing, selected, it says so and waits for one, rather than asking in a box
// of its own - so a headless run is never stopped by it. It changes nothing
// itself: Apply builds ONE TEXTEDIT line of the fields the form changed and
// runs it through the window's one executor (command_runner.hpp), so the edit
// is one undo step, echoed in the command log, and refused in the verb's own
// words. A value no line can carry (a double quote) is refused in the dialog
// before anything runs (cad/annotation/command_words.hpp).
//
// Object names, for the headless driver and the tests:
//   textEditDialog      the dialog
//   textEditStatus      which text it edits, or what to select
//   textEditText        the words, a line a line
//   textEditStyle       the text style, "(none)" for the plain face
//   textEditPaper       height on paper in mm; 0 for a model height
//   textEditHeight      model height (a paper-sized text's is worked out)
//   textEditJustify     TL TC TR ML MC MR BL BC BR
//   textEditRotation    degrees counter-clockwise from east
//   textEditPosition    the justification point, E,N
//   textEditApply, textEditRevert
//   textEditProblem     the last refusal

#include <QDialog>
#include <QString>

#include "command_runner.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"

class QComboBox;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;

namespace katana::qt {

class TextEditDialog final : public QDialog {
  public:
    TextEditDialog(katana::cad::Document& document, CommandRunner run, QWidget* parent = nullptr);

    // The text being edited; 0 when the selection is not one text.
    [[nodiscard]] katana::entity::EntityId textId() const { return id_; }
    // The line Apply runs: empty when the form changes nothing; an error
    // naming a field that does not read, or when there is no text to edit.
    [[nodiscard]] katana::core::Result<QString> applyLine() const;
    [[nodiscard]] QString status() const;
    [[nodiscard]] QString problem() const;

    bool apply();
    // Throws the form's edits away.
    void revert();

  private:
    // The fields as the form shows them, to tell an edit from what is stored.
    struct Shown {
        QString text;
        QString style;
        QString paper;
        QString height;
        QString justify;
        QString rotation;
        QString position;
    };

    void follow();
    void fillStyles();
    void load(katana::entity::EntityId id, const katana::entity::TextGeometry& text);
    [[nodiscard]] Shown form() const;
    void setEditable(bool editable);

    katana::cad::Document& document_;
    CommandRunner run_;
    katana::cad::Document::ListenerHandle listener_;
    QLabel* status_ = nullptr;
    QPlainTextEdit* text_ = nullptr;
    QComboBox* style_ = nullptr;
    QLineEdit* paper_ = nullptr;
    QLineEdit* height_ = nullptr;
    QComboBox* justify_ = nullptr;
    QLineEdit* rotation_ = nullptr;
    QLineEdit* position_ = nullptr;
    QPushButton* apply_ = nullptr;
    QPushButton* revert_ = nullptr;
    QLabel* problem_ = nullptr;
    katana::entity::EntityId id_ = 0;
    // What was loaded, to keep the form's edits through a change that leaves
    // the text alone and to replace them when it does not.
    katana::entity::TextGeometry loaded_{};
    Shown shown_{};
};

} // namespace katana::qt
