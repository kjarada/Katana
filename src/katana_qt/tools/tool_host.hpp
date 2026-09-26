#pragma once

// The host a plan view runs a catalogue tool in (include/katana/cad/
// interactive_tool.hpp). The tool is a state machine in cad that knows nothing
// of Qt; the view knows nothing of any one tool. This sits between them: it
// starts a tool from the catalogue with the document's current attributes and
// the live selection, hands it what the user did - a point (already snapped by
// the view), an entity picked, typed text, Enter, Esc, Undo - and when the tool
// says it is done, executes its ONE command through the Document, so a whole
// chain of lines or a whole Trim session is one undo step. It restarts the tool
// when the tool asks (Circle, Point), and reports through hooks, never by
// opening anything, so a headless session and a test drive it the same way.
//
// No Qt here on purpose: everything below is testable by calling it, and the
// view's part is only turning events into these calls and drawing feedback().

#include <cstdint>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <string_view>

#include "katana/cad/document.hpp"
#include "katana/cad/interactive_tool.hpp"
#include "katana/cad/snapping.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::qt::tools {

class ToolHost {
  public:
    using Outcome = katana::cad::ToolStep::Outcome;
    using Point2 = katana::geometry::Point2;

    explicit ToolHost(katana::cad::Document& document);
    ToolHost(const ToolHost&) = delete;
    ToolHost& operator=(const ToolHost&) = delete;
    ~ToolHost();

    // Starts catalogue tool `id`. A tool already running is stopped first as
    // Esc stops it (cancel()), so switching tools half-way keeps whatever Esc
    // would keep. NotFound for an id the catalogue does not have, and then
    // nothing changes: the running tool, if any, runs on.
    [[nodiscard]] katana::core::Status start(std::string_view id);

    [[nodiscard]] bool active() const { return tool_ != nullptr; }
    // The running tool's catalogue entry; null when none is running.
    [[nodiscard]] const katana::cad::ToolInfo* info() const { return info_; }
    // The running tool's id, or "" when none is running.
    [[nodiscard]] std::string activeId() const;
    // What the running tool wants next; nullopt when none is running.
    [[nodiscard]] std::optional<katana::cad::ToolInput> expects() const;
    // The running tool's prompt, "" when none is running.
    [[nodiscard]] std::string prompt() const;
    // The rubber band for the cursor at `cursor`; empty when none is running.
    [[nodiscard]] katana::cad::ToolFeedback feedback(const Point2& cursor) const;
    // What relative input (@dx,dy) and the Perpendicular and Tangent snaps
    // measure from; nullopt before the tool's first point.
    [[nodiscard]] std::optional<Point2> lastPoint() const;
    // Changes whenever a tool is made, remade for a restart, or dropped: what
    // a caller's own state about "the tool running now" is checked against.
    [[nodiscard]] std::uint64_t generation() const { return generation_; }

    // The view's pick aperture in model units, given to a tool when it starts
    // or restarts (ToolContext::pickTolerance). The view sets it from its zoom.
    void setPickTolerance(double tolerance) { pickTolerance_ = tolerance; }

    // ---- what the user did ---------------------------------------------------
    // Each answers what the tool made of it. With no tool running the answer
    // is Rejected and onRejected says so: an input with nowhere to go is
    // reported, not dropped.
    // A click at `at`; `snap` is what the view's snapping found there, if
    // anything. A snap to an entity's end, middle, centre or vertex reaches
    // the tool as that point of that entity (cad::routeSnappedPoint), so
    // the annotation it makes follows the entity.
    Outcome point(const Point2& at,
                  const std::optional<katana::cad::SnapResult>& snap = std::nullopt);
    Outcome entity(katana::entity::EntityId id, const Point2& at);
    // Typed text, through routeTypedInput: "10,20", "@5,0" and "@5<90" are
    // points (the relative forms from lastPoint()), anything else is a value
    // or an option keyword.
    Outcome typed(std::string_view text);
    // Where the cursor is, for direct distance entry: a number typed at a
    // point prompt is that far from the last point towards it.
    void setCursor(const Point2& at) { cursor_ = at; }
    // Enter, Space or a right-click.
    Outcome enter();
    // Steps back one input inside the tool (the U inside LINE); never the
    // document's undo.
    Outcome undo();
    // Esc: ends the tool, keeping what the tool's own cancel() says it has
    // already placed - a chain of lines, the parts a Trim has cut, the
    // copies Copy has put down - as AutoCAD keeps the segments of a LINE,
    // and executing it as one command. Everything else goes with the tool.
    // No-op when idle.
    void cancel();
    // Ends the running tool WITHOUT committing anything (onFinished, as for
    // Esc): for when the document under it is being replaced, and its picks
    // and ids belong to a drawing that is going. Ended rather than restarted,
    // because a restart reads the current layer and the selection at that
    // moment, and a caller replacing the document may call this before the
    // replacement (MainWindow::newDocument does). No-op when idle.
    void abandon();

    // ---- hooks, all optional -------------------------------------------------
    // The prompt changed: after every input, a start, a restart, and "" when
    // the tool ends.
    std::function<void(const std::string& prompt)> onPrompt;
    // What a tool reports that is not a refusal: "3 lines", "2 selected", a
    // measurement.
    std::function<void(const std::string& message)> onMessage;
    // An input the tool refused, a command the document refused, or input
    // with no tool running - the sentence says which.
    std::function<void(const std::string& message)> onRejected;
    std::function<void(const std::string& toolId)> onStarted;
    // The tool ended: finished without asking to restart, or cancelled.
    std::function<void(const std::string& toolId)> onFinished;
    // A transparent command typed while a tool runs (isTransparentCommand):
    // it is the VIEW's - ZOOM, PAN - and goes here instead of to the tool,
    // which stays at the same step, as AutoCAD runs 'ZOOM inside LINE.
    // Answers whether it was carried out. Unset, or answering false, the
    // host says the command cannot run inside the tool, and the tool is not
    // sent it either: "Z" is never a malformed point.
    std::function<bool(const std::string& command)> onTransparent;

  private:
    // (Re)creates the tool from info_ with a context read NOW: the current
    // layer and style (D9) and the selection as it is.
    void make();
    Outcome apply(katana::cad::ToolStep step);
    // Drops the tool and raises onFinished and onPrompt.
    void end();
    Outcome idle();
    void report(const std::function<void(const std::string&)>& hook, const std::string& text);

    katana::cad::Document& document_;
    std::optional<Point2> cursor_;
    const katana::cad::ToolInfo* info_ = nullptr;
    std::unique_ptr<katana::cad::InteractiveTool> tool_;
    // Bumped whenever tool_ is made or dropped, so apply() can tell that a
    // hook replaced the tool it was dealing with (see apply).
    std::uint64_t generation_ = 0;
    double pickTolerance_ = 0.0;
};

// True for text typed while a tool runs that is a command for the view, not
// an answer for the tool: ZOOM, Z and PAN in any case, with or without
// AutoCAD's leading apostrophe ('ZOOM), and 'P (a bare P is Rotate's and
// Scale's [Points] option), with any arguments after the verb ("ZOOM E").
// When the tool `wantsValue` (ToolInput::Value: a text's string, a layer
// name, a count) a bare word is its answer - a text may well read "Z" - so
// there only the apostrophe forms are the view's.
[[nodiscard]] bool isTransparentCommand(std::string_view text, bool wantsValue = false);

} // namespace katana::qt::tools
