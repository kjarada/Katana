//! The plan view's shortcut menu: a right-click with no tool running (docs/desktop.md, "The plan
//! view's shortcut menu"). The port of plan_context_menu.
//!
//! It holds no commands of its own. Its items are the window's existing actions, found by object
//! name - Erase is editErase, Move is the tool modify.move - so a menu item, its toolbar button
//! and a headless trigger are one action and cannot drift; an action the window does not have (as
//! in a test's window of a few actions) is left out rather than faked. What it adds is the verb
//! lines a person would otherwise type - CHLAYER, STYLE APPLY, COLOR, INFO - which it hands to the
//! window's one executor (host::command_runner), so each is echoed in the log and is one undo
//! step.
//!
//! Its own items, by object name:
//!
//! ```text
//!   planContextLayer                 Put on Layer, a submenu in the layer tree
//!   planContextLayer.<layer path>    CHLAYER <layer path>, quoted when it must be
//!   planContextStyle                 Style, a submenu
//!   planContextStyle.ByLayer         STYLE APPLY -
//!   planContextStyle.<name>          STYLE APPLY <name>, quoted when it must be
//!   planContextColour                Colour, a submenu
//!   planContextColour.ByLayer        COLOR BYLAYER
//!   planContextColour.Choose         COLOR #RRGGBB, the colour asked for
//!   planContextInfo                  INFO #<id>, for one entity
//!   planContextRepeat                the last tool again, with nothing selected
//! ```
//!
//! Opened on a polyline's grip, the grip's own items come first, under a section naming it
//! (docs/drawing.md, "Grips"):
//!
//! ```text
//!   planContextVertex.Delete         VERTEX DELETE <id> <v>
//!   planContextVertex.InsertAfter    VERTEX INSERT <id> #<id>.s<v> after=<v>
//!   planContextVertex.Start          STARTVERTEX <id> <v>, a closed polyline's
//!   planContextVertex.Move|Height|Fillet|Chamfer|Straighten
//!                                    the tool, the vertex its handle
//!   planContextSegment.AddMiddle     VERTEX INSERT <id> #<id>.s<s> after=<s>
//!   planContextSegment.Insert|Arc    the tool, the segment its handle
//!   planContextSegment.Line          VERTEX SET <id> <s> bulge=0, an arc's
//! ```
//!
//! The Rust shape. The C++ menu is a QMenu whose items hold copies of the runner, the colour
//! chooser and the tool starter, and run them when triggered. Here the menu is rows
//! (drive::action's Menu), built for the selection as it is, and each item of its own has a
//! CHOICE - the line to run, the window's action to trigger, the colour to ask for, the tool to
//! start on the grip. `trigger` runs a line through the host at once, as the C++ item does, and
//! answers every choice, so the window carries out the ones that are its own: an immediate-mode
//! window cannot wait inside the item for a colour dialog, so the colour is asked for by the
//! window and its answer run by `colour_chosen`. A choice owns what it needs, which is what the
//! C++'s copies were for: the menu may be gone when the colour comes back.

use std::collections::BTreeMap;

use egui::{Context, Pos2, Ui};
use katana_cad::Document;
use katana_cad::drawing::grips::{Grip, GripKind};
use katana_cad::drawing::vertex_editing::read_polyline;
use katana_core::Result;
use katana_entity::entity::to_string;
use katana_entity::layer_path::layer_leaf;
use katana_entity::{Entity, EntityId, EntityType, Geometry};
use katana_geometry::polyline_vertices::{check_corner, delete_vertex};

use crate::canvas::Color;
use crate::command_word::command_word;
use crate::drive::action::{Action, Menu, MenuItem, action_item};
use crate::host::{Host, VerbOutcome};
use crate::shell::dialog::{MenuShown, popup_menu};
use crate::theme::style::{menu_section, menu_separator};

/// The menu's object name.
pub const PLAN_CONTEXT_MENU: &str = "planContextMenu";

