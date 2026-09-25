#pragma once

// Annotate > Edit Label... (docs/annotation.md, "In the window"): one label's
// style, its own words, where its text is pinned and its layer - what
// LABEL SET changes.
//
// The dialog changes nothing itself. OK and Apply write the LABEL SET line its
// fields describe - shown as it will run in labelEditCommand - naming only
// what differs from the label as it is, and hand it to the window's one
// executor (command_runner.hpp), so the line is echoed, kept in the history
// and one undo step like a typed one; the reply or the refusal comes back into
// labelEditStatus. There is nothing it can do that an agent cannot do by
// typing the line it shows. The line is made by labelSetLine, a pure function
// of the fields, tested without a window.
//
// It opens on the selected label (the action annotateEditLabel). With no label
// selected, or more than one entity, it says so in labelEditStatus and its
// buttons are disabled; it never asks in a box, so a headless run drives it
// like any other.
//
// Object names:
//   labelEditDialog     the dialog
//   labelEditTarget     which label, and what it labels (read-only)
//   labelEditStyle      the label style
//   labelEditOverride   ticked: the label says labelEditText instead of its
//                       style's words; unticked: text=none
//   labelEditText       its own words
//   labelEditPinned     ticked: the text is pinned at labelEditEasting,
//                       labelEditNorthing; unticked: at=none, the placer puts it
//   labelEditEasting, labelEditNorthing
//   labelEditLayer      the layer the label is on
//   labelEditCommand    the line OK and Apply run (read-only)
//   labelEditOk         run the line, and close when it worked
//   labelEditApply      run the line and stay open
//   labelEditCancel     close without running anything
//   labelEditStatus     what happened last

#include <QDialog>
#include <QString>

#include "command_runner.hpp"
#include "katana/cad/document.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"

class QCheckBox;
class QComboBox;
class QLabel;
class QLineEdit;
class QPushButton;

namespace katana::qt {

// What the dialog's fields hold, as typed; also what a label is when it is
// loaded, to compare the fields with.
struct LabelEditForm {
    katana::entity::EntityId id = 0;
    QString style;
    bool override = false; // the label says `text` instead of its style's words
    QString text;
    bool pinned = false; // the text is at easting, northing
    QString easting;
    QString northing;
    QString layer;
};

// The label as `form` holds it, from the model: its style, its own text, its
// pinned place - its anchor when it has none, so ticking Pinned starts from
// where the label is attached - and its layer. NotFound or InvalidArgument
// when `id` is not a label.
[[nodiscard]] katana::core::Result<LabelEditForm> labelEditFormOf(const katana::entity::Model& model,
                                                                  katana::entity::EntityId id);

// The LABEL SET line that makes label `form.id` what `form` says, naming only
// what differs from `current` (the label as it is):
//   LABEL SET id [style=name] [text="words"|none] [at=x,y|none] [layer=name]
// A word holding a blank is double-quoted, as the command line reads it, and
// a line break in the text is written \n, which the verb reads back as one.
// An empty string when nothing differs. InvalidArgument, naming the field,
// for no label, an empty style or layer, own text ticked but empty or the
// bare word "none" (which the verb reads as "no own text"), a coordinate
// that is not a number, and a double quote anywhere - the command line has
// no way to write one inside a quoted word.
[[nodiscard]] katana::core::Result<QString> labelSetLine(const LabelEditForm& form,
                                                         const LabelEditForm& current);

class LabelEditDialog final : public QDialog {
  public:
    LabelEditDialog(katana::cad::Document& document, CommandRunner run, QWidget* parent = nullptr);

    // Shows label `id`; false, with the reason in the status, when it is not
    // a label.
    bool load(katana::entity::EntityId id);
    // The selection's label: false, with the reason in the status, unless
    // exactly one entity is selected and it is a label.
    bool loadSelection();

    [[nodiscard]] LabelEditForm form() const;
    [[nodiscard]] QString status() const;
    // The line Apply would run now; empty when nothing differs or the fields
    // do not make one (the status then says why).
    [[nodiscard]] QString line() const;

    // Runs the line through the runner. True when it ran and was carried out,
    // or when there was nothing to change; the label is then shown afresh.
    bool apply();

  private:
    void showForm(const LabelEditForm& form);
    void setEnabledFields(bool enabled);
    void updateLine();

    katana::cad::Document& document_;
    CommandRunner run_;
    LabelEditForm current_;
    QLabel* target_ = nullptr;
    QComboBox* style_ = nullptr;
    QCheckBox* override_ = nullptr;
    QLineEdit* text_ = nullptr;
    QCheckBox* pinned_ = nullptr;
    QLineEdit* easting_ = nullptr;
    QLineEdit* northing_ = nullptr;
    QComboBox* layer_ = nullptr;
    QLineEdit* command_ = nullptr;
    QPushButton* ok_ = nullptr;
    QPushButton* apply_ = nullptr;
    QLabel* status_ = nullptr;
    // True while showForm fills the fields, so their change signals do not
    // rebuild the line from half a label.
    bool filling_ = false;
};

} // namespace katana::qt
