//! The main window's keyboard shortcuts: each key press of a frame offered to the window's
//! actions, and the action it reaches triggered exactly as a click on its menu item triggers it
//! (`MainWindow::trigger`, after the frame is drawn) - one path, logged and undoable as a click
//! is. The menus have always printed each action's key; until this pass nothing read the
//! keyboard for them, and Ctrl+S, Ctrl+Z and F9 did nothing.
//!
//! THE TABLE is the frame's own rows - the menus' actions and then the toolbars', the order of
//! `Composed::actions` - read as they stand when the key is pressed: an action a workbench or a
//! menu gains is reached by its key with nothing added here, and one that is disabled leaves
//! its key alone. The panels' own actions (`panel_actions`, the Properties tree's Copy) are not
//! in it: a panel answers its own key while it has the keyboard.
//!
//! WHEN IT RUNS. Before anything of the frame that would take a key is drawn. egui hands a
//! frame's key events to every widget that asks for them and takes none away on its own account,
//! so a key that is an action's is removed from the frame's events here: a view that has the
//! keyboard never sees the P of Ctrl+P, and the command line's field never undoes its own text
//! on the Ctrl+Z that undid the drawing. Run after the widgets, the pass could only add an
//! action to what a widget had already done with the key.
//!
//! WHO HAS THE KEY, in the order asked (docs/desktop.md, "A key reaches its action"):
//!
//! ```text
//! 1. something in front      a modal dialog, an open menu, flyout or popup list, or the sheet
//!                            editor shown over the window: the keyboard is its, and no action
//!                            of the main window runs
//! 2. a text box              keeps the keys that type and edit in it; the command line's field
//!                            keeps only the plain keys, since it is the drawing's keyboard
//! 3. the plan view           asked whether it claims the key (PlanView::shortcut_override):
//!                            Ctrl+Z while a tool runs, Delete on a hot vertex or in a vertex
//!                            tool, Escape while a point pick waits
//! 4. the action              when it is enabled; a disabled action leaves the key where it was
//! ```
//!
//! What the pass keeps between frames - which widget had the keyboard when the last pass ended,
//! and the actions the last press reached - is on the egui context, in a plugin, as the drive
//! registry's records are: the window's own state gained no field for it.

use egui::{Context, Event, Id, Key, KeyboardShortcut, LayerId, Modifiers, Order, Ui};
use katana_cad::view_set::ViewId;

use super::actions::Composed;
use super::window::MainWindow;
use crate::drive::action::{Action, Menu};
use crate::drive::registry::{ControlKind, snapshot};
use crate::sheet_editor::SheetEditor;
use crate::tools::flyout_button::expansion_layer;
use crate::viewport_widget::{KeyModifiers, ViewKey};

/// The command line's field, by the name it is drawn under (main_window::panels).
const COMMAND_INPUT: &str = "commandInput";

/// What a plan view's name starts with, its id after it (`PlanView::name`).
const PLAN_VIEW: &str = "PlanView";

/// The actions a held key runs again at each of the system's repeats: stepping back and forth
/// through the history is what holding Ctrl+Z is for. Every other action runs once a press
/// however long the key is held - a toggle (the grid, Ortho, the projection) would end where
/// the repeats happened to stop, and New, Save or a dialog's action has nothing to do twice.
const REPEATING: [&str; 2] = ["editUndo", "editRedo"];

/// One key press of the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Press {
    key: Key,
    modifiers: Modifiers,
    /// egui's mark of a press the system sent again while the key was held.
    repeat: bool,
}

impl Press {
    /// Whether `event` is this press: the event taken from the frame when the press is an
    /// action's.
    fn is(&self, event: &Event) -> bool {
        matches!(
            event,
            Event::Key { key, pressed: true, modifiers, .. }
                if *key == self.key && *modifiers == self.modifiers
        )
    }
}

/// The frame's key presses, in the order they came.
fn presses(events: &[Event]) -> Vec<Press> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Key {
                key,
                pressed: true,
                repeat,
                modifiers,
                ..
            } => Some(Press {
                key: *key,
                modifiers: *modifiers,
                repeat: *repeat,
            }),
            _ => None,
        })
        .collect()
}

