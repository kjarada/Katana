//! The toolbar button of a tool family, the window's other tool buttons, and the check that holds
//! the menu signs of every toolbar and dock title bar off their icons.
//!
//! A family ("Circle": Centre Radius, 2 Points, 3 Points ...) is one button on its toolbar
//! (tool_menus). A click runs the family's first tool. The others are one gesture away: a press on
//! the small triangle in the button's bottom-right corner, a press held, or a right click. The
//! triangle is the sign that the button holds more than one tool, drawn in the corner the centred
//! icon leaves free, as drawing and graphics programs mark their tool groups. The button is framed
//! as running only while the tool its icon shows runs, as every tool button is; while any of the
//! family's tools runs - that one or one chosen from the menu - the triangle takes the accent, and
//! the tooltip names the one that runs.
//!
//! It replaced Qt's split button, whose arrow the stylesheet drew OVER the icon (docs/desktop.md,
//! "Tool families on the toolbars"). Undo and Redo keep a split button, padded so its strip is
//! its own (theme.cpp's QToolButton[popupMode="MenuButtonPopup"] rule).
//!
//! THE GEOMETRY IS THE C++'s, in the widget's pixels (a point here, a logical pixel there): the
//! theme's 1 px border and 4 px padding, Qt's 3 px, the 5 px mark inset 2 px, the 12 px press
//! zone, the 12 px strip, the 12 px overflow button. What is drawn is drawn in whole DEVICE pixels
//! where the C++ draws so - the mark a device row at a time, the overflow arrow a device pixel at
//! a time, the icon a pixmap of its size times the ratio placed on a whole device pixel - so a
//! toolbar at 125 % is the C++'s pixel for pixel where its tests and the headless check pin it
//! (`sign x 33..38 ... clear of its icon x 8..32 (device pixels at a ratio of 1.25)`).
//!
//! ONE PAINTER. Every tool button - a family, an action's own button, a split button, the
//! overflow button, and the title bar buttons D draws - is painted by `paint_tool_button` from a
//! `ButtonPaint`, through the canvas: on the screen as an image kept in the icon cache (icons),
//! and by `drawn_parts` for the check, which paints the button against itself as the C++ grabs it.
//! The C++ split this between Qt's style, the stylesheet and FlyoutButton::paintEvent; one painter
//! here is what makes the check measure what the screen shows.
//!
//! What the C++ needed Qt's machinery for has no Rust: followToolBarIconSize (a button drawn each
//! frame is drawn at its bar's icon size), the event filters that repainted a family when one of
//! its tools started (every frame is painted from the actions as they are), and
//! lightOverflowWhileAHiddenToolRuns's bookkeeping (the arrow's light is worked out each frame
//! from what is hidden and running then - `overflow_lit`).

use std::time::Duration;

use egui::{Color32, Pos2, Rect, Response, Sense, Ui, Vec2};
use katana_core::text::format_general;

use crate::canvas::{
    Brush, Canvas, Color, FillRule, Image, IntRect, Path, Pen, PointF, RectF, RenderHint,
};
use crate::drive::action::{Action, Menu, MenuItem, action_item};
use crate::drive::controls::{accept_fill, accept_press};
use crate::drive::mailbox::{Act, no_field};
use crate::drive::program::MenuSigns;
use crate::drive::registry::{
    ControlKind, ControlRecord, TargetKind, TargetSpec, begin_target, end_target, record, settle,
    take,
};
use crate::icons::{
    IconMode, Rasteriser, device_pixels, device_rect, paint_named_icon, painted_texture,
};
use crate::shell::dialog::popup_menu;
use crate::theme::palette;

// ---- the theme's tool buttons (theme.cpp's stylesheet), in the widget's pixels -----------------

/// Qt's own addition to an icon-only tool button under a stylesheet (QStyleSheetStyle,
/// CT_ToolButton): with the padding and border, a button is its icon and 3 px more than them.
pub const QT_TOOL_BUTTON_EXTRA: i32 = 3;
/// The mark's two short sides, at most and at least. Five is what the theme's buttons leave
/// between the icon's square and the frame at every toolbar icon size; three is the smallest
/// triangle that still reads as one.
pub const MAX_LEG: i32 = 5;
pub const MIN_LEG: i32 = 3;
/// From the button's edge to the mark: the theme's 1 px border and a pixel of air, so that the
/// mark, the accent while a tool of the family runs, never runs into the frame the running tool is
/// drawn in, the accent too. Only the mark's right-angled tip reaches the border's 5 px rounded
/// corner. On a scaled screen the inset is taken down to whole device pixels, and the mark ends at
/// least a whole device pixel before the first device column and row the border touches.
pub const INSET: i32 = 2;
/// The theme's border round every tool button (theme.cpp, QToolButton).
pub const BORDER: i32 = 1;
/// The corner a press opens the menu from, 12 px each way: the width of the drop-down strip the
/// mark replaced (a stylesheet's default ::menu-button is QCommonStyle's 12 px
/// PM_MenuButtonIndicator). Less than the strip's area all the same - the icon keeps its whole
/// square, so at 20 px icons the zone is an L of 119 square pixels against the strip's
/// 12 x 33 = 396 - and the press held and the right click, anywhere on the button, make up for it.
pub const ZONE: i32 = 12;
/// A family names this many of its other tools in its tooltip, or one fewer and how many more:
/// the Vertices family's seventeen others, all named, were a wall of six lines under the tool's
/// own two, and the menu lists them. Five, so Circle's five are named rather than three "and 2
/// more", which was hardly shorter.
pub const NAMED_IN_TOOLTIP: usize = 5;
/// How long a press is held before a family's menu opens: QCommonStyle's SH_ToolButton_PopupDelay,
/// 600 ms, Qt's DelayedPopup.
pub const POPUP_DELAY_SECONDS: f64 = 0.6;
/// A split button's strip (theme.cpp's QToolButton::menu-button width), laid in the button's
/// border rectangle at its right.
pub const STRIP: i32 = 12;
/// A menu indicator's box (QStyleSheetStyle's defaultSize for a ::menu-indicator): a title bar
/// button whose menu opens on a click sets its arrow in it, at the right of its padding box.
pub const INDICATOR: i32 = 13;
/// The overflow button's depth (QCommonStyle's PM_ToolBarExtensionExtent).
pub const EXTENSION_EXTENT: i32 = 12;
/// The overflow button's object name, Qt's own (QToolBarExtension), which the C++'s tests and
/// theme name it by.
pub const OVERFLOW_BUTTON: &str = "qt_toolbar_ext_button";

/// A tool button's look: the border, the padding each side, the corners' radius and the shade
/// under the pointer, as theme.cpp's stylesheet gives them. The background is otherwise the
/// rules every tool button shares: transparent, raised while pressed or checked, the accent border
/// while checked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolButtonLook {
    pub border: i32,
    pub padding_left: i32,
    pub padding_top: i32,
    pub padding_right: i32,
    pub padding_bottom: i32,
    pub radius: f64,
    /// The background under the pointer (QToolButton:hover's; the close button's red).
    pub hover: Color,
}

impl ToolButtonLook {
    /// QToolButton { border: 1px solid transparent; border-radius: 5px; padding: 4px; } with its
    /// :hover shade.
    pub const TOOL: Self = Self {
        border: BORDER,
        padding_left: 4,
        padding_top: 4,
        padding_right: 4,
        padding_bottom: 4,
        radius: 5.0,
        hover: palette::hover(),
    };
    /// QToolButton[popupMode="MenuButtonPopup"] { padding-right: 13px; }: Undo and Redo, the
    /// strip a pixel inside the padding (Qt's own recipe, "Customizing QToolButton").
    pub const SPLIT: Self = Self {
        padding_right: 13,
        ..Self::TOOL
    };
    /// QToolButton#qt_toolbar_ext_button { padding: 0px; }: the border kept, so an expanded
    /// toolbar's overflow button has the outline every checked tool button has.
    pub const OVERFLOW: Self = Self {
        padding_left: 0,
        padding_top: 0,
        padding_right: 0,
        padding_bottom: 0,
        ..Self::TOOL
    };
    /// QToolButton[chrome="button"] { padding: 2px; border: 1px solid transparent;
    /// border-radius: 4px; }: a dock's and a view's title bar buttons.
    pub const TITLE_BAR: Self = Self {
        border: BORDER,
        padding_left: 2,
        padding_top: 2,
        padding_right: 2,
        padding_bottom: 2,
        radius: 4.0,
        hover: palette::hover(),
    };
    /// QToolButton[chrome="button"][popupMode="InstantPopup"] { padding-right: 13px; }: a title
    /// bar button whose menu opens on a click (a view's kind switcher), padded by its arrow's box.
    pub const TITLE_BAR_MENU: Self = Self {
        padding_right: INDICATOR,
        ..Self::TITLE_BAR
    };
    /// QToolButton[chrome="close"]: as the others, its hover the error red.
    pub const TITLE_BAR_CLOSE: Self = Self {
        hover: palette::error(),
        ..Self::TITLE_BAR
    };

    /// The size of an icon-only button with `icon`-pixel icons (QToolButton::sizeHint under the
    /// stylesheet): the icon and Qt's 3 px, the padding and the border each side.
    pub fn button_size(&self, icon: i32) -> (i32, i32) {
        (
            icon + QT_TOOL_BUTTON_EXTRA + self.padding_left + self.padding_right + 2 * self.border,
            icon + QT_TOOL_BUTTON_EXTRA + self.padding_top + self.padding_bottom + 2 * self.border,
        )
    }

    /// The padding box of a button of `width` by `height`: inside its border.
    pub fn padding_box(&self, width: i32, height: i32) -> IntRect {
        IntRect::new(
            self.border,
            self.border,
            width - 2 * self.border,
            height - 2 * self.border,
        )
    }

    /// The contents of a button of `width` by `height`: inside its border and padding.
    pub fn contents(&self, width: i32, height: i32) -> IntRect {
        IntRect::new(
            self.border + self.padding_left,
            self.border + self.padding_top,
            width - 2 * self.border - self.padding_left - self.padding_right,
            height - 2 * self.border - self.padding_top - self.padding_bottom,
        )
    }
}

