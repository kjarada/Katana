//! The theme applied (theme.cpp): the palette's tokens (theme::palette) put on egui's style, the
//! fonts every part of the window takes from the chosen text size, the menu's section titles and
//! checked icons, the scrolling choice list, and the toolbar's overflow arrow.
//!
//! What theme.cpp did through Qt, and what it is here:
//!
//! ```text
//! the Fusion style            egui's own drawing, the same on every platform (Fusion's reason)
//! the palette and stylesheet  visuals(): one Visuals of the tokens, for the dark and the light
//!                             theme both, so the system's preference never changes the look
//! setTextSize                 set_text_size: the chosen size on this thread, and every text style
//!                             of the context at it - the whole window one font, as before
//! SH_Menu_SupportsSections    menu_section: a titled section draws its title
//! SH_ComboBox_Popup = 0       Spacing::combo_height: a choice list shows MAX_VISIBLE_ITEMS rows
//!                             and scrolls the rest (QComboBox::maxVisibleItems)
//! SH_Menu_Scrollable          shell::menu_column (the foundation's): one scrolling column
//! QMenu::icon:checked         menu_icon_cell: a checked item's icon framed in the accent
//! OverflowArrowEngine         paint_overflow_arrow and overflow_arrow, on the canvas
//! ```
//!
//! The stylesheet's text itself has no counterpart: egui has no stylesheet, and its rules that
//! set a colour are the Visuals here, each from its token.
//!
//! The UI's face is egui's embedded sans (Ubuntu-Light; the plan's section 4.10), which has no bold
//! face: a section title is drawn in capitals, spaced and muted, as theme.cpp draws it, but not
//! bold. Which face the window carries is the owner's to decide, and changes here only.
//!
//! The chosen text size is per thread, as theme.cpp's was per process: the window draws on one
//! thread, and a test binary's cases on many each keep their own.

use std::cell::Cell;
use std::collections::BTreeMap;

use egui::style::{Selection, WidgetVisuals, Widgets};
use egui::text::LayoutJob;
use egui::{
    Color32, Context, CornerRadius, FontFamily, FontId, Painter, Rect, Response, Sense, Shadow,
    Spacing, Stroke, StrokeKind, Style, TextFormat, TextStyle, TextureId, Theme, Ui, Visuals, pos2,
    vec2,
};

use super::palette::{
    MONOSPACE_FAMILIES, TextSize, accent, accent_text, border, error, hover, monospace_points,
    overlay_pixels, panel, points_of_pixels, raised, text, text_disabled, text_muted, ui_points,
    window,
};
use crate::canvas::raster::RasterCanvas;
use crate::canvas::{Brush, Canvas, Color, Font, FontSize, Image, IntRect, RectF, RenderHint};

/// The platform's own UI size, in points, that every text size is a step from: Qt's application
/// font on the platforms the window runs on, 9 points (Segoe UI on Windows, the offscreen
/// platform's Sans Serif headless).
pub const PLATFORM_POINTS: f64 = 9.0;

/// How many rows a choice list shows before it scrolls: QComboBox::maxVisibleItems' default.
pub const MAX_VISIBLE_ITEMS: usize = 10;

/// A plain menu separator's height, and a section title's space above and below its text, in
/// points (theme.cpp's kSeparatorHeight and kSectionPadding).
pub const SEPARATOR_HEIGHT: f32 = 9.0;
pub const SECTION_PADDING: f32 = 10.0;

/// How far a separator's rule and a section's title stand in from the menu's sides: the
/// stylesheet's QMenu::item padding-left, so a title starts where an item's text would without an
/// icon (theme.cpp's kIndent).
pub const INDENT: f32 = 12.0;

/// A section title's size against the menu's, and its letter spacing (theme.cpp's sectionFont:
/// 85 %, and QFont::PercentageSpacing 104).
pub const SECTION_SCALE: f64 = 0.85;
pub const SECTION_LETTER_SPACING: f64 = 104.0;

/// A menu item's icon, in points: Fusion's small icon (PM_SmallIconSize), with the stylesheet's
/// QMenu::icon padding-left before it.
pub const MENU_ICON_SIZE: f32 = 16.0;
pub const MENU_ICON_PADDING: f32 = 6.0;

/// The corner radius of a checked icon's frame (QMenu::icon:checked), and of buttons, inputs and
/// list items (QPushButton, QLineEdit, QMenu::item), in points.
pub const CHECKED_ICON_RADIUS: u8 = 3;
pub const CORNER_RADIUS: u8 = 4;

