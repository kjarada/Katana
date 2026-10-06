#pragma once

// The definition editor: ONE linestyle or symbol definition of the session's
// customisation, made, changed, copied or deleted.
//
// One non-modal dialog, made once by the Format workbench and kept, as the
// managers are. The Symbol Library's New, Edit, Duplicate and Delete and the
// Linetypes tab's Edit Definition and New Library Linestyle ask for it
// (CustomisationContext::editDefinition); it has New, Duplicate and Delete of
// its own, so every one of the four is reached from either.
//
//   left   a FORM over entity::LineStyle - name, group, kind (the format's two
//          lists: a linestyle or a symbol), units, at vertices, length,
//          factor, origin, and a two-point definition's anchors and modes -
//          and under it the STROKES as text
//   right  the definition as it is drawn, live and before it has a name: as a
//          symbol at a point and, for a linestyle, along the sample line
//          (StylePreview::setDefinition); under them a line that says when
//          the picture is of the last text that read, not of what is typed
//   below  the message area, and the buttons
//
// THE STROKES ARE TEXT, in the Katana customisation format's own form
// (docs/customisation.md, "The members"), one stroke a line:
//     ["move", 0, 0],
//     ["draw", 8, 0],
//     ["text", {"text": "W", "height": 1.5, "justify": "middle-centre"}]
// so what a person learns here is what a customisation file holds, and a line
// copied from one to the other means the same. They are read as they are
// typed by the format's own reader (entity::definitionFromJson) and by no
// second parser of this dialog's; what joins the lines for it, finds the line
// a refusal is about and words it is below the window, where it is tested
// without one (cad/definition_edit.hpp). The comma that ends a line in a file
// may be left off here, and a blank line is passed over. A number is shown as
// the shortest text that reads back as itself, and an empty number field is
// the value a file's missing member has.
//
// A NAME IS FIXED once its definition exists. Styles and survey code rules
// NAME a definition and a library has no rename, so another name would be
// another definition, and everything naming the first left naming nothing.
// Duplicate is how a definition comes by a new name. A NEW name is refused
// when no command line could name the definition by it - a double quote, or
// a word of the CUSTOMISE REMOVE line itself (cad::removeDefinitionLine) -
// since it could then never be removed.
//
// THE STROKES BOX IS READ CHARACTER FOR CHARACTER (QTextDocument::toRawText),
// never as "plain text", which hands a no-break space back as a blank: a form
// opened on a text holding one would read as edited before anything was typed.
//
// COMMITTING. The library is session data with no undo
// (cad::Document::styleLibrary):
//   Save     installs a copy of the session's library with the definition
//            added or replaced (Document::setStyleLibrary), as the Survey Code
//            Manager's Apply installs its map. Enabled only while the form
//            reads as a definition, and for a new one only while its name is
//            free.
//   Revert   the form as it was opened, or as the library has had the
//            definition since.
//   Delete   the line `CUSTOMISE REMOVE "<name>"` (cad::removeDefinitionLine)
//            through the window's one executor (CustomisationContext::run).
//            That verb refuses a definition something names. What the line
//            answered is shown as it answered it - the editor does not guess
//            why a line failed - and, when something does name the
//            definition (cad::definitionUsers, which is what the verb asks),
//            that is listed under it and Delete Anyway is offered: the same
//            line with FORCE. Delete Anyway exists only while that refusal is
//            on the page: its button is hidden and disabled otherwise, and
//            remove(true) is refused. No box asks either question: a headless
//            run meets none. What was deleted stays in the form, and Save
//            puts it back.
// Round each commit the editor calls CustomisationContext::beginCommit, and
// afterwards what that handed back.
//
// The form holds one definition. Opening another while it has edits that are
// not saved is REFUSED and said, in the message area and the log: there is
// no undo to get typed strokes back from, so Save or Revert comes first.
// Closing the WINDOW over such edits asks (CustomisationWorkbench::
// confirmClose); a yes is remembered here and reverts nothing, since the
// close may yet be cancelled by a later question.
//
// HEADLESS. No button is a default and Enter in a field presses nothing. The
// object names: definitionHeading; definitionName, definitionGroup,
// definitionKind, definitionUnits (and definitionUnitsNote),
// definitionAtVertices, definitionLength, definitionFactor,
// definitionOriginX / Y, definitionAnchor1X / Y, definitionAnchor2X / Y,
// definitionStretchMode, definitionCycleMode, definitionSource;
// definitionStrokes (and definitionStrokesHelp); definitionPreview,
// definitionLinePreview, definitionPreviewNote, definitionPlotScale;
// definitionIssues; definitionDeleteAnyway, definitionDeleteCancel (in
// definitionDeletePanel); definitionNew,
// definitionDuplicate, definitionDelete, definitionRevert, definitionSave,
// definitionClose.
//
// LIFETIME. As the managers: it watches the Document through a
// DocumentWatcher (its last member), acts on a change from the event loop,
// and does nothing once the Document has gone. A definition changed or
// removed elsewhere while the form holds edits is said in the message area,
// and what was typed is kept.

