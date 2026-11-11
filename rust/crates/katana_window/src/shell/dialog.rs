//! The dialog frame: keeping the window's own windows reachable on the screen they are shown on.
//! The port of screen_fit.hpp/.cpp, and the half of theme.cpp that makes a menu taller than the
//! screen one scrolling column.
//!
//! A dialog is laid out for the controls it holds, not for the screen it is shown on: Terrain >
//! Volumes was about 1550 pixels tall, the style manager and the road template manager are as
//! tall, and a 1366 x 768 laptop leaves a window 728 pixels of room under its task bar. A window
//! taller than that has its bottom - the Close button, the results - off the screen. Every dialog
//! that was written to fit is one more that a smaller screen then proves wrong, so this is not
//! left to each dialog:
//!
//! * A dialog is shown no larger than its screen's room: the available area less a margin.
//! * A dialog whose controls cannot be laid out in that room scrolls, so that every control is
//!   reachable by scrolling and the window is as large as the screen lets it be.
//! * The dialog's own buttons are not scrolled with the rest. Its footer - its row of buttons and
//!   what it says under them, the reply to what they did - is kept below the scrolling body. A
//!   footer that needs more than half the room is not kept out, and then everything scrolls.
//! * A dialog that fits is left exactly as it is drawn.
//! * A dialog placed by hand stays where it was put, brought back onto the screen if it was off
//!   it; one that was not is centred in the room.
//!
//! What is the C++'s and not here. In Qt the frame is fitted onto a dialog already laid out, so
//! the footer was FOUND, by reading the dialog's layout (its last row of buttons), and the fitting
//! was done once as the dialog was shown, by an application-wide filter. In egui a dialog is drawn
//! each frame inside its frame, so the footer is DECLARED by the dialog (the second part it gives
//! the frame), and the frame fits it at every frame. The eleven ScreenFit cases about finding a
//! footer in a Qt layout, the filter, and showing a dialog again are exempt for that (the plan's
//! section 3.3).
//!
//! THE MEASURE. The frame does not know a dialog's size before it is drawn, so it measures what it
//! drew: what the frame takes beyond the body (its title bar, borders, footer and spacing) is kept
//! from one frame to the next, and the body is given the room less that. A frame whose measure
//! changed asks egui to lay the frame out again at once (`Context::request_discard`), so the
//! dialog a person sees is the fitted one.
//!
//! Each dialog is a drive target named as its C++ object (drive::registry), its controls recorded
//! in its body or its footer, and its rectangle - the whole of it with its title bar - what a
//! screenshot of it crops to.

use egui::{
    Align, Align2, Area, Context, Frame, Id, Layout, Order, Pos2, Rect, ScrollArea, Sense, Ui,
    UiBuilder, UiKind, Vec2,
};

use crate::drive::registry::{
    Part, ScrollRecord, TargetKind, TargetSpec, begin_part, begin_target, end_part, end_target,
    set_target_frame,
};

/// What is kept free round a window on every side of its screen's available area, so that its
/// frame never touches the task bar or the edge of the display.
pub const MARGIN: f32 = 8.0;

/// How small a scrolling dialog may be made: a scroll area has no minimum of its own worth the
/// name, and a dialog the size of its title bar is no use to anyone. Held to the room there is, on
/// a screen smaller than this.
pub const LEAST_SCROLLING: Vec2 = Vec2::new(360.0, 240.0);

/// The most of the room (half, as a divisor) a footer may need and still be kept out of the
/// scrolling body: a Close row, a reply box and a status line are some 170 px of a 700 px room,
/// and more than half would leave too little to scroll the rest in.
pub const FOOTER_SHARE: f32 = 2.0;

/// The name of a dialog's scrolling body (the C++'s scroll area, kScreenFitScroll).
pub const SCREEN_FIT_SCROLL: &str = "screenFitScroll";

/// The name of a dialog's footer (kScreenFitFooter): the part its controls are recorded in.
pub const SCREEN_FIT_FOOTER: &str = "screenFitFooter";