/// Whether `key` pressed with `held` is the sequence `keys`: egui's own rule for a shortcut
/// (`Modifiers::matches_logically`, what `consume_shortcut` and the sheet editor's pass use), so
/// Ctrl is Cmd on a Mac and a Shift the layout needs to reach the key does not unmake the
/// sequence - but with Alt held exactly as the sequence says. Ctrl+Alt is how Windows reports
/// AltGr, which types a character (the plan view's `is_typed_text` has the same exemption):
/// read loosely, AltGr+E typed into the command line would also have zoomed to the extents.
fn pressed_as(keys: &KeyboardShortcut, key: Key, held: Modifiers) -> bool {
    key == keys.logical_key
        && held.alt == keys.modifiers.alt
        && held.matches_logically(keys.modifiers)
}

/// Every action of `composed` that has a key: the menus' and then the toolbars', the order of
/// `Composed::actions`, so of an action shown twice the first is the one a trigger by its name
/// finds. Read where the rows are, without copying the table: a frame in which a letter is
/// typed pays for no more than this.
fn with_keys(composed: &Composed) -> Vec<&Action> {
    composed
        .menus
        .iter()
        .flat_map(Menu::actions)
        .chain(composed.bars.iter().flat_map(|(bar, _)| bar.actions()))
        .filter(|action| action.shortcut.is_some() && !action.name.is_empty())
        .collect()
}

/// The action of `actions` that `key` pressed with `held` reaches, enabled or not: the one
/// whose sequence names Shift before the one whose sequence leaves it out, since Ctrl+Shift+S
/// also reads as Ctrl+S under egui's rule and must never save where Save As was meant.
/// `--check-shortcuts` holds the table to one action a sequence; were two to share one, the
/// first in the menus' order has it.
fn reached<'a>(actions: &[&'a Action], key: Key, held: Modifiers) -> Option<&'a Action> {
    for shift in [true, false] {
        let found = actions.iter().copied().find(|action| {
            action
                .shortcut
                .is_some_and(|keys| keys.modifiers.shift == shift && pressed_as(&keys, key, held))
        });
        if found.is_some() {
            return found;
        }
    }
    None
}

/// A sequence held with Ctrl (Cmd on a Mac) or Alt, which types nothing.
fn chord(modifiers: Modifiers) -> bool {
    modifiers.command || modifiers.ctrl || modifiers.mac_cmd || modifiers.alt
}

/// F1 to F35, egui's first and last function keys: they type nothing into a text box.
fn function_key(key: Key) -> bool {
    (Key::F1..=Key::F35).contains(&key)
}

/// A sequence a text box acts on by itself: a key with no Ctrl or Alt that is not a function
/// key - Delete, Escape, and any letter an action may one day take.
fn plain(keys: &KeyboardShortcut) -> bool {
    !chord(keys.modifiers) && !function_key(keys.logical_key)
}

/// The keys that move the caret and delete in a text box whatever is held with them (egui's
/// text edit: Ctrl+Left a word, Ctrl+Delete a word, Shift+Home a selection).
fn moves_or_deletes(key: Key) -> bool {
    matches!(
        key,
        Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::Home
            | Key::End
            | Key::Backspace
            | Key::Delete
    )
}

/// Select all, copy, paste, cut, redo and undo: the chords egui's text edit answers on every
/// system. (Its Ctrl+H, K, U and W - delete a character, a line, a word - are a terminal's, not
/// what a person expects of a field in a window, and are the window's actions' when they have
/// them: Ctrl+K cuts a section.)
fn edits_text(keys: &KeyboardShortcut) -> bool {
    let held = keys.modifiers;
    (held.command || held.ctrl || held.mac_cmd)
        && !held.alt
        && matches!(
            keys.logical_key,
            Key::A | Key::C | Key::V | Key::X | Key::Y | Key::Z
        )
}

/// What has the keyboard when a key is pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Holder {
    /// No widget.
    Nobody,
    /// The command line's field.
    CommandLine,
    /// Any other text box: a dialog's field, a panel's filter, a number being typed.
    TextBox,
    /// A plan view.
    PlanView(ViewId),
    /// Anything else: a 3D view, a section, a button reached with Tab.
    Other,
}