/// What the menu is built from.
#[derive(Clone)]
pub struct PlanContextMenuContext<'a> {
    pub document: &'a Document,
    /// The window's actions, where they are looked up by object name.
    pub actions: &'a [Action],
    /// The tool Enter at no prompt would run again (the workspace's last tool id); "" when none
    /// has run, and then there is no Repeat.
    pub last_tool_id: String,
    /// The grip the menu was opened on (the view's `grip_at`), when it was: a polyline's vertex or
    /// segment middle gets its own items first.
    pub grip: Option<Grip>,
    /// Whether the window starts a catalogue tool on the grip (the C++ `startToolOn`): false, the
    /// tools are left out.
    pub tools_on_grip: bool,
}

impl<'a> PlanContextMenuContext<'a> {
    /// A menu over `document` with none of the window's actions, no last tool and no grip.
    pub fn new(document: &'a Document) -> Self {
        Self {
            document,
            actions: &[],
            last_tool_id: String::new(),
            grip: None,
            tools_on_grip: false,
        }
    }
}

/// What choosing an item asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum PlanMenuChoice {
    /// The window's action of that object name, triggered as a click on it.
    Trigger(String),
    /// A verb line for the window's one executor.
    Run(String),
    /// A colour asked of the person, starting from `initial`; the answer goes to
    /// `colour_chosen`.
    ChooseColour { initial: Color },
    /// Catalogue tool `tool` started with `grip` its handle - hot, as a click on the grip before
    /// the tool makes it (ToolStart::handles).
    StartToolOn { tool: String, grip: Grip },
}

/// See the module comment.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanContextMenu {
    menu: Menu,
    /// The menu's own items, by object name, each with what choosing it asks for. An item
    /// offered disabled has none.
    choices: BTreeMap<String, PlanMenuChoice>,
    /// Every item of the menu's own, by object name: what `item` finds.
    own: BTreeMap<String, Action>,
}

/// `verb` followed by `name` as one word of its line, by the one rule every dialog writes words
/// with (command_word): as it is when it is a plain word, otherwise in double quotes, which the
/// interpreter's tokenizer takes off; a name holding a double quote or a line break, which no line
/// can carry, is refused naming `what`.
pub fn named_line(verb: &str, name: &str, what: &str) -> Result<String> {
    Ok(format!("{verb} {}", command_word(name, what)?))
}

/// The one value every selected entity shares, or "" when they differ. `only`, when given,
/// reads the entities of that type alone.
fn shared_by(
    document: &Document,
    only: Option<EntityType>,
    read: impl Fn(&Entity) -> String,
) -> String {
    let mut shared: Option<String> = None;
    for id in document.selection().ids() {
        let Some(entity) = document.model().entities.find(id) else {
            continue;
        };
        if only.is_some_and(|only| entity.r#type() != only) {
            continue;
        }
        let value = read(entity);
        match &shared {
            None => shared = Some(value),
            Some(first) if *first != value => return String::new(),
            Some(_) => {}
        }
    }
    shared.unwrap_or_default()
}

fn selection_has(document: &Document, kind: EntityType) -> bool {
    document.selection().ids().into_iter().any(|id| {
        document
            .model()
            .entities
            .find(id)
            .is_some_and(|entity| entity.r#type() == kind)
    })
}

/// The menu as it is being built: its own items recorded as they are added.
struct Builder<'a> {
    context: PlanContextMenuContext<'a>,
    choices: BTreeMap<String, PlanMenuChoice>,
    own: BTreeMap<String, Action>,
}

