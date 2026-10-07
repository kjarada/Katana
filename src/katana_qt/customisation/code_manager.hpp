#pragma once

// The survey code manager: the feature-code library a surveyor codes against
// - an editor of a customisation's survey codes, Civil 3D's description keys,
// TBC's feature definition manager, Carlson's field-to-finish - in one
// non-modal dialog.
//
// Five tabs, each over one cad foundation, so the dialog decides nothing the
// CLI would say differently:
//   Code Table        cad::codeTable, grouped, with cad::explainCode for the
//                     selected key or a typed code, and the rule form
//   Codes in Drawing  cad::codeCensus over what the tab's scope takes, classed
//                     against the buffer, and the CODE CENSUS line it stands for
//   Issues            cad::lintSurveyMap on the buffer
//   Apply Codes       the CODE line, previewed by cad::applySurveyCodes
//   Linework          the LINEWORK line, previewed by cad::planLinework, and
//                     the customisation's control codes (CUSTOMISE SET)
//
// THE THREE TABS ON DRAWING DATA RUN LINES. Each has the "Apply to" and "Only
// those that match" controls every tool on drawing data has
// (scope_filter_widget.hpp, CLAUDE.md section 1.1), and its Execute builds the
// line a person would type - CODE <scope> [PROPERTY p], LINEWORK <scope>
// [PROPERTY p] [ORDER entity] [CHORD x], and for Codes in Drawing CODE CENSUS
// <scope> [PROPERTY p] - and hands it to the window's executor (CustomisationContext::
// run): echoed, kept in the history, one undo step, answered in the verb's
// words. The line is shown as the controls change (applyCommand,
// lineworkCommand). Preview stays a plan made here, read-only and unlogged,
// because it shows more than a reply does - a row a code, every point in no
// line, every note - but it is planned by what the verb plans with. The
// Linework tab's Use These Codes is the line CUSTOMISE SET linework.*: the
// control codes are the customisation's, the Document's, and the tab shows
// them as they are and follows them as they change.
//
// The tabs have no boxes that switch parts of the coding off (create layers,
// create styles, set attributes) or keep the points a line replaces, because
// no line can say them: CODE always does what the rules say, and LINEWORK never
// removes a point (linework_verbs.hpp), and a second, direct path kept for
// them would be two code paths for one job.
//
// EDITING HAPPENS IN A BUFFER (decision D1). The survey map is session data on
// the Document, not undoable, so edits go to a copy of document->surveyMap()
// through SurveyMap's own mutators, and nothing reaches the Document until
// APPLY (setSurveyMap). REVERT takes the Document's map back. A map changed
// from outside while the buffer is unedited is taken up; with edits pending it
// is not, and the log says so - Apply then replaces it, Revert takes it.
//
// Apply Codes and Linework run against the DOCUMENT'S map: the verbs read the
// Document, which cannot be handed a buffer. The tabs say so while the buffer
// has unapplied edits.
//
// Every widget a test or the headless driver needs has an objectName; the
// tabs are codeTableTab, codesInDrawingTab, codeIssuesTab, applyCodesTab and
// lineworkTab. The dialog may outlive its Document: it watches it through a
// DocumentWatcher and touches it only while documentAlive().

#include <cstddef>
#include <cstdint>
#include <functional>
#include <filesystem>
#include <memory>
#include <optional>
#include <string>
#include <vector>

#include <QDialog>

#include "customisation/customisation_context.hpp"
#include "customisation/scope_filter_widget.hpp"
#include "katana/cad/code_table.hpp"
#include "katana/cad/customisation_merge.hpp"
#include "katana/cad/linework.hpp"
#include "katana/cad/linework_verbs.hpp"
#include "katana/cad/survey_coding.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/survey_map.hpp"

class QCheckBox;
class QComboBox;
class QDoubleSpinBox;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QShowEvent;
class QStackedWidget;
class QTabWidget;
class QTreeWidget;
class QTreeWidgetItem;