/// How far a disabled control is faded towards what it is drawn on. Qt drew disabled text in
/// text_disabled(); egui fades a disabled control as a whole. Faded by this much, text() over
/// window() lands on text_disabled() within a few steps a channel: (0x5c - 0x23) / (0xd7 - 0x23)
/// is 0.32 in red, 0.35 in green and 0.37 in blue.
pub const DISABLED_ALPHA: f32 = 0.35;

/// The height of a line of the UI's face (Ubuntu-Light) per pixel of its size, from its hhea
/// table: an ascender of 932, a descender of -189 and a line gap of 28, per 1000 units.
const UI_LINE_HEIGHT_PER_EM: f32 = 1.149;

thread_local! {
    static CHOSEN: Cell<TextSize> = const { Cell::new(TextSize::Standard) };
}

/// The size chosen on this thread, Standard until set_text_size.
pub fn text_size() -> TextSize {
    CHOSEN.with(Cell::get)
}

/// Points as egui's points, which are logical pixels: 96 of them an inch to Qt's 72 points.
pub fn pixels_of_points(points: f64) -> f32 {
    (points * 96.0 / 72.0) as f32
}

/// The points of a canvas font: its points, or its pixels at 96 an inch (theme.cpp's pointsOf).
fn points_of(font: &Font) -> f64 {
    match font.size {
        FontSize::Points(points) => points,
        FontSize::Pixels(pixels) => points_of_pixels(f64::from(pixels)),
    }
}

/// The chrome's font, as the canvas draws it (theme::uiFont): the UI's face at the chosen size.
pub fn ui_font() -> Font {
    Font::sized(FontSize::Points(ui_points(PLATFORM_POINTS, text_size())))
}

/// The command line's and the logs' font, as the canvas draws it (theme::monospaceFont): fixed
/// pitch - a column of coordinates reads down only in one - at the chrome's size plus `larger`
/// points; the first family the machine has of MONOSPACE_FAMILIES, else its fixed-pitch face.
pub fn monospace_font(larger: f64) -> Font {
    Font {
        families: MONOSPACE_FAMILIES
            .iter()
            .map(|&family| String::from(family))
            .collect(),
        size: FontSize::Points(monospace_points(PLATFORM_POINTS, text_size(), larger)),
        fixed_pitch: true,
        ..Font::default()
    }
}

/// Text drawn over a view - a tool's prompt, a snap's name (theme::overlayFont): the chrome's face,
/// `pixels` high at Standard and scaled with the chosen size.
pub fn overlay_font(pixels: i32) -> Font {
    Font {
        size: FontSize::Pixels(overlay_pixels(pixels, PLATFORM_POINTS, text_size())),
        ..ui_font()
    }
}

/// A menu section title's font made from the menu's `base` (theme.cpp's sectionFont): 85 % of it,
/// never under six points, bold, in capitals, spaced at 104 %.
pub fn section_font(base: &Font) -> Font {
    Font {
        size: FontSize::Points(katana_math::cpp::max(6.0, points_of(base) * SECTION_SCALE)),
        all_uppercase: true,
        letter_spacing_percent: SECTION_LETTER_SPACING,
        ..base.clone()
    }
    .bold(true)
}

/// The chrome's font as egui draws it.
pub fn ui_font_id() -> FontId {
    FontId::new(
        pixels_of_points(ui_points(PLATFORM_POINTS, text_size())),
        FontFamily::Proportional,
    )
}

/// The command line's font as egui draws it: egui's fixed-pitch family at the chosen size plus
/// `larger` points (the command line asks for one more than the log).
pub fn monospace_font_id(larger: f64) -> FontId {
    FontId::new(
        pixels_of_points(monospace_points(PLATFORM_POINTS, text_size(), larger)),
        FontFamily::Monospace,
    )
}

/// A menu section title's font as egui draws it (section_font of the chrome's).
pub fn section_font_id() -> FontId {
    FontId::new(
        pixels_of_points(katana_math::cpp::max(
            6.0,
            ui_points(PLATFORM_POINTS, text_size()) * SECTION_SCALE,
        )),
        FontFamily::Proportional,
    )
}