/// QStyle::alignedRect centred: `width` by `height` in the middle of `rect`, each half taken down
/// before it is subtracted (`w / 2 - size / 2`).
pub fn centred(width: i32, height: i32, rect: IntRect) -> IntRect {
    IntRect::new(
        rect.x + rect.width / 2 - width / 2,
        rect.y + rect.height / 2 - height / 2,
        width,
        height,
    )
}

/// Where a button of `width` by `height` with `icon`-pixel icons draws its icon's square: centred
/// in its contents, which the theme insets alike on every side for a family (FlyoutButton::
/// iconRect, the icon's size centred in the button) and on the left for a split button.
pub fn icon_rect(look: &ToolButtonLook, width: i32, height: i32, icon: i32) -> IntRect {
    let contents = look.contents(width, height);
    // The size the style is handed is the toolbar's icon size, bounded by the button
    // (option.iconSize.boundedTo(size())).
    centred(icon.min(width), icon.min(height), contents)
}

/// The corner mark's square: in the bottom-right corner `icon` leaves free, inside the button's
/// frame, as large as the corner allows up to MAX_LEG. A style that left less than MIN_LEG would
/// get the mark over the icon's corner; the theme leaves MAX_LEG (tested at every toolbar icon
/// size).
pub fn mark_rect(width: i32, height: i32, icon: IntRect) -> IntRect {
    let right = (width - 1) - INSET;
    let bottom = (height - 1) - INSET;
    let room = (right - icon.right()).min(bottom - icon.bottom());
    let leg = room.clamp(MIN_LEG, MAX_LEG);
    IntRect::new(right - leg + 1, bottom - leg + 1, leg, leg)
}

/// Where a left press opens the menu rather than running the tool: the bottom-right corner,
/// ZONE pixels each way as the strip it replaces was wide, less the icon's square, so a press on
/// the icon always runs the tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuZone {
    pub corner: IntRect,
    pub icon: IntRect,
}

impl MenuZone {
    pub fn new(width: i32, height: i32, icon: IntRect) -> Self {
        let corner = IntRect::new(width - ZONE, height - ZONE, ZONE, ZONE);
        let left = corner.x.max(0);
        let top = corner.y.max(0);
        let clipped = IntRect::new(
            left,
            top,
            corner.right().min(width - 1) - left + 1,
            corner.bottom().min(height - 1) - top + 1,
        );
        Self {
            corner: clipped,
            icon,
        }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.corner.contains(x, y) && !self.icon.contains(x, y)
    }

    /// Whether every pixel of `rect` is in the zone.
    pub fn contains_rect(&self, rect: IntRect) -> bool {
        (rect.y..=rect.bottom()).all(|y| (rect.x..=rect.right()).all(|x| self.contains(x, y)))
    }

    /// Whether any pixel of `rect` is in the zone.
    pub fn intersects(&self, rect: IntRect) -> bool {
        (rect.y..=rect.bottom()).any(|y| (rect.x..=rect.right()).any(|x| self.contains(x, y)))
    }
}

/// The colour of a family's mark (FlyoutButton::paintEvent): quiet at rest, brighter under the
/// pointer, the accent with the pointer on it - a press there opens the family - and while any
/// tool of the family runs, framed or not (the checked frame is the accent too), and disabled
/// with the button. So with Delete Vertex running the Vertices button is unframed with its
/// triangle lit: one of the tools behind the triangle runs, and the tooltip names it.
pub fn mark_ink(state: &ButtonState, family_running: bool, pointer_on_mark: bool) -> Color {
    if state.disabled {
        palette::text_disabled()
    } else if family_running || pointer_on_mark {
        palette::accent()
    } else if state.hovered || state.down {
        palette::text()
    } else {
        palette::text_muted()
    }
}

// ---- the one painter ---------------------------------------------------------------------------

/// The state a tool button is drawn in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ButtonState {
    /// The pointer over it (QToolButton:hover).
    pub hovered: bool,
    /// Pressed (QToolButton:pressed).
    pub down: bool,
    /// Its action checked (QToolButton:checked): the frame that says its tool runs.
    pub checked: bool,
    pub disabled: bool,
}

/// What a button's icon is drawn as.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ButtonIcon {
    #[default]
    None,
    /// An icon of the window's set, by name (icons).
    Named(String),
    /// drawnParts's opaque square: where the icon's square is drawn.
    Solid,
    /// drawnParts's clear one.
    Clear,
    /// A toolbar's overflow arrow (KatanaStyle's overflow icon): two chevrons pointing down on a
    /// vertical toolbar or right on a horizontal one, in `ink`.
    Overflow { down: bool, ink: Color },
}

/// The sign of a button's menu.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum MenuSign {
    #[default]
    None,
    /// A tool family's corner mark (FlyoutButton), in its ink.
    Mark(Color),
    /// A split button's arrow (Fusion's), in its strip: the border rectangle's right STRIP pixels.
    Strip(Color),
    /// A menu indicator's arrow (Fusion's) in its INDICATOR box at the right of the padding box,
    /// centred down it: a title bar button whose menu opens on a click.
    Indicator(Color),
}

/// Everything a tool button is painted from: its size in the widget's pixels, its look, its icon
/// and the icon's size, its state and the sign of its menu, over a ground (transparent on the
/// screen, where the bar's own background is under it).
#[derive(Clone, Debug, PartialEq)]
pub struct ButtonPaint {
    pub width: i32,
    pub height: i32,
    pub look: ToolButtonLook,
    pub icon: ButtonIcon,
    pub icon_size: i32,
    pub state: ButtonState,
    pub sign: MenuSign,
    pub ground: Color,
}

impl ButtonPaint {
    /// The square the icon is drawn in, in the widget's pixels.
    pub fn icon_rect(&self) -> IntRect {
        icon_rect(&self.look, self.width, self.height, self.icon_size)
    }

    /// The image of the button at `ratio`: as large as a grab of it (its size times the ratio,
    /// rounded as QSize rounds), painted by `raster`.
    pub fn render(&self, raster: &dyn Rasteriser, ratio: f64) -> Image {
        let width = device_pixels(f64::from(self.width) * ratio);
        let height = device_pixels(f64::from(self.height) * ratio);
        raster.paint(width, height, ratio, &mut |canvas: &mut dyn Canvas| {
            paint_tool_button(canvas, self)
        })
    }
}

/// qRound for the coordinates here: halves up.
fn qt_round(value: f64) -> i32 {
    (value + 0.5).floor() as i32
}

/// Whole device pixels in `widget_pixels` at `ratio`, taken down; the slack keeps a ratio a hair
/// under 2 from losing a pixel of ten.
pub fn whole_device_pixels(widget_pixels: i32, ratio: f64) -> i32 {
    (f64::from(widget_pixels) * ratio + 1e-6).floor() as i32
}

/// Sets the canvas's world so that its units are device pixels: the inverse of what maps the
/// widget's pixels to the device, before the world transform (the C++'s
/// `setWorldTransform(deviceTransform().inverted() * worldTransform())`).
fn to_device_pixels(canvas: &mut dyn Canvas) {
    let device = canvas.device_transform();
    let world = canvas.world_transform();
    if let Some(inverse) = device.inverted() {
        canvas.set_world_transform(inverse * world, false);
    }
}

/// Paints the tool button `paint` describes at the canvas's origin, in the widget's pixels.
pub fn paint_tool_button(canvas: &mut dyn Canvas, paint: &ButtonPaint) {
    let width = f64::from(paint.width);
    let height = f64::from(paint.height);
    let outer = RectF::new(0.0, 0.0, width, height);
    let look = &paint.look;
    let state = &paint.state;
    canvas.save();
    if paint.ground.a != 0 {
        canvas.fill_rect(outer, &Brush::solid(paint.ground));
    }
    canvas.set_render_hint(RenderHint::Antialiasing, true);
    // The stylesheet's rules in their order: :hover, then :pressed, then :checked, the later
    // winning where both hold.
    let background = if state.checked || state.down {
        Some(palette::raised())
    } else if state.hovered && !state.disabled {
        Some(look.hover)
    } else {
        None
    };
    let radius = look.radius;
    if let Some(colour) = background {
        let mut face = Path::new();
        face.add_rounded_rect(outer, radius, radius);
        canvas.fill_path(&face, &Brush::solid(colour));
    }
    if state.checked {
        // The border is the band between the button's rounded rectangle and the one a border's
        // width inside it.
        let inset = f64::from(look.border);
        let inner_radius = katana_math::cpp::max(radius - inset, 0.0);
        let mut band = Path::new();
        band.fill_rule = FillRule::OddEven;
        band.add_rounded_rect(outer, radius, radius);
        band.add_rounded_rect(
            outer.adjusted(inset, inset, -inset, -inset),
            inner_radius,
            inner_radius,
        );
        canvas.fill_path(&band, &Brush::solid(palette::accent()));
    }
    paint_button_icon(canvas, paint);
    match paint.sign {
        MenuSign::None => {}
        MenuSign::Mark(ink) => paint_mark(canvas, paint, ink),
        MenuSign::Strip(colour) => {
            let strip = IntRect::new(paint.width - STRIP, 0, STRIP, paint.height);
            paint_fusion_arrow(canvas, strip, colour);
        }
        MenuSign::Indicator(colour) => {
            let padding = look.padding_box(paint.width, paint.height);
            let indicator = IntRect::new(
                padding.x + padding.width - INDICATOR,
                padding.y + padding.height / 2 - INDICATOR / 2,
                INDICATOR,
                INDICATOR,
            );
            paint_fusion_arrow(canvas, indicator, colour);
        }
    }
    canvas.restore();
}

/// The icon in its square, as the style draws an icon's pixmap: the icon's size times the ratio
/// (rounded) in device pixels, from the device pixel its square's corner falls on (rounded).
fn paint_button_icon(canvas: &mut dyn Canvas, paint: &ButtonPaint) {
    let icon = paint.icon_rect();
    match &paint.icon {
        ButtonIcon::None | ButtonIcon::Clear => {}
        ButtonIcon::Overflow { down, ink } => {
            let contents = paint.look.contents(paint.width, paint.height);
            paint_overflow_arrow(canvas, contents.to_rect_f(), *down, *ink);
        }
        ButtonIcon::Named(_) | ButtonIcon::Solid => {
            let device = canvas.device_transform();
            let ratio = device.m11;
            let side = f64::from(device_pixels(f64::from(paint.icon_size) * ratio));
            let left = f64::from(qt_round(f64::from(icon.x) * ratio + device.dx));
            let top = f64::from(qt_round(f64::from(icon.y) * ratio + device.dy));
            let square = RectF::new(left, top, side, side);
            canvas.save();
            to_device_pixels(canvas);
            match &paint.icon {
                ButtonIcon::Solid => canvas.fill_rect(square, &Brush::solid(Color::WHITE)),
                ButtonIcon::Named(name) => {
                    let mode = if paint.state.disabled {
                        IconMode::Disabled
                    } else {
                        IconMode::Normal
                    };
                    paint_named_icon(canvas, name, square, mode);
                }
                _ => {}
            }
            canvas.restore();
        }
    }
}