namespace katana::qt {

class DocumentWatcher;
struct DocumentChanges;
class FilterBar;
class NamePicker;
class StylePreview;

class SurveyCodeManagerDialog : public QDialog {
  public:
    explicit SurveyCodeManagerDialog(CustomisationContext context, QWidget* parent = nullptr);
    ~SurveyCodeManagerDialog() override;

    SurveyCodeManagerDialog(const SurveyCodeManagerDialog&) = delete;
    SurveyCodeManagerDialog& operator=(const SurveyCodeManagerDialog&) = delete;

    // ---- the buffer ------------------------------------------------------------
    [[nodiscard]] const katana::entity::SurveyMap& buffer() const { return buffer_; }
    // The buffer differs from the drawing's map: the one it was taken from,
    // last applied, or last seen changed elsewhere.
    [[nodiscard]] bool dirty() const;
    // The buffer onto the Document (setSurveyMap), with the commit hook
    // called round it (CustomisationContext::beginCommit): before it, and
    // what it hands back after. InvalidState when the Document is gone - no
    // commit, so the hook is not called.
    [[nodiscard]] katana::core::Status apply();
    // The Document's map back into the buffer, discarding the edits.
    void revert();

    // The edits the form's buttons make, by rule index (SurveyMap's), each
    // refused exactly as SurveyMap refuses it, leaving the buffer as it was.
    [[nodiscard]] katana::core::Status addRule(katana::entity::SurveyRule rule);
    [[nodiscard]] katana::core::Status replaceRule(std::size_t index,
                                                   katana::entity::SurveyRule rule);
    [[nodiscard]] katana::core::Status duplicateRule(std::size_t index);
    [[nodiscard]] katana::core::Status removeRule(std::size_t index);
    // -1 up (earlier: wins more ties), +1 down.
    [[nodiscard]] katana::core::Status moveRule(std::size_t index, int by);

    // The rule the form edits; nullopt while it holds a new rule.
    [[nodiscard]] std::optional<std::size_t> currentRule() const { return formIndex_; }
    // Shows the rule in the Code Table and loads it into the form.
    void selectRule(std::size_t index);
    // Shows the key's row and explains it; its first rule goes into the form.
    void selectKey(const std::string& key);
    // What the form holds, as a rule - InvalidArgument for a field that does
    // not parse (a size that is not a number, an attribute line).
    [[nodiscard]] katana::core::Result<katana::entity::SurveyRule> formRule() const;
    // A new feature rule in the form for a code the drawing carries, keyed
    // by cad::suggestedKey, on the Code Table tab. Nothing is added until Add.
    void newRuleFromCode(const std::string& code);

    // ---- files -----------------------------------------------------------------
    // What the Import / Export buttons do once a file is chosen; the buttons
    // ask for a file only in an interactive session.
    //
    // Reads a Katana customisation file (docs/customisation.md) and merges
    // ITS RULES into the BUFFER through cad::mergeCustomisation - Merge by
    // default, as loading does everywhere (D1) - for review before Apply.
    // The definitions, colours and settings the file also holds are left
    // alone and counted in the log: this edits the survey codes only.
    // NotFound for a file with no rules; what the format's reader refuses
    // keeps its own error (a file in another format is "not a Katana
    // customisation file"). The buffer is as it was after any failure.
    [[nodiscard]] katana::core::Status
    importCodes(const std::filesystem::path& path,
                katana::cad::LoadMode mode = katana::cad::LoadMode::Merge);
    // Runs CUSTOMISE EXPORT <file> CODES through the window's executor: the
    // drawing's rules as a Katana customisation file of survey codes alone, in
    // their order, under the session's name (the file's own for a session with
    // none) and with what of the session belongs with them
    // (cad::customisationPart, the one rule a part is written by) - its
    // description and its author's notice, the sources that brought it rules,
    // each with its notice, the notice of every source left out, and the
    // colours the rules name. Not its definitions, and not its linework codes
    // or automation switches. The verb writes what the DRAWING has, so a
    // buffer with unapplied edits is refused (Apply or Revert first) rather
    // than exported as something no line can name.
    [[nodiscard]] katana::core::Status exportCodes(const std::filesystem::path& path) const;
    // The buffer's code list (cad::codeListCsv), UTF-8.
    [[nodiscard]] katana::core::Status exportCodeList(const std::filesystem::path& path) const;