/// Every text style at `size`: the whole window one font at the chosen size (setTextSize sets the
/// application font with no class name, which dropped the fonts the platform set for single
/// classes), the monospace style the command line's.
pub fn text_styles(size: TextSize) -> BTreeMap<TextStyle, FontId> {
    let chrome = FontId::new(
        pixels_of_points(ui_points(PLATFORM_POINTS, size)),
        FontFamily::Proportional,
    );
    let fixed = FontId::new(
        pixels_of_points(monospace_points(PLATFORM_POINTS, size, 0.0)),
        FontFamily::Monospace,
    );
    [
        (TextStyle::Small, chrome.clone()),
        (TextStyle::Body, chrome.clone()),
        (TextStyle::Button, chrome.clone()),
        (TextStyle::Heading, chrome),
        (TextStyle::Monospace, fixed),
    ]
    .into()
}

/// The height a choice list's popup scrolls in: MAX_VISIBLE_ITEMS rows of the list, each a line of
/// text at `font_pixels` with the button padding above and below, at least the interact height,
/// and the spacing to the next.
fn combo_height(spacing: &Spacing, font_pixels: f32) -> f32 {
    let line = (font_pixels * UI_LINE_HEIGHT_PER_EM).ceil();
    let row = (line + 2.0 * spacing.button_padding.y).max(spacing.interact_size.y)
        + spacing.item_spacing.y;
    MAX_VISIBLE_ITEMS as f32 * row
}

/// The palette and the stylesheet as egui's Visuals: every colour from its token.
pub fn visuals() -> Visuals {
    let c = |color: Color| color.to_egui();
    let edge = Stroke::new(1.0, c(border()));
    let focus = Stroke::new(1.0, c(accent()));
    let widget = |fill: Color, frame: Stroke| WidgetVisuals {
        bg_fill: c(fill),
        weak_bg_fill: c(fill),
        bg_stroke: frame,
        corner_radius: CornerRadius::same(CORNER_RADIUS),
        fg_stroke: Stroke::new(1.0, c(text())),
        expansion: 0.0,
    };
    Visuals {
        dark_mode: true,
        // PlaceholderText and the muted labels.
        weak_text_color: Some(c(text_muted())),
        widgets: Widgets {
            // Frames, separators and the normal text.
            noninteractive: widget(panel(), edge),
            // QPushButton, QLineEdit, QComboBox: raised, with a border.
            inactive: widget(raised(), edge),
            hovered: widget(hover(), edge),
            // Pressed is raised; focus is framed in the accent (QLineEdit:focus).
            active: widget(raised(), focus),
            open: widget(raised(), focus),
        },
        // QPalette's Highlight and HighlightedText: lists' selection and selected text.
        selection: Selection {
            bg_fill: c(accent()),
            stroke: Stroke::new(1.0, c(accent_text())),
        },
        hyperlink_color: c(accent()),
        // alternate-background-color.
        faint_bg_color: c(window()),
        // QPalette::Base: lists, trees and text boxes.
        extreme_bg_color: c(panel()),
        // QLineEdit's background.
        text_edit_bg_color: Some(c(raised())),
        code_bg_color: c(raised()),
        // QPalette::BrightText.
        error_fg_color: c(error()),
        // QMainWindow and QDialog's background; a menu or a dialog has no shadow of its own.
        window_fill: c(window()),
        window_stroke: edge,
        window_shadow: Shadow::NONE,
        popup_shadow: Shadow::NONE,
        panel_fill: c(window()),
        disabled_alpha: DISABLED_ALPHA,
        ..Visuals::dark()
    }
}

/// The whole style at `size`: makeStyle and styleSheet in one.
fn style_for(size: TextSize) -> Style {
    let spacing = Spacing::default();
    let combo_height = combo_height(&spacing, pixels_of_points(ui_points(PLATFORM_POINTS, size)));
    Style {
        text_styles: text_styles(size),
        spacing: Spacing {
            combo_height,
            ..spacing
        },
        visuals: visuals(),
        ..Style::default()
    }
}

/// The style at the chosen size, for one Ui a test or a preview puts it on without changing the
/// context's (theme.cpp's makeStyle with its stylesheet).
pub fn style() -> Style {
    style_for(text_size())
}

/// Sets this thread's text size to `size` and every text style of `ctx` to it (setTextSize):
/// everything drawn in a text style follows at the next frame; a control with a font of its own
/// (the command line's, monospace_font_id) is its owner's to set again.
pub fn set_text_size(ctx: &Context, size: TextSize) {
    CHOSEN.with(|chosen| chosen.set(size));
    let styles = text_styles(size);
    let font_pixels = pixels_of_points(ui_points(PLATFORM_POINTS, size));
    ctx.all_styles_mut(|style| {
        style.text_styles = styles.clone();
        style.spacing.combo_height = combo_height(&style.spacing, font_pixels);
    });
}