/// A family's mark (FlyoutButton::paintEvent): a right-angled triangle filling the mark's square
/// on and below its diagonal, the right angle in the corner, row by row, one pixel wider each, so
/// that it is the same pixels on every platform. A polygon fill was not: without antialiasing Qt
/// leaves out the pixels whose centres lie on the diagonal, and a 5 px triangle came out 4 px;
/// antialiased, at five pixels its edge is a smudge. The rows are the DEVICE's: rows of the
/// widget's pixels were whole at a ratio of 1 and 2, but at 1.25 and 1.5 each became one device
/// row or two and the staircase came out uneven (rows 1, 2, 3, 3, 5, 6 at 125%). The square is the
/// mark's legs and inset times the ratio, each taken down to whole device pixels and set back from
/// the button's last device pixel, so the triangle is the same on every button: snapping the
/// mark's own square to the device, as was done first, made its size hang on where the button sat
/// in the window. Taken down, not rounded, since the mark's square starts the pixel after the
/// icon's ends.
fn paint_mark(canvas: &mut dyn Canvas, paint: &ButtonPaint, ink: Color) {
    let device = canvas.device_transform();
    let ratio = device.m11;
    let mark = mark_rect(paint.width, paint.height, paint.icon_rect());
    let last_column = qt_round(f64::from(paint.width) * ratio + device.dx) - 1;
    let last_row = qt_round(f64::from(paint.height) * ratio + device.dy) - 1;
    let inset = whole_device_pixels((paint.width - 1) - mark.right(), ratio);
    let leg = 1.max(whole_device_pixels(mark.width, ratio));
    // The first device column and row the border touches, however little. The border is
    // `border` widget pixels at the button's edge, 1.25 device pixels at 125%, and wherever the
    // button starts off the device grid - in the window, never in a grab - it falls across two
    // device columns; set back by the inset alone, the mark met the first of them along both its
    // sides, and while the tool ran it read as a thickened corner of the frame. A whole device
    // pixel is kept clear of it. Where the inset leaves that already - everywhere at 100, 150 and
    // 200% - nothing moves; where it does not, the mark moves a pixel in, which the icon's square
    // leaves room for at 125 and 175% at every toolbar icon size (docs/desktop.md, "The mark is
    // drawn in the screen's own pixels").
    let first_touched = |widget_pixels: i32, offset: f64| {
        (f64::from(widget_pixels) * ratio + offset + 1e-6).floor() as i32
    };
    let border = paint.look.border;
    let right = (last_column - inset).min(first_touched(paint.width - border, device.dx) - 2);
    let bottom = (last_row - inset).min(first_touched(paint.height - border, device.dy) - 2);
    let square = IntRect::new(right - leg + 1, bottom - leg + 1, leg, leg);
    canvas.save();
    to_device_pixels(canvas);
    let brush = Brush::solid(ink);
    for row in 0..leg {
        canvas.fill_rect(
            RectF::new(
                f64::from(square.right() - row),
                f64::from(square.bottom() - leg + 1 + row),
                f64::from(row + 1),
                1.0,
            ),
            &brush,
        );
    }
    canvas.restore();
}

/// Fusion's menu arrow (qt_fusion_draw_arrow), pointing down, in `rect`: 14 by 8 px at most, as
/// wide as the rectangle's smaller side allows and 8/14 of that deep, centred, filled
/// antialiased in `colour`. Drawn directly where Fusion caches it as a pixmap first; at a ratio of
/// 1 the two are the same pixels.
pub fn paint_fusion_arrow(canvas: &mut dyn Canvas, rect: IntRect, colour: Color) {
    if rect.is_empty() {
        return;
    }
    // Fusion's arrow at 96 dpi: int(dpiScaled(14)) by int(dpiScaled(8)).
    const ARROW_WIDTH: i32 = 14;
    const ARROW_HEIGHT: i32 = 8;
    let arrow_max = ARROW_HEIGHT.min(ARROW_WIDTH);
    let rect_max = rect.height.min(rect.width);
    let size = arrow_max.min(rect_max);
    let width = f64::from(size);
    let height = f64::from(ARROW_HEIGHT * size / ARROW_WIDTH);
    let x = f64::from(rect.x) + (f64::from(rect.width) - width) / 2.0;
    let y = f64::from(rect.y) + (f64::from(rect.height) - height) / 2.0;
    canvas.save();
    canvas.set_render_hint(RenderHint::Antialiasing, true);
    canvas.set_pen(Pen::none());
    canvas.set_brush(Brush::solid(colour));
    canvas.draw_polygon(
        &[
            PointF::new(x, y),
            PointF::new(x + width, y),
            PointF::new(x + width / 2.0, y + height),
        ],
        FillRule::OddEven,
    );
    canvas.restore();
}

/// A toolbar's overflow arrow (theme.cpp's OverflowArrowEngine): two chevrons, pointing down on a
/// vertical toolbar (its hidden buttons are below its end) or right on a horizontal one, in
/// `ink`.
///
/// Painted in whole pixels of the device's, whatever the canvas's transform, and without
/// antialiasing: a 45-degree line a pixel wide is a clean staircase at any size, where an
/// antialiased one is a smudge at this one - at 12 px the two 1.5 px chevrons all but ran into one
/// shape. The arrow was first two pixmaps, 12 and 24 px, and at 125% and 150% Qt resampled the 24
/// down to 15 and 18: the chevrons came out in grey halos.
///
/// The geometry is worked from the rows the arrow has along its way - 10 in the overflow button
/// at 100%, its 12 px less its border: each chevron `half` pixels either side of its apex and
/// `half` + the stroke rows deep, the second right below the first, so the two fill all but a row
/// at each end (at 10 rows: 7 across, 4 deep each, 8 in all, as Qt's own arrow and the first
/// design were). A chevron's arms then never touch the other's: a clear pixel is left between
/// them wherever they run side by side. The stroke is a pixel wide per 10 px of that room, two at
/// 200%.
pub fn paint_overflow_arrow(canvas: &mut dyn Canvas, rect: RectF, down: bool, ink: Color) {
    // In the device's pixels: the rectangle mapped and aligned outwards (QRectF::toAlignedRect).
    let mapped = canvas.device_transform().map_rect(&rect);
    let left = mapped.x.floor() as i32;
    let top = mapped.y.floor() as i32;
    let area = IntRect::new(
        left,
        top,
        mapped.right().ceil() as i32 - left,
        mapped.bottom().ceil() as i32 - top,
    );
    let vertical = down;
    let across_room = if vertical { area.width } else { area.height };
    let along_room = if vertical { area.height } else { area.width };
    let stroke = 1.max(along_room / 10);
    let half = ((along_room - 2 * stroke) / 2 - 1).min((across_room - 1) / 2);
    if half < 2 {
        return; // no room for a chevron that reads as one
    }
    let step = half + stroke;
    // Across the arrow and along it: its width, and its depth from the first chevron's top to the
    // second's bottom.
    let across = 2 * half + 1;
    let along = step + half + stroke;
    let centre = (if vertical { area.x } else { area.y }) + (across_room - across) / 2 + half;
    let start = (if vertical { area.y } else { area.x }) + (along_room - along) / 2;

    canvas.save();
    to_device_pixels(canvas);
    canvas.set_render_hint(RenderHint::Antialiasing, false);
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

/// The colour of the overflow arrow: disabled with its button, the accent while a tool runs whose
/// button it hides (palette::TOOL_RUNS_BEHIND_OVERFLOW), muted otherwise.
pub fn overflow_ink(disabled: bool, lit: bool) -> Color {
    if disabled {
        palette::text_disabled()
    } else if lit {
        palette::accent()
    } else {
        palette::text_muted()
    }
}

// ---- what is drawn where: the toolbar check ----------------------------------------------------

/// Where a tool button draws its icon and the sign of its menu, each measured from its own
/// rendering against itself: the icon's square as where the button with an opaque square for
/// its icon differs from the button with a clear one, the sign as where the button with its menu
/// differs from the button without. None for what it does not draw.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DrawnParts {
    /// In the widget's own pixels; on a scaled screen each box is rounded outwards to the widget's
    /// pixels it touches.
    pub icon: Option<IntRect>,
    pub sign: Option<IntRect>,
    /// The same in the grab's device pixels, where a clash is judged. At a ratio of 1.25 an icon's
    /// square ending at device column 32 and a mark starting at 33 both touch the widget's pixel
    /// 26: rounded outwards they overlapped though nothing on the screen did.
    pub icon_device: Option<IntRect>,
    pub sign_device: Option<IntRect>,
    pub ratio: f64,
}

/// The box, in device pixels, of the pixels where two images of one size differ; None for none
/// (or for images of different sizes).
pub fn difference_box(a: &Image, b: &Image) -> Option<IntRect> {
    if a.width() != b.width() || a.height() != b.height() || a.format() != b.format() {
        return None;
    }
    let size = a.format().bytes_per_pixel();
    let width = a.width() as usize;
    let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, -1, -1);
    for (at, (one, two)) in a
        .bytes()
        .chunks_exact(size)
        .zip(b.bytes().chunks_exact(size))
        .enumerate()
    {
        if one != two {
            let x = (at % width) as i32;
            let y = (at / width) as i32;
            left = left.min(x);
            right = right.max(x);
            top = top.min(y);
            bottom = bottom.max(y);
        }
    }
    if right < 0 {
        None
    } else {
        Some(IntRect::new(left, top, right - left + 1, bottom - top + 1))
    }
}

