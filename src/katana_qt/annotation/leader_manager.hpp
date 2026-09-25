#pragma once

// The Leaders manager (Annotate > Leaders...; docs/annotation.md "In the
// window"): smart leaders from the window, a thin front end over
// cad/annotation/leader_edit.hpp - the same edits the LEADER and BALLOON
// verbs make, each ONE undo step, refused in the command line's own words.
//
//   Leader         the drawing's leaders in a list, and a form for the one
//                  chosen (it follows the drawing's selection): what its tip
//                  is on and how far along; its note as text, a template or
//                  a label style's, checked as it is typed and shown as it
//                  would read; the values the entity its tip is on offers,
//                  a double-click putting {name} into the note; its arrow,
//                  callout, text style and sizes; an attribute of that
//                  entity set through it; the tip attached to a selected
//                  entity; Freeze, Detach and Apply. With several leaders
//                  selected, Apply, Freeze and Detach change them all.
//   For Selection  a leader, or a numbered balloon, to each selected entity
//                  (LEADER FOR, BALLOON FOR), its note checked and shown as
//                  it would read for the first, with that entity's values
//                  and the look the leaders are made with.
//   Arrange        the selected leaders' notes in a column (LEADER ALIGN),
//                  and the balloons numbered again (BALLOON RENUMBER).
//
// Apply sends only what the user CHANGED: the form remembers what it showed,
// and a field still showing that is left out of the edit, so a value the
// widgets cannot hold exactly (a height of 2.345 mm, a landing of 60 mm, a
// non-breaking space) is never rewritten, and an untouched form applied is
// no step.
//
// It follows docs/desktop.md, "The rules a dialog or panel follows": no moc,
// non-modal and kept by AnnotationWorkbench, a command never run from the
// list's or the tables' own signals, the document watched through a
// coalescing DocumentWatcher, and every widget named for the tests and the
// headless driver. It holds no copy of the drawing: the form is re-read from
// the leader whenever the leader changes under it (an undo, a verb typed)
// or the drawing is replaced, and left alone - unapplied edits and all -
// when neither happens.

#include <memory>
#include <optional>
#include <vector>

#include <QDialog>
#include <QString>

#include "katana/cad/annotation/leader_edit.hpp"
#include "katana/cad/document.hpp"
#include "katana/entity/entity.hpp"

class QCheckBox;
class QComboBox;
class QDoubleSpinBox;
class QLabel;
class QLineEdit;
class QListWidget;
class QPlainTextEdit;
class QPushButton;
class QSpinBox;
class QTableWidget;
class QTabWidget;

namespace katana::qt {

class DocumentWatcher;
struct DocumentChanges;

class LeaderManagerDialog final : public QDialog {
  public:
    // The tabs, in order.
    enum class Tab { Leader = 0, ForSelection = 1, Arrange = 2 };

    explicit LeaderManagerDialog(katana::cad::Document& document, QWidget* parent = nullptr);
    ~LeaderManagerDialog() override;

    void showTab(Tab tab);

    // ---- the Leader tab
    // Shows leader `id` in the form; false when it is not a leader.
    bool showLeader(katana::entity::EntityId id);
    // The leader the form is for; 0 for none.
    [[nodiscard]] katana::entity::EntityId currentLeader() const { return shown_; }
    // The leaders Apply, Freeze and Detach change: the selected ones when
    // the shown leader is one of several selected, else the shown one.
    [[nodiscard]] std::vector<katana::entity::EntityId> targets() const;
    // What the user changed in the form: what Apply sends. Empty for an
    // untouched form.
    [[nodiscard]] katana::cad::annotation::LeaderChange formChange() const;
    // What the note would say with the form as it is, or why it cannot be.
    [[nodiscard]] QString preview() const;
    // What the last refused action said; empty after one that worked.
    [[nodiscard]] QString problem() const;
    // Puts "{name}" into the note at the cursor (the end of the note, until
    // the user puts the cursor elsewhere), making the note a template.
    void insertValue(const QString& name);

    // The actions, as the buttons run them.
    bool apply();
    bool freeze();
    bool detach();
    // The shown leader's tip put on the first selected entity that is not a
    // leader (else any other selected entity), at the place of it nearest
    // the tip.
    bool attachToSelected();
    // The attribute named in the form set on the entity the tip is on, to
    // the form's value (of the stated type), or removed.
    bool setAttribute();
    bool removeAttribute();

    // ---- the For Selection tab
    bool makeForSelection();
    // What the last one made, as LEADER FOR replies.
    [[nodiscard]] QString forReport() const;
    // What a leader for the first selected entity would say, or why not.
    [[nodiscard]] QString forPreview() const;
    void insertForValue(const QString& name);

    // ---- the Arrange tab
    bool alignSelection();
    bool renumberBalloons();
    [[nodiscard]] QString arrangeReport() const;