impl Holder {
    /// Whether the widget keeps the sequence `keys` from the window's actions.
    ///
    /// A text box keeps what types and edits in it. The command line's field is the drawing's
    /// keyboard - what is typed anywhere goes to it, and Enter in it is Enter in the drawing -
    /// so it keeps only the plain keys: Ctrl+Z there undoes the drawing, as in any CAD program,
    /// not the last word typed, and Ctrl+A selects the drawing. And of the plain keys not
    /// Delete while it holds no text: there is nothing in it for Delete to take, the Edit menu
    /// shows Delete beside Erase Selection, and after a typed SELECT the field is where the
    /// keyboard still is.
    fn keeps(self, keys: &KeyboardShortcut, command_line_empty: bool) -> bool {
        match self {
            Self::TextBox => {
                plain(keys) || moves_or_deletes(keys.logical_key) || edits_text(keys)
            }
            Self::CommandLine => {
                plain(keys) && !(keys.logical_key == Key::Delete && command_line_empty)
            }
            Self::Nobody | Self::PlanView(_) | Self::Other => false,
        }
    }
}

/// The key as a plan view tells keys apart, for the three it may claim from the window's
/// actions (`PlanView::shortcut_override`); None for a key it never claims.
fn view_key(key: Key) -> Option<ViewKey> {
    match key {
        Key::Z => Some(ViewKey::Z),
        Key::Delete => Some(ViewKey::Delete),
        Key::Escape => Some(ViewKey::Escape),
        _ => None,
    }
}

/// What the pass keeps on the egui context between frames (see the module comment).
#[derive(Debug, Default)]
struct Keyboard {
    /// The widget that had the keyboard when the last pass ended, and whether it is a text box.
    held: Option<Id>,
    held_text: bool,
    /// The actions the last key press reached (`taken`).
    taken: Vec<String>,
}

impl egui::Plugin for Keyboard {
    fn debug_name(&self) -> &'static str {
        "katana_window::main_window::shortcuts"
    }

    /// Read as the pass ends rather than when the next begins: Escape takes the keyboard from a
    /// text box before a frame's first widget is drawn (egui's focus, at the start of a pass),
    /// and a click that gave a box the keyboard late in one frame may be followed by Escape in
    /// the very next, with no frame drawn between.
    fn on_end_pass(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx();
        self.held = ctx.memory(|memory| memory.focused());
        self.held_text = self.held.is_some() && ctx.text_edit_focused();
    }
}

/// The plugin put on `ctx`, once.
fn install(ctx: &Context) {
    if ctx.plugin_opt::<Keyboard>().is_none() {
        ctx.add_plugin(Keyboard::default());
    }
}

/// The actions the last key press reached on `ctx`, in the order queued; none when the press
/// was something else's - a text box's, a view's, a dialog's - or its action was disabled. For
/// a driver and the tests: the command log says what an action did, not which key asked.
pub fn taken(ctx: &Context) -> Vec<String> {
    ctx.with_plugin::<Keyboard, _>(|keyboard| keyboard.taken.clone())
        .unwrap_or_default()
}

/// What widget `id` is, `text` saying whether it is a text box: by the name the last frame
/// recorded it under (the drive registry, which the desktop program installs too).
fn classify(ctx: &Context, id: Option<Id>, text: bool) -> Holder {
    let Some(id) = id else {
        return Holder::Nobody;
    };
    let frame = snapshot(ctx);
    let record = frame.controls.iter().find(|control| control.id == id);
    match record {
        Some(record) if record.name == COMMAND_INPUT => Holder::CommandLine,
        _ if text => Holder::TextBox,
        Some(record) if record.kind == ControlKind::View => record
            .name
            .strip_prefix(PLAN_VIEW)
            .and_then(|rest| rest.parse::<ViewId>().ok())
            .map_or(Holder::Other, Holder::PlanView),
        _ => Holder::Other,
    }
}

