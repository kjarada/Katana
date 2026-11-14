//! GIS > Online Data... (gis_online_dialog.hpp; docs/gis_online.md): the window onto the
//! provider catalogue.
//!
//! The dialog is non-modal and holds no state of its own beyond what is typed into it: the
//! providers come from the catalogue the workbench owns, and an import is not performed here -
//! the dialog builds the SAME interop OnlineCommand the ONLINE IMPORT verb parses, and hands it
//! to the workbench, which runs it as a background job exactly as it runs the typed verb. So
//! there is no path through the dialog that an agent cannot take through the command line.
//!
//! Every control has an object name, which is how the tests and the headless --dialog, --fill
//! and --press switches drive it:
//!
//! ```text
//! onlineDataDialog      the dialog (the action onlineData + "Dialog")
//! onlineSearch          filter the tree as you type
//! onlineProviders       the tree: Australia > state > provider > layer, Global, Custom
//! onlineDetails         the chosen layer: kind, service, service CRS, version, limits, time,
//!                       licence, attribution, coverage, key needed, verified
//! onlineCatalogue       a catalogue layer's search (shown only for one):
//! onlineCatalogueSearch   the words, as ONLINE LAYERS <provider> <words> takes them
//! onlineCatalogueRun      search
//! onlineCatalogueResults  what it found: title, dataset, format, address
//! onlineCatalogueAdd      add the chosen result's service, as ONLINE CUSTOM <url>
//! onlineArea            Current view / Drawing extents / Selection / Typed box
//! onlineBox             x0,y0,x1,y1 for a typed box
//! onlineBoxCrs          the typed box's CRS: the project's, or WGS 84 lon/lat
//! onlineResolutionAuto  choose the resolution from the area and the data
//! onlineResolution      ground units per pixel, for rasters
//! onlineTargetLayer     the Katana layer vectors go on
//! onlineTag             an OpenStreetMap tag test (building, highway=primary)
//! onlineFrom, onlineTo  acquisition dates for Sentinel-2
//! onlineCloud           the cloud ceiling, percent
//! onlineTimeDefault     the layer's own date (the latest), for a time-enabled layer
//! onlineTime            the date to ask for otherwise: time=YYYY-MM-DD
//! onlineAdvanced        the Advanced group:
//! onlineTimeoutOn         give up after onlineTimeout seconds: timeout=<s>
//! onlineTimeout
//! onlineProjectCrs      what the project's CRS is, or that it has none
//! onlineCrs             a CRS to use when the project has none
//! onlineKey             this provider's key (never shown back)
//! onlineSaveKey         store it in the application's settings
//! onlineAddCustom       Add Custom Service... (a URL; its layers are discovered)
//! onlineCustomUrl       the URL for Add Custom Service
//! onlineImport          import the chosen layer
//! onlineStatus          what happened last
//! onlineClose           close
//! ```
//!
//! THE RUST SHAPE. What the dialog asks of whoever shows it is a trait (`OnlineDialogContext`),
//! lent to each call that needs it, where the C++ held a struct of functions: the workbench
//! answers it with itself and the window, a test with its own recorder. The dialog keeps a copy
//! of the catalogue from its last `refresh_catalogue`, which the workbench calls whenever its
//! catalogue changes, as the C++'s did. A date is a field of text, yyyy-MM-dd, that keeps the
//! last date it read. The details are plain text, a line a fact, where the C++ wrote a table in
//! markup.

use egui::{RichText, Ui};
use katana_core::civil_date::{YearMonthDay, civil_from_days, days_from_civil, today_utc};
use katana_core::error::make_error;
use katana_core::text::{format_general, parse_finite_double, parse_integer};
use katana_core::{ErrorCode, Result, Status};
use katana_entity::entity::to_string;
use katana_gis::web_access::redact_url;
use katana_interop::online_verbs::parse_online_command;
use katana_interop::{
    CkanResource, OnlineAreaKind, OnlineCatalogue, OnlineCommand, OnlineLayer, OnlineLayerKind,
    OnlineServiceType,
};

use crate::annotation::dialog_support::table;
use crate::drive::controls::custom;
use crate::drive::mailbox::{Act, fill_disabled, lists_no, no_button};
use crate::drive::registry::{ControlKind, ControlRecord, settle, take};
use crate::drive::{
    Choice, LineOptions, SpinRange, button, check, choice, label, label_as, line_with, spin,
    whole,
};
use crate::shell::DialogFrame;
use crate::survey::survey_dialogs::hidden;
use crate::theme::palette;

/// What a headless import gives a service before it cancels (gis_online's
/// HEADLESS_DEADLINE_SECONDS): the Advanced group's starting value, so ticking it changes nothing
/// until the number does.
const DEFAULT_TIMEOUT_SECONDS: i64 = 600;

/// The body's width: the C++ dialog's 980 less its frame.
const BODY_WIDTH: f32 = 950.0;

/// What the dialog asks of whoever shows it; the workbench supplies these, a test supplies its
/// own.
pub trait OnlineDialogContext {
    fn catalogue(&self) -> &OnlineCatalogue;
    /// The project's coordinate system, "EPSG:7856"; empty when it has none.
    fn project_crs(&self) -> String;
    fn has_key(&mut self, key_name: &str) -> bool;
    fn save_key(&mut self, key_name: &str, value: &str);
    /// Runs the command (an Import, a catalogue search - ONLINE LAYERS - or ONLINE CUSTOM) in
    /// the background. An error is shown in the status line; success means the job started. A
    /// search's results come back through `show_catalogue_results`.
    fn run(&mut self, command: &OnlineCommand) -> Status;
    /// Discovers the service at a URL and adds it to the user catalogue, in the background; the
    /// dialog is refreshed by the workbench when it lands.
    fn add_custom(&mut self, url: &str) -> Status;
    /// Whether the project coordinate system dialog can be opened from here.
    fn can_choose_project_crs(&self) -> bool {
        false
    }
    /// Opens the project coordinate system dialog, suggesting systems for the typed box's centre
    /// when it is in longitude and latitude; true when the project's coordinate system changed.
    fn choose_project_crs(&mut self, place: Option<(f64, f64)>) -> bool {
        let _ = place;
        false
    }
}