/// A box of device pixels taken back to the widget's pixels at `ratio`, rounded outwards: every
/// widget pixel a device pixel of the box falls in.
pub fn widget_pixels(device: IntRect, ratio: f64) -> IntRect {
    let down = |at: i32| (f64::from(at) / ratio).floor() as i32;
    let up = |at: i32| (f64::from(at + 1) / ratio).ceil() as i32 - 1;
    let left = down(device.x);
    let top = down(device.y);
    IntRect::new(
        left,
        top,
        up(device.right()) - left + 1,
        up(device.bottom()) - top + 1,
    )
}

/// The button `paint` describes, measured at `ratio` (drawnParts): rendered with an opaque square
/// for its icon and a clear one, and with its menu's sign and without.
pub fn drawn_parts(paint: &ButtonPaint, ratio: f64, raster: &dyn Rasteriser) -> DrawnParts {
    let mut solid = paint.clone();
    solid.icon = ButtonIcon::Solid;
    let mut clear = paint.clone();
    clear.icon = ButtonIcon::Clear;
    let mut without = paint.clone();
    without.sign = MenuSign::None;
    let icon_device = difference_box(&solid.render(raster, ratio), &clear.render(raster, ratio));
    let sign_device = difference_box(&paint.render(raster, ratio), &without.render(raster, ratio));
    DrawnParts {
        icon: icon_device.map(|device| widget_pixels(device, ratio)),
        sign: sign_device.map(|device| widget_pixels(device, ratio)),
        icon_device,
        sign_device,
        ratio,
    }
}

/// "x L..R y T..B".
pub fn rect_text(rect: &IntRect) -> String {
    format!(
        "x {}..{} y {}..{}",
        rect.x,
        rect.right(),
        rect.y,
        rect.bottom()
    )
}

/// Whether two boxes share a pixel (QRect::intersects).
pub fn rects_intersect(a: &IntRect, b: &IntRect) -> bool {
    !a.is_empty()
        && !b.is_empty()
        && a.x <= b.right()
        && b.x <= a.right()
        && a.y <= b.bottom()
        && b.y <= a.bottom()
}

/// A button the check measures: its object name, whether it has a menu and shows its icon alone
/// (only those are measured), and how it is painted as it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredButton {
    pub name: String,
    pub has_menu: bool,
    pub icon_only: bool,
    pub paint: ButtonPaint,
}

/// What holds buttons the check measures: "toolbar button DrawToolBar" for a toolbar,
/// "title bar button View1" for a dock's title bar, and its buttons in order.
#[derive(Clone, Debug, PartialEq)]
pub struct SignHolder {
    pub holder: String,
    pub buttons: Vec<MeasuredButton>,
}

/// Every icon-only button with a menu among `holders` whose menu sign is drawn over its icon's
/// square, in device pixels at `ratio`, or not drawn at all: "<holder> > <button>: <why>". The
/// toolbars come first, then the title bars, as the caller lists them (menuSignClashes). Each
/// button measured gets a line in `measured`, clear or not, and `buttons` counts them. A
/// toolbar's own overflow button is not one of them: it draws its menu arrow as its icon.
/// `katana --check-toolbars` fails a run with any (docs/headless.md).
pub fn menu_sign_clashes(holders: &[SignHolder], ratio: f64, raster: &dyn Rasteriser) -> MenuSigns {
    let mut signs = MenuSigns::default();
    for holder in holders {
        for button in &holder.buttons {
            if !button.has_menu
                || button.name == OVERFLOW_BUTTON
                || !button.icon_only
                || button.paint.icon == ButtonIcon::None
            {
                continue;
            }
            signs.buttons += 1;
            let at = format!("{} > {}", holder.holder, button.name);
            let parts = drawn_parts(&button.paint, ratio, raster);
            // Judged, and so said, in device pixels on a scaled screen.
            let scaled = parts.ratio != 1.0;
            let (sign, icon) = if scaled {
                (parts.sign_device, parts.icon_device)
            } else {
                (parts.sign, parts.icon)
            };
            let unit = if scaled {
                format!(
                    " (device pixels at a ratio of {})",
                    format_general(parts.ratio, 6)
                )
            } else {
                String::new()
            };
            let line = match (sign, icon) {
                (None, _) => {
                    let line = format!("{at}: draws no sign of its menu");
                    signs.clashes.push(line.clone());
                    line
                }
                (Some(_), None) => {
                    let line = format!("{at}: draws no icon");
                    signs.clashes.push(line.clone());
                    line
                }
                (Some(sign), Some(icon)) if rects_intersect(&sign, &icon) => {
                    let line = format!(
                        "{at}: its menu's sign ({}) is drawn over its icon ({}){unit}",
                        rect_text(&sign),
                        rect_text(&icon)
                    );
                    signs.clashes.push(line.clone());
                    line
                }
                (Some(sign), Some(icon)) => format!(
                    "{at}: sign {} clear of its icon {}{unit}",
                    rect_text(&sign),
                    rect_text(&icon)
                ),
            };
            signs.measured.push(line);
        }
    }
    signs
}

// ---- tooltips ----------------------------------------------------------------------------------

/// QString::toHtmlEscaped: the four characters rich text reads as markup.
pub fn html_escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

/// A tool's name as the tooltip lists it: its menu letter's '&' gone, its words held together by
/// no-break spaces, so that the line wraps between names and never inside one ("3 / Points" was
/// Circle's), and escaped for the tooltip's rich text.
pub fn listed_name(text: &str) -> String {
    html_escaped(&text.replace('&', "").replace(' ', "\u{a0}"))
}

/// The family's tool that runs, if one does: the first checked action of its menu.
pub fn running_tool(menu: &Menu) -> Option<&Action> {
    menu.items.iter().find_map(|item| match item {
        MenuItem::Action(action) if action.checked == Some(true) => Some(action),
        _ => None,
    })
}

/// The tooltip a family's button shows (FlyoutButton::toolTipWithMenu): `tool_tip`, the tool's
/// own, then a line saying how to open the menu and naming the family's other tools - five at
/// most, past that four and how many more - and, while another of them runs, a line naming it.
/// `default` is the object name of the action a click runs.
pub fn tooltip_with_menu(tool_tip: &str, menu: &Menu, default: &str) -> String {
    let mut others = Vec::new();
    for item in &menu.items {
        match item {
            MenuItem::Action(action) if action.name != default => {
                others.push(listed_name(&action.text));
            }
            MenuItem::Menu(submenu) => others.push(listed_name(&submenu.title)),
            _ => {}
        }
    }
    if others.is_empty() {
        return String::from(tool_tip);
    }
    // Slashes between them, as the family's status tip has: a variant's name may hold a comma.
    let mut named = others.join(" / ");
    if others.len() > NAMED_IN_TOOLTIP {
        named = format!(
            "{} and {} more",
            others[..NAMED_IN_TOOLTIP - 1].join(" / "),
            others.len() - (NAMED_IN_TOOLTIP - 1)
        );
    }
    // "for its other tools:" before the names, so that the triangle does not read as the first
    // of them ("the corner triangle for Delete Vertex").
    let mut tip = format!(
        "{tool_tip}<br><span style='color:{}'>Hold, right-click or press the corner triangle for \
         its other tools: {named}</span>",
        palette::text_muted().name()
    );
    // Another of them running lights the mark but frames nothing - the icon is not its - so
    // this is where it is named, in the accent the mark is.
    if let Some(running) = running_tool(menu)
        && running.name != default
    {
        tip.push_str(&format!(
            "<br><span style='color:{}'>Running now: {}</span>",
            palette::accent().name(),
            listed_name(&running.text)
        ));
    }
    tip
}

/// A run of a tooltip's text in one look.
#[derive(Clone, Debug, PartialEq)]
pub struct TipRun {
    pub text: String,
    pub bold: bool,
    pub colour: Option<Color>,
}

/// The runs of the simple rich text the window's tooltips are written in: <b>, <br>,
/// <span style='color:#rrggbb'> and the four entities toHtmlEscaped writes, and &nbsp;. A <br> is
/// a run of "\n"; anything else between angle brackets is dropped.
pub fn tooltip_runs(html: &str) -> Vec<TipRun> {
    let mut runs = Vec::new();
    let mut bold = false;
    let mut colours: Vec<Option<Color>> = Vec::new();
    let mut text = String::new();
    let mut rest = html;
    let flush = |text: &mut String, runs: &mut Vec<TipRun>, bold: bool, colour: Option<Color>| {
        if !text.is_empty() {
            runs.push(TipRun {
                text: std::mem::take(text),
                bold,
                colour,
            });
        }
    };
    while let Some(character) = rest.chars().next() {
        let colour = colours.last().copied().flatten();
        if character == '<'
            && let Some(end) = rest.find('>')
        {
            let tag = &rest[1..end];
            flush(&mut text, &mut runs, bold, colour);
            match tag {
                "b" => bold = true,
                "/b" => bold = false,
                "br" | "br/" | "br /" => runs.push(TipRun {
                    text: String::from("\n"),
                    bold,
                    colour,
                }),
                "/span" => {
                    colours.pop();
                }
                _ if tag.starts_with("span") => {
                    let named = tag
                        .find("color:")
                        .and_then(|at| Color::from_name(tag[at + 6..].get(..7).unwrap_or("")));
                    colours.push(named);
                }
                _ => {}
            }
            rest = &rest[end + 1..];
            continue;
        }
        if character == '&'
            && let Some(end) = rest.find(';')
        {
            let entity = &rest[1..end];
            let decoded = match entity {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "quot" => Some('"'),
                "nbsp" => Some('\u{a0}'),
                _ => None,
            };
            if let Some(decoded) = decoded {
                text.push(decoded);
                rest = &rest[end + 1..];
                continue;
            }
        }
        text.push(character);
        rest = &rest[character.len_utf8()..];
    }
    let colour = colours.last().copied().flatten();
    flush(&mut text, &mut runs, bold, colour);
    runs
}

/// The lines a paragraph takes when it is word-wrapped at `width` (QTextOption::WordWrap): broken
/// at spaces only - a no-break space holds its words together - and a word wider than the line
/// running past it rather than breaking.
pub fn wrap_words(paragraph: &str, width: f32, measure: &dyn Fn(&str) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current: Option<String> = None;
    for word in paragraph.split(' ') {
        current = Some(match current {
            None => String::from(word),
            Some(line) => {
                let longer = format!("{line} {word}");
                if measure(&longer) <= width {
                    longer
                } else {
                    lines.push(line);
                    String::from(word)
                }
            }
        });
    }
    if let Some(line) = current {
        lines.push(line);
    }
    lines
}