/// The rectangle a window's frame may take on a screen whose available area (what a task bar and
/// a dock leave it) is `available`: that area less MARGIN on every side. An empty rectangle when
/// there is no screen to measure, which a caller takes as no limit.
pub fn room_on_screen(available: Rect) -> Rect {
    if !available.is_positive() {
        return Rect::NOTHING;
    }
    available.shrink(MARGIN)
}

/// What the dialog frame adds round a dialog's content, by `style`: its borders and margins, and
/// the title bar above (a row of an interactive control's height, and the spacing under it). Never
/// zero: a title bar is allowed for.
pub fn frame_of(style: &egui::Style) -> Vec2 {
    let margins = Frame::window(style).total_margin().sum();
    margins
        + Vec2::new(
            0.0,
            style.spacing.interact_size.y + style.spacing.item_spacing.y,
        )
}

/// What a dialog's frame drew this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct DialogShown<R> {
    /// What the dialog's body answered.
    pub body: R,
    /// The whole dialog, its title bar included, in points.
    pub rect: Rect,
    /// Whether its body scrolls: its controls do not fit the room.
    pub scrolls: bool,
    /// Whether the title bar's close button was clicked.
    pub closed: bool,
}

/// A dialog's frame and what it keeps between frames: its object name and title, whether it is
/// modal, where it was put by hand, and what it measured.
#[derive(Clone, Debug, PartialEq)]
pub struct DialogFrame {
    name: String,
    title: String,
    modal: bool,
    placed: Option<Pos2>,
    chrome: Option<Vec2>,
    footer_height: Option<f32>,
    scrolled: bool,
    rect: Option<Rect>,
}

impl DialogFrame {
    /// The frame of the dialog the C++ named `name`, titled `title`; not modal, centred.
    pub fn new(name: &str, title: &str) -> Self {
        Self {
            name: String::from(name),
            title: String::from(title),
            modal: false,
            placed: None,
            chrome: None,
            footer_height: None,
            scrolled: false,
            rect: None,
        }
    }

    /// The same frame, modal (QDialog::setModal): the rest of the window takes no input while it
    /// is shown. A headless run is never stopped by it.
    pub fn modal(mut self, modal: bool) -> Self {
        self.modal = modal;
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: &str) {
        self.title = String::from(title);
    }

    pub fn is_modal(&self) -> bool {
        self.modal
    }

    /// QWidget::move: the dialog put by hand at `at` (its top left), where it stays - brought back
    /// onto the screen if `at` is off it.
    pub fn place(&mut self, at: Pos2) {
        self.placed = Some(at);
    }

    /// Where it was put by hand, as the last frame kept it on the screen; None when it is centred.
    pub fn placed(&self) -> Option<Pos2> {
        self.placed
    }

    /// The whole dialog as the last frame drew it.
    pub fn rect(&self) -> Option<Rect> {
        self.rect
    }

    /// Draws the dialog in its frame: `body`, which scrolls when the dialog does not fit, and
    /// `footer` - its button row and what it says under it - kept below the body, both lent
    /// `state` (the dialog's model) in turn.
    pub fn show<T: ?Sized, R>(
        &mut self,
        ctx: &Context,
        state: &mut T,
        body: impl FnOnce(&mut Ui, &mut T) -> R,
        footer: impl FnOnce(&mut Ui, &mut T),
    ) -> DialogShown<R> {
        self.show_parts(ctx, state, body, Some(Box::new(footer)))
    }

    /// Draws a dialog with no footer: everything it holds is its body, which scrolls.
    pub fn show_body<T: ?Sized, R>(
        &mut self,
        ctx: &Context,
        state: &mut T,
        body: impl FnOnce(&mut Ui, &mut T) -> R,
    ) -> DialogShown<R> {
        self.show_parts(ctx, state, body, None)
    }

