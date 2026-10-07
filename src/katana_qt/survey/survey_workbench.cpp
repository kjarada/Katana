#include "survey/survey_workbench.hpp"

#include <utility>

#include <QAction>
#include <QMainWindow>
#include <QMenu>
#include <QToolBar>

#include "dock_chrome.hpp"
#include "katana/cad/document.hpp"
#include "katana/cad/survey_tools.hpp"
#include "survey/survey_dialogs.hpp"
#include "survey/survey_import_wizard.hpp"
#include "survey/survey_jobs_dialog.hpp"
#include "survey/survey_parcel_dialog.hpp"
#include "survey/survey_points_ui.hpp"

namespace katana::qt {

SurveyWorkbench::SurveyWorkbench(QMainWindow& window, SurveyServices services, QMenu& menu,
                                 QToolBar& toolBar)
    : window_(window), services_(std::move(services))
{
    const auto make = [this](Icon icon, const QString& text, const QString& tip,
                             const QString& name) {
        return services_.makeAction(icon, text, tip, QKeySequence(), name);
    };
    QAction* inverse = make(Icon::SurveyInverse, "In&verse...",
                            "Distance, azimuth, bearing and height difference between two points",
                            "surveyInverse");
    QAction* forward = make(Icon::SurveyForward, "&Forward Point...",
                            "A new point from a start, a direction and a distance (radiation)",
                            "surveyForward");
    QAction* area = make(Icon::SurveyArea, "&Area of Selection",
                         "Area and perimeter of the selected closed polylines and circles",
                         "surveyArea");
    // No mnemonic: every letter of its name is taken in this menu, and a
    // shared one would make Alt+letter cycle instead of choose.
    QAction* parcel =
        make(Icon::SurveyArea, "Parcel Report...",
             "A closed polyline's courses, area and legal description, and course labels",
             "surveyParcelReport");
    QAction* angle = make(Icon::SurveyAngle, "Angle and &Bearing Calculator...",
                          "Convert angles between DMS, decimal degrees, gons, radians and bearings",
                          "surveyAngleCalculator");
    QAction* traverse = make(Icon::SurveyTraverse, "&Traverse...",
                             "Compute and adjust a traverse; add its stations to the drawing",
                             "surveyTraverse");
    QAction* levelBook = make(Icon::SurveyLevelBook, "&Level Book...",
                              "Reduce a level book: reduced levels, misclosure and allowance",
                              "surveyLevelBook");
    QAction* converter =
        make(Icon::SurveyConverter, "&Coordinate Converter...",
             "Convert coordinates between coordinate systems by EPSG code, with grid factors",
             "surveyCoordinateConverter");

    QAction* importPoints =
        make(Icon::SurveyImport, "&Import Survey Points...",
             "Read a file of surveyed points into the drawing, one undoable step",
             "surveyImport");
    QAction* surveyJobs =
        make(Icon::SurveyPointReport, "Survey &Jobs...",
             "The field files imported as survey jobs: their reports, and adjusting them again",
             "surveyJobs");
    QAction* exportPoints = make(Icon::SurveyExport, "&Export Survey Points...",
                                 "Write the drawing's survey points to a delimited text file",
                                 "surveyExport");
    pointManagerAction_ =
        make(Icon::SurveyPointManager, "Point &Manager",
             "The drawing's survey points in a table: filter, sort, select, zoom to",
             "surveyPointManager");
    pointManagerAction_->setCheckable(true);
    // The dock it shows, by object name: how --survey-dock finds it.
    pointManagerAction_->setData(QString("SurveyPointsDock"));
    QAction* pointReport = make(Icon::SurveyPointReport, "Point Re&port...",
                                "The survey points as a report, to copy or save as CSV",
                                "surveyPointReport");

    QObject::connect(importPoints, &QAction::triggered, &window_, [this] {
        open(import_, [this] {
            // No action of the window's: the wizard codes and strings what it
            // imports inside its own command (cad::surveyImportFinish).
            SurveyImportContext context{services_.document, services_.views, services_.log};
            return new SurveyImportWizard(std::move(context), &window_);
        });
    });
    QObject::connect(surveyJobs, &QAction::triggered, &window_, [this] {
        open(jobs_, [this] { return new SurveyJobsDialog(dialogContext(), &window_); });
    });
    QObject::connect(exportPoints, &QAction::triggered, &window_, [this] {
        open(export_, [this] { return new SurveyExportDialog(dialogContext(), &window_); });
    });
    QObject::connect(pointManagerAction_, &QAction::toggled, &window_,
                     [this](bool show) { showPointManager(show); });
    QObject::connect(pointReport, &QAction::triggered, &window_, [this] {
        open(pointReport_,
             [this] { return new SurveyPointReportDialog(dialogContext(), &window_); });
    });
    QObject::connect(inverse, &QAction::triggered, &window_, [this] {
        open(inverse_, [this] { return new SurveyInverseDialog(dialogContext(), &window_); });
    });
    QObject::connect(forward, &QAction::triggered, &window_, [this] {
        open(forward_, [this] { return new SurveyForwardDialog(dialogContext(), &window_); });
    });
    QObject::connect(area, &QAction::triggered, &window_, [this] { areaOfSelection(); });
    QObject::connect(parcel, &QAction::triggered, &window_, [this] {
        open(parcel_, [this] {
            return new SurveyParcelDialog(dialogContext(), services_.run, services_.headless,
                                          &window_);
        });
    });
    QObject::connect(angle, &QAction::triggered, &window_, [this] {
        open(angle_, [this] { return new SurveyAngleDialog(dialogContext(), &window_); });
    });
    QObject::connect(traverse, &QAction::triggered, &window_, [this] {
        open(traverse_, [this] { return new SurveyTraverseDialog(dialogContext(), &window_); });
    });
    QObject::connect(levelBook, &QAction::triggered, &window_, [this] {
        open(levelBook_, [this] { return new SurveyLevelBookDialog(dialogContext(), &window_); });
    });
    QObject::connect(converter, &QAction::triggered, &window_, [this] {
        open(converter_, [this] { return new SurveyConverterDialog(dialogContext(), &window_); });
    });

    // Sections rather than plain separators, as the GIS menu has them: a
    // style that draws their titles says what each group is for.
    menu.addSection("Survey Points");
    menu.addActions({importPoints, surveyJobs, exportPoints, pointManagerAction_, pointReport});
    menu.addSection("Coordinate Geometry");
    menu.addActions({inverse, forward, area, parcel, angle});
    menu.addSection("Traverse and Levelling");
    menu.addActions({traverse, levelBook});
    menu.addSection("Coordinates");
    menu.addAction(converter);
    menu.addSection("Survey Coding");
    if (services_.codeManager != nullptr) {
        menu.addAction(services_.codeManager);
    }
    if (services_.applySurveyCodes != nullptr) {
        menu.addAction(services_.applySurveyCodes);
    }
    if (services_.surveyLinework != nullptr) {
        menu.addAction(services_.surveyLinework);
    }

    toolBar.addActions({importPoints, surveyJobs, exportPoints, pointManagerAction_});
    toolBar.addSeparator();
    toolBar.addActions({inverse, forward, area, parcel, angle});
    toolBar.addSeparator();
    toolBar.addActions({traverse, levelBook});
    toolBar.addSeparator();
    toolBar.addAction(converter);
    if (services_.applySurveyCodes != nullptr) {
        toolBar.addSeparator();
        toolBar.addAction(services_.applySurveyCodes);
    }
}

SurveyWorkbench::~SurveyWorkbench()
{
    for (QPointer<QDialog>* slot : {&inverse_, &forward_, &angle_, &traverse_, &levelBook_,
                                    &converter_, &import_, &jobs_, &export_, &pointReport_,
                                    &parcel_}) {
        delete slot->data();
    }
    // The dock holds a listener on the document too, and goes for the same
    // reason the dialogs do.
    if (!pointManager_.isNull()) {
        // The chrome drops its record of the dock first, as a workspace does
        // before deleting a view's dock (DockChrome::forget).
        if (services_.chrome != nullptr) {
            services_.chrome->forget(pointManager_.data());
        }
        window_.removeDockWidget(pointManager_.data());
        delete pointManager_.data();
    }
}

SurveyDialogContext SurveyWorkbench::dialogContext() const
{
    return SurveyDialogContext{services_.document, services_.views, services_.log};
}

void SurveyWorkbench::open(QPointer<QDialog>& slot, const std::function<QDialog*()>& make)
{
    if (slot.isNull()) {
        slot = make();
    }
    slot->show();
    slot->raise();
    slot->activateWindow();
}

void SurveyWorkbench::showPointManager(bool show)
{
    if (pointManager_.isNull()) {
        if (!show) {
            return;
        }
        pointManager_ = new SurveyPointsDock(*services_.document, services_.views, &window_);
        window_.addDockWidget(Qt::RightDockWidgetArea, pointManager_.data());
        if (services_.chrome != nullptr) {
            services_.chrome->install(pointManager_.data(), Icon::SurveyPointManager,
                                      DockRole::Panel);
        }
        // Wide enough for the id, code and both coordinates without scrolling.
        window_.resizeDocks({pointManager_.data()}, {460}, Qt::Horizontal);
        // Closing the dock by its own button unticks the menu item. Hidden,
        // not "not visible": a dock tabbed behind another is still open.
        // Compared first, so the action and the dock do not call each other
        // round.
        QObject::connect(pointManager_.data(), &QDockWidget::visibilityChanged, &window_,
                         [this](bool) {
                             if (pointManager_.isNull()) {
                                 return;
                             }
                             const bool shown = !pointManager_->isHidden();
                             if (pointManagerAction_->isChecked() != shown) {
                                 pointManagerAction_->setChecked(shown);
                             }
                         });
    }
    pointManager_->setVisible(show);
    if (show) {
        pointManager_->raise();
    }
}

void SurveyWorkbench::areaOfSelection()
{
    const katana::cad::Document& document = *services_.document;
    if (document.selection().empty()) {
        services_.log("Area of Selection: nothing is selected - select closed polylines or circles",
                      true);
        return;
    }
    const auto result = katana::cad::computeArea(document, document.selection().ids());
    if (!result) {
        services_.log("Area of Selection: " + QString::fromStdString(result.error().describe()),
                      true);
        return;
    }
    services_.log(QString::fromStdString(katana::cad::formatAreaReport(*result)), false);
}

} // namespace katana::qt