/// Draws a tooltip's rich text, wrapped at a quarter of the screen's width as QTipLabel wraps a
/// rich tooltip, between the words of its lines and never inside a name.
pub fn tooltip_ui(ui: &mut Ui, html: &str) {
    let runs = tooltip_runs(html);
    let plain: String = runs.iter().map(|run| run.text.as_str()).collect();
    let font = egui::TextStyle::Body.resolve(ui.style());
    let width = ui.ctx().content_rect().width() / 4.0;
    let ctx = ui.ctx().clone();
    let measure = |text: &str| {
        ctx.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(String::from(text), font.clone(), Color32::WHITE)
                .size()
                .x
        })
    };
    // Wrapping only turns a space into a line break, a byte for a byte, so each run keeps its
    // place in the wrapped text.
    let wrapped: Vec<String> = plain
        .split('\n')
        .map(|paragraph| wrap_words(paragraph, width, &measure).join("\n"))
        .collect();
    let wrapped = wrapped.join("\n");
    let mut job = egui::text::LayoutJob::default();
    let mut at = 0;
    for run in &runs {
        let end = at + run.text.len();
        let colour = run.colour.unwrap_or_else(palette::text).to_egui();
        let mut format = egui::TextFormat::simple(font.clone(), colour);
        if run.bold {
            format.color = palette::text().to_egui();
        }
        job.append(&wrapped[at..end], 0.0, format);
        at = end;
    }
    ui.add(egui::Label::new(job).wrap_mode(egui::TextWrapMode::Extend));
}

// ---- the buttons on the screen -----------------------------------------------------------------

/// Draws `paint` at `rect`'s corner, one device pixel for one, from the icon cache (icons). Nothing
/// when the context has no rasteriser.
pub fn show_button(ui: &Ui, rect: Rect, paint: &ButtonPaint) {
    let ctx = ui.ctx();
    let ratio = f64::from(ctx.pixels_per_point());
    let width = device_pixels(f64::from(paint.width) * ratio);
    let height = device_pixels(f64::from(paint.height) * ratio);
    let key = format!("button {paint:?} {ratio}");
    let Some(texture) = painted_texture(
        ctx,
        &key,
        width,
        height,
        ratio,
        &mut |canvas: &mut dyn Canvas| paint_tool_button(canvas, paint),
    ) else {
        return;
    };
    let at = device_rect(ctx, rect.min, width, height);
    let whole = Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
    ui.painter().image(texture.id(), at, whole, Color32::WHITE);
}

/// The status bar's line from a button this frame: what to show while the pointer is on it, the
/// empty line the frame it leaves (QWidget clears its status tip on Leave), None otherwise.
fn status_line(hovered: bool, was_hovered: bool, line: &str) -> Option<String> {
    if hovered {
        Some(String::from(line))
    } else if was_hovered {
        Some(String::new())
    } else {
        None
    }
}

/// What a family's button is drawn from each frame: its object name, the family's menu (its
/// items' texts and check states as they are now), the action a click runs, that action's
/// tooltip, and the family's own status line ("Vertices: Insert Vertex / Delete Vertex / ...").
#[derive(Clone, Debug, PartialEq)]
pub struct Flyout {
    pub name: String,
    pub menu: Menu,
    pub default: Action,
    pub tool_tip: String,
    pub family_tip: String,
}

impl Flyout {
    /// The family's tool that runs, if one does: the one a click starts or any other from its
    /// menu.
    pub fn running_tool(&self) -> Option<&Action> {
        running_tool(&self.menu)
    }

    /// Whether a tool of the family runs (FlyoutButton::familyRunning): the mark is lit in the
    /// accent while one does; the button is framed only while it is the one its icon shows,
    /// since a frame round Insert Vertex's icon says Insert Vertex runs.
    pub fn family_running(&self) -> bool {
        self.default.checked == Some(true) || self.running_tool().is_some()
    }

    /// The tooltip shown (FlyoutButton::toolTipWithMenu), built when shown since a family's tools
    /// are added to its menu after the button is made.
    pub fn tooltip_with_menu(&self) -> String {
        tooltip_with_menu(&self.tool_tip, &self.menu, &self.default.name)
    }

    /// How it is painted at `icon_size` in `state`, the pointer on its mark or not.
    pub fn paint(&self, icon_size: i32, state: ButtonState, pointer_on_mark: bool) -> ButtonPaint {
        let look = ToolButtonLook::TOOL;
        let (width, height) = look.button_size(icon_size);
        ButtonPaint {
            width,
            height,
            look,
            icon: ButtonIcon::Named(String::from(self.default.icon)),
            icon_size,
            state,
            sign: MenuSign::Mark(mark_ink(&state, self.family_running(), pointer_on_mark)),
            ground: Color::TRANSPARENT,
        }
    }
}

/// What a frame of a family's button did.
#[derive(Debug)]
pub struct FlyoutResponse {
    pub response: Response,
    /// The object name of the action that fired - the family's first tool on a click or a press,
    /// another from the menu - for the window to start (fillToolMenus's `start`).
    pub triggered: Option<String>,
    /// Whether the family's menu opened this frame.
    pub opened: bool,
    /// Whether the menu is open after this frame.
    pub menu_open: bool,
    /// Whether the pointer is over the menu zone, where the mark is lit.
    pub pointer_on_mark: bool,
    pub hovered: bool,
    /// The status bar's line (see `status_line`).
    pub status_tip: Option<String>,
    /// How it was painted.
    pub paint: ButtonPaint,
}

/// What a button remembers between frames.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ButtonMemory {
    menu_open: bool,
    /// The press under way opened the menu (on the corner, or by being held), so its release runs
    /// nothing.
    press_opened: bool,
    hovered: bool,
}

/// What a control kept from the frame drawn at `at`: its state as that frame began and as it
/// left it, and the act a driver's step made of it then.
#[derive(Clone, Debug, Default)]
struct Remembered<T> {
    at: f64,
    before: T,
    after: T,
    acted: Option<String>,
}

/// The state control `id` begins this frame from, and the act an earlier pass of this same frame
/// took. egui draws a frame again when a pass asks it to (a new popup's measuring pass does), and
/// replays the frame's input: a second pass of the frame drawn at `now` begins where the first
/// began, so a press opens the menu in the pass that counts, and an act the first pass took from
/// the mailbox - which a later pass finds no more - is carried over.
fn recall<T: 'static + Clone + Default + Send + Sync>(
    ctx: &egui::Context,
    id: egui::Id,
    now: f64,
) -> (T, Option<String>) {
    match ctx.data_mut(|data| data.get_temp::<Remembered<T>>(id)) {
        Some(kept) if kept.at == now => (kept.before, kept.acted),
        Some(kept) => (kept.after, None),
        None => (T::default(), None),
    }
}

/// Keeps control `id`'s state for the next frame (see `recall`).
fn keep<T: 'static + Clone + Default + Send + Sync>(
    ctx: &egui::Context,
    id: egui::Id,
    now: f64,
    before: T,
    after: T,
    acted: Option<String>,
) {
    let kept = Remembered {
        at: now,
        before,
        after,
        acted,
    };
    ctx.data_mut(|data| data.insert_temp(id, kept));
}

/// Where a button's menu pops up: beside a button on a vertical toolbar, below one otherwise.
fn menu_place(rect: Rect, vertical: bool) -> Pos2 {
    if vertical {
        rect.right_top()
    } else {
        rect.left_bottom()
    }
}

/// The pixel of the widget `at` falls on, in a button whose corner is `origin`.
fn widget_pixel(origin: Pos2, at: Pos2) -> (i32, i32) {
    (
        (at.x - origin.x).floor() as i32,
        (at.y - origin.y).floor() as i32,
    )
}