/// The theme on `ctx`, before any window is drawn (theme::apply): the style for the dark and the
/// light theme alike, the dark one chosen, and the text size Standard.
pub fn apply(ctx: &Context) {
    ctx.set_theme(Theme::Dark);
    let style = style_for(TextSize::Standard);
    ctx.set_style_of(Theme::Dark, style.clone());
    ctx.set_style_of(Theme::Light, style);
    set_text_size(ctx, TextSize::Standard);
}

// ---- menus ---------------------------------------------------------------------------------------

/// A menu's plain separator (KatanaStyle's sizeFromContents and drawControl for an untitled
/// separator): SEPARATOR_HEIGHT high, its rule a pixel of the border colour across the middle row,
/// INDENT in from either side.
pub fn menu_separator(ui: &mut Ui) -> Response {
    let (rect, response) =
        ui.allocate_at_least(vec2(2.0 * INDENT, SEPARATOR_HEIGHT), Sense::hover());
    // QRect::center().y() of the row: its top plus (height - 1) / 2 in whole pixels.
    let y = ui
        .painter()
        .round_to_pixel_center(rect.top() + ((SEPARATOR_HEIGHT - 1.0) / 2.0).floor());
    ui.painter().hline(
        rect.left() + INDENT..=rect.right() - INDENT,
        y,
        Stroke::new(1.0, border().to_egui()),
    );
    response
}

/// A menu's titled section (KatanaStyle's sizeFromContents and drawControl for a separator with
/// text): the title alone, no rule - the capitals and the space above already part it from the
/// items before - in the section font, muted, a line of it high with SECTION_PADDING shared above
/// and below, and set below the middle, since a title belongs to the items after it.
pub fn menu_section(ui: &mut Ui, title: &str) -> Response {
    let font = section_font_id();
    let color = text_muted().to_egui();
    let shown = title.to_uppercase();
    let plain = ui
        .painter()
        .layout_no_wrap(shown.clone(), font.clone(), color);
    // QFont::PercentageSpacing: each letter's advance at 104 % of itself, egui's spacing being
    // one amount after every letter - the mean letter's share.
    let letters = shown.chars().count().max(1) as f32;
    let spacing = plain.size().x * (SECTION_LETTER_SPACING as f32 / 100.0 - 1.0) / letters;
    let galley = ui.painter().layout_job(LayoutJob::single_section(
        shown,
        TextFormat {
            font_id: font,
            extra_letter_spacing: spacing,
            color,
            ..TextFormat::default()
        },
    ));
    let width = galley.size().x + 4.0 * INDENT;
    let height = plain.size().y + SECTION_PADDING;
    let (rect, response) = ui.allocate_at_least(vec2(width, height), Sense::hover());
    // drawText(area.adjusted(kIndent, kSectionPadding / 2, -kIndent, 0), AlignLeft | AlignVCenter).
    let area = Rect::from_min_max(
        pos2(rect.left() + INDENT, rect.top() + SECTION_PADDING / 2.0),
        pos2(rect.right() - INDENT, rect.bottom()),
    );
    let at = pos2(area.left(), area.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, color);
    response
}

/// The frame a checked menu item's icon wears (QMenu::icon:checked): raised, a pixel of the
/// accent round it, corners of CHECKED_ICON_RADIUS - as a checked toolbar button is framed, so a
/// toggle (Grid, Object Snap, Link This View) looks on when it is on.
pub fn paint_checked_icon_frame(painter: &Painter, cell: Rect) {
    painter.rect(
        cell,
        CornerRadius::same(CHECKED_ICON_RADIUS),
        raised().to_egui(),
        Stroke::new(1.0, accent().to_egui()),
        StrokeKind::Inside,
    );
}