impl Builder<'_> {
    fn window_action(&self, name: &str) -> Option<&Action> {
        self.context
            .actions
            .iter()
            .find(|action| action.name == name)
    }

    /// The window's action of that object name, when it has one.
    fn add_existing(&self, menu: &mut Menu, name: &str) {
        if let Some(action) = self.window_action(name) {
            menu.items.push(MenuItem::Action(action.clone()));
        }
    }

    /// An item of this menu's own, kept by its name.
    fn add_own(&mut self, menu: &mut Menu, action: Action, choice: Option<PlanMenuChoice>) {
        if let Some(choice) = choice {
            self.choices.insert(action.name.clone(), choice);
        }
        self.own.insert(action.name.clone(), action.clone());
        menu.items.push(MenuItem::Action(action));
    }

    /// An item of this menu's own that runs `line` when chosen; offered disabled, saying why,
    /// when the line cannot be written (named_line). `checked` makes it a ticked item. `refused`
    /// disables an item whose line is written but whose edit would be refused, saying why.
    fn add_line(
        &mut self,
        menu: &mut Menu,
        text: &str,
        name: &str,
        line: Result<String>,
        checked: Option<bool>,
        refused: Option<String>,
    ) {
        let mut action = Action::new(name, text);
        action.checked = checked;
        match line {
            Err(error) => {
                // Shown, since the name is really there, but with nothing to run: a line cut
                // short at the quote would act on another name.
                action.enabled = false;
                action.tip = error.message;
                self.add_own(menu, action, None);
            }
            Ok(line) => {
                // The line itself in the status bar, so a person learns the verb.
                action.tip.clone_from(&line);
                if let Some(why) = refused {
                    action.enabled = false;
                    action.tip = why;
                    self.add_own(menu, action, None);
                } else {
                    self.add_own(menu, action, Some(PlanMenuChoice::Run(line)));
                }
            }
        }
    }

    /// An item that starts `tool` on the grip; left out when the window starts none. `refused`
    /// offers it disabled, saying why.
    fn add_tool(
        &mut self,
        menu: &mut Menu,
        text: &str,
        name: &str,
        tool: &str,
        refused: Option<String>,
    ) {
        let Some(grip) = self.context.grip else {
            return;
        };
        if !self.context.tools_on_grip {
            return;
        }
        let mut action = Action::new(name, text).tip(&format!(
            "Starts the tool on this {}",
            if grip.kind == GripKind::Vertex {
                "vertex"
            } else {
                "segment"
            }
        ));
        match refused {
            Some(why) => {
                action.enabled = false;
                action.tip = why;
                self.add_own(menu, action, None);
            }
            None => self.add_own(
                menu,
                action,
                Some(PlanMenuChoice::StartToolOn {
                    tool: String::from(tool),
                    grip,
                }),
            ),
        }
    }

    /// The layers under `parent` ("" for the roots), `shared` ticked: the layer every selected
    /// entity is on, "" when they are on several.
    fn add_layer_items(&mut self, menu: &mut Menu, parent: &str, shared: &str) {
        // The document's own borrow, not the builder's: the items are added while it is read.
        let document = self.context.document;
        let table = &document.model().layers;
        let names = if parent.is_empty() {
            table.roots()
        } else {
            table.children(parent)
        };
        for name in names {
            let leaf = layer_leaf(&name);
            let object_name = format!("planContextLayer.{name}");
            let line = named_line("CHLAYER", &name, "the layer");
            let checked = Some(name == shared);
            if table.has_children(&name) {
                // A layer with layers under it is a submenu of them, headed by the layer itself,
                // which entities can be on too.
                let mut under = Menu::new(&format!("planContextLayerTree.{name}"), leaf);
                self.add_line(&mut under, leaf, &object_name, line, checked, None);
                under.items.push(MenuItem::Separator);
                self.add_layer_items(&mut under, &name, shared);
                menu.items.push(MenuItem::Menu(under));
            } else {
                self.add_line(menu, leaf, &object_name, line, checked, None);
            }
        }
    }

    /// The items of the polyline grip the menu was opened on, when it was: what vertex editing
    /// offers for THAT vertex or segment, before the selection's items. The edits are verb lines,
    /// the tools start with the grip as their handle, so each is what a person would type or
    /// pick.
    fn add_grip_items(&mut self, menu: &mut Menu) {
        let Some(grip) = self.context.grip else {
            return;
        };
        let Some(polyline) = self
            .context
            .document
            .model()
            .entities
            .find(grip.entity)
            .and_then(read_polyline)
        else {
            return;
        };
        let id = grip.entity;
        if grip.kind == GripKind::Vertex && grip.index < polyline.vertices.len() {
            let v = grip.index;
            menu.items
                .push(MenuItem::Section(format!("Vertex {v} of polyline {id}")));
            self.add_line(
                menu,
                "&Delete Vertex",
                "planContextVertex.Delete",
                Ok(format!("VERTEX DELETE {id} {v}")),
                None,
                delete_vertex(&polyline, v).err().map(|error| error.message),
            );
            self.add_line(
                menu,
                "&Insert Vertex After",
                "planContextVertex.InsertAfter",
                Ok(format!("VERTEX INSERT {id} #{id}.s{v} after={v}")),
                None,
                (v >= polyline.segment_count()).then(|| {
                    String::from("the last vertex of an open polyline has no segment after it")
                }),
            );
            if polyline.closed {
                self.add_line(
                    menu,
                    "Make It the &Start",
                    "planContextVertex.Start",
                    Ok(format!("STARTVERTEX {id} {v}")),
                    None,
                    (v == 0).then(|| String::from("vertex 0 is already the start")),
                );
            }
            self.add_tool(
                menu,
                "&Move Vertex...",
                "planContextVertex.Move",
                "draw.vertex.move",
                None,
            );
            self.add_tool(
                menu,
                "Set &Height...",
                "planContextVertex.Height",
                "draw.vertex.height",
                None,
            );
            let corner = check_corner(&polyline, v).err().map(|error| error.message);
            self.add_tool(
                menu,
                "&Fillet Corner...",
                "planContextVertex.Fillet",
                "draw.vertex.fillet",
                corner.clone(),
            );
            self.add_tool(
                menu,
                "C&hamfer Corner...",
                "planContextVertex.Chamfer",
                "draw.vertex.chamfer",
                corner,
            );
            self.add_tool(
                menu,
                "S&traighten From Here...",
                "planContextVertex.Straighten",
                "draw.vertex.straighten",
                None,
            );
        } else if grip.kind == GripKind::SegmentMid && grip.index < polyline.segment_count() {
            let s = grip.index;
            menu.items
                .push(MenuItem::Section(format!("Segment {s} of polyline {id}")));
            self.add_line(
                menu,
                "Add Vertex at the &Middle",
                "planContextSegment.AddMiddle",
                Ok(format!("VERTEX INSERT {id} #{id}.s{s} after={s}")),
                None,
                None,
            );
            self.add_tool(
                menu,
                "&Insert Vertex...",
                "planContextSegment.Insert",
                "draw.vertex.insert",
                None,
            );
            self.add_tool(
                menu,
                "Make an &Arc...",
                "planContextSegment.Arc",
                "draw.vertex.arc",
                None,
            );
            if polyline.is_arc(s) {
                self.add_line(
                    menu,
                    "Make &Straight",
                    "planContextSegment.Line",
                    Ok(format!("VERTEX SET {id} {s} bulge=0")),
                    None,
                    None,
                );
            }
        } else {
            return;
        }
        menu.items.push(MenuItem::Separator);
    }

    /// With nothing to act on: the things that make a selection, and the tool Enter would repeat,
    /// named - AutoCAD's "Repeat LINE".
    fn build_for_nothing(&mut self, menu: &mut Menu) {
        if !self.context.last_tool_id.is_empty()
            && let Some(tool) = self.window_action(&self.context.last_tool_id)
        {
            let repeat = Action::new(
                "planContextRepeat",
                &format!("Repeat {}", tool.text.replace('&', "")),
            );
            let choice = PlanMenuChoice::Trigger(tool.name.clone());
            self.add_own(menu, repeat, Some(choice));
            menu.items.push(MenuItem::Separator);
        }
        self.add_existing(menu, "editSelectAll");
        self.add_existing(menu, "editSelectById");
        self.add_existing(menu, "select.quick");
        menu.items.push(MenuItem::Separator);
        self.add_existing(menu, "viewZoomExtents");
    }

    fn build_for_selection(&mut self, menu: &mut Menu, selected: &[EntityId]) {
        let document = self.context.document;
        self.add_grip_items(menu);
        self.add_existing(menu, "editErase");
        menu.items.push(MenuItem::Separator);
        for tool in [
            "modify.move",
            "modify.copy",
            "modify.rotate",
            "modify.scale",
            "modify.mirror",
            "modify.offset",
        ] {
            self.add_existing(menu, tool);
        }
        menu.items.push(MenuItem::Separator);

        // Put on Layer, in the layer tree's shape: a drawing brought in from a survey has
        // hundreds of layers, and a flat list of them would run off the screen. The layer every
        // selected entity is on is ticked.
        let mut layers = Menu::new("planContextLayer", "Put on &Layer");
        let on_layer = shared_by(document, None, |entity| entity.layer.clone());
        self.add_layer_items(&mut layers, "", &on_layer);
        menu.items.push(MenuItem::Menu(layers));

        let mut styles = Menu::new("planContextStyle", "St&yle");
        let style = shared_by(document, None, |entity| entity.style.clone());
        // "-" is STYLE APPLY's ByLayer; an empty style is how an entity wears it.
        let all_by_layer = selected.iter().all(|&id| {
            document
                .model()
                .entities
                .find(id)
                .is_none_or(|entity| entity.style.is_empty())
        });
        self.add_line(
            &mut styles,
            "ByLayer",
            "planContextStyle.ByLayer",
            Ok(String::from("STYLE APPLY -")),
            Some(all_by_layer),
            None,
        );
        let style_names = document.model().styles.names();
        if !style_names.is_empty() {
            styles.items.push(MenuItem::Separator);
        }
        for name in style_names {
            self.add_line(
                &mut styles,
                &name,
                &format!("planContextStyle.{name}"),
                named_line("STYLE APPLY", &name, "the style"),
                Some(!style.is_empty() && name == style),
                None,
            );
        }
        menu.items.push(MenuItem::Menu(styles));

        let mut colours = Menu::new("planContextColour", "&Colour");
        self.add_line(
            &mut colours,
            "ByLayer",
            "planContextColour.ByLayer",
            Ok(String::from("COLOR BYLAYER")),
            None,
            None,
        );
        // The colour asked for starts from the colour the selection shares, when it does.
        let hex = shared_by(document, None, |entity| {
            entity.color.map(|colour| colour.to_hex()).unwrap_or_default()
        });
        let initial = if hex.is_empty() {
            Color::WHITE
        } else {
            Color::from_name(&hex).unwrap_or(Color::BLACK)
        };
        self.add_own(
            &mut colours,
            Action::new("planContextColour.Choose", "C&hoose..."),
            Some(PlanMenuChoice::ChooseColour { initial }),
        );
        menu.items.push(MenuItem::Menu(colours));
        self.add_existing(menu, "editAttributes");
        menu.items.push(MenuItem::Separator);

        // INFO names one entity; for several, List says what each is. #id, which is always the
        // entity: INFO 12 is a file when one is called 12.
        if let [only] = selected {
            self.add_line(
                menu,
                "Entity &Information",
                "planContextInfo",
                Ok(format!("INFO #{only}")),
                None,
                None,
            );
        }
        self.add_existing(menu, "inquiry.list");
        menu.items.push(MenuItem::Separator);
        self.add_existing(menu, "select.similar");
        self.add_existing(menu, "select.quick");
        self.add_existing(menu, "formatGlobalModify");
        // The editors of what the selection holds, when the window has them.
        if selection_has(document, EntityType::Text) {
            self.add_existing(menu, "annotateEditText");
        }
        if selection_has(document, EntityType::Label) {
            self.add_existing(menu, "annotateEditLabel");
        }
        if selection_has(document, EntityType::Leader) {
            if let Some(editor) = self.window_action("annotateLeaders") {
                let edit =
                    Action::new("planContextEditLeader", "Edit Leader...").enabled(editor.enabled);
                let choice = editor
                    .enabled
                    .then(|| PlanMenuChoice::Trigger(editor.name.clone()));
                self.add_own(menu, edit, choice);
            }
            let mut frames = Menu::new("planContextLeaderFrame", "Leader Note Frame");
            let shared = shared_by(document, Some(EntityType::Leader), |entity| {
                match &entity.geometry {
                    Geometry::Leader(leader) => String::from(to_string(leader.callout)),
                    _ => String::new(),
                }
            });
            for (label, word) in [("None", "none"), ("Box", "box"), ("Circle", "circle")] {
                self.add_line(
                    &mut frames,
                    label,
                    &format!("planContextLeaderFrame.{word}"),
                    Ok(format!("LEADER SET SELECTION callout={word}")),
                    Some(shared == word),
                    None,
                );
            }
            menu.items.push(MenuItem::Menu(frames));
        }
        menu.items.push(MenuItem::Separator);
        self.add_existing(menu, "editDeselect");
    }
}