    #[allow(
        clippy::type_complexity,
        reason = "the footer is the one optional closure of show and show_body"
    )]
    fn show_parts<T: ?Sized, R>(
        &mut self,
        ctx: &Context,
        state: &mut T,
        body: impl FnOnce(&mut Ui, &mut T) -> R,
        footer: Option<Box<dyn FnOnce(&mut Ui, &mut T) + '_>>,
    ) -> DialogShown<R> {
        let room = room_on_screen(ctx.content_rect());
        let style = ctx.global_style();
        let frame_size = frame_of(&style);
        let client = (room.size() - frame_size).max(Vec2::ZERO);
        // The frame beyond the body, as the last frame measured it; the frame alone at first.
        let chrome = self.chrome.unwrap_or(frame_size);
        let body_room = (room.size() - chrome).max(Vec2::ZERO);
        let footer_in_body = footer.is_some()
            && self
                .footer_height
                .is_some_and(|height| height > client.y / FOOTER_SHARE);
        // A dialog that scrolls is kept no smaller than the least a scrolling one may be.
        let least_body = if self.scrolled {
            (Vec2::new(
                LEAST_SCROLLING.x.min(client.x),
                LEAST_SCROLLING.y.min(client.y),
            ) - chrome)
                .max(Vec2::ZERO)
        } else {
            Vec2::ZERO
        };

        let id = Id::new(("katanaDialog", self.name.as_str()));
        let order = if self.modal {
            Order::Foreground
        } else {
            Order::Middle
        };
        let mut area = Area::new(id)
            .kind(UiKind::Window)
            .order(order)
            .constrain_to(room)
            .movable(false)
            .fade_in(false);
        area = match self.placed {
            Some(at) => area.pivot(Align2::LEFT_TOP).current_pos(at),
            None => area.pivot(Align2::CENTER_CENTER).current_pos(room.center()),
        };
        if self.modal {
            let layer = area.layer();
            ctx.memory_mut(|memory| memory.set_modal_layer(layer));
        }
        let spec = TargetSpec::new(&self.name, TargetKind::Dialog, &self.title).modal(self.modal);
        let title = self.title.clone();
        let mut footer = footer;

        let shown = area.show(ctx, |ui| {
            let origin = ui.max_rect().min;
            begin_target(ui, spec);
            let framed = Frame::window(ui.style()).show(ui, |ui| {
                let title_row = ui.horizontal(|ui| {
                    ui.strong(title.as_str());
                    ui.add_space(12.0);
                    ui.add(egui::Button::new("\u{d7}").small().frame(false))
                        .clicked()
                });
                let drag = ui.interact(title_row.response.rect, id.with("title"), Sense::drag());
                let moved = drag.dragged().then(|| origin + drag.drag_delta());

                begin_part(ui, Part::Body);
                let body_rect = Rect::from_min_size(ui.cursor().min, body_room);
                let scope = ui.scope_builder(UiBuilder::new().max_rect(body_rect), |ui| {
                    ScrollArea::both()
                        .id_salt(SCREEN_FIT_SCROLL)
                        .max_width(body_room.x)
                        .max_height(body_room.y)
                        .min_scrolled_width(least_body.x)
                        .min_scrolled_height(least_body.y)
                        .auto_shrink([true, true])
                        .animated(false)
                        .show(ui, |ui| {
                            let answer = body(ui, &mut *state);
                            if footer_in_body && let Some(footer) = footer.take() {
                                footer(ui, &mut *state);
                            }
                            answer
                        })
                });
                end_part(ui);

                let mut footer_rect = None;
                if let Some(footer) = footer.take() {
                    begin_part(ui, Part::Footer);
                    let drawn = ui.vertical(|ui| footer(ui, &mut *state));
                    end_part(ui);
                    footer_rect = Some(drawn.response.rect);
                }
                (
                    title_row.inner,
                    moved,
                    scope.response.rect,
                    scope.inner,
                    footer_rect,
                )
            });
            let (closed, moved, body_outer, output, footer_rect) = framed.inner;
            let frame_rect = framed.response.rect;
            let scroll = ScrollRecord {
                viewport: output.inner_rect,
                content: output.content_size,
                offset: output.state.offset,
            };
            let scrolls = scroll.scrolls();
            set_target_frame(
                ui,
                scrolls.then_some(scroll),
                footer_rect.filter(|_| scrolls && !footer_in_body),
            );
            end_target(ui, frame_rect);
            (
                closed,
                moved,
                body_outer,
                output.inner,
                scrolls,
                footer_rect,
                frame_rect,
            )
        });

        let (closed, moved, body_outer, answer, scrolls, footer_rect, frame_rect) = shown.inner;
        // What the frame took beyond the body: a change is laid out again at once.
        let chrome_now = frame_rect.size() - body_outer.size();
        let measured_again = self
            .chrome
            .is_none_or(|before| (before - chrome_now).length() > 0.5);
        self.chrome = Some(chrome_now);
        // A footer drawn in the body this frame was not measured apart: its height stays.
        if !footer_in_body {
            self.footer_height = footer_rect.map(|rect| rect.height());
        }
        if measured_again || self.scrolled != scrolls {
            ctx.request_discard("the dialog frame measured what it holds");
        }
        self.scrolled = scrolls;
        if let Some(at) = moved {
            self.placed = Some(at);
        } else if self.placed.is_some() {
            // Kept where the screen brought it back to, as QWidget::move keeps it.
            self.placed = Some(shown.response.rect.min);
        }
        self.rect = Some(frame_rect);
        DialogShown {
            body: answer,
            rect: frame_rect,
            scrolls,
            closed,
        }
    }
}