    // ---- session -----------------------------------------------------------------
    // Interactive: file dialogs, and a question on closing with unapplied
    // edits. Off, the dialog never opens a modal box (a headless session has
    // no one to close it). Defaults to on unless the platform is offscreen or
    // minimal.
    void setInteractive(bool interactive) { interactive_ = interactive; }
    [[nodiscard]] bool interactive() const { return interactive_; }

    // The workspace's open views, for the View scope of the two action tabs:
    // asked whenever a view list or a View scope is needed, as Global
    // Modify's is. Unset (a test, a manager built on its own): the View scope
    // offers one "Whole drawing view", which a preview reads and no line can
    // name (ScopeFilterWidget::noWorkspaceViews).
    std::function<std::vector<ScopeFilterView>()> views{};

    // Closing (Escape, Close, the window's close button): with unapplied
    // edits an interactive session is asked Apply / Discard / Cancel; a
    // headless one closes and keeps the buffer, and says so in the log.
    void reject() override;

  protected:
    // The layers and views the two action tabs offer are the drawing's and
    // the window's as they are when the manager is shown.
    void showEvent(QShowEvent* event) override;

  private:
    // ---- building
    QWidget* buildCodeTableTab();
    QWidget* buildRuleForm();
    QWidget* buildDetailPane();
    QWidget* buildCensusTab();
    QWidget* buildIssuesTab();
    QWidget* buildApplyTab();
    QWidget* buildLineworkTab();

    // ---- reacting
    [[nodiscard]] katana::cad::Document* document() const;
    void documentChanged(const DocumentChanges& changes);
    void bufferChanged();
    void updateDirty();
    void log(const QString& message, bool isError = false) const;

    // ---- code table and detail
    void rebuildCodeTable();
    void treeSelectionChanged();
    void explain(const std::string& code);
    [[nodiscard]] QTreeWidgetItem* findRuleItem(std::size_t index) const;
    [[nodiscard]] QTreeWidgetItem* findKeyItem(const std::string& key) const;

    // ---- form
    void loadForm(const katana::entity::SurveyRule& rule, std::optional<std::size_t> index);
    void showSectionPage();
    void updateFormIssues();
    void formButton(const std::function<katana::core::Status()>& edit);

    // ---- other tabs
    void refreshCensus();
    void filterCensus();
    [[nodiscard]] std::string censusProperty() const;
    [[nodiscard]] std::string selectedCensusCode() const;
    void selectEntitiesWithCode();
    void rebuildIssues();
    // The scope widget of each action tab, set up alike: its views are the
    // dialog's, and an edit of it rewrites the line shown.
    [[nodiscard]] ScopeFilterWidget* makeScope(const char* prefix, QWidget* parent);
    // The layer and view lists of both, refilled; what is ticked is kept.
    void reloadScopes();
    // What a tab's scope and filter take NOW: cad::matchEntities over the
    // shared widget's reading of its controls, as Global Modify's preview
    // resolves them. Fails as the widget does (no layer ticked, a view that
    // has closed), and with NotFound for a layer the drawing lacks.
    [[nodiscard]] katana::core::Result<std::vector<katana::entity::EntityId>>
    scopeTakes(const ScopeFilterWidget& scope) const;
    // The lines the two Execute buttons run, as the controls stand; an error
    // names the control that cannot be said on a line.
    [[nodiscard]] katana::core::Result<QString> codesLine() const;
    [[nodiscard]] katana::core::Result<QString> lineworkLine() const;
    // The census tab's line, CODE CENSUS <scope> [PROPERTY p]: what its Run
    // hands to the executor, and what the table is counted by.
    [[nodiscard]] katana::core::Result<QString> censusLine() const;
    void runCensusLine();
    // CUSTOMISE SET with the control codes typed that are not the
    // customisation's already; an empty line when all seven are.
    [[nodiscard]] katana::core::Result<QString> lineworkCodesLine() const;
    // The two lines shown again (applyCommand, lineworkCommand).
    void showLines();
    // A line through the window's executor. False, with `said` why, when the
    // manager was given none: it then does NOT do the work itself.
    [[nodiscard]] bool runLine(const QString& line, VerbOutcome& outcome, QString& said) const;
    [[nodiscard]] katana::cad::SurveyCodingOptions codingOptions() const;
    // The previews fill the panes and say, with the log, why they could not;
    // true when they planned.
    bool previewCodes();
    void executeCodes();
    // The seven fields from the customisation's control codes.
    void loadLineworkCodes();
    [[nodiscard]] katana::cad::LineworkCodes typedLineworkCodes() const;
    void useLineworkCodes();
    // What the Linework line says beyond its scope: the property, the order
    // and the chord length, each only when it is not the verb's default.
    [[nodiscard]] katana::cad::LineworkWords lineworkWords() const;
    bool previewLinework();
    void executeLinework();