/// A family's button at `rect` (its size the TOOL look's at `icon_size`): a click on the icon
/// runs the family's first tool; a press on the corner opens the family at once, as does a press
/// held past POPUP_DELAY_SECONDS or a right click, and the release of such a press runs nothing.
/// `--press` (a driver's press) is QAbstractButton::click: the tool, no menu.
pub fn flyout_button_at(
    ui: &mut Ui,
    rect: Rect,
    flyout: &Flyout,
    icon_size: i32,
    vertical: bool,
) -> FlyoutResponse {
    let ctx = ui.ctx().clone();
    let id = egui::Id::new(("katanaToolButton", flyout.name.as_str()));
    let now = ctx.input(|input| input.time);
    let (start, acted) = recall::<ButtonMemory>(&ctx, id, now);
    let mut memory = start;
    let enabled = ui.is_enabled() && flyout.default.enabled;
    let taken = take(ui, &flyout.name);
    let mut response = ui.interact(rect, id, Sense::click());
    let look = ToolButtonLook::TOOL;
    let (width, height) = look.button_size(icon_size);
    let icon = icon_rect(&look, width, height, icon_size);
    let zone = MenuZone::new(width, height, icon);
    let mut triggered = None;
    let mut opened = false;

    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Press => {
                if accept_press(ui, taken) {
                    triggered = Some(flyout.default.name.clone());
                    settle(ui, taken, Ok(()));
                }
            }
            Act::Fill(text) => {
                if accept_fill(ui, taken) {
                    settle(ui, taken, Err(no_field(&flyout.name, text)));
                }
            }
            Act::ScrollTo => {
                response.scroll_to_me(None);
                settle(ui, taken, Ok(()));
            }
        }
    }
    if triggered.is_none() {
        triggered.clone_from(&acted);
    }
    let pressed_by_act = triggered.clone();

    let hovered = response.hovered();
    let pointer_on_mark = hovered
        && enabled
        && response.hover_pos().is_some_and(|at| {
            let (x, y) = widget_pixel(rect.min, at);
            zone.contains(x, y)
        });
    let (pressed, press_origin, press_start, escape) = ctx.input(|input| {
        (
            input.pointer.primary_pressed(),
            input.pointer.press_origin(),
            input.pointer.press_start_time(),
            input.key_pressed(egui::Key::Escape),
        )
    });
    if enabled
        && pressed
        && let Some(origin) = press_origin.filter(|at| rect.contains(*at))
    {
        // The mark's corner opens the menu at once, as the split button's strip did; anywhere
        // else is the tool's, the menu following only if the press is held.
        let (x, y) = widget_pixel(rect.min, origin);
        memory.press_opened = zone.contains(x, y);
        if memory.press_opened && !memory.menu_open {
            memory.menu_open = true;
            opened = true;
        }
    }
    if enabled
        && response.is_pointer_button_down_on()
        && !memory.press_opened
        && let Some(started) = press_start
    {
        let held = now - started;
        if held >= POPUP_DELAY_SECONDS {
            memory.press_opened = true;
            memory.menu_open = true;
            opened = true;
        } else {
            ctx.request_repaint_after(Duration::from_secs_f64(POPUP_DELAY_SECONDS - held));
        }
    }
    if response.clicked() && enabled && !memory.press_opened && triggered.is_none() {
        triggered = Some(flyout.default.name.clone());
    }
    // The family's menu, not the window's list of toolbars: a right click on a tool group opens
    // the group in drawing and graphics programs alike.
    if enabled && response.secondary_clicked() && !memory.menu_open {
        memory.menu_open = true;
        opened = true;
    }
    if memory.menu_open && escape && !opened {
        memory.menu_open = false;
    }
    if memory.menu_open {
        let place = menu_place(rect, vertical);
        let mut chosen = None;
        let shown = popup_menu(&ctx, &flyout.menu.name, place, |ui| {
            for item in &flyout.menu.items {
                match item {
                    MenuItem::Action(action) => {
                        if action_item(ui, action).clicked() {
                            chosen = Some(action.name.clone());
                        }
                    }
                    MenuItem::Section(title) => {
                        ui.weak(title.as_str());
                    }
                    MenuItem::Separator => {
                        ui.separator();
                    }
                    MenuItem::Menu(_) => {}
                }
            }
        });
        let outside = ctx.input(|input| {
            input.pointer.any_pressed()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|at| !shown.rect.contains(at) && !rect.contains(at))
        });
        if let Some(chosen) = chosen {
            triggered = Some(chosen);
            memory.menu_open = false;
        } else if outside && !opened {
            memory.menu_open = false;
        }
    }

    let state = ButtonState {
        hovered: hovered && enabled,
        down: response.is_pointer_button_down_on(),
        checked: flyout.default.checked == Some(true),
        disabled: !enabled,
    };
    let paint = flyout.paint(icon_size, state, pointer_on_mark);
    show_button(ui, rect, &paint);

    // The status bar's line for where the pointer is: the family's own on the corner that opens
    // it, the tool's elsewhere on the button, as QWidget shows it on Enter.
    let line = if pointer_on_mark && !flyout.family_tip.is_empty() {
        flyout.family_tip.as_str()
    } else {
        flyout.default.tip.as_str()
    };
    let status_tip = status_line(hovered, memory.hovered, line);
    memory.hovered = hovered;
    // The press is over: the next one decides afresh.
    if !ctx.input(|input| input.pointer.primary_down()) {
        memory.press_opened = false;
    }

    let mut record_of = ControlRecord::new(&flyout.name, ControlKind::Toggle);
    record_of.text = flyout.default.text.clone();
    record_of.checked = flyout.default.checked;
    record_of.enabled = enabled;
    record_of.visible = ui.is_visible();
    record(ui, &response, record_of);

    if hovered && !memory.menu_open && !flyout.tool_tip.is_empty() {
        let tip = flyout.tooltip_with_menu();
        response = response.on_hover_ui(|ui| tooltip_ui(ui, &tip));
    }
    keep(&ctx, id, now, start, memory, pressed_by_act);
    FlyoutResponse {
        response,
        triggered,
        opened,
        menu_open: memory.menu_open,
        pointer_on_mark,
        hovered,
        status_tip,
        paint,
    }
}

/// A family's button laid out in `ui` at its size.
pub fn flyout_button(
    ui: &mut Ui,
    flyout: &Flyout,
    icon_size: i32,
    vertical: bool,
) -> FlyoutResponse {
    let (width, height) = ToolButtonLook::TOOL.button_size(icon_size);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width as f32, height as f32), Sense::hover());
    flyout_button_at(ui, rect, flyout, icon_size, vertical)
}

/// What a frame of an action's own button, a split button or the overflow button did.
#[derive(Debug)]
pub struct ToolButtonResponse {
    pub response: Response,
    /// The action fired (a click, a press).
    pub triggered: bool,
    /// The menu opened this frame (a split button's strip).
    pub opened: bool,
    /// The object name of the item chosen from the menu this frame.
    pub chosen: Option<String>,
    pub menu_open: bool,
    pub hovered: bool,
    /// The status bar's line (see `status_line`).
    pub status_tip: Option<String>,
    pub paint: ButtonPaint,
}

/// A toolbar's button for `action` at `rect` (QToolBar's own for an action it holds): its icon
/// alone, framed while the action is checked, `tooltip` under the pointer; a click or a press
/// fires it.
pub fn tool_button_at(
    ui: &mut Ui,
    rect: Rect,
    action: &Action,
    tooltip: &str,
    icon_size: i32,
) -> ToolButtonResponse {
    let look = ToolButtonLook::TOOL;
    let (width, height) = look.button_size(icon_size);
    let mut paint = ButtonPaint {
        width,
        height,
        look,
        icon: ButtonIcon::Named(String::from(action.icon)),
        icon_size,
        state: ButtonState::default(),
        sign: MenuSign::None,
        ground: Color::TRANSPARENT,
    };
    button_at(
        ui,
        rect,
        &action.name,
        action,
        tooltip,
        &mut paint,
        None,
        false,
    )
}

/// The split button's strip: the border rectangle's right STRIP pixels of a button `width` by
/// `height`, which its menu opens from.
pub fn split_strip(width: i32, height: i32) -> IntRect {
    IntRect::new(width - STRIP, 0, STRIP, height)
}

/// How a split button with `look` is painted (Undo's and Redo's: ToolButtonLook::SPLIT): `icon`
/// in its square, Fusion's arrow in its strip in the text colour, or the disabled one.
pub fn split_paint(
    look: ToolButtonLook,
    icon: &str,
    icon_size: i32,
    state: ButtonState,
) -> ButtonPaint {
    let (width, height) = look.button_size(icon_size);
    ButtonPaint {
        width,
        height,
        look,
        icon: ButtonIcon::Named(String::from(icon)),
        icon_size,
        state,
        sign: MenuSign::Strip(if state.disabled {
            palette::text_disabled()
        } else {
            palette::text()
        }),
        ground: Color::TRANSPARENT,
    }
}

/// A split button at `rect` (Undo's and Redo's, MenuButtonPopup): `action` its click, `menu` its
/// history dropped from the strip at its right, which a press there opens at once.
pub fn split_button_at(
    ui: &mut Ui,
    rect: Rect,
    name: &str,
    action: &Action,
    menu: &Menu,
    icon_size: i32,
    vertical: bool,
) -> ToolButtonResponse {
    let mut paint = split_paint(
        ToolButtonLook::SPLIT,
        action.icon,
        icon_size,
        ButtonState::default(),
    );
    let tooltip = action.tooltip();
    button_at(
        ui,
        rect,
        name,
        action,
        &tooltip,
        &mut paint,
        Some(menu),
        vertical,
    )
}

/// The button a split button and an action's button share: its acts, its pointer, its strip's
/// menu when it has one, its painting and its record.
#[allow(
    clippy::too_many_arguments,
    reason = "One button, given what QToolButton holds: its name, action, tooltip, look and menu."
)]
fn button_at(
    ui: &mut Ui,
    rect: Rect,
    name: &str,
    action: &Action,
    tooltip: &str,
    paint: &mut ButtonPaint,
    menu: Option<&Menu>,
    vertical: bool,
) -> ToolButtonResponse {
    let ctx = ui.ctx().clone();
    let id = egui::Id::new(("katanaToolButton", name));
    let now = ctx.input(|input| input.time);
    let (start, acted) = recall::<ButtonMemory>(&ctx, id, now);
    let mut memory = start;
    let enabled = ui.is_enabled() && action.enabled;
    let taken = take(ui, name);
    let mut response = ui.interact(rect, id, Sense::click());
    let mut triggered = false;
    let mut opened = false;
    let mut chosen = None;
    if let Some(taken) = &taken {
        match &taken.posted.act {
            Act::Press => {
                if accept_press(ui, taken) {
                    triggered = true;
                    settle(ui, taken, Ok(()));
                }
            }
            Act::Fill(text) => {
                if accept_fill(ui, taken) {
                    settle(ui, taken, Err(no_field(name, text)));
                }
            }
            Act::ScrollTo => {
                response.scroll_to_me(None);
                settle(ui, taken, Ok(()));
            }
        }
    }
    triggered = triggered || acted.is_some();
    let pressed_by_act = triggered.then(|| String::from(name));
    let hovered = response.hovered();
    let strip = split_strip(paint.width, paint.height);
    let (pressed, press_origin, escape) = ctx.input(|input| {
        (
            input.pointer.primary_pressed(),
            input.pointer.press_origin(),
            input.key_pressed(egui::Key::Escape),
        )
    });
    if let Some(menu) = menu {
        if enabled
            && pressed
            && let Some(origin) = press_origin.filter(|at| rect.contains(*at))
        {
            let (x, y) = widget_pixel(rect.min, origin);
            memory.press_opened = strip.contains(x, y);
            if memory.press_opened && !memory.menu_open {
                memory.menu_open = true;
                opened = true;
            }
        }
        if memory.menu_open && escape && !opened {
            memory.menu_open = false;
        }
        if memory.menu_open {
            let place = menu_place(rect, vertical);
            let mut picked = None;
            let shown = popup_menu(&ctx, &menu.name, place, |ui| {
                for item in &menu.items {
                    match item {
                        MenuItem::Action(entry) => {
                            if action_item(ui, entry).clicked() {
                                picked = Some(entry.name.clone());
                            }
                        }
                        MenuItem::Section(title) => {
                            ui.weak(title.as_str());
                        }
                        MenuItem::Separator => {
                            ui.separator();
                        }
                        MenuItem::Menu(_) => {}
                    }
                }
            });
            let outside = ctx.input(|input| {
                input.pointer.any_pressed()
                    && input
                        .pointer
                        .interact_pos()
                        .is_some_and(|at| !shown.rect.contains(at) && !rect.contains(at))
            });
            if picked.is_some() {
                chosen = picked;
                memory.menu_open = false;
            } else if outside && !opened {
                memory.menu_open = false;
            }
        }
    }
    if response.clicked() && enabled && !memory.press_opened {
        triggered = true;
    }
    if !ctx.input(|input| input.pointer.primary_down()) {
        memory.press_opened = false;
    }
    paint.state = ButtonState {
        hovered: hovered && enabled,
        down: response.is_pointer_button_down_on(),
        checked: action.checked == Some(true),
        disabled: !enabled,
    };
    if menu.is_some() {
        *paint = split_paint(paint.look, action.icon, paint.icon_size, paint.state);
    }
    show_button(ui, rect, paint);
    let status_tip = status_line(hovered, memory.hovered, &action.tip);
    memory.hovered = hovered;
    let kind = if action.checked.is_some() {
        ControlKind::Toggle
    } else {
        ControlKind::Button
    };
    let mut record_of = ControlRecord::new(name, kind);
    record_of.text = action.text.clone();
    record_of.checked = action.checked;
    record_of.enabled = enabled;
    record_of.visible = ui.is_visible();
    record(ui, &response, record_of);
    if hovered && !memory.menu_open && !tooltip.is_empty() {
        response = response.on_hover_ui(|ui| tooltip_ui(ui, tooltip));
    }
    keep(&ctx, id, now, start, memory, pressed_by_act);
    ToolButtonResponse {
        response,
        triggered,
        opened,
        chosen,
        menu_open: memory.menu_open,
        hovered,
        status_tip,
        paint: paint.clone(),
    }
}

