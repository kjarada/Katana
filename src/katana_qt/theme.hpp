#pragma once

// The application's look, in one place.
//
// A CAD viewport is dark - it has been #1e2329 here from the start - and the
// window around it was whatever grey Qt defaults to, which is most of why the
// application looked unfinished: a dark drawing in a light frame. The theme
// makes the chrome belong to the viewport.
//
// Every colour the application uses for chrome is a NAMED TOKEN below. Nothing
// else in src/katana_qt should contain a colour literal for a widget; a
// literal is a colour that the next theme change will miss. (The viewports
// keep their own constants for DRAWING colours - grid, snap marker, selection
// - which are content, not chrome.)

#include <QColor>

class QApplication;

namespace katana::qt::theme {

// Surfaces, darkest to lightest. The viewport sits below all of them so that
// the drawing is the deepest thing on screen and the panels float above it.
[[nodiscard]] QColor viewport(); // #1e2329, the existing drawing background
[[nodiscard]] QColor window();   // the frame: menu bar, toolbars, status bar
[[nodiscard]] QColor panel();    // dock contents, lists, trees
[[nodiscard]] QColor raised();   // inputs, buttons
[[nodiscard]] QColor hover();
[[nodiscard]] QColor border();

[[nodiscard]] QColor text();
[[nodiscard]] QColor textMuted();
[[nodiscard]] QColor textDisabled();

// One accent, used for everything interactive: the checked tool, focus,
// selection in lists, and the accent stroke of every icon. Blue rather than
// the red of the logo, because in an engineering program red already means
// "error" and a checked tool must not read as one.
[[nodiscard]] QColor accent();
[[nodiscard]] QColor accentText(); // text drawn on the accent
[[nodiscard]] QColor error();

// Applies the Fusion style, the palette and the stylesheet. Call once, after
// constructing the QApplication and before creating any window.
void apply(QApplication& application);

} // namespace katana::qt::theme