  private:
    // What the Leader tab's widgets say, compared field by field with what
    // showForm put there to find what the user changed.
    struct FormValues {
        int noteKind = 0;
        QString noteText;
        QString labelStyle;
        int arrow = 0;
        int callout = 0;
        QString textStyle;
        double paperHeight = 0.0;
        double arrowSize = 0.0;
        double landing = 0.0;
        double along = 0.0;
    };
    // The look before a label style lent its own, to put back when the note
    // stops being the style's.
    struct Look {
        QString textStyle;
        double paperHeight = 0.0;
    };

    void refresh(const DocumentChanges& changes);
    void fillList();
    void fillStyles();
    void showForm(const katana::entity::LeaderGeometry& leader);
    void showValues();
    void updatePreview();
    void updateButtons();
    void updateNoteKind();
    void noteKindChosen();
    void lendStyleLook();
    void updateForNoteKind();
    void updateForPreview();
    void report(const katana::core::Status& status);
    [[nodiscard]] FormValues readForm() const;
    [[nodiscard]] std::vector<katana::entity::EntityId> selectedLeaders() const;
    [[nodiscard]] const katana::entity::LeaderGeometry* leader() const;
    [[nodiscard]] const katana::entity::Entity* forTarget() const;
    [[nodiscard]] katana::cad::annotation::LeadersForOptions forOptions() const;

    katana::cad::Document& document_;
    katana::entity::EntityId shown_ = 0;
    // The leader as the form last showed it: re-read only when the drawing's
    // differs, so an unrelated edit leaves unapplied changes in the form.
    std::optional<katana::entity::LeaderGeometry> shownGeometry_;
    // The widgets as showForm left them.
    std::optional<FormValues> loaded_;
    // The note kind the form shows now, the user's own note while a label
    // style's template stands in for it, and the look before a style lent
    // its own.
    int shownKind_ = 0;
    std::optional<QString> stash_;
    std::optional<Look> lentFrom_;
    // Set by the document when the drawing is replaced (File > New, Open):
    // the next refresh starts the form afresh rather than applying edits
    // meant for the old drawing to a leader of the new one.
    bool replaced_ = false;

    QTabWidget* tabs_ = nullptr;
    QLabel* problem_ = nullptr;

    // Leader
    QListWidget* list_ = nullptr;
    QLabel* scope_ = nullptr;
    QLabel* target_ = nullptr;
    QDoubleSpinBox* along_ = nullptr;
    QComboBox* noteKind_ = nullptr;
    QPlainTextEdit* note_ = nullptr;
    QComboBox* labelStyle_ = nullptr;
    QLabel* check_ = nullptr;
    QLabel* preview_ = nullptr;
    QTableWidget* values_ = nullptr;
    QComboBox* arrow_ = nullptr;
    QComboBox* callout_ = nullptr;
    QComboBox* textStyle_ = nullptr;
    QDoubleSpinBox* paperHeight_ = nullptr;
    QDoubleSpinBox* arrowSize_ = nullptr;
    QDoubleSpinBox* landing_ = nullptr;
    QLineEdit* attributeName_ = nullptr;
    QLineEdit* attributeValue_ = nullptr;
    QComboBox* attributeType_ = nullptr;
    QPushButton* attributeSet_ = nullptr;
    QPushButton* attributeRemove_ = nullptr;
    QPushButton* attach_ = nullptr;
    QPushButton* freeze_ = nullptr;
    QPushButton* detach_ = nullptr;
    QPushButton* apply_ = nullptr;

    // For Selection
    QLabel* forSelection_ = nullptr;
    QComboBox* forNoteKind_ = nullptr;
    QPlainTextEdit* forNote_ = nullptr;
    QComboBox* forLabelStyle_ = nullptr;
    QLabel* forCheck_ = nullptr;
    QLabel* forPreview_ = nullptr;
    QTableWidget* forValues_ = nullptr;
    QComboBox* forArrow_ = nullptr;
    QComboBox* forCallout_ = nullptr;
    QComboBox* forTextStyle_ = nullptr;
    QDoubleSpinBox* forPaperHeight_ = nullptr;
    QDoubleSpinBox* forArrowSize_ = nullptr;
    QDoubleSpinBox* forLanding_ = nullptr;
    QDoubleSpinBox* forAngle_ = nullptr;
    QDoubleSpinBox* forLength_ = nullptr;
    QCheckBox* forBalloon_ = nullptr;
    QLabel* forReport_ = nullptr;

    // Arrange
    QCheckBox* alignUseX_ = nullptr;
    QDoubleSpinBox* alignX_ = nullptr;
    QCheckBox* alignUseSpacing_ = nullptr;
    QDoubleSpinBox* alignSpacing_ = nullptr;
    QSpinBox* renumberStart_ = nullptr;
    QComboBox* renumberOrder_ = nullptr;
    QLabel* arrangeReport_ = nullptr;

    katana::cad::Document::ListenerHandle replacedListener_{};
    // Last, so it goes first (docs/desktop.md).
    std::unique_ptr<DocumentWatcher> watcher_;
};

} // namespace katana::qt