/// What has the keyboard now, and what had it for an Escape pressed in this frame.
///
/// They differ for a text box alone: Escape has already taken the keyboard from it, and the
/// press was made in the box - it leaves the field, as it always has, and the command line's
/// field answers it itself (it clears what is typed and cancels in the drawing). Deselect on
/// top of that would cancel twice at one press.
fn holders(ctx: &Context) -> (Holder, Holder) {
    let focused = ctx.memory(|memory| memory.focused());
    let now = classify(ctx, focused, focused.is_some() && ctx.text_edit_focused());
    if focused.is_some() {
        return (now, now);
    }
    let (held, text) = ctx
        .with_plugin::<Keyboard, _>(|keyboard| (keyboard.held, keyboard.held_text))
        .unwrap_or((None, false));
    let at_escape = match classify(ctx, held, text) {
        holder @ (Holder::CommandLine | Holder::TextBox) => holder,
        Holder::Nobody | Holder::PlanView(_) | Holder::Other => Holder::Nobody,
    };
    (now, at_escape)
}

impl MainWindow {
    /// The frame's key presses offered to the actions of `composed` (see the module comment):
    /// each action reached is queued with the frame's clicks, and its press taken from the
    /// frame's events.
    pub(crate) fn shortcut_pass(&mut self, ui: &Ui, composed: &Composed) {
        let ctx = ui.ctx().clone();
        install(&ctx);
        let pressed = ctx.input(|input| presses(&input.events));
        // Nearly every frame: nothing below is paid for without a key.
        if pressed.is_empty() {
            return;
        }
        let table = with_keys(composed);
        let asked: Vec<(Press, &Action, KeyboardShortcut)> = pressed
            .iter()
            .filter_map(|press| {
                let action = reached(&table, press.key, press.modifiers)?;
                Some((*press, action, action.shortcut?))
            })
            .collect();
        let mut taken = Vec::new();
        // A letter typed, an arrow, Enter: no action's key, and nothing more to find out.
        if !asked.is_empty() && !self.keyboard_is_in_front(&ctx, composed) {
            let (now, at_escape) = holders(&ctx);
            let command_line_empty = self.ui.panels.command_text.is_empty();
            let mut consumed: Vec<Press> = Vec::new();
            for (press, action, keys) in asked {
                let holder = if press.key == Key::Escape {
                    at_escape
                } else {
                    now
                };
                if holder.keeps(&keys, command_line_empty) {
                    continue;
                }
                if let Some(key) = view_key(press.key)
                    && let Some((view, has_keyboard)) = self.offered_view(holder)
                {
                    let held = KeyModifiers::from_egui(press.modifiers);
                    if self.view_claims(view, key, held) {
                        // A view with the keyboard reads the press itself when it is drawn. One
                        // without it never would, so it is handed the key here and the press is
                        // taken: Ctrl+Z on the command line steps a tool back as it does over
                        // the view.
                        if !has_keyboard {
                            self.press_in_view(view, key, held);
                            consumed.push(press);
                        }
                        continue;
                    }
                }
                if !action.enabled {
                    continue;
                }
                if !press.repeat || REPEATING.contains(&action.name.as_str()) {
                    self.ui.triggered.push(action.name.clone());
                    taken.push(action.name.clone());
                }
                // A repeat of a key that runs once is still that action's key, and reaches
                // nothing else while it is held.
                consumed.push(press);
            }
            if !consumed.is_empty() {
                ctx.input_mut(|input| {
                    input
                        .events
                        .retain(|event| !consumed.iter().any(|press| press.is(event)));
                });
            }
        }
        ctx.with_plugin::<Keyboard, _>(|keyboard| keyboard.taken = taken);
    }