// ---- a toolbar and its overflow ----------------------------------------------------------------

/// A toolbar's padding along it and across it (theme.cpp: QToolBar { padding: 3px 6px; } and
/// QToolBar:left, QToolBar:right { padding: 6px 3px; }), and the space between its items.
pub const TOOL_BAR_PADDING_ALONG: i32 = 6;
pub const TOOL_BAR_PADDING_ACROSS: i32 = 3;
pub const TOOL_BAR_SPACING: i32 = 2;
/// A separator's line and its margins (QToolBar::separator { width: 1px; height: 1px; margin:
/// 4px 5px; }): along a horizontal bar the line is crossed, 5 px each side; along a vertical one,
/// 4 px each side.
pub const SEPARATOR_LINE: i32 = 1;

/// One item of a toolbar as the strip draws it.
#[derive(Clone, Debug, PartialEq)]
pub enum StripItem {
    /// An action's own button (QToolBar's for addAction), with the tooltip it shows: `tool` when
    /// it is a catalogue tool's, which alone lights the overflow arrow (Select, checked while NO
    /// tool runs, does not).
    Action {
        action: Action,
        tool: bool,
        tooltip: String,
    },
    /// A tool family's button.
    Family(Flyout),
    Separator,
    /// A control the toolbar's owner draws itself (Undo's split button, a choice), by its object
    /// name, `width` by `height` widget pixels; it never shows a tool running.
    Widget {
        name: String,
        width: i32,
        height: i32,
    },
}

impl StripItem {
    /// Its object name; empty for a separator.
    pub fn name(&self) -> &str {
        match self {
            Self::Action { action, .. } => &action.name,
            Self::Family(flyout) => &flyout.name,
            Self::Separator => "",
            Self::Widget { name, .. } => name,
        }
    }

    /// Its size in the widget's pixels at `icon_size`. A separator is its line and its margins:
    /// across a horizontal bar's length 5 px each side of it, along a vertical bar's 4 px.
    pub fn size(&self, icon_size: i32) -> (i32, i32) {
        match self {
            Self::Action { .. } | Self::Family(_) => ToolButtonLook::TOOL.button_size(icon_size),
            Self::Separator => (SEPARATOR_LINE + 2 * 5, SEPARATOR_LINE + 2 * 4),
            Self::Widget { width, height, .. } => (*width, *height),
        }
    }

    /// Whether, hidden behind the overflow arrow, it shows a tool running: a tool's button
    /// checked, or a family with one of its tools running.
    pub fn shows_a_tool_running(&self) -> bool {
        match self {
            Self::Action { action, tool, .. } => *tool && action.checked == Some(true),
            Self::Family(flyout) => flyout.family_running(),
            Self::Separator | Self::Widget { .. } => false,
        }
    }
}

/// Where a toolbar's items go along it (QToolBarLayout, one row): which are shown, each one's
/// offset from the bar's start, and whether the overflow arrow is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StripLayout {
    pub shown: Vec<bool>,
    pub offsets: Vec<i32>,
    pub overflow: bool,
}

/// Lays `extents` (each item's length along the bar) out in `room`: all of them when they fit
/// with the bar's padding; otherwise as many as fit before the overflow button and its spacing,
/// the rest hidden behind it, and a separator left last before them hidden too.
pub fn lay_out_strip(extents: &[i32], separators: &[bool], room: i32) -> StripLayout {
    let count = extents.len();
    let mut layout = StripLayout {
        shown: vec![true; count],
        offsets: vec![0; count],
        overflow: false,
    };
    let mut at = TOOL_BAR_PADDING_ALONG;
    for (index, &extent) in extents.iter().enumerate() {
        layout.offsets[index] = at;
        at += extent + TOOL_BAR_SPACING;
    }
    let total = at - TOOL_BAR_SPACING + TOOL_BAR_PADDING_ALONG;
    if count == 0 || total <= room {
        return layout;
    }
    layout.overflow = true;
    let limit = room - TOOL_BAR_PADDING_ALONG - EXTENSION_EXTENT - TOOL_BAR_SPACING;
    let mut hiding = false;
    for (index, &extent) in extents.iter().enumerate() {
        if !hiding && layout.offsets[index] + extent > limit {
            hiding = true;
        }
        layout.shown[index] = !hiding;
    }
    // A separator with nothing shown after it parts nothing.
    for (shown, &separator) in layout.shown.iter_mut().zip(separators).rev() {
        if !*shown {
            continue;
        }
        if separator {
            *shown = false;
        } else {
            break;
        }
    }
    layout
}

/// The toolbar check's view of `strip` (menuSignClashes measures a toolbar's buttons): each
/// family with its menu, as it stands, hidden behind the arrow or not ("measured at its size
/// hint"), and each action's own button, which has no menu. Its Widget rows are its owner's to
/// add (Undo's and Redo's split buttons: `split_paint`).
pub fn strip_sign_holder(strip: &Strip) -> SignHolder {
    let mut buttons = Vec::new();
    for item in &strip.items {
        match item {
            StripItem::Family(flyout) => buttons.push(MeasuredButton {
                name: flyout.name.clone(),
                has_menu: true,
                icon_only: true,
                paint: flyout.paint(strip.icon_size, ButtonState::default(), false),
            }),
            StripItem::Action { action, .. } => {
                let look = ToolButtonLook::TOOL;
                let (width, height) = look.button_size(strip.icon_size);
                buttons.push(MeasuredButton {
                    name: action.name.clone(),
                    has_menu: false,
                    icon_only: true,
                    paint: ButtonPaint {
                        width,
                        height,
                        look,
                        icon: ButtonIcon::Named(String::from(action.icon)),
                        icon_size: strip.icon_size,
                        state: ButtonState {
                            checked: action.checked == Some(true),
                            ..ButtonState::default()
                        },
                        sign: MenuSign::None,
                        ground: Color::TRANSPARENT,
                    },
                });
            }
            StripItem::Separator | StripItem::Widget { .. } => {}
        }
    }
    SignHolder {
        holder: format!("toolbar button {}", strip.name),
        buttons,
    }
}

/// Whether the overflow arrow is lit (lightOverflowWhileAHiddenToolRuns): a hidden item shows a
/// tool running. Worked out afresh from what is hidden and running each time it is asked - never
/// kept as a count that could drift - and never while the bar is expanded, its buttons showing
/// their own frames and marks.
pub fn overflow_lit(items: &[StripItem], layout: &StripLayout, expanded: bool) -> bool {
    !expanded
        && items
            .iter()
            .zip(&layout.shown)
            .any(|(item, shown)| !*shown && item.shows_a_tool_running())
}

/// The overflow button's painting: OVERFLOW_BUTTON's look, its arrow its icon, checked while the
/// bar is expanded, over `ground`.
pub fn overflow_paint(
    width: i32,
    height: i32,
    vertical: bool,
    state: ButtonState,
    lit: bool,
    ground: Color,
) -> ButtonPaint {
    ButtonPaint {
        width,
        height,
        look: ToolButtonLook::OVERFLOW,
        icon: ButtonIcon::Overflow {
            down: vertical,
            ink: overflow_ink(state.disabled, lit),
        },
        icon_size: 0,
        state,
        sign: MenuSign::None,
        ground,
    }
}

/// A toolbar as `tool_bar_strip` draws it: its object name and title (a ToolBar target), whether
/// it runs down the window's side, its icon size, and its items.
#[derive(Clone, Debug, PartialEq)]
pub struct Strip {
    pub name: String,
    pub title: String,
    pub vertical: bool,
    pub icon_size: i32,
    pub items: Vec<StripItem>,
}

/// What a frame of a toolbar did.
#[derive(Debug, Default)]
pub struct StripResponse {
    /// The object name of each action fired: an action's button, a family's tool, an item of a
    /// family's menu.
    pub triggered: Vec<String>,
    /// The object names of the items hidden behind the arrow.
    pub hidden: Vec<String>,
    pub overflow: bool,
    pub expanded: bool,
    pub lit: bool,
    /// The status bar's line from the button under the pointer, or the empty line from the one it
    /// left.
    pub status_tip: Option<String>,
    /// How the overflow button was painted, when it was.
    pub overflow_paint: Option<ButtonPaint>,
    /// The rectangle of each item drawn on the bar (not those of the expansion), by object name.
    pub rects: Vec<(String, Rect)>,
}