    CustomisationContext context_{};
    katana::entity::SurveyMap buffer_{};
    // The drawing's map as the manager last saw it - taken, applied, or
    // changed elsewhere: what dirty() is measured against.
    katana::entity::SurveyMap baseline_{};
    // The customisation's control codes as the seven fields were last filled
    // from them: a change of the Document's is taken up, and what a person is
    // typing is not overwritten by a change of anything else.
    katana::cad::LineworkCodes shownLinework_{};
    bool interactive_ = true;
    std::optional<std::size_t> formIndex_{};
    std::string explained_{};
    bool loadingForm_ = false;
    bool rebuildingTree_ = false;
    // The linestyle and symbol names the form was last loaded with. Once the
    // drawing has gone the pickers cannot be loaded (they read its library),
    // so formRule() takes these instead of what the pickers still hold - the
    // names of whichever rule was loaded before the drawing closed.
    std::string formLinestyle_{};
    std::string formSymbol_{};
    bool pickersLoaded_ = true;

    struct CensusRow {
        std::string code{};
        std::size_t entities = 0;
        katana::entity::SurveyMatchKind kind = katana::entity::SurveyMatchKind::None;
        bool matched = false;
        std::string model{};
    };
    std::vector<CensusRow> census_{};
    std::string censusFoundProperty_{};
    // What the census tab's scope took at the last count, and why it took
    // nothing when its controls could not be read (no layer ticked, a view
    // that closed): the table is of exactly these entities.
    std::vector<katana::entity::EntityId> censusIds_{};
    QString censusProblem_{};
    std::vector<katana::cad::LintIssue> issues_{};

