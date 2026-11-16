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
//! QMenu::icon:checked         paint_menu_gutter: a checked item's tick in the accent beside its
//!                             icon (drive::action::action_item draws every item with it)
//! OverflowArrowEngine         paint_overflow_arrow and overflow_arrow, on the canvas
//! ```
//!
//! The stylesheet's text itself has no counterpart: egui has no stylesheet, and its rules that
//! set a colour are the Visuals here, each from its token.
//!
//! The UI's face is egui's embedded sans (Ubuntu-Light; the plan's section 4.10), which has no bold
//! face: a section title is drawn a size smaller and muted, in the case it is written in (the
//! design of 2026-11-16: sentence case, no capitals; theme.cpp drew it in spaced capitals). Which
//! face the window carries is the owner's to decide, and changes here only.
//!
//! The chosen text size is per thread, as theme.cpp's was per process: the window draws on one
//! thread, and a test binary's cases on many each keep their own.

use std::cell::Cell;
use std::collections::BTreeMap;

use egui::style::{Selection, WidgetVisuals, Widgets};
use egui::text::LayoutJob;
use egui::{
    Color32, Context, CornerRadius, FontFamily, FontId, Painter, Pos2, Rect, Response, Sense,
    Shadow, Shape, Spacing, Stroke, Style, TextFormat, TextStyle, Theme, Ui, Visuals, pos2, vec2,
};

use super::palette::{
    MONOSPACE_FAMILIES, TextSize, accent, border, error, hover, input, monospace_points,
    overlay_pixels, panel, points_of_pixels, raised, selection, text, text_disabled, text_muted,
    ui_points, window,
};
use crate::canvas::raster::RasterCanvas;
use crate::canvas::{Brush, Canvas, Color, Font, FontSize, Image, IntRect, RectF, RenderHint};

/// The window's standard UI size, in points, that every text size is a step from: 9.75 points,
/// 13 pixels at 96 an inch - the design of 2026-11-16 ("one UI face at 13 px"). It was the 9
/// points of Qt's application font (12 px), which in the UI's light face read small and thin
/// against the panels.
pub const PLATFORM_POINTS: f64 = 9.75;

/// What secondary text (TextStyle::Small: a status line, a count, a hint) and a heading
/// (TextStyle::Heading) take from the UI size, in pixels: 12 and 14 beside the body's 13, the
/// design's scale. One pixel each way is all the hierarchy the UI's one weight can carry without
/// a heading shouting.
pub const SMALL_PIXELS_LESS: f32 = 1.0;
pub const HEADING_PIXELS_MORE: f32 = 1.0;

/// The design's spacing, in points: a control is CONTROL_HEIGHT high; within a group things are
/// GAP apart, groups GROUP_GAP; a panel's contents stand PANEL_PADDING in from its edges, a
/// dialog's DIALOG_PADDING (the scale 2 / 4 / 8 / 12 / 16).
pub const CONTROL_HEIGHT: f32 = 24.0;
pub const GAP: f32 = 4.0;
pub const GROUP_GAP: f32 = 8.0;
pub const PANEL_PADDING: f32 = 8.0;
pub const DIALOG_PADDING: i8 = 12;

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

/// A section title's size against the menu's, and its letter spacing: the secondary text's 12
/// pixels to the body's 13, at the face's own spacing. theme.cpp's sectionFont was 85 % in
/// capitals spaced at 104 %; the design of 2026-11-16 has no capitals, and spacing was only ever
/// there to let capitals breathe.
pub const SECTION_SCALE: f64 = 12.0 / 13.0;
pub const SECTION_LETTER_SPACING: f64 = 100.0;

/// A menu item's icon, in points: Fusion's small icon (PM_SmallIconSize), the design's 16 point
/// grid.
pub const MENU_ICON_SIZE: f32 = 16.0;

/// A menu item's check column, before its icon: room for the tick a checkable item shows while it
/// is on (paint_menu_gutter), the tick's 8 px and 2 either side.
pub const MENU_CHECK_WIDTH: f32 = 12.0;

/// A menu item's gutter before its text, the check column and the icon: every item has it,
/// with a tick or an icon or neither, so the texts of a menu start at one edge.
pub const MENU_GUTTER_WIDTH: f32 = MENU_CHECK_WIDTH + MENU_ICON_SIZE;

/// The corner radius of buttons, inputs and list items (QPushButton, QLineEdit, QMenu::item), and
/// of a dialog or a popup, in points.
pub const CORNER_RADIUS: u8 = 4;
pub const WINDOW_RADIUS: u8 = 6;

/// How far a disabled control is faded towards what it is drawn on. Qt drew disabled text in
/// text_disabled(); egui fades a disabled control as a whole. Faded by this much, text() over
/// window() lands on text_disabled() within a few steps a channel: (0x5f - 0x1e) / (0xd4 - 0x1e)
/// is 0.36 in red, (0x63 - 0x1f) / (0xd6 - 0x1f) 0.37 in green and (0x6b - 0x22) / (0xdb - 0x22)
/// 0.39 in blue.
pub const DISABLED_ALPHA: f32 = 0.37;

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