/// The submenu `name` of `menu`, at any depth.
fn find_submenu<'m>(menu: &'m Menu, name: &str) -> Option<&'m Menu> {
    menu.items.iter().find_map(|item| match item {
        MenuItem::Menu(under) if under.name == name => Some(under),
        MenuItem::Menu(under) => find_submenu(under, name),
        _ => None,
    })
}

impl PlanContextMenu {
    /// Built for the document's selection as it is now: the selection's verbs when something is
    /// selected, otherwise selecting and the last tool.
    pub fn new(context: PlanContextMenuContext<'_>) -> Self {
        let selected = context.document.selection().ids();
        let mut builder = Builder {
            context,
            choices: BTreeMap::new(),
            own: BTreeMap::new(),
        };
        let mut menu = Menu::new(PLAN_CONTEXT_MENU, "");
        if selected.is_empty() {
            builder.build_for_nothing(&mut menu);
        } else {
            builder.build_for_selection(&mut menu, &selected);
        }
        Self {
            menu,
            choices: builder.choices,
            own: builder.own,
        }
    }

    /// The menu's rows: its items in order, the window's actions among its own.
    pub fn menu(&self) -> &Menu {
        &self.menu
    }

    /// The submenu of that object name, at any depth.
    pub fn submenu(&self, name: &str) -> Option<&Menu> {
        find_submenu(&self.menu, name)
    }

