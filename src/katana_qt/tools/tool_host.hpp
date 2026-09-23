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
    Outcome point(const Point2& at);
    Outcome entity(katana::entity::EntityId id, const Point2& at);
    // Typed text, through routeTypedInput: "10,20", "@5,0" and "@5<90" are
    // points (the relative forms from lastPoint()), anything else is a value
    // or an option keyword.
    Outcome typed(std::string_view text);
    // Enter, Space or a right-click.
    Outcome enter();
    // Steps back one input inside the tool (the U inside LINE); never the
    // document's undo.
    Outcome undo();
    // Esc: ends the tool. A tool whose Enter commits work it holds - a chain
    // of lines, the parts a Trim has cut - is sent Enter first, so Esc keeps
    // that work, as AutoCAD keeps the segments of a LINE; every other tool is
    // dropped with nothing done (see escapeKeepsWork). Such a tool at a value
    // prompt (Fillet's radius, Chamfer's distances) is first stepped back to
    // the prompt whose Enter commits, because Enter at a value prompt takes
    // its default, which Esc must never do. No-op when idle.
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
    const katana::cad::ToolInfo* info_ = nullptr;
    std::unique_ptr<katana::cad::InteractiveTool> tool_;
    // Bumped whenever tool_ is made or dropped, so apply() can tell that a
    // hook replaced the tool it was dealing with (see apply).
    std::uint64_t generation_ = 0;
    double pickTolerance_ = 0.0;
};

// True for a tool whose Enter only ever commits work it has collected - a
// LINE or PLINE chain, the cuts of a Trim or Extend, Offset's copies, a run of
// Fillets or Chamfers - so Esc sends it Enter rather than losing that work.
// False for the rest, whose Enter at some step applies a DEFAULT (Move's "use
// the first point as the displacement", Join's "join what is selected"), which
// Esc must never do.
//
// A list, because the tool interface has no "commit on cancel" of its own;
// the Modify Edit report asked for one, and a virtual cancel() on
// InteractiveTool would replace this list (outstanding for the lead).
[[nodiscard]] bool escapeKeepsWork(std::string_view toolId);

} // namespace katana::qt::tools