    /// Whether the keyboard belongs to something in front of the window, so that no action of
    /// the window answers a key.
    ///
    /// The sheet editor shown is the window in front, drawn over the whole screen, and its own
    /// pass takes its keys: this one running too would undo twice at one Ctrl+Z. A modal dialog
    /// stops the window as it stops the pointer (egui's modal layer, which the dialog frame
    /// sets). And whatever else is drawn above the dialogs - an open menu, a toolbar's flyout,
    /// a view's Layers popup, a context menu - closes on Escape and is what a key is meant for
    /// while it is open: they are egui's Foreground areas, read as the last frame drew them,
    /// since this frame has drawn none yet. But for a toolbar's expansion, the row of the
    /// buttons its room hides: it is drawn there too and stays until its arrow is clicked
    /// again, and the keys would have been dead for as long as a bar stood expanded.
    fn keyboard_is_in_front(&self, ctx: &Context, composed: &Composed) -> bool {
        if self
            .ui
            .dialogs
            .sheets
            .as_ref()
            .is_some_and(SheetEditor::is_visible)
        {
            return true;
        }
        let expansions: Vec<LayerId> = composed
            .bars
            .iter()
            .map(|(bar, _)| expansion_layer(&bar.name))
            .collect();
        ctx.memory(|memory| {
            memory.top_modal_layer().is_some()
                || memory.areas().visible_layer_ids().iter().any(|layer| {
                    layer.order == Order::Foreground && !expansions.contains(layer)
                })
        })
    }

    /// The plan view a key is offered to before an action may have it, and whether that view
    /// has the keyboard: the one that has it, else the one a typed line goes to (the view
    /// running a tool, else the active plan view). The command line is the drawing's keyboard,
    /// so Ctrl+Z pressed there while a tool runs is the tool's as it is over the view, and
    /// Delete with a vertex hot takes the vertex. A text box is offered nothing: the keys a view
    /// claims are the box's own.
    fn offered_view(&self, holder: Holder) -> Option<(ViewId, bool)> {
        match holder {
            Holder::PlanView(view) => Some((view, true)),
            Holder::TextBox => None,
            Holder::Nobody | Holder::CommandLine | Holder::Other => self
                .exec
                .workspace
                .command_target_view()
                .map(|view| (view, false)),
        }
    }

    /// Whether plan view `view` claims `key` held with `held` from the window's actions.
    fn view_claims(&mut self, view: ViewId, key: ViewKey, held: KeyModifiers) -> bool {
        self.exec
            .with_parts(|workspace, parts| {
                workspace.with_plan_view_lent(parts, view, |plan, state, cx| {
                    plan.shortcut_override(&*cx, state, key, held)
                })
            })
            .unwrap_or(false)
    }