/// A menu item's icon cell: MENU_ICON_PADDING then the MENU_ICON_SIZE icon, with the frame's pixel
/// on each side, framed when the item is checked (paint_checked_icon_frame); `icon` is the
/// icon's texture, drawn as it is.
pub fn menu_icon_cell(ui: &mut Ui, icon: Option<TextureId>, checked: bool) -> Response {
    let size = vec2(
        1.0 + MENU_ICON_PADDING + MENU_ICON_SIZE + 1.0,
        1.0 + MENU_ICON_SIZE + 1.0,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if checked {
        paint_checked_icon_frame(ui.painter(), rect);
    }
    if let Some(texture) = icon {
        let place = Rect::from_min_size(
            pos2(rect.left() + 1.0 + MENU_ICON_PADDING, rect.top() + 1.0),
            vec2(MENU_ICON_SIZE, MENU_ICON_SIZE),
        );
        let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        ui.painter().image(texture, place, whole, Color32::WHITE);
    }
    response
}

// ---- the toolbar's overflow arrow ----------------------------------------------------------------

/// QRectF::toAlignedRect: the whole pixels the rectangle touches.
fn aligned(rect: RectF) -> IntRect {
    let left = rect.left().floor() as i32;
    let top = rect.top().floor() as i32;
    let right = rect.right().ceil() as i32;
    let bottom = rect.bottom().ceil() as i32;
    IntRect::new(left, top, right - left, bottom - top)
}

/// A toolbar's overflow arrow (theme.cpp's OverflowArrowEngine::paint): two chevrons, pointing
/// down on a vertical toolbar (`down`; its hidden buttons are below its end) or right on a
/// horizontal one, in the muted text colour - textDisabled for a disabled button, the accent while
/// a tool runs whose button the arrow hides (`lit`).
///
/// Painted in whole device pixels of `rect`, whatever the canvas's transform, without
/// antialiasing: a 45-degree line a pixel wide is a clean staircase at any size, where an
/// antialiased one is a smudge at this one. The geometry is worked from the rows the arrow has
/// along its way - 10 in the overflow button at 100 %, its 12 px less its border: each chevron
/// `half` pixels either side of its apex and `half` + the stroke rows deep, the second right
/// below the first, so the two fill all but a row at each end (at 10 rows: 7 across, 4 deep each,
/// 8 in all). A chevron's arms never touch the other's: a clear pixel is left between them
/// wherever they run side by side. The stroke is a pixel wide per 10 px of that room, two at 200 %.
pub fn paint_overflow_arrow(
    canvas: &mut dyn Canvas,
    rect: IntRect,
    down: bool,
    disabled: bool,
    lit: bool,
) {
    // In the device's pixels, whatever the painter's transform.
    let area = aligned(canvas.device_transform().map_rect(&rect.to_rect_f()));
    let vertical = down;
    let across_room = if vertical { area.width } else { area.height };
    let along_room = if vertical { area.height } else { area.width };
    let stroke = 1.max(along_room / 10);
    let half = ((along_room - 2 * stroke) / 2 - 1).min((across_room - 1) / 2);
    if half < 2 {
        return; // no room for a chevron that reads as one
    }
    let step = half + stroke;
    // Across the arrow and along it: its width, and its depth from the first chevron's top to
    // the second's bottom.
    let across = 2 * half + 1;
    let along = step + half + stroke;
    let centre = (if vertical { area.x } else { area.y }) + (across_room - across) / 2 + half;
    let start = (if vertical { area.y } else { area.x }) + (along_room - along) / 2;

    canvas.save();
    // QTransform::inverted gives the identity for a transform with no inverse.
    let back = canvas
        .device_transform()
        .inverted()
        .unwrap_or(crate::canvas::Transform::IDENTITY);
    let world = canvas.world_transform();
    canvas.set_world_transform(back * world, false);
    canvas.set_render_hint(RenderHint::Antialiasing, false);
    let ink = if disabled {
        text_disabled()
    } else if lit {
        accent()
    } else {
        text_muted()
    };
    let brush = Brush::solid(ink);
    // Pixel (a, b): a across the arrow, b along it.
    let mut dot = |a: i32, b: i32| {
        let (x, y) = if vertical { (a, b) } else { (b, a) };
        canvas.fill_rect(RectF::new(f64::from(x), f64::from(y), 1.0, 1.0), &brush);
    };
    for top in [start, start + step] {
        for row in 0..=half {
            for thick in 0..stroke {
                dot(centre - half + row, top + row + thick);
                dot(centre + half - row, top + row + thick);
            }
        }
    }
    canvas.restore();
}

/// The overflow arrow as an image `width` by `height` device pixels, transparent around it
/// (OverflowArrowEngine::pixmap), for the toolbar to show.
pub fn overflow_arrow(width: u32, height: u32, down: bool, disabled: bool, lit: bool) -> Image {
    let mut canvas = RasterCanvas::new(width, height);
    let room = IntRect::new(
        0,
        0,
        i32::try_from(width).unwrap_or(i32::MAX),
        i32::try_from(height).unwrap_or(i32::MAX),
    );
    paint_overflow_arrow(&mut canvas, room, down, disabled, lit);
    canvas.into_image()
}