/// The kind as a person reads it.
fn kind_text(kind: OnlineLayerKind) -> &'static str {
    match kind {
        OnlineLayerKind::Imagery => "Raster imagery",
        OnlineLayerKind::Elevation => "Raster elevation",
        OnlineLayerKind::Vector => "Vector features",
        OnlineLayerKind::Catalogue => "Catalogue search",
    }
}

/// "Australia/NSW" -> ("Australia", "NSW"); "Global" -> ("Global", "").
fn split_group(group: &str) -> (String, String) {
    match group.find('/') {
        Some(slash) => (
            String::from(&group[..slash]),
            String::from(&group[slash + 1..]),
        ),
        None => (String::from(group), String::new()),
    }
}

/// QString::number(value): six significant digits, as %g writes them.
fn number(value: f64) -> String {
    format_general(value, 6)
}

/// One item of the provider tree: a heading, a state, a provider or a layer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct TreeItem {
    text: String,
    /// The second column: a layer's kind.
    kind: String,
    /// A provider's and a layer's provider id.
    provider: String,
    /// A layer's id; empty for what is no layer.
    layer: String,
    hidden: bool,
    expanded: bool,
    children: Vec<TreeItem>,
}

impl TreeItem {
    fn heading(text: &str) -> Self {
        Self {
            text: String::from(text),
            expanded: true,
            ..Self::default()
        }
    }

    fn is_layer(&self) -> bool {
        !self.layer.is_empty()
    }
}

/// A date field (QDateEdit, yyyy-MM-dd): the text shown, and the last date it read.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DateField {
    date: YearMonthDay,
    text: String,
}

fn date_text(date: YearMonthDay) -> String {
    format!("{:04}-{:02}-{:02}", date.year, date.month, date.day)
}

/// The date `text` writes as yyyy-MM-dd; None for what is no date of the calendar.
fn parse_date(text: &str) -> Option<YearMonthDay> {
    let mut parts = text.trim().split('-');
    let year = parse_integer(parts.next()?)?;
    let month = u32::try_from(parse_integer(parts.next()?)?).ok()?;
    let day = u32::try_from(parse_integer(parts.next()?)?).ok()?;
    if parts.next().is_some() {
        return None;
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let date = YearMonthDay::new(year, month, day);
    // A day the month does not have comes back as another date.
    (civil_from_days(days_from_civil(date)) == date).then_some(date)
}

impl DateField {
    fn new(date: YearMonthDay) -> Self {
        Self {
            date,
            text: date_text(date),
        }
    }

    fn value(&self) -> String {
        date_text(self.date)
    }

    fn ui(&mut self, ui: &mut Ui, name: &str) {
        let response = line_with(
            ui,
            name,
            &mut self.text,
            LineOptions {
                placeholder: "yyyy-MM-dd",
                width: Some(90.0),
                ..LineOptions::default()
            },
        );
        if response.changed()
            && let Some(date) = parse_date(&self.text)
        {
            self.date = date;
        }
        // Left holding what is no date, the field shows the date it has, as a QDateEdit does.
        if response.lost_focus() {
            self.text = date_text(self.date);
        }
    }
}

/// GIS > Online Data... (see the module comment).
pub struct OnlineDataDialog {
    frame: DialogFrame,
    visible: bool,
    catalogue: OnlineCatalogue,
    search: String,
    tree: Vec<TreeItem>,
    /// The tree's current item: its provider and, for a layer, its layer.
    current: Option<(String, String)>,
    area_items: Vec<String>,
    area: Choice,
    typed_box: String,
    box_crs_items: Vec<String>,
    box_crs: Choice,
    resolution_auto: bool,
    resolution: f64,
    target_layer: String,
    tag: String,
    use_dates: bool,
    from: DateField,
    to: DateField,
    cloud: f64,
    time_default: bool,
    time: DateField,
    timeout_on: bool,
    timeout: i64,
    catalogue_search: String,
    results: Vec<CkanResource>,
    result: Option<usize>,
    project_crs: String,
    crs: String,
    key: String,
    custom_url: String,
    status: String,
    status_error: bool,
}

/// Which of the layer's own controls apply to the layer chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Applies {
    key: bool,
    tag: bool,
    dates: bool,
    time_default: bool,
    time: bool,
    resolution_auto: bool,
    resolution: bool,
    target_layer: bool,
}

/// The areas, in the order the choice lists them.
const AREAS: [(&str, OnlineAreaKind); 4] = [
    ("Current view", OnlineAreaKind::View),
    ("Drawing extents", OnlineAreaKind::Drawing),
    ("Selection", OnlineAreaKind::Selection),
    ("Typed box", OnlineAreaKind::Box),
];

impl OnlineDataDialog {
    pub fn new(context: &dyn OnlineDialogContext) -> Self {
        let today = today_utc();
        let before = civil_from_days(days_from_civil(today) - 90);
        let mut dialog = Self {
            frame: DialogFrame::new("onlineDataDialog", "Online Data"),
            visible: false,
            catalogue: OnlineCatalogue::default(),
            search: String::new(),
            tree: Vec::new(),
            current: None,
            area_items: AREAS.iter().map(|(text, _)| String::from(*text)).collect(),
            area: Choice::at(0),
            typed_box: String::new(),
            box_crs_items: vec![
                String::from("Project coordinates"),
                String::from("WGS 84 longitude, latitude"),
            ],
            box_crs: Choice::at(0),
            resolution_auto: true,
            resolution: 1.0,
            target_layer: String::new(),
            tag: String::new(),
            use_dates: false,
            from: DateField::new(before),
            to: DateField::new(today),
            cloud: 20.0,
            time_default: true,
            time: DateField::new(today),
            timeout_on: false,
            timeout: DEFAULT_TIMEOUT_SECONDS,
            catalogue_search: String::new(),
            results: Vec::new(),
            result: None,
            project_crs: String::new(),
            crs: String::new(),
            key: String::new(),
            custom_url: String::new(),
            status: String::new(),
            status_error: false,
        };
        dialog.refresh_catalogue(context);
        dialog
    }

