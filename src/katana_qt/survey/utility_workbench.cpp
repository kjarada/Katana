#include "survey/utility_workbench.hpp"

#include <QAction>
#include <QKeySequence>
#include <QMainWindow>
#include <QMenu>

#include <cstddef>
#include <utility>

#include "customisation/scope_filter_widget.hpp"
#include "katana/cad/utilities/utility_verbs.hpp"
#include "view_workspace.hpp"

namespace katana::qt {
namespace {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;

QString qs(const std::string& text)
{
    return QString::fromStdString(text);
}

// The menu section, in the order of the dialog's tabs. The letters are the
// ones the Survey menu had left (--check-shortcuts): N and X for the two that
// act on what is drawn.
struct ToolAction {
    UtilityTool tool;
    Icon icon;
    const char* text;
    const char* tip;
    const char* name;
};

constexpr std::array<ToolAction, kUtilityToolCount> kActions{{
    {UtilityTool::Draw, Icon::FormatStyles, "Draw &Utility Schedule...",
     "Grade a utility schedule by AS 5488 quality level and add it to the drawing: a layer for "
     "each type and level, a linetype for each level, a point at each located vertex - one undo "
     "step",
     "utilityDraw"},
    {UtilityTool::Report, Icon::SurveyPointReport, "Utility Investigation Rep&ort...",
     "Grade a utility schedule by AS 5488 quality level: each service's length at each level, "
     "depth of cover, and what is claimed better than its evidence supports",
     "utilityReport"},
    {UtilityTool::Verify, Icon::SurveyInverse, "Verify Detections A&gainst Exposures...",
     "Compare the QL-B detections with the QL-A exposures that check them, in plan and level",
     "utilityVerify"},
    {UtilityTool::Clearance, Icon::Section, "Clearance of Proposed &Works...",
     "Clearance of proposed works from each service, widened by its quality level's tolerance",
     "utilityClearance"},
    {UtilityTool::Check, Icon::DatasetInfo, "Check Against a Delivery Sc&hema...",
     "Check a utility schedule against a client's delivery schema: mandatory attributes and "
     "their value lists",
     "utilityCheck"},
    {UtilityTool::Regrade, Icon::GlobalModify, "Regrade Draw&n Utilities...",
     "Grade the drawn utility lines in a scope again from their points as they are now - moved, "
     "levels or methods edited - and draw their runs again: one undo step",
     "utilityRegrade"},
    {UtilityTool::Schedule, Icon::SurveyExport, "E&xport Drawn Utilities as a Schedule...",
     "Write the drawn utility lines in a scope as a schedule (.csv) that reads back exactly: a "
     "drawing edited in CAD made a deliverable",
     "utilityWriteSchedule"},
}};

} // namespace

UtilityWorkbench::UtilityWorkbench(QMainWindow& window, UtilityServices services,
                                   QMenu& surveyMenu)
    : window_(window), services_(std::move(services))
{
    // A section, as the Survey menu's others are: a style that draws its
    // title says what the group is for.
    surveyMenu.addSection("Subsurface Utilities (AS 5488)");
    for (const ToolAction& entry : kActions) {
        QAction* made =
            services_.makeAction(entry.icon, entry.text, entry.tip, QKeySequence(), entry.name);
        // The dialog it opens, by object name: how --dialog finds it, since
        // seven actions share one dialog.
        made->setData(QString("utilityDialog"));
        const UtilityTool tool = entry.tool;
        QObject::connect(made, &QAction::triggered, &window_, [this, tool] { open(tool); });
        surveyMenu.addAction(made);
        actions_[static_cast<std::size_t>(tool)] = made;
    }
}

UtilityWorkbench::~UtilityWorkbench()
{
    // The dialog is the window's child only so that it floats over it; it
    // calls back into this object, so it goes with it.
    delete dialog_.data();
}

QAction* UtilityWorkbench::action(UtilityTool tool) const
{
    return actions_[static_cast<std::size_t>(tool)];
}

UtilityToolsDialog& UtilityWorkbench::dialog()
{
    if (dialog_ == nullptr) {
        UtilityDialogContext context;
        context.execute = [this](const QString& line) { return execute(line); };
        context.headless = services_.headless;
        context.document = services_.document;
        if (services_.views != nullptr) {
            ViewWorkspace* views = services_.views;
            context.views = [views] { return scopeFilterViews(views->viewSet()); };
        }
        dialog_ = new UtilityToolsDialog(std::move(context), &window_);
    }
    return *dialog_;
}

void UtilityWorkbench::open(UtilityTool tool)
{
    UtilityToolsDialog& shown = dialog();
    shown.showTool(tool);
    // The views open now, when the dialog is already showing too.
    shown.reload();
    shown.show();
    shown.raise();
    shown.activateWindow();
}

bool UtilityWorkbench::runLine(const QString& line)
{
    const QString words = line.simplified();
    if (words.section(' ', 0, 0).compare("UTILITY", Qt::CaseInsensitive) != 0) {
        return false;
    }
    Result<std::string> reply =
        services_.interpret ? services_.interpret(line.toStdString())
                            : Result<std::string>(makeError(
                                  ErrorCode::InvalidState, "there is no command interpreter here"));
    if (!reply) {
        services_.log(qs(reply.error().describe()), true);
    } else {
        QString text = qs(*reply);
        while (text.endsWith('\n')) {
            text.chop(1);
        }
        services_.log(text, false);
        // A DRAW adds the services and a REGRADE draws them again: either
        // way the views are shown what the reply's bounds= box holds.
        const QString action = words.section(' ', 1, 1);
        const bool draws = action.compare("DRAW", Qt::CaseInsensitive) == 0 ||
                           action.compare("REGRADE", Qt::CaseInsensitive) == 0;
        if (draws && services_.views != nullptr) {
            if (const auto box = katana::cad::utilities::drawReplyBounds(*reply)) {
                services_.views->zoomTo(*box);
            }
        }
    }
    lastReply_ = std::move(reply);
    return true;
}

Result<std::string> UtilityWorkbench::execute(const QString& line)
{
    lastReply_.reset();
    if (services_.runCommand) {
        services_.runCommand(line);
    }
    if (!lastReply_) {
        return makeError(ErrorCode::InvalidState,
                         "the command line did not run this as a UTILITY line", line.toStdString());
    }
    return *lastReply_;
}

} // namespace katana::qt