    // widgets (owned by Qt's parent chain)
    QTabWidget* tabs_ = nullptr;
    QLabel* dirtyLabel_ = nullptr;
    QPushButton* applyButton_ = nullptr;
    QPushButton* revertButton_ = nullptr;
    QCheckBox* importReplace_ = nullptr;
    // code table
    FilterBar* tableFilter_ = nullptr;
    QTreeWidget* tree_ = nullptr;
    QLineEdit* testCode_ = nullptr;
    QLabel* explainHeading_ = nullptr;
    QTreeWidget* explainFields_ = nullptr;
    QLabel* explainDefinitions_ = nullptr;
    QLabel* explainNearMisses_ = nullptr;
    StylePreview* linestylePreview_ = nullptr;
    StylePreview* symbolPreview_ = nullptr;
    // form
    QLabel* formTitle_ = nullptr;
    QLineEdit* ruleKey_ = nullptr;
    QComboBox* ruleSection_ = nullptr;
    QStackedWidget* rulePages_ = nullptr;
    QLineEdit* ruleModel_ = nullptr;
    QLabel* ruleModelStatus_ = nullptr;
    QComboBox* ruleColour_ = nullptr;
    QComboBox* ruleBreakline_ = nullptr;
    NamePicker* ruleLinestyle_ = nullptr;
    QLineEdit* ruleWeight_ = nullptr;
    QLineEdit* ruleGroup_ = nullptr;
    NamePicker* ruleSymbol_ = nullptr;
    QComboBox* ruleSymbolColour_ = nullptr;
    QLineEdit* ruleSymbolSize_ = nullptr;
    QLineEdit* ruleSymbolRotation_ = nullptr;
    QLineEdit* ruleSymbolOffset_ = nullptr;
    QLineEdit* ruleSymbolRaise_ = nullptr;
    QComboBox* ruleHide_ = nullptr;
    QLineEdit* ruleTextStyle_ = nullptr;
    QComboBox* ruleTextColour_ = nullptr;
    QComboBox* ruleTextType_ = nullptr;
    QLineEdit* ruleTextSize_ = nullptr;
    QLineEdit* ruleTextJustifyX_ = nullptr;
    QLineEdit* ruleTextJustifyY_ = nullptr;
    QComboBox* rulePipeJustify_ = nullptr;
    QComboBox* rulePipeShape_ = nullptr;
    QLineEdit* rulePipeSize1_ = nullptr;
    QLineEdit* rulePipeSize2_ = nullptr;
    QCheckBox* rulePipeActive_ = nullptr;
    QPlainTextEdit* ruleAttributes_ = nullptr;
    QComboBox* ruleTinable_ = nullptr;
    QLineEdit* ruleComment_ = nullptr;
    QLabel* ruleIssues_ = nullptr;
    QPushButton* ruleUpdate_ = nullptr;
    QPushButton* ruleDuplicate_ = nullptr;
    QPushButton* ruleDelete_ = nullptr;
    QPushButton* ruleUp_ = nullptr;
    QPushButton* ruleDown_ = nullptr;
    // codes in drawing
    ScopeFilterWidget* censusScope_ = nullptr;
    QLineEdit* censusCommand_ = nullptr;
    QComboBox* censusPropertyBox_ = nullptr;
    FilterBar* censusFilter_ = nullptr;
    QTreeWidget* censusTree_ = nullptr;
    QLabel* censusSummary_ = nullptr;
    // issues
    FilterBar* issuesFilter_ = nullptr;
    QTreeWidget* issuesTree_ = nullptr;
    QLabel* issuesSummary_ = nullptr;
    // apply codes
    ScopeFilterWidget* applyScope_ = nullptr;
    QComboBox* applyProperty_ = nullptr;
    QLabel* applyDirtyNote_ = nullptr;
    QLineEdit* applyCommand_ = nullptr;
    QPlainTextEdit* applyReport_ = nullptr;
    QTreeWidget* applyRows_ = nullptr;
    // linework
    ScopeFilterWidget* lineworkScope_ = nullptr;
    std::vector<QLineEdit*> lineworkCodeFields_{};
    QLabel* lineworkCodesStatus_ = nullptr;
    QPushButton* lineworkCodesUse_ = nullptr;
    QComboBox* lineworkOrder_ = nullptr;
    QDoubleSpinBox* lineworkChord_ = nullptr;
    QComboBox* lineworkProperty_ = nullptr;
    QLabel* lineworkDirtyNote_ = nullptr;
    QLineEdit* lineworkCommand_ = nullptr;
    QLabel* lineworkSummary_ = nullptr;
    QTreeWidget* lineworkStrings_ = nullptr;
    QTreeWidget* lineworkUnplaced_ = nullptr;
    QTreeWidget* lineworkNotes_ = nullptr;

    // Last, so it is destroyed first: no delivery reaches a half-destroyed
    // dialog.
    std::unique_ptr<DocumentWatcher> watcher_;
};

} // namespace katana::qt