/// A menu section title's font made from the menu's `base` (theme.cpp's sectionFont): SECTION_SCALE
/// of it, never under six points, bold where the face has a bold, in the case it is written in.
pub fn section_font(base: &Font) -> Font {
    Font {
        size: FontSize::Points(katana_math::cpp::max(6.0, points_of(base) * SECTION_SCALE)),
        all_uppercase: false,
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

/// Every text style at `size`: the whole window one face at the chosen size (setTextSize sets the
/// application font with no class name, which dropped the fonts the platform set for single
/// classes), secondary text a pixel smaller and a heading a pixel larger, the monospace style the
/// command line's. All three move with the size together.
pub fn text_styles(size: TextSize) -> BTreeMap<TextStyle, FontId> {
    let pixels = pixels_of_points(ui_points(PLATFORM_POINTS, size));
    let chrome = FontId::new(pixels, FontFamily::Proportional);
    let fixed = FontId::new(
        pixels_of_points(monospace_points(PLATFORM_POINTS, size, 0.0)),
        FontFamily::Monospace,
    );
    [
        (
            TextStyle::Small,
            FontId::new((pixels - SMALL_PIXELS_LESS).max(1.0), FontFamily::Proportional),
        ),
        (TextStyle::Body, chrome.clone()),
        (TextStyle::Button, chrome),
        (
            TextStyle::Heading,
            FontId::new(pixels + HEADING_PIXELS_MORE, FontFamily::Proportional),
        ),
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
///
/// The design of 2026-11-16: a button is a raised key with a pixel of border, a field a well
/// below its panel, both framed in the accent while focused or pressed; a selected row is the
/// muted accent (`selection`) under the ordinary text, where the full accent under dark text made
/// a list read as a row of buttons; a dialog and a popup are the panel's grey with a border and a
/// soft shadow under them, so a dialog lifts off the window without a frame of its own colour.
pub fn visuals() -> Visuals {
    let c = |color: Color| color.to_egui();
    let edge = Stroke::new(1.0, c(border()));
    // Under the pointer the border lightens a third of the way to the secondary text.
    let lit_edge = Stroke::new(1.0, c(border()).lerp_to_gamma(c(text_muted()), 0.33));
    let focus = Stroke::new(1.0, c(accent()));
    let widget = |fill: Color, frame: Stroke, ink: Color| WidgetVisuals {
        bg_fill: c(fill),
        weak_bg_fill: c(fill),
        bg_stroke: frame,
        corner_radius: CornerRadius::same(CORNER_RADIUS),
        fg_stroke: Stroke::new(1.0, c(ink)),
        expansion: 0.0,
    };
    // A soft shadow straight down: enough to lift a dialog or a menu off the window, never a
    // second outline (egui's own is 10 across and 20 down, a smear on a dark ground).
    let shadow = |blur: u8, down: i8| Shadow {
        offset: [0, down],
        blur,
        spread: 0,
        color: Color32::from_black_alpha(110),
    };
    Visuals {
        dark_mode: true,
        // PlaceholderText and the muted labels.
        weak_text_color: Some(c(text_muted())),
        widgets: Widgets {
            // Frames, separators and the normal text.
            noninteractive: widget(panel(), edge, text()),
            // QPushButton, QComboBox: raised, with a border.
            inactive: widget(raised(), edge, text()),
            hovered: widget(hover(), lit_edge, text()),
            // Pressed is raised; focus is framed in the accent (QLineEdit:focus).
            active: widget(hover(), focus, text()),
            open: widget(raised(), focus, text()),
        },
        // QPalette's Highlight and HighlightedText: lists' selection and selected text.
        selection: Selection {
            bg_fill: c(selection()),
            stroke: Stroke::new(1.0, c(text())),
        },
        hyperlink_color: c(accent()),
        // alternate-background-color: a striped row, a step off the panel.
        faint_bg_color: c(window()).lerp_to_gamma(c(panel()), 0.5),
        // QPalette::Base: lists, trees, scroll areas' troughs.
        extreme_bg_color: c(input()),
        // QLineEdit's background: a well below the panel.
        text_edit_bg_color: Some(c(input())),
        code_bg_color: c(raised()),
        // QPalette::BrightText.
        error_fg_color: c(error()),
        // QDialog's background and a menu's: the panel's grey, a pixel of border round it, its
        // corners rounded as a popup's are, and the shadow under it.
        window_fill: c(panel()),
        window_stroke: edge,
        window_corner_radius: CornerRadius::same(WINDOW_RADIUS),
        window_shadow: shadow(16, 6),
        menu_corner_radius: CornerRadius::same(WINDOW_RADIUS),
        popup_shadow: shadow(10, 4),
        // QMainWindow's: the frame round the docks.
        panel_fill: c(window()),
        disabled_alpha: DISABLED_ALPHA,
        slider_trailing_fill: true,
        ..Visuals::dark()
    }
}

/// The design's spacing (see CONTROL_HEIGHT): egui's defaults made rows of 18 points with 3
/// between them - text crowded against the frame of every field - and dialogs with 6 points of
/// margin; these are the scale's numbers.
fn spacing() -> Spacing {
    Spacing {
        // Between the items of a row or a column: GROUP_GAP across (a label and its field), GAP
        // down (one row of a form and the next).
        item_spacing: vec2(GROUP_GAP, GAP),
        window_margin: egui::Margin::same(DIALOG_PADDING),
        menu_margin: egui::Margin::same(GAP as i8),
        // A button's label 8 in from its sides; down, CONTROL_HEIGHT centres it.
        button_padding: vec2(GROUP_GAP, 3.0),
        // A tree's step in: a checkbox's width and its gap, so a child's box sits under its
        // parent's label.
        indent: 16.0,
        interact_size: vec2(40.0, CONTROL_HEIGHT),
        combo_width: 140.0,
        text_edit_width: 220.0,
        icon_width: 16.0,
        icon_width_inner: 8.0,
        icon_spacing: 6.0,
        menu_spacing: 2.0,
        ..Spacing::default()
    }
}

/// The whole style at `size`: makeStyle and styleSheet in one.
fn style_for(size: TextSize) -> Style {
    let spacing = spacing();
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
/// text): the title alone, no rule - the smaller muted face and the space above already part it
/// from the items before - in the section font, a line of it high with SECTION_PADDING shared
/// above and below, and set below the middle, since a title belongs to the items after it.
pub fn menu_section(ui: &mut Ui, title: &str) -> Response {
    let font = section_font_id();
    let color = text_muted().to_egui();
    let shown = String::from(title);
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
    // The title starts where the items' text does - their button padding, their gutter of tick
    // and icon and the gap after it in from the menu's side (menu_text_indent) - so a section and
    // its items read as one column (theme.cpp's kIndent was the stylesheet's item padding).
    let indent = menu_text_indent(ui);
    let width = galley.size().x + indent + ui.spacing().button_padding.x + 2.0 * INDENT;
    let height = plain.size().y + SECTION_PADDING;
    let (rect, response) = ui.allocate_at_least(vec2(width, height), Sense::hover());
    let area = Rect::from_min_max(
        pos2(rect.left() + indent, rect.top() + SECTION_PADDING / 2.0),
        pos2(rect.right().max(rect.left() + indent), rect.bottom()),
    );
    let at = pos2(area.left(), area.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, color);
    response
}

/// Where a menu item's text starts from the item's left edge: its button padding, its gutter
/// (MENU_GUTTER_WIDTH) and the gap egui leaves between the parts of a button (icon_spacing). A
/// section's title starts there too (menu_section).
pub fn menu_text_indent(ui: &Ui) -> f32 {
    ui.spacing().button_padding.x + MENU_GUTTER_WIDTH + ui.spacing().icon_spacing
}

/// A menu item's gutter, `gutter` MENU_GUTTER_WIDTH wide before its text: a tick in the accent in
/// the check column while a checkable item is on (`checked`), and the item's icon - its name in
/// the window's icon set (crate::icons), drawn through the icon painter - after it; an item with
/// no icon leaves its place empty.
///
/// Until 2026-11-16 a checkable item was drawn as a pill filled with the selection's blue
/// (egui's selectable button): fourteen of them in View > Toolbars read as a column of pressed
/// buttons, and no item showed its icon. A tick beside the icon keeps both: what the item is,
/// and whether it is on. Rejected: the icon framed in the accent (QMenu::icon:checked, the C++
/// stylesheet's), which said "on" only for an item that had an icon, and in a frame the eye takes
/// for hover.
pub fn paint_menu_gutter(painter: &Painter, gutter: Rect, icon: &str, checked: bool) {
    if checked {
        let check = Rect::from_min_size(
            gutter.left_top(),
            vec2(MENU_CHECK_WIDTH, gutter.height()),
        );
        paint_menu_tick(painter, check.center());
    }
    if !icon.is_empty() {
        let place = Rect::from_min_size(
            pos2(
                gutter.left() + MENU_CHECK_WIDTH,
                gutter.center().y - MENU_ICON_SIZE / 2.0,
            ),
            vec2(MENU_ICON_SIZE, MENU_ICON_SIZE),
        );
        crate::icons::show_icon(painter, place, icon, crate::icons::IconMode::Normal);
    }
}

/// The tick of a checked menu item, centred on `centre`: 8 px across and 6 high, in the accent, a
/// stroke of 1.5 - the icons' weight at a menu's size.
fn paint_menu_tick(painter: &Painter, centre: Pos2) {
    painter.add(Shape::line(
        vec![
            pos2(centre.x - 4.0, centre.y),
            pos2(centre.x - 1.5, centre.y + 2.5),
            pos2(centre.x + 4.0, centre.y - 3.5),
        ],
        Stroke::new(1.5, accent().to_egui()),
    ));
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