#include <memory>
#include <optional>
#include <string>
#include <string_view>

#include <QDialog>
#include <QString>

#include "customisation_context.hpp"
#include "katana/core/error.hpp"
#include "katana/entity/style_library.hpp"

class QCheckBox;
class QComboBox;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QPushButton;
class QSpinBox;
class QWidget;

namespace katana::qt {

class DocumentWatcher;
class StylePreview;
struct DocumentChanges;

class DefinitionEditorDialog : public QDialog {
  public:
    explicit DefinitionEditorDialog(const CustomisationContext& context, QWidget* parent = nullptr);
    ~DefinitionEditorDialog() override;

    DefinitionEditorDialog(const DefinitionEditorDialog&) = delete;
    DefinitionEditorDialog& operator=(const DefinitionEditorDialog&) = delete;

    // ---- what the form is opened on -----------------------------------------------
    //
    // Each is refused - false, said in the message area and the log - while
    // the form has edits that are not saved, once the Document has gone, and
    // for a name the library does not define.

    // An empty form for a symbol or a linestyle.
    bool newDefinition(bool symbol);
    // The library's definition `name`, its name fixed.
    bool editDefinition(std::string_view name);
    // A copy of it under the first free name ("TEST Valve 2"), as a new
    // definition whose name can still be changed.
    bool duplicateDefinition(std::string_view name);
    // Shows it, then runs Delete on it (remove, below). True when it went.
    bool deleteDefinition(std::string_view name);
    // CustomisationContext::editDefinition's request, as one of the four.
    // (Not `open`: that is QDialog's, and shows the dialog modal.)
    bool request(DefinitionEdit what, const std::string& name);

    // ---- what it holds --------------------------------------------------------------

    // The library definition Save replaces; empty while a new one is being
    // made. It may no longer be IN the library: deleted here, or elsewhere.
    [[nodiscard]] std::string editedName() const { return existing_.value_or(std::string()); }
    // The form differs from what it was opened with.
    [[nodiscard]] bool dirty() const;
    // The definition the form describes, or why it describes none - the
    // format's own refusal, or entity::validate's.
    [[nodiscard]] katana::core::Result<katana::entity::LineStyle> definition() const;
    // The line of the strokes box, counted from 1, that the last reading
    // stopped at; 0 when it read, or stopped at something else.
    [[nodiscard]] int problemLine() const { return problemLine_; }

    // ---- the buttons ----------------------------------------------------------------

    // False, and nothing changed, when the form does not read, a new
    // definition's name is taken, or the Document has gone.
    bool save();
    void revert();
    // `CUSTOMISE REMOVE "<name>"`, with FORCE when `anyway`. True when the
    // line was carried out. `anyway` is refused, and nothing run, unless the
    // plain line's refusal is showing with what uses the definition: FORCE
    // answers a list somebody has read, not a press out of nowhere.
    bool remove(bool anyway);
    // Whether that refusal is showing, and Delete Anyway with it.
    [[nodiscard]] bool deleteAnywayOffered() const;
    // Closing hides the dialog and keeps what was typed; said in the log, so
    // a headless run that closed it over edits reads that it did.
    void reject() override;

    // The window is closing, a person was asked whether the unsaved edits
    // here may be discarded, and said yes
    // (CustomisationWorkbench::confirmClose). Only REMEMBERED, until the form
    // next changes: a close has more questions after this one, and one
    // answered Cancel must find what was typed still here. The edits go when
    // the window does.
    void agreeToDiscard() { discardAgreed_ = dirty(); }
    [[nodiscard]] bool discardAgreed() const { return discardAgreed_; }

    [[nodiscard]] StylePreview* symbolPreview() const { return preview_; }
    [[nodiscard]] StylePreview* linePreview() const { return linePreview_; }

  private:
    // Everything the form shows that a person can change. Two of them equal
    // is "nothing was edited"; it is compared as TEXT because that is what
    // was typed, and a number field holding "0.50" has been edited although
    // the definition it describes has not.
    struct Form {
        QString name{};
        QString group{};
        bool symbol = false;
        int units = 0;
        bool atVertices = false;
        QString length{};
        QString factor{};
        QString originX{};
        QString originY{};
        QString anchor1X{};
        QString anchor1Y{};
        QString anchor2X{};
        QString anchor2Y{};
        int stretchMode = 0;
        int cycleMode = 0;
        QString strokes{};