    /// The item of the menu's OWN of that object name, at any depth: never one of the window's
    /// actions, which the menu shows and does not make again.
    pub fn item(&self, name: &str) -> Option<&Action> {
        self.own.get(name)
    }

    /// What choosing the menu's own item `name` asks for; None for a name that is none of its
    /// own, and for an item offered disabled.
    pub fn choice(&self, name: &str) -> Option<&PlanMenuChoice> {
        self.choices.get(name)
    }

    /// The item `name` chosen, as a click on it: a line of the menu's own is run through `host`
    /// at once - what it said, refusals too, is in the log - and every choice is answered, so the
    /// caller triggers the window's action, asks for the colour (then `colour_chosen`) or starts
    /// the tool. None, and nothing done, for a disabled item and for a name the menu does not
    /// show.
    pub fn trigger(&self, name: &str, host: &mut dyn Host) -> Option<PlanMenuChoice> {
        if self.own.contains_key(name) {
            let choice = self.choices.get(name)?.clone();
            if let PlanMenuChoice::Run(line) = &choice {
                let _ = host.run(line);
            }
            return Some(choice);
        }
        // One of the window's actions, shown here: the window triggers it.
        self.menu
            .actions()
            .into_iter()
            .find(|action| action.name == name && action.enabled)
            .map(|action| PlanMenuChoice::Trigger(action.name.clone()))
    }

