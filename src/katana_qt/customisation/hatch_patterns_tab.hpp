#pragma once

// The Hatch Patterns tab of Format > Styles and Linetypes (docs/desktop.md,
// "Styles and Linetypes"): the drawing's hatch patterns - which the HATCH verb
// always had and the window only offered by name in pick lists - listed with
// what uses them, and edited.
//
// NOTHING IS CHANGED HERE. Every button writes the line a person would type -
// HATCH NEW, SOLID, SET, DELETE, PURGE HATCHES, STYLE NEW - and runs it
// through the window's one executor (CustomisationContext::run), so it is
// echoed, kept in the history and one undo step. The rows are
// cad::hatchPatternRows, below Qt.
//
// The family grid is a buffer, as the Linetypes tab's pattern grid is: edited
// in place, saved by one HATCH SET line, and never a line run from the table's
// own signal. The swatch draws the grid as it stands, through the hatcher the
// plan view draws with (cad::hatchSegments), so what is seen before Save is
// what Save will draw. A reload that did not change the pattern the grid was
// filled from keeps its unsaved edits.
//
// No modal box: names are typed in the tab's name field. It hears the
// Document through a DocumentWatcher, so it may outlive the Document and then
// does nothing.
//
// Object names:
//   hatchPatternsPage          the tab
//   hatchPatternTable          name, kind (solid, N families), used by, description
//   hatchPatternEditor         the editor of the chosen pattern:
//     hatchPatternSolid        a solid fill rather than line families
//     hatchPatternFamilies     angle (degrees) and spacing (model units), a row a family
//     hatchFamilyAdd, hatchFamilyRemove
//     hatchPatternPreview      the swatch
//     hatchPatternSave         HATCH SET name a s [a s ...] | HATCH SET name SOLID
//     hatchPatternRevert       the editor as the drawing holds the pattern again
//   hatchPatternName           the name New, New Solid, Duplicate and New Style use
//   hatchPatternNew            HATCH NEW name a s ... from the family grid
//   hatchPatternNewSolid       HATCH SOLID name
//   hatchPatternDuplicate      the chosen pattern again under the name
//   hatchPatternDelete         HATCH DELETE name
//   hatchPatternPurge          PURGE HATCHES
//   hatchPatternSelectUsers    select the entities drawn with the chosen pattern
//   hatchPatternNewStyle       STYLE NEW name HATCH pattern: a style that hatches with it
//   hatchPatternStatus         what the last line replied, or why it was refused

#include <memory>

#include <QString>
#include <QWidget>

#include "customisation/customisation_context.hpp"

namespace katana::qt {

// No Q_OBJECT: it connects to lambdas, which keeps moc out of this target.
class HatchPatternsTab final : public QWidget {
  public:
    // `context.document` and `context.log` must be set; `run` may be empty
    // (the tab then says it cannot run the line), as may `selectAndShow`.
    explicit HatchPatternsTab(const CustomisationContext& context, QWidget* parent = nullptr);
    ~HatchPatternsTab() override;

    // Chooses the pattern `name` in the table; false when there is none.
    bool selectPattern(const QString& name);
    // The chosen pattern's name; empty when there is none.
    [[nodiscard]] QString currentPattern() const;

  private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

} // namespace katana::qt
