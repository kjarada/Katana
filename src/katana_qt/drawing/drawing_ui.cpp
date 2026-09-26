#include "drawing/drawing_ui.hpp"

#include <functional>
#include <memory>
#include <vector>

#include <QAction>
#include <QDockWidget>
#include <QKeySequence>
#include <QMainWindow>
#include <QMenu>
#include <QToolBar>

#include "drawing/vertex_panel.hpp"
#include "tools/tool_icons.hpp"

namespace katana::qt::drawing {

namespace cad = katana::cad;

void DrawingUi::showVertices() const
{
    if (verticesDock != nullptr) {
        verticesDock->show();
        verticesDock->raise();
    }
}

namespace {

// A checkable action bound to one flag of the drafting settings: it sets the
// flag when toggled, and reads it back whenever the document changes.
QAction* bound(QToolBar& bar, cad::Document& document, const QString& text, const QString& name,
               const QString& tip, std::function<bool&(cad::DraftingSettings&)> flag,
               std::vector<std::function<void()>>& readers)
{
    auto* action = bar.addAction(text);
    action->setObjectName(name);
    action->setCheckable(true);
    action->setToolTip(tip);
    action->setStatusTip(tip);
    action->setChecked(flag(document.drafting()));
    QObject::connect(action, &QAction::toggled, &bar, [&document, flag](bool on) {
        if (flag(document.drafting()) != on) {
            flag(document.drafting()) = on;
            document.notifyDraftingChanged();
        }
    });
    readers.push_back([action, &document, flag] {
        const bool on = flag(document.drafting());
        if (action->isChecked() != on) {
            action->setChecked(on);
        }
    });
    return action;
}

} // namespace

DrawingUi installDrawingUi(QMainWindow& window, cad::Document& document, QMenu* panels,
                           QMenu* drawMenu)
{
    DrawingUi ui;
    ui.verticesDock = new QDockWidget("Vertices", &window);
    ui.verticesDock->setObjectName("VerticesDock");
    ui.vertexPanel = new VertexPanel(document, ui.verticesDock);
    ui.verticesDock->setWidget(ui.vertexPanel);
    window.addDockWidget(Qt::RightDockWidgetArea, ui.verticesDock);
    ui.verticesDock->hide(); // until asked for: Edit Vertices or its toggle
    QAction* toggle = ui.verticesDock->toggleViewAction();
    toggle->setObjectName("actionVerticesPanel");
    toggle->setText("&Vertices");
    // Edit Vertices' picture: that tool opens this panel.
    toggle->setIcon(tools::toolIcon("draw.vertex.edit"));
    toggle->setStatusTip("Show the selected polyline's vertices in a table to edit in place");
    if (panels != nullptr) {
        panels->addAction(toggle);
    }
    // Not added to Draw > Vertices: that menu's letters are chosen when the
    // catalogue fills it, and Edit Vertices opens the panel from there.
    (void)drawMenu;

    ui.draftingBar = new QToolBar("Drafting", &window);
    ui.draftingBar->setObjectName("DraftingToolbar");
    window.addToolBar(Qt::BottomToolBarArea, ui.draftingBar);
    auto readers = std::make_shared<std::vector<std::function<void()>>>();
    QAction* ortho = bound(*ui.draftingBar, document, "Ortho", "actionOrtho",
                           "Ortho: points from a base go straight across or straight up (F8)",
                           [](cad::DraftingSettings& s) -> bool& { return s.ortho; }, *readers);
    ortho->setShortcut(QKeySequence(Qt::Key_F8));
    QAction* polar = bound(*ui.draftingBar, document, "Polar", "actionPolar",
                           "Polar tracking: a direction near a multiple of 15 degrees snaps to "
                           "it (F10)",
                           [](cad::DraftingSettings& s) -> bool& { return s.polar; }, *readers);
    polar->setShortcut(QKeySequence(Qt::Key_F10));
    QAction* tracking = bound(*ui.draftingBar, document, "Tracking", "actionObjectTracking",
                              "Object snap tracking: paths through points the cursor has "
                              "rested on (F11)",
                              [](cad::DraftingSettings& s) -> bool& { return s.objectTracking; },
                              *readers);
    tracking->setShortcut(QKeySequence(Qt::Key_F11));
    auto* bearings = ui.draftingBar->addAction("Bearings");
    bearings->setObjectName("actionBearings");
    bearings->setCheckable(true);
    bearings->setToolTip("Angles typed after < are whole-circle bearings, clockwise from north, "
                         "instead of degrees counter-clockwise from east");
    bearings->setChecked(document.drafting().angles == cad::AngleConvention::Bearing);
    QObject::connect(bearings, &QAction::toggled, ui.draftingBar, [&document](bool on) {
        const auto angles = on ? cad::AngleConvention::Bearing : cad::AngleConvention::Counterclockwise;
        if (document.drafting().angles != angles) {
            document.drafting().angles = angles;
            document.notifyDraftingChanged();
        }
    });
    readers->push_back([bearings, &document] {
        const bool on = document.drafting().angles == cad::AngleConvention::Bearing;
        if (bearings->isChecked() != on) {
            bearings->setChecked(on);
        }
    });
    ui.draftingBar->addSeparator();
    struct NewSnap {
        cad::SnapMode mode;
        const char* name;
    };
    for (const NewSnap snap : {NewSnap{cad::SnapMode::Quadrant, "actionSnapQuadrant"},
                               NewSnap{cad::SnapMode::Node, "actionSnapNode"},
                               NewSnap{cad::SnapMode::Extension, "actionSnapExtension"},
                               NewSnap{cad::SnapMode::Parallel, "actionSnapParallel"},
                               NewSnap{cad::SnapMode::ApparentIntersection,
                                       "actionSnapApparentIntersection"}}) {
        auto* action = ui.draftingBar->addAction(QString::fromUtf8(cad::toString(snap.mode)));
        action->setObjectName(snap.name);
        action->setCheckable(true);
        action->setToolTip(QString("Object snap: %1").arg(cad::toString(snap.mode)));
        const auto bit = static_cast<cad::SnapModes>(snap.mode);
        action->setChecked((document.drafting().snapModes & bit) != 0);
        QObject::connect(action, &QAction::toggled, ui.draftingBar, [&document, bit](bool on) {
            auto& modes = document.drafting().snapModes;
            const auto next = on ? (modes | bit) : (modes & ~bit);
            if (next == modes) {
                return;
            }
            // Told, so View > Snap Modes and the views follow (one owner:
            // the document's drafting settings).
            modes = next;
            document.notifyDraftingChanged();
        });
        readers->push_back([action, &document, bit] {
            const bool on = (document.drafting().snapModes & bit) != 0;
            if (action->isChecked() != on) {
                action->setChecked(on);
            }
        });
    }
    // Kept alive by the listener, which the toolbar owns through its handle.
    auto handle = std::make_shared<cad::Document::ListenerHandle>(
        document.addListener([readers] {
            for (const auto& read : *readers) {
                read();
            }
        }));
    QObject::connect(ui.draftingBar, &QObject::destroyed, [handle]() mutable { handle.reset(); });
    return ui;
}

} // namespace katana::qt::drawing