    pub fn object_name(&self) -> &str {
        self.frame.name()
    }

    /// QDialog::isModal: never.
    pub fn is_modal(&self) -> bool {
        self.frame.is_modal()
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn show(&mut self) {
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Rebuilds the tree from the catalogue, keeping the chosen layer.
    pub fn refresh_catalogue(&mut self, context: &dyn OnlineDialogContext) {
        let keep = self.current.take();
        self.catalogue = context.catalogue().clone();
        // Australia first, then Global, then the person's own.
        let mut tree: Vec<TreeItem> = ["Australia", "Global", "Custom"]
            .iter()
            .map(|name| TreeItem::heading(name))
            .collect();
        for provider in &self.catalogue.providers {
            let (top_name, sub_name) = split_group(if provider.user_defined {
                "Custom"
            } else {
                provider.group.as_str()
            });
            let top_at = match tree.iter().position(|top| top.text == top_name) {
                Some(at) => at,
                None => {
                    tree.push(TreeItem::heading(&top_name));
                    tree.len() - 1
                }
            };
            let top = &mut tree[top_at];
            // The tree opens to its second level: a heading's states and providers show, a
            // state's providers show, and a provider under a state is closed.
            let (parent, provider_open) = if sub_name.is_empty() {
                (top, true)
            } else {
                let group_at = match top.children.iter().position(|group| {
                    group.text == sub_name && group.provider.is_empty() && !group.is_layer()
                }) {
                    Some(at) => at,
                    None => {
                        top.children.push(TreeItem::heading(&sub_name));
                        top.children.len() - 1
                    }
                };
                (&mut top.children[group_at], false)
            };
            let mut item = TreeItem {
                text: provider.title.clone(),
                provider: provider.id.clone(),
                expanded: provider_open,
                ..TreeItem::default()
            };
            for layer in provider.layers() {
                item.children.push(TreeItem {
                    text: layer.title.clone(),
                    kind: String::from(kind_text(layer.kind)),
                    provider: provider.id.clone(),
                    layer: layer.id.clone(),
                    ..TreeItem::default()
                });
            }
            parent.children.push(item);
        }
        for top in &mut tree {
            top.hidden = top.children.is_empty();
        }
        self.tree = tree;
        let search = self.search.clone();
        self.apply_filter(&search);
        if let Some((provider, layer)) = keep
            && !layer.is_empty()
        {
            self.select_layer(&provider, &layer);
        }
        self.refresh_project_crs(context);
    }

    /// Shows the project's coordinate system as it is now.
    pub fn refresh_project_crs(&mut self, context: &dyn OnlineDialogContext) {
        let crs = context.project_crs();
        self.project_crs = if crs.is_empty() {
            String::from("none set - Set Project CRS..., or give one below")
        } else {
            crs
        };
    }

    /// Chooses a layer in the tree. False when there is no such layer.
    pub fn select_layer(&mut self, provider: &str, layer: &str) -> bool {
        fn find(items: &[TreeItem], provider: &str, layer: &str) -> Option<(String, String)> {
            for item in items {
                if item.provider.eq_ignore_ascii_case(provider)
                    && item.layer.eq_ignore_ascii_case(layer)
                {
                    return Some((item.provider.clone(), item.layer.clone()));
                }
                if let Some(found) = find(&item.children, provider, layer) {
                    return Some(found);
                }
            }
            None
        }
        match find(&self.tree, provider, layer) {
            Some(found) => {
                self.current = Some(found);
                true
            }
            None => false,
        }
    }

    /// The headings of the tree, in order, each with whether it is hidden.
    pub fn top_items(&self) -> Vec<(String, bool)> {
        self.tree
            .iter()
            .map(|top| (top.text.clone(), top.hidden))
            .collect()
    }

    /// The texts of what is directly under the heading at `top`.
    pub fn children_of_top(&self, top: usize) -> Vec<String> {
        self.tree
            .get(top)
            .map(|item| {
                item.children
                    .iter()
                    .map(|child| child.text.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The layers a person can see, in the tree's order: each "provider layer", by their ids.
    pub fn visible_layers(&self) -> Vec<String> {
        fn visit(items: &[TreeItem], out: &mut Vec<String>) {
            for item in items {
                if item.hidden {
                    continue;
                }
                if item.is_layer() {
                    out.push(format!("{} {}", item.provider, item.layer));
                }
                visit(&item.children, out);
            }
        }
        let mut out = Vec::new();
        visit(&self.tree, &mut out);
        out
    }

    fn chosen_layer(&self) -> Option<&OnlineLayer> {
        let (provider, layer) = self.current.as_ref()?;
        if layer.is_empty() {
            return None;
        }
        self.catalogue.find_provider(provider)?.find_layer(layer)
    }

    fn applies(&self) -> Applies {
        let Some(layer) = self.chosen_layer() else {
            return Applies::default();
        };
        let dated = has_time(layer);
        let raster =
            layer.kind == OnlineLayerKind::Imagery || layer.kind == OnlineLayerKind::Elevation;
        Applies {
            key: true,
            tag: layer.r#type == OnlineServiceType::Overpass,
            dates: layer.r#type == OnlineServiceType::Stac,
            time_default: dated,
            time: dated && !self.time_default,
            resolution_auto: raster,
            resolution: raster && !self.resolution_auto,
            target_layer: layer.kind == OnlineLayerKind::Vector,
        }
    }

    /// What onlineDetails says of the layer chosen: a line a fact.
    pub fn details(&self, context: &mut dyn OnlineDialogContext) -> String {
        let Some(layer) = self.chosen_layer() else {
            return String::from(
                "Choose a layer to see what it is, where it comes from and on what terms.",
            );
        };
        let key_stored = context.has_key(&layer.key_name);
        let mut lines = vec![layer.title.clone()];
        let mut row = |name: &str, value: String| lines.push(format!("{name}: {value}"));
        row(
            "Provider",
            format!("{} ({})", layer.provider_title, layer.provider_id),
        );
        row("Id", format!("{} {}", layer.provider_id, layer.id));
        row("Kind", String::from(kind_text(layer.kind)));
        row(
            "Service",
            if layer.layer_name.is_empty() {
                String::from(to_string(layer.r#type))
            } else {
                format!("{} - {}", to_string(layer.r#type), layer.layer_name)
            },
        );
        row("Address", redact_url(&layer.endpoint));
        // What ONLINE INFO says of the service, in a person's words: the system it is asked in,
        // its version, and what one import may take of it.
        row(
            "Service CRS",
            if layer.crs.is_empty() {
                String::from("the service's own")
            } else {
                layer.crs.clone()
            },
        );
        if !layer.version.is_empty() {
            row("Version", layer.version.clone());
        }
        let limits = limits_text(layer);
        if !limits.is_empty() {
            row("Limits", limits);
        }
        if has_time(layer) {
            row(
                "Date",
                String::from("a time series; the latest unless a date is chosen below"),
            );
        }
        row(
            "Coverage",
            format!(
                "{}, {} to {}, {} (WGS 84)",
                number(layer.coverage[0]),
                number(layer.coverage[1]),
                number(layer.coverage[2]),
                number(layer.coverage[3])
            ),
        );
        if let Some(resolution) = layer.resolution {
            row("Resolution", format!("{} m", number(resolution)));
        }
        row("Licence", layer.licence.clone());
        row("Attribution", layer.attribution.clone());
        row(
            "Key",
            String::from(if !layer.key_required {
                "not needed"
            } else if key_stored {
                "needed - stored"
            } else {
                "needed - not stored"
            }),
        );
        row(
            "Checked",
            if layer.verified.is_empty() {
                format!("documented, not yet seen to answer: {}", layer.evidence)
            } else {
                format!("answered {}", layer.verified)
            },
        );
        if layer.kind == OnlineLayerKind::Catalogue {
            lines.push(format!(
                "This is a search, not data: search it below and add a service it finds, or \
                 type ONLINE LAYERS {} <words> and ONLINE CUSTOM <address>.",
                layer.provider_id
            ));
        }
        lines.join("\n")
    }

    fn apply_filter(&mut self, text: &str) {
        let lower = text.to_lowercase();
        let words: Vec<&str> = lower.split(' ').filter(|word| !word.is_empty()).collect();
        // A layer shows when every word is in its own text or its provider's; a group or
        // provider shows when anything under it does.
        fn visit(item: &mut TreeItem, inherited: &str, words: &[&str]) -> bool {
            let here = format!(
                "{inherited} {} {} {} {}",
                item.text.to_lowercase(),
                item.kind.to_lowercase(),
                item.provider.to_lowercase(),
                item.layer.to_lowercase()
            );
            if item.is_layer() {
                let shown = words.iter().all(|word| here.contains(word));
                item.hidden = !shown;
                return shown;
            }
            let mut any = false;
            for child in &mut item.children {
                any = visit(child, &here, words) || any;
            }
            item.hidden = !any;
            if any && !words.is_empty() {
                item.expanded = true;
            }
            any
        }
        for top in &mut self.tree {
            visit(top, "", &words);
        }
        // A search that hides the chosen layer chooses the first it left, so the details always
        // describe something the list shows.
        fn own_hidden(items: &[TreeItem], current: &(String, String)) -> Option<bool> {
            for item in items {
                if item.provider == current.0 && item.layer == current.1 {
                    return Some(item.hidden);
                }
                if let Some(found) = own_hidden(&item.children, current) {
                    return Some(found);
                }
            }
            None
        }
        let lost = match &self.current {
            None => true,
            Some(current) => {
                current.1.is_empty() || own_hidden(&self.tree, current).unwrap_or(true)
            }
        };
        if !words.is_empty()
            && lost
            && let Some(first) = self.visible_layers().first()
            && let Some((provider, layer)) = first.split_once(' ')
        {
            self.current = Some((String::from(provider), String::from(layer)));
        }
    }

    fn area_kind(&self) -> OnlineAreaKind {
        self.area
            .index
            .and_then(|at| AREAS.get(at))
            .map_or(OnlineAreaKind::View, |(_, kind)| *kind)
    }

    /// The Import the dialog's fields describe. InvalidArgument, saying what is missing, when no
    /// layer is chosen or the typed box does not parse.
    pub fn command(&self) -> Result<OnlineCommand> {
        let Some(layer) = self.chosen_layer() else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "choose a layer in the list first",
                String::new(),
            ));
        };
        let applies = self.applies();
        // Written as the verb would be typed, and read by the verb's own parser, so the dialog
        // cannot mean anything the command line cannot.
        let mut line = vec![
            String::from("ONLINE"),
            String::from("IMPORT"),
            layer.provider_id.clone(),
            layer.id.clone(),
        ];
        match self.area_kind() {
            OnlineAreaKind::View => line.push(String::from("area=view")),
            OnlineAreaKind::Drawing => line.push(String::from("area=drawing")),
            OnlineAreaKind::Selection => line.push(String::from("area=selection")),
            OnlineAreaKind::Box => line.push(format!(
                "area={}{}",
                if self.box_crs.index == Some(1) {
                    "lonlat:"
                } else {
                    ""
                },
                self.typed_box.replace(' ', "")
            )),
        }
        if applies.resolution_auto && !self.resolution_auto {
            line.push(format!("res={}", format_general(self.resolution, 12)));
        }
        if applies.target_layer && !self.target_layer.trim().is_empty() {
            line.push(format!(
                "layer=\"{}\"",
                self.target_layer.trim().replace('"', "")
            ));
        }
        if applies.tag && !self.tag.trim().is_empty() {
            line.push(format!("tag=\"{}\"", self.tag.trim().replace('"', "")));
        }
        if applies.dates && self.use_dates {
            line.push(format!("from={}", self.from.value()));
            line.push(format!("to={}", self.to.value()));
        }
        if applies.dates {
            line.push(format!("cloud={}", number(self.cloud)));
        }
        if applies.time {
            line.push(format!("time={}", self.time.value()));
        }
        if self.timeout_on {
            line.push(format!("timeout={}", self.timeout));
        }
        if !self.crs.trim().is_empty() {
            line.push(format!("crs={}", self.crs.trim().replace(' ', "")));
        }
        parse_online_command(&line.join(" "))
    }

    /// What onlineStatus says; an error is shown as one.
    pub fn set_status(&mut self, text: &str, is_error: bool) {
        self.status = String::from(text);
        self.status_error = is_error;
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    fn import_chosen(&mut self, context: &mut dyn OnlineDialogContext) {
        let parsed = match self.command() {
            Ok(parsed) => parsed,
            Err(error) => {
                let said = if error.context.is_empty() {
                    error.message.clone()
                } else {
                    format!("{} ({})", error.message, error.context)
                };
                self.set_status(&said, true);
                return;
            }
        };
        // Said first: a run that finishes at once (a headless session waits for it) reports its
        // outcome here, which this must not then overwrite.
        self.set_status(
            &format!(
                "Importing {} {} in the background - the status bar shows its progress and can \
                 cancel it.",
                parsed.provider, parsed.layer
            ),
            false,
        );
        if let Err(error) = context.run(&parsed) {
            self.set_status(&error.describe(), true);
        }
    }

    fn add_custom_service(&mut self, context: &mut dyn OnlineDialogContext) {
        let url = String::from(self.custom_url.trim());
        if url.is_empty() {
            self.set_status(
                "Type the service's address beside Add Custom Service first.",
                true,
            );
            return;
        }
        // Said first, for the reason import_chosen gives.
        self.set_status(
            &format!("Asking {} what it offers...", redact_url(&url)),
            false,
        );
        if let Err(error) = context.add_custom(&url) {
            self.set_status(&error.describe(), true);
        }
    }

    /// Runs ONLINE LAYERS <provider> <words> for the chosen catalogue layer.
    fn search_catalogue(&mut self, context: &mut dyn OnlineDialogContext) {
        let Some(layer) = self
            .chosen_layer()
            .filter(|layer| layer.kind == OnlineLayerKind::Catalogue)
            .cloned()
        else {
            self.set_status("Choose a catalogue in the list first.", true);
            return;
        };
        // QString::simplified: the words a blank apart.
        let words = self
            .catalogue_search
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if words.is_empty() {
            self.set_status("Type what to search the catalogue for first.", true);
            return;
        }
        // The line the verb would be, read by the verb's own parser, as Import's.
        let parsed =
            match parse_online_command(&format!("ONLINE LAYERS {} {words}", layer.provider_id)) {
                Ok(parsed) => parsed,
                Err(error) => {
                    self.set_status(&error.describe(), true);
                    return;
                }
            };
        self.results.clear();
        self.result = None;
        // Said first, for the reason import_chosen gives.
        self.set_status(
            &format!("Searching {} for \"{words}\"...", layer.provider_title),
            false,
        );
        if let Err(error) = context.run(&parsed) {
            self.set_status(&error.describe(), true);
        }
    }

    /// What a catalogue search found - the workbench's answer to the ONLINE LAYERS line it ran,
    /// the dialog's or a typed one - listed in onlineCatalogueResults for Add to take a service
    /// from.
    pub fn show_catalogue_results(&mut self, found: &[CkanResource]) {
        self.results = found.to_vec();
        self.result = if found.is_empty() { None } else { Some(0) };
        let said = if found.is_empty() {
            String::from("The catalogue found no web service for those words.")
        } else {
            format!(
                "{} web service{} found: choose one and Add Service.",
                found.len(),
                if found.len() == 1 { "" } else { "s" }
            )
        };
        self.set_status(&said, false);
    }

    /// The results as onlineCatalogueResults lists them: title, dataset, format and the address
    /// with its key taken out.
    pub fn catalogue_results(&self) -> Vec<Vec<String>> {
        self.results
            .iter()
            .map(|resource| {
                vec![
                    resource.title.clone(),
                    resource.dataset.clone(),
                    resource.format.clone(),
                    redact_url(&resource.url),
                ]
            })
            .collect()
    }

    /// Runs ONLINE CUSTOM <url> for the chosen result.
    fn add_catalogue_result(&mut self, context: &mut dyn OnlineDialogContext) {
        let Some(url) = self
            .result
            .and_then(|at| self.results.get(at))
            .map(|resource| resource.url.clone())
        else {
            self.set_status("Choose a service in the results first.", true);
            return;
        };
        // Quoted, as the verb's words may be: an address is one word however the catalogue
        // spelt it.
        let parsed = match parse_online_command(&format!("ONLINE CUSTOM \"{url}\"")) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.set_status(&error.describe(), true);
                return;
            }
        };
        // Said first, for the reason import_chosen gives.
        self.set_status(
            &format!("Asking {} what it offers...", redact_url(&url)),
            false,
        );
        if let Err(error) = context.run(&parsed) {
            self.set_status(&error.describe(), true);
        }
    }

    fn save_key(&mut self, context: &mut dyn OnlineDialogContext) {
        let Some(key_name) = self.chosen_layer().map(|layer| layer.key_name.clone()) else {
            return;
        };
        context.save_key(&key_name, &self.key);
        let removed = self.key.is_empty();
        self.key.clear();
        let said = if removed {
            format!("The key for {key_name} was removed.")
        } else {
            format!("The key for {key_name} is stored in Katana's settings.")
        };
        self.set_status(&said, false);
    }

    /// Set Project CRS...: the typed box's centre suggested, when it is in longitude and
    /// latitude.
    fn set_project_crs(&mut self, context: &mut dyn OnlineDialogContext) {
        let mut place = None;
        if self.area_kind() == OnlineAreaKind::Box && self.box_crs.index == Some(1) {
            let text = self.typed_box.replace(' ', "");
            let parts: Vec<Option<f64>> = text.split(',').map(parse_finite_double).collect();
            if let [Some(west), Some(south), Some(east), Some(north)] = parts.as_slice() {
                place = Some(((west + east) / 2.0, (south + north) / 2.0));
            }
        }
        if context.choose_project_crs(place) {
            self.refresh_project_crs(&*context);
            self.set_status(
                "The project's coordinate system is set: imports go into it.",
                false,
            );
        }
    }

    /// One frame of the dialog, when it is shown.
    pub fn ui(&mut self, ui: &mut Ui, context: &mut dyn OnlineDialogContext) {
        if !self.visible {
            return;
        }
        // The project's system as it is now: it may have been set since the dialog last looked.
        self.refresh_project_crs(&*context);
        let ctx = ui.ctx().clone();
        let mut frame = self.frame.clone();
        let mut lent: (&mut Self, &mut dyn OnlineDialogContext) = (&mut *self, &mut *context);
        let shown = frame.show(
            &ctx,
            &mut lent,
            |ui, lent| lent.0.body(ui, &mut *lent.1),
            |ui, lent| lent.0.footer(ui, &mut *lent.1),
        );
        self.frame = frame;
        if shown.closed {
            self.hide();
        }
    }

    fn body(&mut self, ui: &mut Ui, context: &mut dyn OnlineDialogContext) {
        ui.set_max_width(BODY_WIDTH);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_max_width(450.0);
                self.left(ui, context);
            });
            ui.vertical(|ui| {
                self.right(ui, context);
            });
        });
    }

    /// The search, the tree, and Add Custom Service.
    fn left(&mut self, ui: &mut Ui, context: &mut dyn OnlineDialogContext) {
        let searched = line_with(
            ui,
            "onlineSearch",
            &mut self.search,
            LineOptions {
                placeholder: "Search providers and layers",
                width: Some(f32::INFINITY),
                ..LineOptions::default()
            },
        )
        .changed();
        if searched {
            let text = self.search.clone();
            self.apply_filter(&text);
        }
        self.tree_ui(ui);
        let add = ui
            .horizontal(|ui| {
                line_with(
                    ui,
                    "onlineCustomUrl",
                    &mut self.custom_url,
                    LineOptions {
                        placeholder: "https://... a WMS, WMTS, WFS, WCS, ArcGIS REST, OGC API, \
                                      STAC or COG address",
                        ..LineOptions::default()
                    },
                );
                button(ui, "onlineAddCustom", "Add Custom Service...")
                    .on_hover_text(
                        "Ask the service at this address what it offers, and add its layers \
                         under Custom",
                    )
                    .clicked()
            })
            .inner;
        if add {
            self.add_custom_service(context);
        }
    }

    /// The provider tree, named onlineProviders: a fill chooses the layer of that title, as a
    /// click on it does.
    fn tree_ui(&mut self, ui: &mut Ui) {
        let name = "onlineProviders";
        let taken = take(ui, name);
        if let Some(taken) = &taken {
            match &taken.posted.act {
                Act::Fill(text) => {
                    fn titled(items: &[TreeItem], title: &str) -> Option<(String, String)> {
                        for item in items.iter().filter(|item| !item.hidden) {
                            if item.is_layer() && item.text == title {
                                return Some((item.provider.clone(), item.layer.clone()));
                            }
                            if let Some(found) = titled(&item.children, title) {
                                return Some(found);
                            }
                        }
                        None
                    }
                    if ui.is_visible() && !ui.is_enabled() {
                        settle(ui, taken, Err(fill_disabled(&taken.posted)));
                    } else if let Some(found) = titled(&self.tree, text) {
                        self.current = Some(found);
                        settle(ui, taken, Ok(()));
                    } else {
                        settle(ui, taken, Err(lists_no(name, text)));
                    }
                }
                Act::Press => settle(ui, taken, Err(no_button(&taken.posted))),
                Act::ScrollTo => {}
            }
        }
        fn draw(
            ui: &mut Ui,
            items: &mut [TreeItem],
            depth: usize,
            current: &mut Option<(String, String)>,
            rows: &mut Vec<String>,
        ) {
            for item in items.iter_mut().filter(|item| !item.hidden) {
                let indent = "    ".repeat(depth);
                if item.is_layer() {
                    rows.push(format!("{} | {}", item.text, item.kind));
                    let chosen = current
                        .as_ref()
                        .is_some_and(|(provider, layer)| {
                            *provider == item.provider && *layer == item.layer
                        });
                    let shown = format!("{indent}{}   {}", item.text, item.kind);
                    if ui
                        .selectable_label(chosen, shown)
                        .on_hover_text(format!("{} {}", item.provider, item.layer))
                        .clicked()
                    {
                        *current = Some((item.provider.clone(), item.layer.clone()));
                    }
                    continue;
                }
                rows.push(item.text.clone());
                let mark = if item.expanded { "\u{25be}" } else { "\u{25b8}" };
                if ui
                    .selectable_label(false, format!("{indent}{mark} {}", item.text))
                    .clicked()
                {
                    item.expanded = !item.expanded;
                }
                if item.expanded {
                    draw(ui, &mut item.children, depth + 1, current, rows);
                }
            }
        }
        let mut rows = Vec::new();
        let tree = &mut self.tree;
        let current = &mut self.current;
        let area = egui::ScrollArea::vertical()
            .id_salt(name)
            .max_height(420.0)
            .auto_shrink([false, false])
            .show(ui, |ui| draw(ui, tree, 0, current, &mut rows))
            .inner_rect;
        let response = ui.interact(area, ui.id().with(name), egui::Sense::hover());
        if let Some(taken) = &taken
            && taken.posted.act == Act::ScrollTo
        {
            response.scroll_to_me(None);
            settle(ui, taken, Ok(()));
        }
        let mut record = ControlRecord::new(name, ControlKind::Rows);
        record.items = rows;
        custom(ui, &response, record);
    }

    /// The details, a catalogue's search, the import's fields and the Advanced group.
    fn right(&mut self, ui: &mut Ui, context: &mut dyn OnlineDialogContext) {
        let details = self.details(&mut *context);
        label_as(
            ui,
            "onlineDetails",
            &details,
            egui::Label::new(details.as_str()).wrap(),
        );

        // A catalogue layer's search, in place of the import it cannot do.
        let is_catalogue = self
            .chosen_layer()
            .is_some_and(|layer| layer.kind == OnlineLayerKind::Catalogue);
        let (search, add) = if is_catalogue {
            self.catalogue_ui(ui)
        } else {
            hidden(ui, "onlineCatalogue", |ui| self.catalogue_ui(ui))
        };

        let applies = self.applies();
        ui.horizontal(|ui| {
            ui.label("Area:");
            choice(ui, "onlineArea", &self.area_items, &mut self.area);
        });
        let typed = self.area_kind() == OnlineAreaKind::Box;
        ui.horizontal(|ui| {
            ui.label("Box:");
            ui.add_enabled_ui(typed, |ui| {
                line_with(
                    ui,
                    "onlineBox",
                    &mut self.typed_box,
                    LineOptions {
                        placeholder: "x0,y0,x1,y1",
                        ..LineOptions::default()
                    },
                );
                choice(ui, "onlineBoxCrs", &self.box_crs_items, &mut self.box_crs);
            });
        });
        ui.horizontal(|ui| {
            ui.label("Resolution:");
            ui.add_enabled_ui(applies.resolution_auto, |ui| {
                check(
                    ui,
                    "onlineResolutionAuto",
                    "Automatic",
                    &mut self.resolution_auto,
                );
            });
            ui.add_enabled_ui(applies.resolution, |ui| {
                spin(
                    ui,
                    "onlineResolution",
                    &mut self.resolution,
                    SpinRange::new(0.01, 100_000.0, 1.0, 3).with_suffix(" per pixel"),
                );
            });
        });
        ui.horizontal(|ui| {
            ui.label("Target layer:");
            ui.add_enabled_ui(applies.target_layer, |ui| {
                line_with(
                    ui,
                    "onlineTargetLayer",
                    &mut self.target_layer,
                    LineOptions {
                        placeholder: "online/<provider>/<layer>",
                        ..LineOptions::default()
                    },
                );
            });
        });
        ui.horizontal(|ui| {
            ui.label("OpenStreetMap tag:");
            ui.add_enabled_ui(applies.tag, |ui| {
                line_with(
                    ui,
                    "onlineTag",
                    &mut self.tag,
                    LineOptions {
                        placeholder: "the layer's own, or e.g. building, highway=primary",
                        ..LineOptions::default()
                    },
                );
            });
        });
        ui.horizontal(|ui| {
            ui.label("Scene dates:");
            ui.add_enabled_ui(applies.dates, |ui| {
                check(ui, "onlineUseDates", "Between", &mut self.use_dates);
                self.from.ui(ui, "onlineFrom");
                ui.label("and");
                self.to.ui(ui, "onlineTo");
            });
        });
        ui.horizontal(|ui| {
            ui.label("Cloud:");
            ui.add_enabled_ui(applies.dates, |ui| {
                spin(
                    ui,
                    "onlineCloud",
                    &mut self.cloud,
                    SpinRange::new(0.0, 100.0, 1.0, 2).with_suffix(" % cloud at most"),
                );
            });
        });
        // A time-enabled layer's date: the layer's own (the latest) unless one is chosen.
        ui.horizontal(|ui| {
            ui.label("Date:");
            ui.add_enabled_ui(applies.time_default, |ui| {
                check(
                    ui,
                    "onlineTimeDefault",
                    "Default (latest)",
                    &mut self.time_default,
                );
            });
            ui.add_enabled_ui(applies.time, |ui| {
                self.time.ui(ui, "onlineTime");
            });
        });
        // The project's coordinate system, and the way to set it: an import needs one, and the
        // dialog suggests the zone of the box being fetched.
        let set_crs = ui
            .horizontal(|ui| {
                ui.label("Project CRS:");
                label(ui, "onlineProjectCrs", &self.project_crs);
                ui.add_enabled_ui(context.can_choose_project_crs(), |ui| {
                    button(ui, "onlineSetProjectCrs", "Set Project CRS...").clicked()
                })
                .inner
            })
            .inner;
        ui.horizontal(|ui| {
            ui.label("Use CRS:");
            line_with(
                ui,
                "onlineCrs",
                &mut self.crs,
                LineOptions {
                    placeholder: "EPSG:7856 - used, and set on the project, when it has none",
                    ..LineOptions::default()
                },
            );
        });
        let save = ui
            .horizontal(|ui| {
                ui.label("Key:");
                ui.add_enabled_ui(applies.key, |ui| {
                    line_with(
                        ui,
                        "onlineKey",
                        &mut self.key,
                        LineOptions {
                            placeholder: "kept in Katana's settings, never in the project",
                            password: true,
                            ..LineOptions::default()
                        },
                    );
                    button(ui, "onlineSaveKey", "Save Key").clicked()
                })
                .inner
            })
            .inner;

        // Advanced: a deadline. A person can always cancel from the status bar; this is for an
        // import left to run, and for a script's.
        let advanced = ui.group(|ui| {
            ui.label("Advanced");
            ui.horizontal(|ui| {
                check(ui, "onlineTimeoutOn", "Give up after", &mut self.timeout_on);
                ui.add_enabled_ui(self.timeout_on, |ui| {
                    // The range ONLINE IMPORT's timeout= takes (parse_online_command).
                    whole(ui, "onlineTimeout", &mut self.timeout, 1..=86_400);
                    ui.label("s");
                });
            });
        });
        custom(
            ui,
            &advanced.response,
            ControlRecord::new("onlineAdvanced", ControlKind::Other),
        );

        if search {
            self.search_catalogue(context);
        }
        if add {
            self.add_catalogue_result(context);
        }
        if set_crs {
            self.set_project_crs(context);
        }
        if save {
            self.save_key(context);
        }
    }

    /// The catalogue group; answers whether Search and Add Service were pressed.
    fn catalogue_ui(&mut self, ui: &mut Ui) -> (bool, bool) {
        let mut search = false;
        let mut add = false;
        let group = ui.group(|ui| {
            ui.label("Search this catalogue for services");
            ui.horizontal(|ui| {
                let typed = line_with(
                    ui,
                    "onlineCatalogueSearch",
                    &mut self.catalogue_search,
                    LineOptions {
                        placeholder: "words, e.g. flood mapping",
                        ..LineOptions::default()
                    },
                );
                // Return in the field searches, as the button does.
                let entered =
                    typed.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                search = button(ui, "onlineCatalogueRun", "Search").clicked() || entered;
            });
            let rows = self.catalogue_results();
            let shown = egui::ScrollArea::vertical()
                .id_salt("onlineCatalogueResults")
                .max_height(120.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    table(
                        ui,
                        "onlineCatalogueResults",
                        &["Title", "Dataset", "Format", "Address"],
                        &rows,
                        self.result,
                        &[],
                        None,
                    )
                })
                .inner;
            if shown.chosen.is_some() {
                self.result = shown.chosen;
            }
            // A double click on a result adds it, as the button does.
            if shown.double_clicked.is_some() {
                self.result = shown.double_clicked;
                add = true;
            }
            ui.add_enabled_ui(self.result.is_some(), |ui| {
                if button(ui, "onlineCatalogueAdd", "Add Service")
                    .on_hover_text(
                        "Ask the chosen service what it offers and add its layers under Custom, \
                         as ONLINE CUSTOM <address> does",
                    )
                    .clicked()
                {
                    add = true;
                }
            });
        });
        custom(
            ui,
            &group.response,
            ControlRecord::new("onlineCatalogue", ControlKind::Other),
        );
        (search, add)
    }

    /// The status, Import and Close: kept below the scrolling body.
    fn footer(&mut self, ui: &mut Ui, context: &mut dyn OnlineDialogContext) {
        let importable = self
            .chosen_layer()
            .is_some_and(|layer| layer.kind != OnlineLayerKind::Catalogue);
        let status = self.status.clone();
        let is_error = self.status_error;
        let (import, close) = ui
            .horizontal(|ui| {
                let mut text = RichText::new(status.as_str());
                if is_error {
                    text = text.color(palette::error().to_egui());
                }
                label_as(ui, "onlineStatus", &status, egui::Label::new(text).wrap());
                let import = ui
                    .add_enabled_ui(importable, |ui| {
                        button(ui, "onlineImport", "Import").clicked()
                    })
                    .inner;
                let close = button(ui, "onlineClose", "Close").clicked();
                (import, close)
            })
            .inner;
        if import {
            self.import_chosen(context);
        }
        if close {
            self.hide();
        }
    }
}

/// A layer's limits in a person's words, by its kind of service: the facts ONLINE INFO gives as
/// max_zoom, page_size and the rest. Empty for a service with none.
pub fn limits_text(layer: &OnlineLayer) -> String {
    // The same facts, from the same fields, as interop's format_info reports.
    match layer.r#type {
        OnlineServiceType::XyzTiles | OnlineServiceType::Wmts => format!(
            "tiles to zoom {}, at most {} tiles an import",
            layer.max_zoom, layer.max_tiles
        ),
        OnlineServiceType::ArcgisQuery
        | OnlineServiceType::Wfs
        | OnlineServiceType::OgcApiFeatures => format!("{} features a page", layer.page_size),
        OnlineServiceType::ArcgisExport
        | OnlineServiceType::ArcgisExportImage
        | OnlineServiceType::Wms
        | OnlineServiceType::Wcs => format!(
            "at most {0} x {0} pixels a request; a larger area is several",
            layer.max_request_pixels
        ),
        OnlineServiceType::Overpass => format!(
            "at most {} square kilometres an import",
            number(layer.max_area_km2)
        ),
        OnlineServiceType::Stac => format!(
            "scenes of the last {} days, cloud at most {}%",
            layer.days,
            number(layer.max_cloud)
        ),
        OnlineServiceType::Cog | OnlineServiceType::File | OnlineServiceType::Ckan => {
            String::new()
        }
    }
}

/// Whether the layer has a date to choose: a time dimension, or {time} in its address.
pub fn has_time(layer: &OnlineLayer) -> bool {
    !layer.time.is_empty() || layer.endpoint.contains("{time}")
}
