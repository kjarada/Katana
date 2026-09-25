#pragma once

// Annotate > Label Layout Report... (docs/annotation.md, "In the window"):
// how the drawing's labels fare at the annotation scale - how many the placer
// put at their own place, how many it moved (displaced), how many found no
// room (suppressed) and how many label nothing (orphaned) - with the labels
// that found no room selected on request, to move or restyle them.
//
// The dialog works nothing out itself. Run hands "LABEL LAYOUT" - with
// collisions=off when labelLayoutCollisions is unticked - to the window's one
// executor (command_runner.hpp), and the counts are read back out of the
// verb's reply by parseLabelLayoutReply. Select Suppressed runs
// "SELECT id...". Both lines are echoed and kept in the history as typed
// ones; neither changes the drawing. It runs once when it opens.
//
// Object names:
//   labelLayoutDialog            the dialog (the action annotateLabelLayout)
//   labelLayoutCollisions        ticked: labels are kept clear of each other and
//                                of the linework, as the plan view places them;
//                                unticked: collisions=off, each at its own place
//   labelLayoutRun               run LABEL LAYOUT
//   labelLayoutCommand           the line Run runs (read-only)
//   labelLayoutPlaced, labelLayoutDisplaced, labelLayoutSuppressed,
//   labelLayoutOrphaned          the counts
//   labelLayoutSelectSuppressed  select the labels with a piece that found no
//                                room; disabled when there are none
//   labelLayoutOutput            the verb's whole reply (read-only)
//   labelLayoutStatus            what happened last
//   labelLayoutClose             close

#include <cstddef>
#include <vector>

#include <QDialog>
#include <QString>

#include "command_runner.hpp"
#include "katana/entity/entity.hpp"

class QCheckBox;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;

namespace katana::qt {

// What LABEL LAYOUT replied: its summary record, and the labels named by its
// "suppressed=yes" records, each once, in the order the verb gave them.
struct LabelLayoutSummary {
    bool read = false; // the summary record was found
    double scale = 0.0;
    std::size_t considered = 0;
    std::size_t placed = 0;
    std::size_t displaced = 0;
    std::size_t suppressed = 0;
    std::size_t orphaned = 0;
    std::vector<katana::entity::EntityId> suppressedLabels;
};

// "LABEL LAYOUT", or "LABEL LAYOUT collisions=off".
[[nodiscard]] QString labelLayoutLine(bool collisions);
[[nodiscard]] LabelLayoutSummary parseLabelLayoutReply(const QString& reply);
// "SELECT 4 9"; empty for no ids.
[[nodiscard]] QString selectLine(const std::vector<katana::entity::EntityId>& ids);

class LabelLayoutReportDialog final : public QDialog {
  public:
    explicit LabelLayoutReportDialog(CommandRunner runner, QWidget* parent = nullptr);

    // Runs the layout; false, with the reason in the status, when the line
    // was refused or its reply could not be read.
    bool run();
    // Selects the suppressed labels of the last run.
    bool selectSuppressed();

    [[nodiscard]] const LabelLayoutSummary& summary() const { return summary_; }
    [[nodiscard]] QString status() const;

  private:
    void showSummary(const LabelLayoutSummary& summary);

    CommandRunner run_;
    LabelLayoutSummary summary_;
    QCheckBox* collisions_ = nullptr;
    QLineEdit* command_ = nullptr;
    QLabel* placed_ = nullptr;
    QLabel* displaced_ = nullptr;
    QLabel* suppressed_ = nullptr;
    QLabel* orphaned_ = nullptr;
    QPushButton* select_ = nullptr;
    QPlainTextEdit* output_ = nullptr;
    QLabel* status_ = nullptr;
};

} // namespace katana::qt
