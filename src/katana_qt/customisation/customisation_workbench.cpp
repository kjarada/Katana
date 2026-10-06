#include "customisation/customisation_workbench.hpp"

#include <string>
#include <utility>

#include <QAction>
#include <QLineEdit>
#include <QMenu>
#include <QMessageBox>
#include <QPushButton>
#include <QStringList>
#include <QToolBar>
#include <QWidget>

#include "customisation/code_manager.hpp"
#include "customisation/customisation_context.hpp"
#include "customisation/definition_thumbnails.hpp"
#include "customisation/document_watcher.hpp"
#include "customisation/global_modify_dialog.hpp"
#include "customisation/symbol_library.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/purge.hpp"
#include "style_manager.hpp"
#include "view_workspace.hpp"
#include "viewport_widget.hpp"

namespace katana::qt {

namespace {

// The object names the headless driver finds each manager by, carried on its
// action as data. The code manager and the symbol library name themselves
// the same; the style manager is named here, since it names no dialog.
constexpr const char* kStyleManagerName = "styleManagerDialog";
constexpr const char* kSymbolLibraryName = "symbolLibraryDialog";
constexpr const char* kCodeManagerName = "surveyCodeManagerDialog";
constexpr const char* kGlobalModifyName = "globalModifyDialog";

QString counted(std::size_t count, const char* one, const char* many)
{
    return QString::number(count) + " " + (count == 1 ? one : many);
}

QString listed(const std::vector<std::string>& names)
{
    QStringList quoted;
    for (const std::string& name : names) {
        quoted << QString::fromStdString(name);
    }
    return quoted.join(", ");
}

void raise(QWidget& dialog)
{
    dialog.show();
    dialog.raise();
    dialog.activateWindow();
}

} // namespace

CustomisationWorkbench::CustomisationWorkbench(QWidget& window, CustomisationServices services,
                                               QMenu& menu, QToolBar& toolBar)
    : window_(window), services_(std::move(services)),
      thumbnails_(std::make_unique<DefinitionThumbnails>())
{
    const auto make = [this](Icon icon, const QString& text, const QString& tip,
                             const QString& name, const char* dialog) {
        QAction* action = services_.makeAction(icon, text, tip, QKeySequence(), name);
        if (dialog != nullptr) {
            action->setData(QString::fromLatin1(dialog));
        }
        return action;
    };
    stylesAction_ =
        make(Icon::FormatStyles, "St&yles and Linetypes...",
             "The drawing's styles, its dash linetypes, its hatch patterns and the loaded "
             "library linestyles: edit, merge, rename, find what uses each, and what nothing "
             "defines",
             "formatStyles", kStyleManagerName);
    symbolsAction_ =
        make(Icon::FormatSymbols, "Sym&bol Library...",
             "Every symbol the drawing can draw, by group: what each prints at a scale, "
             "which styles and survey codes use it; put one on the selected points",
             "formatSymbols", kSymbolLibraryName);
    codesAction_ =
        make(Icon::FormatSurveyCodes, "&Survey Code Manager...",
             "The survey code library (the loaded survey code files): test a code, edit its "
             "rules, see the drawing's codes and their issues, apply the codes, run linework",
             "formatSurveyCodes", kCodeManagerName);
    globalModifyAction_ =
        make(Icon::GlobalModify, "&Global Modify...",
             "Change the selection, what a view shows, whole layers or the drawing at once: "
             "layer, colour, style, symbol, properties, and their layers' and styles' "
             "settings, as one undo step",
             "formatGlobalModify", kGlobalModifyName);
    purgeAction_ = make(Icon::Purge, "&Purge Unused...",
                        "Delete the styles, linetypes and hatch patterns nothing uses, as one "
                        "undo step",
                        "formatPurge", nullptr);

    QObject::connect(stylesAction_, &QAction::triggered, &window_, [this] { showStyleManager(); });
    QObject::connect(symbolsAction_, &QAction::triggered, &window_,
                     [this] { showSymbolLibrary(); });
    QObject::connect(codesAction_, &QAction::triggered, &window_, [this] { showCodeManager(); });
    QObject::connect(purgeAction_, &QAction::triggered, &window_, [this] { purgeUnused(); });
    QObject::connect(globalModifyAction_, &QAction::triggered, &window_,
                     [this] { showGlobalModify(); });

    // Titled sections: a style that draws titles (theme.cpp) shows what each
    // group is for, and one that does not shows the separators these were.
    menu.addSection(QStringLiteral("Tables and Libraries"));
    if (services_.layers != nullptr) {
        menu.addAction(services_.layers);
    }
    menu.addActions({stylesAction_, symbolsAction_, codesAction_});
    menu.addSection(QStringLiteral("Across the Drawing"));
    menu.addAction(globalModifyAction_);
    menu.addAction(purgeAction_);

    toolBar.addActions({stylesAction_, symbolsAction_, codesAction_, globalModifyAction_});

    if (services_.document != nullptr) {
        // What the session has now - the defaults, in a window whose
        // customisation is installed after it is built - and then every
        // change: the start-up install, a load, CUSTOMISE SET, RESET, REVERT.
        // The watcher delivers on any notification, whichever part it names.
        followedLinework_ = services_.document->customisationState().linework;
        lineworkCodes_ = followedLinework_;
        watcher_ = std::make_unique<DocumentWatcher>(
            *services_.document, [this](const DocumentChanges&) { followLineworkCodes(); });
    }
}

CustomisationWorkbench::~CustomisationWorkbench()
{
    // Here, not left to the window's children: the dialogs paint from the
    // cache and read the linework codes this object owns, which go as soon
    // as this body ends - and the Document goes after that.
    delete globalModify_.data();
    delete codes_.data();
    delete symbols_.data();
    delete styles_.data();
}

void CustomisationWorkbench::followLineworkCodes()
{
    if (services_.document == nullptr || (watcher_ != nullptr && !watcher_->documentAlive())) {
        return;
    }
    const katana::cad::LineworkCodes& now = services_.document->customisationState().linework;
    if (now == followedLinework_) {
        return;
    }
    followedLinework_ = now;
    lineworkCodes_ = now;
    if (codes_.isNull()) {
        return;
    }
    // The tab that is already built: its fields by the object names the
    // headless driver fills them by - "linework" and the control's own word,
    // lineworkStart to lineworkRectangle - and then its own button, as a
    // person taking the codes into use would. Setting the copy above is not
    // enough for a tab that exists: it would go on showing the old codes,
    // and Execute would build a plan it had made with them. (The tab is to
    // read the Document itself, and its button to run CUSTOMISE SET; this
    // goes with the copy then.)
    for (const katana::entity::LineworkCodeMember& code : katana::entity::lineworkCodeMembers()) {
        QString name = QString::fromUtf8(code.name.data(), static_cast<qsizetype>(code.name.size()));
        name[0] = name[0].toUpper();
        if (auto* field = codes_->findChild<QLineEdit*>("linework" + name)) {
            field->setText(QString::fromStdString(now.*code.spelling));
        }
    }
    if (auto* use = codes_->findChild<QPushButton*>(QStringLiteral("lineworkCodesUse"))) {
        use->click();
    }
}

StyleManagerDialog* CustomisationWorkbench::styleManager() const { return styles_.data(); }
SymbolLibraryDialog* CustomisationWorkbench::symbolLibrary() const { return symbols_.data(); }
SurveyCodeManagerDialog* CustomisationWorkbench::codeManager() const { return codes_.data(); }
GlobalModifyDialog* CustomisationWorkbench::globalModify() const
{
    return globalModify_.data();
}

bool CustomisationWorkbench::headless() const
{
    return services_.headless && services_.headless();
}

void CustomisationWorkbench::log(const QString& text, bool isError) const
{
    if (services_.log) {
        services_.log(text, isError);
    }
}

CustomisationContext CustomisationWorkbench::context()
{
    CustomisationContext context;
    context.document = services_.document;
    context.log = [this](const QString& text, bool isError) { log(text, isError); };
    context.thumbnails = thumbnails_.get();
    context.selectAndShow = [this](const std::vector<katana::entity::EntityId>& ids) {
        selectAndShow(ids);
    };
    context.lineworkCodes = &lineworkCodes_;
    context.run = services_.run;
    return context;
}

void CustomisationWorkbench::selectAndShow(const std::vector<katana::entity::EntityId>& ids)
{
    katana::cad::Document& document = *services_.document;
    document.selection().set(ids);
    document.notifySelectionChanged();
    // Framed where the person is looking - the ACTIVE plan view - so what a
    // style or a symbol is used by is seen, not only counted. The other
    // views keep their own zoom, but for the views linked with it. By that
    // view's Zoom to Selection line, so the log says what moved it.
    ViewportWidget* plan = services_.views != nullptr ? services_.views->activePlanView() : nullptr;
    if (plan != nullptr) {
        services_.views->zoomToSelection(plan->state().id);
    }
}

StyleManagerDialog& CustomisationWorkbench::showStyleManager()
{
    const bool first = styles_.isNull();
    if (first) {
        styles_ = new StyleManagerDialog(context(), &window_);
        styles_->setObjectName(QString::fromLatin1(kStyleManagerName));
        // Non-modal, beside the drawing: a style is edited while the lines
        // wearing it are watched change.
        styles_->setModal(false);
    }
    raise(*styles_);
    if (first) {
        // The forms and previews filled in, rather than an empty pane, the
        // first time it is seen (its tables fill once it is shown).
        styles_->showFirstRows();
    }
    return *styles_;
}

SymbolLibraryDialog& CustomisationWorkbench::showSymbolLibrary()
{
    if (symbols_.isNull()) {
        symbols_ = new SymbolLibraryDialog(context(), &window_);
        symbols_->setModal(false);
        // Opened on the first symbol, so its details and picture show what
        // the pane is for, rather than a blank "Choose a symbol".
        if (const std::vector<std::string> shown = symbols_->shownNames(); !shown.empty()) {
            symbols_->selectSymbol(shown.front());
        }
    }
    // A headless session opens no file dialog: Load and Export then say in
    // the log what to call instead.
    symbols_->setHeadless(headless());
    raise(*symbols_);
    return *symbols_;
}

SurveyCodeManagerDialog& CustomisationWorkbench::showCodeManager()
{
    if (codes_.isNull()) {
        // Before it reads them: a line typed in this very turn may have
        // changed the codes, and the watcher has not delivered yet.
        followLineworkCodes();
        codes_ = new SurveyCodeManagerDialog(context(), &window_);
        codes_->setModal(false);
        // Opened on the first rule, explained and in the form, as the other
        // managers open on their first row - not on an empty explanation.
        if (!codes_->buffer().rules().empty()) {
            codes_->selectRule(0);
        }
    }
    // Interactive: file dialogs, and Apply / Discard / Cancel on closing with
    // unapplied edits. A headless session never opens either.
    codes_->setInteractive(!headless());
    raise(*codes_);
    return *codes_;
}

GlobalModifyDialog& CustomisationWorkbench::showGlobalModify()
{
    if (globalModify_.isNull()) {
        globalModify_ = new GlobalModifyDialog(context(), &window_);
        globalModify_->setObjectName(QString::fromLatin1(kGlobalModifyName));
        globalModify_->setModal(false);
        if (services_.views != nullptr) {
            // Read afresh each time (scopeFilterViews).
            ViewWorkspace* views = services_.views;
            globalModify_->views = [views] { return scopeFilterViews(views->viewSet()); };
        }
    }
    globalModify_->reload();
    raise(*globalModify_);
    globalModify_->preview();
    return *globalModify_;
}

bool CustomisationWorkbench::confirmClose()
{
    if (codes_.isNull() || !codes_->dirty()) {
        return true;
    }
    // The manager's own headless close would say its edits are kept in it,
    // which is untrue when the window goes; and a quit must not take them
    // unasked. A script can press Apply (applyMap) or Revert (revertMap).
    if (headless()) {
        log("Unapplied Edits: the Survey Code Manager has rule edits that are not on the "
            "drawing, and a headless run has nobody to ask whether to discard them; Apply or "
            "Revert them first.",
            true);
        return false;
    }
    // Shown and raised first, so the question comes over the rules it is
    // about - the manager may have been closed with its edits kept.
    // Closing a visible dialog runs its reject(), which asks.
    showCodeManager().close();
    return codes_.isNull() || !(codes_->isVisible() && codes_->dirty());
}

bool CustomisationWorkbench::purgeUnused()
{
    katana::cad::Document& document = *services_.document;
    katana::cad::PurgeOptions options;
    // What the next thing drawn will wear is not unused, although nothing
    // wears it yet - the command line's PURGE keeps it for the same reason.
    if (!document.currentStyle().empty()) {
        options.keepStyles.push_back(document.currentStyle());
    }
    const katana::commands::TableItems plan = katana::cad::planPurge(document.model(), options);
    if (plan.empty()) {
        log("Purge Unused: nothing to purge - every style, linetype and hatch pattern is used.",
            false);
        return false;
    }
    const QString what = counted(plan.styles.size(), "style", "styles") + ", " +
                         counted(plan.linetypes.size(), "linetype", "linetypes") + " and " +
                         counted(plan.hatchPatterns.size(), "hatch pattern", "hatch patterns");
    if (!headless()) {
        const QString question = "Delete " + what + " that nothing uses? One Undo restores them.";
        const bool yes = confirm ? confirm(question)
                                 : QMessageBox::question(&window_, "Purge Unused", question) ==
                                       QMessageBox::Yes;
        if (!yes) {
            log("Purge Unused: cancelled; nothing was deleted.", false);
            return false;
        }
    }
    auto command = katana::cad::purgeCommand(document.model(), options);
    if (!command) {
        log("Purge Unused: " + QString::fromStdString(command.error().describe()), true);
        return false;
    }
    if (*command == nullptr) {
        // planPurge said otherwise a moment ago; the model has not changed
        // since, so this is only reached if the two ever disagree.
        log("Purge Unused: nothing to purge.", false);
        return false;
    }
    if (const auto status = document.execute(std::move(*command)); !status) {
        log("Purge Unused: " + QString::fromStdString(status.error().describe()), true);
        return false;
    }
    QString report = "Purge Unused: purged " + what + " (one Undo restores them).";
    if (!plan.styles.empty()) {
        report += " Styles: " + listed(plan.styles) + ".";
    }
    if (!plan.linetypes.empty()) {
        report += " Linetypes: " + listed(plan.linetypes) + ".";
    }
    if (!plan.hatchPatterns.empty()) {
        report += " Hatch patterns: " + listed(plan.hatchPatterns) + ".";
    }
    log(report, false);
    return true;
}

} // namespace katana::qt
