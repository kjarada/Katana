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
#include <QFont>

#include <optional>
#include <string_view>

class QApplication;
class QStyle;

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

// ---- text -------------------------------------------------------------------------------
//
// Fonts are tokens too. The command line named "Consolas" and the view's
// prompt and snap tag "Segoe UI", literals that are Windows' fonts and a
// substitute's anywhere else, at sizes that ignored the platform's and any
// person's choice. Now the chrome is the platform's UI face (Segoe UI on
// Windows) at one size a person can change (View > Text Size), and every
// other font is worked out from it, so the command line and the overlays grow
// with the menus.

// How large the window's text is, in steps from the platform's own size.
enum class TextSize { Small, Standard, Large, ExtraLarge };

// "small", "standard", "large", "extra-large": how a setting stores it.
[[nodiscard]] std::string_view toString(TextSize size);
[[nodiscard]] std::optional<TextSize> textSizeFrom(std::string_view text);
// The points a size adds to the platform's: -1, 0, +2, +4. Two points a step
// above Standard, because one (9 to 10 pt) is a difference few people see.
[[nodiscard]] int textSizeSteps(TextSize size);

// The chrome's font: the platform's UI face at the chosen size.
[[nodiscard]] QFont uiFont();
// The command line and the logs: fixed pitch - a column of coordinates reads
// down only in one - at the chrome's size plus `larger` points. The first
// family the machine has of Cascadia Mono, Consolas, DejaVu Sans Mono,
// Liberation Mono and Menlo, else the platform's fixed font.
[[nodiscard]] QFont monospaceFont(double larger = 0.0);
// Text drawn over a view - a tool's prompt, a snap's name - in the chrome's
// face, `pixels` high at Standard and scaled with the chosen size.
[[nodiscard]] QFont overlayFont(int pixels);

// The size chosen, Standard until setTextSize.
[[nodiscard]] TextSize textSize();
// Sets the application font to `size`: every widget without a font of its
// own follows at once; one with its own (the command line) is the owner's to
// set again from monospaceFont().
void setTextSize(QApplication& application, TextSize size);

// Applies the Fusion style, the palette and the stylesheet. Call once, after
// constructing the QApplication and before creating any window.
void apply(QApplication& application);

// The two halves of apply's look, for a test to put on one widget without
// changing the whole application's: the style (Fusion, drawing menu section
// titles and scrolling choice lists; the caller owns it) and the stylesheet.
[[nodiscard]] QStyle* makeStyle();
[[nodiscard]] QString styleSheet();

} // namespace katana::qt::theme