/// What a popup menu drew.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuShown {
    /// The whole menu, in points.
    pub rect: Rect,
    /// Its column of items: scrolled when the menu does not fit the screen.
    pub scroll: ScrollRecord,
}

/// A menu's items in ONE column the height of the room there is at most, scrolled when they need
/// more (theme.cpp's SH_Menu_Scrollable): Fusion laid a menu taller than its screen out in several
/// columns side by side, which a menu of sections and submenus turns into a wall nobody can read
/// down. For a menu's contents wherever a menu is drawn; the wheel and the keys scroll it.
pub fn menu_column(ui: &mut Ui, add: impl FnOnce(&mut Ui)) -> ScrollRecord {
    let screen = ui.ctx().content_rect();
    let frame = Frame::menu(ui.style()).total_margin().sum();
    let height = (screen.height() - frame.y).max(0.0);
    let output = ScrollArea::vertical()
        .id_salt("katanaMenuColumn")
        .max_height(height)
        .auto_shrink([true, true])
        .animated(false)
        .show(ui, |ui| {
            ui.with_layout(Layout::top_down_justified(Align::LEFT), add);
        });
    ScrollRecord {
        viewport: output.inner_rect,
        content: output.content_size,
        offset: output.state.offset,
    }
}

/// A menu popped up at `at` (QMenu::popup): its items drawn by `add` in one column
/// (`menu_column`), the menu kept on the screen, and a Menu target named `name` whose record
/// carries the column's scroll, scrolled or not.
pub fn popup_menu(ctx: &Context, name: &str, at: Pos2, add: impl FnOnce(&mut Ui)) -> MenuShown {
    let screen = ctx.content_rect();
    let area = Area::new(Id::new(("katanaMenu", name)))
        .kind(UiKind::Menu)
        .order(Order::Foreground)
        .constrain_to(screen)
        .movable(false)
        .fade_in(false)
        .pivot(Align2::LEFT_TOP)
        .current_pos(at);
    let spec = TargetSpec::new(name, TargetKind::Menu, "");
    let shown = area.show(ctx, |ui| {
        begin_target(ui, spec);
        let framed = Frame::menu(ui.style()).show(ui, |ui| menu_column(ui, add));
        let rect = framed.response.rect;
        set_target_frame(ui, Some(framed.inner), None);
        end_target(ui, rect);
        MenuShown {
            rect,
            scroll: framed.inner,
        }
    });
    shown.inner
}
