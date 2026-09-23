#include "survey/survey_workbench.hpp"

#include <utility>

#include <QAction>
#include <QMainWindow>
#include <QMenu>
#include <QToolBar>

#include "katana/cad/document.hpp"
#include "katana/cad/survey_tools.hpp"
#include "survey/survey_dialogs.hpp"

namespace katana::qt {

SurveyWorkbench::SurveyWorkbench(QMainWindow& window, SurveyServices services, QMenu& menu,
                                 QToolBar& toolBar)
    : window_(window), services_(std::move(services))
{
    const auto make = [this](Icon icon, const QString& text, const QString& tip,
                             const QString& name) {
        return services_.makeAction(icon, text, tip, QKeySequence(), name);
    };
    QAction* inverse = make(Icon::SurveyInverse, "&Inverse...",
                            "Distance, azimuth, bearing and height difference between two points",
                            "surveyInverse");
    QAction* forward = make(Icon::SurveyForward, "&Forward Point...",
                            "A new point from a start, a direction and a distance (radiation)",
                            "surveyForward");
    QAction* area = make(Icon::SurveyArea, "&Area of Selection",
                         "Area and perimeter of the selected closed polylines and circles",
                         "surveyArea");
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

    QObject::connect(inverse, &QAction::triggered, &window_, [this] {
        open(inverse_, [this] { return new SurveyInverseDialog(dialogContext(), &window_); });
    });
    QObject::connect(forward, &QAction::triggered, &window_, [this] {
        open(forward_, [this] { return new SurveyForwardDialog(dialogContext(), &window_); });
    });
    QObject::connect(area, &QAction::triggered, &window_, [this] { areaOfSelection(); });
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
    menu.addSection("Coordinate Geometry");
    menu.addActions({inverse, forward, area, angle});
    menu.addSection("Traverse and Levelling");
    menu.addActions({traverse, levelBook});
    menu.addSection("Coordinates");
    menu.addAction(converter);
    menu.addSection("Survey Coding");
    if (services_.loadCustomisation != nullptr) {
        menu.addAction(services_.loadCustomisation);
    }
    if (services_.applySurveyCodes != nullptr) {
        menu.addAction(services_.applySurveyCodes);
    }

    toolBar.addActions({inverse, forward, area, angle});
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
                                    &converter_}) {
        delete slot->data();
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
