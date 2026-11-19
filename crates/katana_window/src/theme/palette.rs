//! The application's look, as data: the port of theme.hpp's tokens (C applies them to egui's
//! style, as theme.cpp applies them to Qt's).
//!
//! A CAD viewport is dark - it has been #1e2329 here from the start - and the window around it was
//! whatever grey the toolkit defaults to, which is most of why the application looked unfinished:
//! a dark drawing in a light frame. The theme makes the chrome belong to the viewport.
//!
//! Every colour the application uses for chrome is a NAMED TOKEN below. Nothing else in the window
//! should contain a colour literal for a control; a literal is a colour that the next theme change
//! will miss. (The views keep their own constants for DRAWING colours - grid, snap marker,
//! selection - which are content, not chrome.)
//!
//! Fonts are tokens too: the chrome is the platform's UI face at one size a person can change
//! (View > Text Size), and every other font is worked out from it, so the command line and the
//! overlays grow with the menus. What a font IS (its family, its metrics) is the canvas's and C's;
//! the sizes are worked out here.

use crate::canvas::Color;

// ---- surfaces, darkest to lightest ---------------------------------------------------------------
//
// The design of 2026-11-16 (the owner's "modern dark": low-contrast chrome in the manner of VS Code,
// Figma and Blender, the drawing the brightest thing on screen, one accent). The chrome is neutral
// grey, a step of about 7 in each channel between the frame, the panels, the raised controls and
// the hover, so that a surface reads as above the one under it without a border having to say so;
// borders are one pixel of `border`, a little lighter again. The viewport keeps its own #1e2329 -
// the drawing's ground is content, and the plan painter's contrast figures (feedback_painter) are
// worked against it.

/// #1e2329, the existing drawing background.
pub const fn viewport() -> Color {
    Color::rgb(0x1e, 0x23, 0x29)
}

/// The frame: menu bar, toolbars, status bar, and the gaps between the docks (#1e1f22).
pub const fn window() -> Color {
    Color::rgb(0x1e, 0x1f, 0x22)
}

/// Dock contents, dialogs, lists and trees (#25262a).
pub const fn panel() -> Color {
    Color::rgb(0x25, 0x26, 0x2a)
}

/// Buttons, a checked tool's ground, a menu's open item (#2d2f34).
pub const fn raised() -> Color {
    Color::rgb(0x2d, 0x2f, 0x34)
}

/// Under the pointer (#363940).
pub const fn hover() -> Color {
    Color::rgb(0x36, 0x39, 0x40)
}

/// What a line of text is typed into: a step BELOW the panel it sits in (#1a1b1e), so a field
/// reads as a well and a button as a raised key, and the two are never mistaken.
pub const fn input() -> Color {
    Color::rgb(0x1a, 0x1b, 0x1e)
}

/// One pixel round a panel, a field, a dialog; separators (#3a3c42).
pub const fn border() -> Color {
    Color::rgb(0x3a, 0x3c, 0x42)
}

pub const fn text() -> Color {
    Color::rgb(0xd4, 0xd6, 0xdb)
}

/// Secondary text: labels of a status line, a toolbar's name, a count beside a layer (#9a9ea6).
pub const fn text_muted() -> Color {
    Color::rgb(0x9a, 0x9e, 0xa6)
}

pub const fn text_disabled() -> Color {
    Color::rgb(0x5f, 0x63, 0x6b)
}

/// One accent, used for everything interactive: the checked tool's frame, focus, selection in
/// lists, the primary button, and the bright part of an active General icon (icons::icon_colours;
/// the disciplines' hues keep clear of it). Blue rather than the red of the logo,
/// because in an engineering program red already means "error" and a checked tool must not read
/// as one (#4c8dff).
pub const fn accent() -> Color {
    Color::rgb(0x4c, 0x8d, 0xff)
}

/// The accent under the pointer: a primary button hovered (#6aa1ff).
pub const fn accent_hover() -> Color {
    Color::rgb(0x6a, 0xa1, 0xff)
}

/// Text drawn on the accent: near black. On #4c8dff it is 5.8 : 1, past the WCAG 2.1 (1.4.3)
/// 4.5 : 1 for body text; white there is 3.2 : 1, which fails it. (#0f1419)
pub const fn accent_text() -> Color {
    Color::rgb(0x0f, 0x14, 0x19)
}

/// A selected row of a list or tree, and selected text: the accent mixed a third into the panel
/// (#2f4a7a), light enough to find, dark enough that the row's text stays `text`.
pub const fn selection() -> Color {
    Color::rgb(0x2f, 0x4a, 0x7a)
}

