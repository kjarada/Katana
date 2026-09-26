#pragma once

// The drawing system's part of the window (docs/drawing.md): the Vertices
// dock and the drafting aids' toggles, made in one call so that the window
// - which other work is changing at the same time - gains a few lines, not
// a section.
//
//   Vertices dock      "VerticesDock", holding the Vertices panel
//                      ("vertexPanel"); its toggle is in View > Panels
//                      ("actionVerticesPanel"), and Draw > Vertices > Edit
//                      Vertices opens it
//   Drafting toolbar   "DraftingToolbar": Ortho (F8, "actionOrtho"), Polar
//                      tracking (F10, "actionPolar"), Object snap tracking
//                      (F11, "actionObjectTracking"), angles as bearings
//                      ("actionBearings"), and the snaps the drawing system
//                      added ("actionSnapQuadrant", "actionSnapNode",
//                      "actionSnapExtension", "actionSnapParallel",
//                      "actionSnapApparentIntersection"), each bound to the
//                      document's drafting settings and re-read whenever the
//                      document changes, so a verb that turns ortho on checks
//                      its button.

#include <QString>

#include "katana/cad/document.hpp"

class QDockWidget;
class QMainWindow;
class QMenu;
class QToolBar;

namespace katana::qt::drawing {

class VertexPanel;

struct DrawingUi {
    QDockWidget* verticesDock = nullptr;
    VertexPanel* vertexPanel = nullptr;
    QToolBar* draftingBar = nullptr;
    // Shows and raises the Vertices dock (Edit Vertices does this).
    void showVertices() const;
};

// `panels` is View > Panels (may be null); `drawMenu` the Draw menu (unused
// for now: its letters are the catalogue's to choose).
DrawingUi installDrawingUi(QMainWindow& window, katana::cad::Document& document, QMenu* panels,
                           QMenu* drawMenu);

} // namespace katana::qt::drawing