        friend bool operator==(const Form&, const Form&) = default;
    };
    // What reading the form gave: a definition, or why none and at which line.
    struct Reading {
        std::optional<katana::entity::LineStyle> definition{};
        // What the fields and the strokes DRAW, which needs no name: a new
        // definition is pictured from its first stroke, under a stand-in name
        // while it has none. Set whenever `definition` is.
        std::optional<katana::entity::LineStyle> drawable{};
        katana::core::Error error{};
        QString problem{};
        int line = 0;
    };

    void buildUi();
    [[nodiscard]] bool alive() const;
    void log(const QString& message, bool isError) const;
    void onDocumentChanged(const DocumentChanges& changes);

    // The form a definition is shown as. Fails, saying why as the end of a
    // sentence ("because ..."), when the format cannot write its strokes (a
    // stroke carrying what its kind does not use) or the strokes box cannot
    // hold them one a line (a text holding a paragraph separator).
    [[nodiscard]] static katana::core::Result<Form>
    formOf(const katana::entity::LineStyle& definition);
    // The same without its strokes, which is all of a definition that has none.
    [[nodiscard]] static Form fieldsOf(const katana::entity::LineStyle& definition);
    [[nodiscard]] Form shownForm() const;
    // The strokes box's text, character for character.
    [[nodiscard]] QString strokesShown() const;
    void showForm(const Form& form);
    [[nodiscard]] Reading read() const;
    // Whether another definition may take the form's place; says so when not.
    [[nodiscard]] bool mayReplace(const QString& what);
    // The form loaded with `form` as what there is to go back to.
    void load(const Form& form, std::optional<std::string> existing, std::string source,
              const QString& origin);
    // Everything that follows from what the form holds: the previews, the
    // message area, the heading and which buttons can be pressed.
    void refresh();
    void say(const QString& message, bool isError);
    void edited();
    void markProblemLine();

    CustomisationContext context_{};
    std::optional<std::string> existing_{};
    // The library's definition of that name as the editor last knew it; none
    // when the library had none. What "changed elsewhere" is measured against.
    std::optional<katana::entity::LineStyle> seen_{};
    Form baseline_{};
    // LineStyle::source: where the definition came from, which no field edits.
    std::string source_{};
    // How the form came to hold what it does, for the heading: "a copy of ...".
    QString origin_{};
    // What happened to the definition outside the form; kept until Save or Revert.
    QString note_{};
    // What the editor last did or refused, shown first until the next edit.
    QString said_{};
    bool saidIsError_ = false;
    // The plain REMOVE line was refused and something names the definition:
    // what does is listed, and Delete Anyway offered, until the form changes.
    bool removeRefused_ = false;
    bool discardAgreed_ = false;
    // A picture is up in the panes; it is of the last form that read.
    bool pictured_ = false;
    bool loading_ = false;
    int problemLine_ = 0;

    QLabel* heading_ = nullptr;
    QLineEdit* name_ = nullptr;
    QLineEdit* group_ = nullptr;
    QComboBox* kind_ = nullptr;
    QComboBox* units_ = nullptr;
    QLabel* unitsNote_ = nullptr;
    QCheckBox* atVertices_ = nullptr;
    QLineEdit* length_ = nullptr;
    QLineEdit* factor_ = nullptr;
    QLineEdit* originX_ = nullptr;
    QLineEdit* originY_ = nullptr;
    QLineEdit* anchor1X_ = nullptr;
    QLineEdit* anchor1Y_ = nullptr;
    QLineEdit* anchor2X_ = nullptr;
    QLineEdit* anchor2Y_ = nullptr;
    QSpinBox* stretchMode_ = nullptr;
    QSpinBox* cycleMode_ = nullptr;
    QLabel* sourceLabel_ = nullptr;
    QPlainTextEdit* strokes_ = nullptr;
    StylePreview* preview_ = nullptr;
    StylePreview* linePreview_ = nullptr;
    QWidget* lineBox_ = nullptr;
    QComboBox* plotScale_ = nullptr;
    QLabel* previewNote_ = nullptr;
    QPlainTextEdit* issues_ = nullptr;
    QWidget* removePanel_ = nullptr;
    QPushButton* removeAnyway_ = nullptr;
    QPushButton* removeCancel_ = nullptr;
    QPushButton* new_ = nullptr;
    QPushButton* duplicate_ = nullptr;
    QPushButton* delete_ = nullptr;
    QPushButton* revert_ = nullptr;
    QPushButton* save_ = nullptr;

    // Last, so it is destroyed first: no delivery reaches a half-destroyed
    // dialog, and it is how everything here knows the Document is still there.
    std::unique_ptr<DocumentWatcher> watcher_;
};

} // namespace katana::qt