pub const fn error() -> Color {
    Color::rgb(0xff, 0x6b, 0x6b)
}

/// A warning: what was done, with something to look at - the command log's "Warning: ..." lines,
/// where an error's line is `error`. An amber (#ffb84d; OKLCH 0.83 0.145 74; 8.8 : 1 on `panel`
/// by WCAG 2.1's relative luminance, past 1.4.3's 4.5 : 1 for text), 0.19 from the error red in
/// OKLab and 0.16 from the text colour, so the three tones of a log line are told apart; it is
/// text, not an icon's discipline, and the nearest discipline hue, Utilities' yellow, is 0.07
/// from it. Worked out from the sRGB values with Ottosson's OKLab matrices.
pub const fn warning() -> Color {
    Color::rgb(0xff, 0xb8, 0x4d)
}

// ---- the icons' disciplines ------------------------------------------------------------------------
//
// The owner's ask of 2026-11-19: "make the icons colorful based on their discipline and function".
// An icon's bright part - what the command acts on (icons, the module comment) - takes the hue of
// the discipline the command belongs to; the object it acts on stays in the quiet grey. The hues
// were chosen as a SET, in OKLCH (Ottosson's OKLab in polar form, where equal steps of lightness and
// chroma look equal across hues): lightness about 0.76 and chroma about 0.13, so that no discipline
// shouts over another and each sits a little above the muted text (L 0.70) and well below the text
// colour (L 0.88), which keeps the drawing the brightest thing on screen. The hues go round the
// wheel clear of the accent's blue (h 261: the accent means active, chosen, primary), the error red
// (h 23: it means remove), and the create green below. The earth tones are spread by lightness as
// well as hue - Terrain a darker amber, Utilities a lighter yellow - because orange, amber and
// yellow lie 30 degrees apart.
//
// Each figure below is worked out from the sRGB value: the OKLCH by Ottosson's matrices, and the
// contrast against `panel` (#25262a) as WCAG 2.1's relative-luminance ratio. Every one is far past
// WCAG 2.1's 3 : 1 for a graphical object (1.4.11); the smallest is Terrain's 5.8 : 1. Every pair,
// and each against the accent, the error red, the selection and the three text greys, is at least
// 0.08 apart in OKLab (the closest: Survey and the create green, 0.084; Plotting and the text
// colour, 0.086), about four times the 0.02 that is just noticeable. tests/tool_bars/theme.rs
// works the same figures out again from these values.

/// Drawing and modifying geometry: a cool cyan, 33 degrees round from the accent's blue
/// (#3fc0f2; OKLCH 0.76 0.13 228; 7.2 : 1 on the panel).
pub const fn discipline_drafting() -> Color {
    Color::rgb(0x3f, 0xc0, 0xf2)
}

/// Text, labels, dimensions and leaders: violet (#be9df7; OKLCH 0.76 0.13 300; 6.7 : 1).
pub const fn discipline_annotation() -> Color {
    Color::rgb(0xbe, 0x9d, 0xf7)
}

/// Survey: a leaf green, held a little yellow of the create green (#92c268; OKLCH 0.76 0.13 132;
/// 7.3 : 1).
pub const fn discipline_survey() -> Color {
    Color::rgb(0x92, 0xc2, 0x68)
}

/// Terrain, surfaces, volumes and their analysis: an earth amber, a step darker than the rest
/// (#c9983f; OKLCH 0.71 0.12 79; 5.8 : 1).
pub const fn discipline_terrain() -> Color {
    Color::rgb(0xc9, 0x98, 0x3f)
}

/// GIS data and its processing: teal (#43cdba; OKLCH 0.77 0.12 182; 7.7 : 1).
pub const fn discipline_gis() -> Color {
    Color::rgb(0x43, 0xcd, 0xba)
}

/// BIM and roads: templates, corridors, road objects, IFC: orange (#fc9664; OKLCH 0.77 0.14 46;
/// 7.0 : 1).
pub const fn discipline_roads() -> Color {
    Color::rgb(0xfc, 0x96, 0x64)
}

/// Subsurface utilities: the yellow of a locator's paint, a step lighter (#e0d159; OKLCH 0.85
/// 0.14 102; 9.7 : 1).
pub const fn discipline_utilities() -> Color {
    Color::rgb(0xe0, 0xd1, 0x59)
}

/// Plotting and sheets: a paper slate, little chroma, so output reads as the quiet end of the set
/// (#a6c1d9; OKLCH 0.80 0.05 245; 8.1 : 1).
pub const fn discipline_plotting() -> Color {
    Color::rgb(0xa6, 0xc1, 0xd9)
}

