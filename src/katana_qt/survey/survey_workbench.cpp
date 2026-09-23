#include "survey/survey_workbench.hpp"

#include <utility>

#include <QAction>
#include <QMainWindow>
#include <QMenu>
#include <QToolBar>

namespace katana::qt {

SurveyWorkbench::SurveyWorkbench(QMainWindow& window, SurveyServices services, QMenu& menu,
                                 QToolBar& toolBar)
    : window_(window), services_(std::move(services))
{
    // Sections rather than plain separators, as the GIS menu has them: a
    // style that draws their titles says what each group is for.
    menu.addSection("Survey Coding");
    if (services_.loadCustomisation != nullptr) {
        menu.addAction(services_.loadCustomisation);
    }
    if (services_.applySurveyCodes != nullptr) {
        menu.addAction(services_.applySurveyCodes);
        toolBar.addAction(services_.applySurveyCodes);
    }
}

SurveyWorkbench::~SurveyWorkbench() = default;

} // namespace katana::qt
