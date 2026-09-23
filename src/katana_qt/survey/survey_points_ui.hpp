#pragma once

// The Survey menu's point tools (PLAN.MD 45 slices 11 and 13): Export Survey
// Points, the Point Manager dock and the Point Report.
//
// As with every survey dialog, nothing survey-specific is computed here: the
// points come from cad::drawingSurveyPoints / surveyPointsAmong, the file is
// written by surveyio's delimited writer, the report text by
// cad::formatPointReport and cad::pointReportCsv.
//
// Object names (the headless --survey-dialog and --survey-dock switches use
// them):
//   surveyExportDialog       fields  file scope template preset columns
//                                    delimiter header quoting comment decimals
//                                    unit
//                            buttons browse export
//   SurveyPointsDock         fields  filter
//                            shown   points (the table) status
//   surveyPointReportDialog  shown   report
//                            buttons refresh copy saveCsv close

#include <QDialog>
#include <QDockWidget>

#include <vector>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_points.hpp"
#include "survey/survey_dialogs.hpp"

class QCheckBox;
class QComboBox;
class QLabel;
class QLineEdit;
class QPlainTextEdit;
class QTableWidget;

namespace katana::qt {

class ViewWorkspace;

class SurveyExportDialog final : public SurveyToolDialog {
  public:
    SurveyExportDialog(SurveyDialogContext context, QWidget* parent);

  private:
    [[nodiscard]] katana::core::Result<std::string> exportPoints();

    QLineEdit* file_ = nullptr;
    QComboBox* scope_ = nullptr;
    QComboBox* template_ = nullptr;
    QComboBox* preset_ = nullptr;
    QLineEdit* columns_ = nullptr;
    QComboBox* delimiter_ = nullptr;
    QCheckBox* header_ = nullptr;
    QComboBox* quoting_ = nullptr;
    QLineEdit* comment_ = nullptr;
    QLineEdit* decimals_ = nullptr;
    QComboBox* unit_ = nullptr;
};

// Survey > Point Manager: the drawing's survey points in a table - filter
// box, sortable columns, rows selecting their entities, a double-click
// zooming to the point. READ-ONLY in this slice. Editing would need each
// change to go through a cad command (move the point entity, rewrite its
// number, code or height property) so that it is undoable, and a rule for an
// edited id that another point already has - the import's ExistingPointPolicy
// question asked of one cell.
//
// The table is rebuilt from a Document listener ON THE EVENT LOOP, never
// inside the listener (docs/cad.md, "Panels refresh on the event loop"):
// selecting rows changes the document's selection, which calls the listener,
// and a table rebuilt there would delete the rows whose signal is still
// running. A refresh whose points are unchanged - a selection click - only
// mirrors the selection onto the rows.
class SurveyPointsDock final : public QDockWidget {
  public:
    SurveyPointsDock(katana::cad::Document& document, ViewWorkspace* views, QWidget* parent);

    // Points listed, and shown under the filter: "12 points, 3 shown".
    [[nodiscard]] QString status() const;

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void scheduleRefresh();
    void refresh();
    void applyFilter();
    void mirrorSelection();
    void selectionEdited();
    void zoomToRow(int row);

    katana::cad::Document& document_;
    ViewWorkspace* views_ = nullptr;
    QLineEdit* filter_ = nullptr;
    QLabel* status_ = nullptr;
    QTableWidget* table_ = nullptr;
    std::vector<katana::cad::DrawingSurveyPoint> points_;
    bool refreshPending_ = false;
    bool listed_ = false;     // points_ has been read at least once
    bool mirroring_ = false;  // the selection is being copied TO the rows
    // Last, so it is destroyed first and no listener outlives what it uses.
    katana::cad::Document::ListenerHandle listener_;
};

// Survey > Point Report...: the points - the selected survey points when any
// are selected, all of them otherwise - as a table of text, with Copy and
// Save As CSV. Non-modal, rebuilt each time it is shown or refreshed.
class SurveyPointReportDialog final : public QDialog {
  public:
    SurveyPointReportDialog(SurveyDialogContext context, QWidget* parent);

  protected:
    void showEvent(QShowEvent* event) override;

  private:
    void refresh();

    SurveyDialogContext context_;
    QLabel* scope_ = nullptr;
    QPlainTextEdit* report_ = nullptr;
    std::vector<katana::cad::DrawingSurveyPoint> points_;
};

} // namespace katana::qt