/// Format and customisation: styles, symbols, codes, layers: pink (#eb8ec4; OKLCH 0.76 0.13 345;
/// 6.6 : 1).
pub const fn discipline_format() -> Color {
    Color::rgb(0xeb, 0x8e, 0xc4)
}

/// An icon's create mark - the plus of New, of a new layer, of an added vertex - and the tick of a
/// check: a clear green, more chroma than the disciplines so a mark is read as a sign rather than
/// a discipline (#4bde81; OKLCH 0.80 0.18 152; 8.7 : 1). Its opposite, the cross or the struck line
/// of a delete, is `error`.
pub const fn icon_create() -> Color {
    Color::rgb(0x4b, 0xde, 0x81)
}

// ---- text ----------------------------------------------------------------------------------------

/// How large the window's text is, in steps from the platform's own size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextSize {
    Small,
    #[default]
    Standard,
    Large,
    ExtraLarge,
}

/// Every size, in the order View > Text Size lists them.
pub const TEXT_SIZES: [TextSize; 4] = [
    TextSize::Small,
    TextSize::Standard,
    TextSize::Large,
    TextSize::ExtraLarge,
];

/// "small", "standard", "large", "extra-large": how a setting stores it.
pub const fn to_string(size: TextSize) -> &'static str {
    match size {
        TextSize::Small => "small",
        TextSize::Standard => "standard",
        TextSize::Large => "large",
        TextSize::ExtraLarge => "extra-large",
    }
}

/// The size a setting's word names; None for any other word.
pub fn text_size_from(text: &str) -> Option<TextSize> {
    TEXT_SIZES.into_iter().find(|&size| text == to_string(size))
}

/// The points a size adds to the platform's: -1, 0, +2, +4. Two points a step above Standard,
/// because one (9 to 10 pt) is a difference few people see.
pub const fn text_size_steps(size: TextSize) -> i32 {
    match size {
        TextSize::Small => -1,
        TextSize::Standard => 0,
        TextSize::Large => 2,
        TextSize::ExtraLarge => 4,
    }
}

/// The smallest point size any font of the window is given (theme.cpp's `std::max(6.0, ...)`).
pub const LEAST_POINTS: f64 = 6.0;

/// Points of a font given in pixels, at 96 dots an inch - the ratio Qt itself assumes: 72 points
/// an inch over 96 pixels.
pub fn points_of_pixels(pixels: f64) -> f64 {
    pixels * 72.0 / 96.0
}

/// The chrome's size in points (`uiFont`): the platform's UI size `platform_points` moved by the
/// chosen size's steps, never under six points.
pub fn ui_points(platform_points: f64, size: TextSize) -> f64 {
    katana_math::cpp::max(
        LEAST_POINTS,
        platform_points + f64::from(text_size_steps(size)),
    )
}

/// The command line's and the logs' size in points (`monospaceFont`): the chrome's plus `larger`
/// points, never under six.
pub fn monospace_points(platform_points: f64, size: TextSize, larger: f64) -> f64 {
    katana_math::cpp::max(LEAST_POINTS, ui_points(platform_points, size) + larger)
}

/// The fixed-pitch families the command line asks for, in order (`monospaceFont`): the first the
/// machine has, else the platform's own fixed font. The platform's fixed font is Courier New on
/// Windows, wider and lighter than the chrome beside it; these are drawn for screens.
pub const MONOSPACE_FAMILIES: [&str; 5] = [
    "Cascadia Mono",
    "Consolas",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Menlo",
];

/// Text drawn over a view - a tool's prompt, a snap's name - in pixels (`overlayFont`): `pixels`
/// high at Standard, scaled with the chosen size as the chrome's points scale from the platform's,
/// rounded to a whole pixel and never under one.
pub fn overlay_pixels(pixels: i32, platform_points: f64, size: TextSize) -> i32 {
    let scale = ui_points(platform_points, size) / platform_points;
    // std::lround: half away from zero, which f64::round is.
    let scaled = (f64::from(pixels) * scale).round();
    // A pixel size beyond i32 is no font; the C++ narrows the long it rounds to.
    1.max(scaled.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32)
}

/// The property, true or false, that lights a toolbar's overflow arrow in the accent: set on the
/// arrow's button while a tool runs whose button the arrow hides. The arrow reads it from the
/// button it is drawn for.
pub const TOOL_RUNS_BEHIND_OVERFLOW: &str = "toolRunsBehind";
