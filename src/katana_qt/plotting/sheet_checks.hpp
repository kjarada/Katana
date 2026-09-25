#pragma once

// The preflight checks in the sheet editor (docs/plotting.md, "Preflight
// checks"): the headless checker (cad/plotting/preflight.hpp) given what the
// painter knows, and the Checks dock that lists what it finds.
//
// The checker needs no Qt and no window. What it cannot know by itself is
// what the WINDOW draws from: the imagery, point clouds and meshes a plan
// shows, the surfaces the sections are cut from, whether the logo decoded,
// and the painter's own rule for an automatic plan. The painter's SheetSource
// carries all of it, so preflightOptionsFor turns one into the other, and
// what the checks call empty is what the painter would leave empty.
//
// The dock checks again on its own a moment after the edits stop (schedule),
// so a burst of edits costs one check, not one per edit; Check Sheets and a
// plot run it at once (checkNow). What it finds never stops a plot.

#include <array>
#include <cstddef>
#include <functional>
#include <span>
#include <vector>

#include <QDockWidget>
#include <QIcon>
#include <QShowEvent>
#include <QStringList>

#include "katana/cad/document.hpp"
#include "katana/cad/plotting/preflight.hpp"
#include "sheet_painter.hpp"

class QLabel;
class QTimer;
class QTreeWidget;

namespace katana::qt {

// The checker's options for sheets painted from `source`: an automatic plan
// resolved by resolvePlanViewport, the visible imagery, point clouds and
// meshes counted as content, the visible surfaces as what sections are cut
// from, the decoded logo, the assets folder and the spatial index. They
// refer to `source`, so they must not outlive it.
[[nodiscard]] katana::cad::plotting::PreflightOptions preflightOptionsFor(const SheetSource& source);

// checkSheets for the sheets at `sheets` of `set` (every sheet when empty),
// with those options. A source with no drawing is checked against an empty
// one, as the painter would draw it.
[[nodiscard]] std::vector<katana::cad::plotting::Finding>
checkSheetsFor(const katana::cad::plotting::SheetSet& set, const SheetSource& source,
               std::span<const std::size_t> sheets = {});

// What a log gets before a plot: a summary line, then a line for each error
// (findingLine). Nothing when there are only notes or nothing at all.
[[nodiscard]] QStringList preflightLog(std::span<const katana::cad::plotting::Finding> findings);

// The toolbar's Check Sheets glyph, and a finding's severity mark. Painted
// here rather than taken from the platform's style, so they read the same
// everywhere.
[[nodiscard]] QIcon checkSheetsIcon();
[[nodiscard]] QIcon severityIcon(katana::cad::plotting::Severity severity);

// The Checks dock: every finding of the last run, one row each - its
// severity, sheet, view, what is wrong and what to do - errors first, then
// warnings, then notes, each in the checker's order. Double-click (or Enter
// on) a row to go to it.
class SheetChecksDock final : public QDockWidget {
  public:
    using Checker = std::function<std::vector<katana::cad::plotting::Finding>()>;

    // How long after the last change the checks run again: long enough for a
    // burst of edits to settle, short enough to feel live.
    static constexpr int kRecheckDelayMs = 400;

    SheetChecksDock(const katana::cad::Document& document, Checker checker,
                    QWidget* parent = nullptr);

    // Runs the checks now and lists what they found; a scheduled run is
    // dropped, since this one is newer.
    const std::vector<katana::cad::plotting::Finding>& checkNow();
    // Runs them kRecheckDelayMs after the last call (setDelay changes it).
    // While the window the dock is in is hidden, only marks the findings out
    // of date: they are checked again when it is shown.
    void schedule();
    [[nodiscard]] bool pending() const;
    void setDelay(int milliseconds);

    [[nodiscard]] const std::vector<katana::cad::plotting::Finding>& findings() const
    {
        return findings_;
    }
    [[nodiscard]] QTreeWidget* list() const { return list_; }
    [[nodiscard]] QLabel* summary() const { return summary_; }
    // How many times the checks have run, for tests of the debounce.
    [[nodiscard]] int runs() const { return runs_; }

    // The list's row of findings()[finding]; -1 for none.
    [[nodiscard]] int rowOf(std::size_t finding) const;
    // Goes to findings()[finding], as a double-click on its row does.
    void activate(std::size_t finding);

    // Called with the finding a row leads to when it is activated.
    std::function<void(const katana::cad::plotting::Finding&)> onActivated;

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void rebuild();

    const katana::cad::Document& document_;
    Checker checker_;
    std::vector<katana::cad::plotting::Finding> findings_;
    std::vector<std::size_t> rows_; // the finding each row of the list shows
    QLabel* summary_ = nullptr;
    QTreeWidget* list_ = nullptr;
    QTimer* timer_ = nullptr;
    std::array<QIcon, 3> icons_;
    int runs_ = 0;
    bool stale_ = false; // a change came while the window was hidden
};

} // namespace katana::qt
