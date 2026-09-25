#pragma once

// The Leaders manager (Annotate > Leaders...; docs/annotation.md "In the
// window"): smart leaders from the window, a thin front end over
// cad/annotation/leader_edit.hpp - the same edits the LEADER and BALLOON
// verbs make, each ONE undo step, refused in the command line's own words.
//
//   Leader         the drawing's leaders in a list, and a form for the one
//                  chosen (it follows the drawing's selection): its note as
//                  text, a template or a label style's, checked as it is
//                  typed and shown as it would read; the values the entity
//                  its tip is on offers, a double-click putting {name} into
//                  the template; its arrow, callout, text style and sizes;
//                  an attribute of that entity set through it; Freeze,
//                  Detach and Apply.
//   For Selection  a leader, or a numbered balloon, to each selected entity
//                  (LEADER FOR, BALLOON FOR).
//   Arrange        the selected leaders' notes in a column (LEADER ALIGN),
//                  and the balloons numbered again (BALLOON RENUMBER).
//
// It follows docs/desktop.md, "The rules a dialog or panel follows": no moc,
// non-modal and kept by AnnotationWorkbench, a command never run from the
// list's or the table's own signal, the document watched through a
// coalescing DocumentWatcher, and every widget named for the tests and the
// headless driver. It holds no copy of the drawing: the form is re-read
// from the leader whenever the leader changes under it (an undo, a verb
// typed), and left alone - unapplied edits and all - when it does not.

#include <memory>
#include <optional>

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
    // The form as an edit: what Apply would make of the leader.
    [[nodiscard]] katana::cad::annotation::LeaderChange formChange() const;
    // What the note would say with the form as it is, or why it cannot be.
    [[nodiscard]] QString preview() const;
    // What the last refused action said; empty after one that worked.
    [[nodiscard]] QString problem() const;
    // Puts "{name}" into the note at the cursor, making the note a template.
    void insertValue(const QString& name);

    // The actions, as the buttons run them.
    bool apply();
    bool freeze();
    bool detach();
    // The attribute named in the form set on the entity the tip is on, to
    // the form's value (of the stated type), or removed.
    bool setAttribute();
    bool removeAttribute();

    // ---- the For Selection tab
    bool makeForSelection();
    // What the last one made, as LEADER FOR replies.
    [[nodiscard]] QString forReport() const;

    // ---- the Arrange tab
    bool alignSelection();
    bool renumberBalloons();
    [[nodiscard]] QString arrangeReport() const;

  private:
    void refresh(const DocumentChanges& changes);
    void fillList();
    void fillStyles();
    void showForm(const katana::entity::LeaderGeometry& leader);
    void showValues();
    void updatePreview();
    void updateNoteKind();
    void lendStyleLook();
    void updateForNoteKind();
    void report(const katana::core::Status& status);
    [[nodiscard]] std::vector<katana::entity::EntityId> selectedLeaders() const;
    [[nodiscard]] const katana::entity::LeaderGeometry* leader() const;

    katana::cad::Document& document_;
    katana::entity::EntityId shown_ = 0;
    // The leader as the form last showed it: re-read only when the drawing's
    // differs, so an unrelated edit leaves unapplied changes in the form.
    std::optional<katana::entity::LeaderGeometry> shownGeometry_;

    QTabWidget* tabs_ = nullptr;
    QLabel* problem_ = nullptr;

    // Leader
    QListWidget* list_ = nullptr;
    QLabel* target_ = nullptr;
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

    // For Selection
    QLabel* forSelection_ = nullptr;
    QComboBox* forNoteKind_ = nullptr;
    QPlainTextEdit* forNote_ = nullptr;
    QComboBox* forLabelStyle_ = nullptr;
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

    // Last, so it goes first (docs/desktop.md).
    std::unique_ptr<DocumentWatcher> watcher_;
};

} // namespace katana::qt