    /// The answer to a `ChooseColour`: `colour` run as COLOR #RRGGBB through `host`, or nothing
    /// when the person cancelled (None). An associated function on purpose: the menu the choice
    /// came from may be gone by now.
    pub fn colour_chosen(host: &mut dyn Host, colour: Option<Color>) -> Option<VerbOutcome> {
        colour.map(|colour| host.run(&format!("COLOR {}", colour.name().to_uppercase())))
    }

    /// The menu's items drawn in `ui`; answers the object name of the item a person - or a
    /// driver's press - chose this frame.
    pub fn ui(&self, ui: &mut Ui) -> Option<String> {
        let mut chosen = None;
        items_ui(ui, &self.menu, &mut chosen);
        chosen
    }

    /// The menu popped up at `at` (QMenu::popup), a Menu target named "planContextMenu"; answers
    /// what it drew and the item chosen this frame.
    pub fn popup(&self, ctx: &Context, at: Pos2) -> (MenuShown, Option<String>) {
        let mut chosen = None;
        let shown = popup_menu(ctx, PLAN_CONTEXT_MENU, at, |ui| {
            items_ui(ui, &self.menu, &mut chosen);
        });
        (shown, chosen)
    }
}

fn items_ui(ui: &mut Ui, menu: &Menu, chosen: &mut Option<String>) {
    for item in &menu.items {
        match item {
            MenuItem::Action(action) => {
                if action_item(ui, action).clicked() {
                    *chosen = Some(action.name.clone());
                }
            }
            MenuItem::Section(title) => {
                menu_section(ui, title);
            }
            MenuItem::Separator => {
                menu_separator(ui);
            }
            MenuItem::Menu(under) => {
                ui.menu_button(under.title.replace('&', ""), |ui| {
                    items_ui(ui, under, chosen);
                });
            }
        }
    }
}