/// Draws `strip` in `room` points along it from where `ui` is, as a ToolBar target: its items in a
/// row, those that do not fit hidden behind the overflow arrow (recorded, not visible, so a press
/// of one is refused "is hidden" as Qt's), the arrow lit while one of them shows a tool running,
/// and a click on the arrow expanding the bar beside it. `widget` draws a Widget item at its
/// rectangle and answers whether it was clicked.
pub fn tool_bar_strip(
    ui: &mut Ui,
    strip: &Strip,
    room: f32,
    widget: &mut dyn FnMut(&mut Ui, Rect, &str) -> bool,
) -> StripResponse {
    let ctx = ui.ctx().clone();
    let vertical = strip.vertical;
    let sizes: Vec<(i32, i32)> = strip
        .items
        .iter()
        .map(|item| item.size(strip.icon_size))
        .collect();
    let extents: Vec<i32> = sizes
        .iter()
        .map(|&(width, height)| if vertical { height } else { width })
        .collect();
    let separators: Vec<bool> = strip
        .items
        .iter()
        .map(|item| *item == StripItem::Separator)
        .collect();
    let breadth = sizes
        .iter()
        .map(|&(width, height)| if vertical { width } else { height })
        .max()
        .unwrap_or(0);
    let across = breadth + 2 * TOOL_BAR_PADDING_ACROSS;
    let room_pixels = room.floor() as i32;
    let layout = lay_out_strip(&extents, &separators, room_pixels);
    let size = if vertical {
        Vec2::new(across as f32, room)
    } else {
        Vec2::new(room, across as f32)
    };
    let (bar, _) = ui.allocate_exact_size(size, Sense::hover());
    let expanded_id = egui::Id::new(("katanaToolBarExpanded", strip.name.as_str()));
    let now = ctx.input(|input| input.time);
    let (expanded_before, toggled_by_act) = recall::<bool>(&ctx, expanded_id, now);
    let mut expanded = expanded_before;
    let mut pressed_by_act = None;
    if !layout.overflow {
        expanded = false;
    }
    let mut out = StripResponse {
        overflow: layout.overflow,
        ..StripResponse::default()
    };

    begin_target(
        ui,
        TargetSpec::new(&strip.name, TargetKind::ToolBar, &strip.title),
    );
    let painter = ui.painter();
    painter.rect_filled(bar, 0.0, palette::window().to_egui());
    let edge = if vertical {
        [bar.right_top(), bar.right_bottom()]
    } else {
        [bar.left_bottom(), bar.right_bottom()]
    };
    painter.line_segment(edge, egui::Stroke::new(1.0, palette::border().to_egui()));

    let place = |offset: i32, (width, height): (i32, i32)| -> Rect {
        let (x, y) = if vertical {
            (TOOL_BAR_PADDING_ACROSS + (breadth - width) / 2, offset)
        } else {
            (offset, TOOL_BAR_PADDING_ACROSS + (breadth - height) / 2)
        };
        Rect::from_min_size(
            bar.min + Vec2::new(x as f32, y as f32),
            Vec2::new(width as f32, height as f32),
        )
    };
    for (index, item) in strip.items.iter().enumerate() {
        let rect = place(layout.offsets[index], sizes[index]);
        if layout.shown[index] {
            draw_strip_item(ui, rect, item, strip, widget, &mut out);
            out.rects.push((String::from(item.name()), rect));
        } else if *item != StripItem::Separator {
            out.hidden.push(String::from(item.name()));
            if !expanded {
                record_hidden(ui, item);
            }
        }
    }

    if layout.overflow {
        out.lit = overflow_lit(&strip.items, &layout, expanded);
        let (width, height) = if vertical {
            (breadth, EXTENSION_EXTENT)
        } else {
            (EXTENSION_EXTENT, breadth)
        };
        let end = room_pixels - TOOL_BAR_PADDING_ALONG - EXTENSION_EXTENT;
        let rect = if vertical {
            Rect::from_min_size(
                bar.min + Vec2::new(TOOL_BAR_PADDING_ACROSS as f32, end as f32),
                Vec2::new(width as f32, height as f32),
            )
        } else {
            Rect::from_min_size(
                bar.min + Vec2::new(end as f32, TOOL_BAR_PADDING_ACROSS as f32),
                Vec2::new(width as f32, height as f32),
            )
        };
        let id = egui::Id::new(("katanaToolButton", strip.name.as_str(), OVERFLOW_BUTTON));
        let taken = take(ui, OVERFLOW_BUTTON);
        let response = ui.interact(rect, id, Sense::click());
        let mut toggled = response.clicked();
        if let Some(taken) = &taken {
            match &taken.posted.act {
                Act::Press => {
                    if accept_press(ui, taken) {
                        toggled = true;
                        pressed_by_act = Some(String::from(OVERFLOW_BUTTON));
                        settle(ui, taken, Ok(()));
                    }
                }
                Act::Fill(text) => {
                    if accept_fill(ui, taken) {
                        settle(ui, taken, Err(no_field(OVERFLOW_BUTTON, text)));
                    }
                }
                Act::ScrollTo => settle(ui, taken, Ok(())),
            }
        }
        if toggled_by_act.is_some() && pressed_by_act.is_none() {
            toggled = true;
            pressed_by_act.clone_from(&toggled_by_act);
        }
        if toggled {
            expanded = !expanded;
            out.lit = overflow_lit(&strip.items, &layout, expanded);
        }
        let state = ButtonState {
            hovered: response.hovered(),
            down: response.is_pointer_button_down_on(),
            checked: expanded,
            disabled: !ui.is_enabled(),
        };
        let paint = overflow_paint(width, height, vertical, state, out.lit, Color::TRANSPARENT);
        show_button(ui, rect, &paint);
        let mut record_of = ControlRecord::new(OVERFLOW_BUTTON, ControlKind::Toggle);
        record_of.checked = Some(expanded);
        record_of.enabled = ui.is_enabled();
        record_of.visible = ui.is_visible();
        record(ui, &response, record_of);
        out.overflow_paint = Some(paint);
        if expanded {
            // The hidden buttons, in a row of their own beside the arrow, as Qt expands a bar.
            let at = if vertical {
                rect.right_top() + Vec2::new(2.0, 0.0)
            } else {
                rect.left_bottom() + Vec2::new(0.0, 2.0)
            };
            let hidden: Vec<usize> = (0..strip.items.len())
                .filter(|&index| !layout.shown[index] && strip.items[index] != StripItem::Separator)
                .collect();
            egui::Area::new(egui::Id::new((
                "katanaToolBarExpansion",
                strip.name.as_str(),
            )))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(&ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    let mut offset = 0;
                    for &index in &hidden {
                        let (width, height) = sizes[index];
                        let origin = ui.min_rect().min;
                        let rect = if vertical {
                            Rect::from_min_size(
                                origin + Vec2::new(0.0, offset as f32),
                                Vec2::new(width as f32, height as f32),
                            )
                        } else {
                            Rect::from_min_size(
                                origin + Vec2::new(offset as f32, 0.0),
                                Vec2::new(width as f32, height as f32),
                            )
                        };
                        let _ = ui.allocate_rect(rect, Sense::hover());
                        draw_strip_item(ui, rect, &strip.items[index], strip, widget, &mut out);
                        offset += (if vertical { height } else { width }) + TOOL_BAR_SPACING;
                    }
                });
            });
        }
    }
    out.expanded = expanded;
    keep(
        &ctx,
        expanded_id,
        now,
        expanded_before,
        expanded,
        pressed_by_act,
    );
    end_target(ui, bar);
    out
}

/// One shown item of a toolbar, at `rect`.
fn draw_strip_item(
    ui: &mut Ui,
    rect: Rect,
    item: &StripItem,
    strip: &Strip,
    widget: &mut dyn FnMut(&mut Ui, Rect, &str) -> bool,
    out: &mut StripResponse,
) {
    let mut status = None;
    match item {
        StripItem::Action {
            action, tooltip, ..
        } => {
            let shown = tool_button_at(ui, rect, action, tooltip, strip.icon_size);
            if shown.triggered {
                out.triggered.push(action.name.clone());
            }
            status = shown.status_tip;
        }
        StripItem::Family(flyout) => {
            let shown = flyout_button_at(ui, rect, flyout, strip.icon_size, strip.vertical);
            if let Some(name) = shown.triggered {
                out.triggered.push(name);
            }
            status = shown.status_tip;
        }
        StripItem::Separator => {
            let line = if strip.vertical {
                let y = rect.center().y.floor() + 0.5;
                [
                    Pos2::new(rect.left() + 5.0, y),
                    Pos2::new(rect.right() - 5.0, y),
                ]
            } else {
                let x = rect.center().x.floor() + 0.5;
                [
                    Pos2::new(x, rect.top() + 4.0),
                    Pos2::new(x, rect.bottom() - 4.0),
                ]
            };
            ui.painter()
                .line_segment(line, egui::Stroke::new(1.0, palette::border().to_egui()));
        }
        StripItem::Widget { name, .. } => {
            if widget(ui, rect, name) {
                out.triggered.push(name.clone());
            }
        }
    }
    // The line of the button the pointer is on wins over the empty line of the one it left.
    match (&out.status_tip, status) {
        (Some(current), Some(line)) if !current.is_empty() && line.is_empty() => {}
        (_, Some(line)) => out.status_tip = Some(line),
        (_, None) => {}
    }
}

/// A hidden item of a toolbar, recorded so a driver finds it hidden (QWidget::isHidden): a press
/// of it is refused "is hidden", as the C++'s --press refuses a button its owner hid.
fn record_hidden(ui: &mut Ui, item: &StripItem) {
    let (name, text, checked) = match item {
        StripItem::Action { action, .. } => (&action.name, action.text.clone(), action.checked),
        StripItem::Family(flyout) => (
            &flyout.name,
            flyout.default.text.clone(),
            flyout.default.checked,
        ),
        StripItem::Separator | StripItem::Widget { .. } => return,
    };
    let id = egui::Id::new(("katanaToolButton", name.as_str()));
    let response = ui.interact(Rect::NOTHING, id, Sense::hover());
    if let Some(taken) = take(ui, name) {
        let sentence = match &taken.posted.act {
            Act::Press => Some(crate::drive::mailbox::press_hidden(&taken.posted)),
            Act::Fill(text) => Some(no_field(name, text)),
            Act::ScrollTo => None,
        };
        settle(ui, &taken, sentence.map_or(Ok(()), Err));
    }
    let mut record_of = ControlRecord::new(name, ControlKind::Toggle);
    record_of.text = text;
    record_of.checked = checked;
    record_of.visible = false;
    record_of.enabled = ui.is_enabled();
    record(ui, &response, record_of);
}