    /// `key` pressed in plan view `view`, as the view takes a key it has the keyboard for. What
    /// it raises - a prompt, a tool's message - is the frame's, with the views' other events.
    fn press_in_view(&mut self, view: ViewId, key: ViewKey, held: KeyModifiers) {
        self.exec.with_parts(|workspace, parts| {
            let _ = workspace.with_plan_view_lent(parts, view, |plan, state, cx| {
                plan.key_press(cx, state, key, held, "")
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use egui::{Key, KeyboardShortcut, Modifiers};

    use super::{Holder, plain, pressed_as, reached};
    use crate::drive::action::Action;

    /// Ctrl as winit reports it off a Mac: the Ctrl key, which is also egui's "command".
    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        }
    }

    fn sequence(modifiers: Modifiers, key: Key) -> KeyboardShortcut {
        KeyboardShortcut::new(modifiers, key)
    }

    fn save_and_save_as() -> Vec<Action> {
        vec![
            Action::new("fileSave", "&Save").shortcut(sequence(Modifiers::COMMAND, Key::S)),
            Action::new("fileSaveAs", "Save &As...")
                .shortcut(sequence(Modifiers::COMMAND | Modifiers::SHIFT, Key::S)),
            Action::new("viewGrid", "&Grid").shortcut(sequence(Modifiers::NONE, Key::F7)),
        ]
    }

    fn name_reached(actions: &[Action], key: Key, held: Modifiers) -> Option<&str> {
        let table: Vec<&Action> = actions.iter().collect();
        reached(&table, key, held).map(|action| action.name.as_str())
    }

    #[test]
    fn a_sequence_with_shift_is_reached_before_the_one_that_leaves_shift_out() {
        let actions = save_and_save_as();
        assert_eq!(name_reached(&actions, Key::S, ctrl()), Some("fileSave"));
        assert_eq!(
            name_reached(&actions, Key::S, ctrl() | Modifiers::SHIFT),
            Some("fileSaveAs")
        );
        // The table's order does not decide it.
        let reversed: Vec<Action> = actions.iter().rev().cloned().collect();
        assert_eq!(name_reached(&reversed, Key::S, ctrl()), Some("fileSave"));
        assert_eq!(
            name_reached(&reversed, Key::S, ctrl() | Modifiers::SHIFT),
            Some("fileSaveAs")
        );
    }

    #[test]
    fn a_letter_with_no_ctrl_and_a_key_no_action_has_reach_nothing() {
        let actions = save_and_save_as();
        assert_eq!(name_reached(&actions, Key::S, Modifiers::NONE), None);
        assert_eq!(name_reached(&actions, Key::S, Modifiers::SHIFT), None);
        assert_eq!(name_reached(&actions, Key::F8, Modifiers::NONE), None);
        assert_eq!(name_reached(&actions, Key::F7, ctrl()), None);
        assert_eq!(name_reached(&[], Key::S, ctrl()), None);
    }

    // AltGr is Ctrl+Alt on Windows and types a character: AltGr+S is no Ctrl+S.
    #[test]
    fn a_key_typed_with_alt_gr_is_no_shortcut() {
        let save = sequence(Modifiers::COMMAND, Key::S);
        assert!(pressed_as(&save, Key::S, ctrl()));
        assert!(!pressed_as(&save, Key::S, ctrl() | Modifiers::ALT));
        // A sequence that names Alt is pressed with it, and only with it.
        let with_alt = sequence(Modifiers::COMMAND | Modifiers::ALT, Key::S);
        assert!(pressed_as(&with_alt, Key::S, ctrl() | Modifiers::ALT));
        assert!(!pressed_as(&with_alt, Key::S, ctrl()));
    }

    #[test]
    fn a_plain_key_is_one_with_no_ctrl_or_alt_that_is_not_a_function_key() {
        assert!(plain(&sequence(Modifiers::NONE, Key::Delete)));
        assert!(plain(&sequence(Modifiers::NONE, Key::Escape)));
        assert!(plain(&sequence(Modifiers::SHIFT, Key::Home)));
        assert!(!plain(&sequence(Modifiers::NONE, Key::F1)));
        assert!(!plain(&sequence(Modifiers::NONE, Key::F35)));
        assert!(!plain(&sequence(Modifiers::COMMAND, Key::S)));
        assert!(!plain(&sequence(Modifiers::ALT, Key::S)));
    }

    #[test]
    fn a_text_box_keeps_what_types_and_edits_and_the_command_line_only_the_plain_keys() {
        let delete = sequence(Modifiers::NONE, Key::Delete);
        let escape = sequence(Modifiers::NONE, Key::Escape);
        let undo = sequence(Modifiers::COMMAND, Key::Z);
        let select_all = sequence(Modifiers::COMMAND, Key::A);
        let save = sequence(Modifiers::COMMAND, Key::S);
        let grid = sequence(Modifiers::NONE, Key::F7);

        for kept in [delete, escape, undo, select_all] {
            assert!(Holder::TextBox.keeps(&kept, true), "{kept:?}");
            assert!(Holder::TextBox.keeps(&kept, false), "{kept:?}");
        }
        for passed in [save, grid] {
            assert!(!Holder::TextBox.keeps(&passed, true), "{passed:?}");
        }

        // The command line: Escape always; Delete while it holds text; no chord and no function
        // key.
        assert!(Holder::CommandLine.keeps(&escape, true));
        assert!(Holder::CommandLine.keeps(&escape, false));
        assert!(Holder::CommandLine.keeps(&delete, false));
        assert!(!Holder::CommandLine.keeps(&delete, true));
        for passed in [undo, select_all, save, grid] {
            assert!(!Holder::CommandLine.keeps(&passed, false), "{passed:?}");
        }

        // A view, another widget and nothing keep no key this way: a view is asked what it
        // claims.
        for holder in [Holder::Nobody, Holder::PlanView(1), Holder::Other] {
            for passed in [delete, escape, undo, save, grid] {
                assert!(!holder.keeps(&passed, true), "{holder:?} {passed:?}");
            }
        }
    }
}
