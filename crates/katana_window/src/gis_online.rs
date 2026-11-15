//! GIS > Online - Web Services: the online data workbench (gis_online.hpp; docs/gis_online.md).
//!
//! Everything online the window does belongs to this workbench, which the main window only
//! constructs, hands a few callbacks (`OnlineServices`), and asks to run any line that starts
//! with ONLINE. That keeps the work out of the main window and gives the dialog and the verbs
//! ONE executor: the dialog builds an interop OnlineCommand and calls `run`, the command line
//! parses one and calls `run`, and `run` is the only place an import is started, applied and
//! reported.
//!
//! Threading follows the window's jobs (host::jobs): the download, the warp and the reading run
//! in a background job on inputs copied on the interface thread (the layer, the area already in
//! coordinates, the keys); the job hands back an apply step that runs on the interface thread,
//! where the entities are added as ONE command (so one undo removes an import) or the raster is
//! added to the reference data. Cancelling the job from the status bar stops the transfer;
//! nothing is applied. A headless session (--command) blocks on the job, with a deadline
//! (timeout=, 600 s by default) after which the job is cancelled.
//!
//! Settings, never the project: keys under online/keys/<name> in Katana's settings, the user
//! catalogue as online_sources.json in the application's configuration folder, the cache in its
//! cache folder (KATANA_ONLINE_CACHE overrides it, for tests and for a shared cache).
//!
//! THE RUST SHAPE. What the C++ services carried besides the version and the project coordinate
//! system dialog - the drawing, the views, the log, whether the session is headless, the
//! reference data - is the `Host` each call is lent. A job's end is told on the interface thread
//! with the window but without this workbench, so what the end changes here - the dialog's
//! status, its search results, the catalogue read again - is queued (`OnlineEvent`) and taken at
//! the workbench's next look: straight after a wait, at the start of every call and every frame.
//! What a job logs it logs at once. A deadline is kept by a watcher beside the transfer on the
//! job's own thread, where the C++ set a timer on the interface thread: a headless wait here is
//! the host's, and runs no timers.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use egui::Ui;
use katana_commands::{Transaction, create_entities, create_layer};
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result, Status};
use katana_entity::entity::to_string;
use katana_entity::entity_geometry::bounding_box;
use katana_entity::tables::Layer;
use katana_geometry::Box2;
use katana_gis::CrsBox;
use katana_gis::reproject::{crs_epsg_code, crs_to_wkt};
use katana_gis::web_access::redact_url;
use katana_interop::online_catalogue::{
    built_in_catalogue, catalogue_to_json, merge_catalogues, parse_catalogue,
};
use katana_interop::online_fetch::{
    discover_online, extract_key, fetch_online_layer, online_user_agent, prune_cache,
    search_catalogue,
};
use katana_interop::online_verbs::{
    format_catalogue_search, format_error, format_import, format_info, format_layers,
    format_providers, parse_online_command, reply_value,
};
use katana_interop::{
    CkanResource, OnlineAreaKind, OnlineCatalogue, OnlineCommand, OnlineEnvironment, OnlineImport,
    OnlineLayer, OnlineLayerKind, OnlineProvider, OnlineRequestOptions, OnlineServiceType,
    OnlineVerb,
};

use crate::drive::{Action, Menu, ToolBar};
use crate::gis_online_dialog::{OnlineDataDialog, OnlineDialogContext};
use crate::host::{
    Host, JobControl, JobOutcome, JobReport, UserSettings, job_apply, job_finished, job_work,
};
use crate::shell::workbench::{DockSpec, Workbench};

/// A headless import that has not finished in this long is cancelled: a scripted run must end,
/// and ten minutes is far past any import the limits allow on a working connection.
const HEADLESS_DEADLINE_SECONDS: i32 = 600;
/// What the cache may hold before its oldest answers go, and how long any answer is kept at
/// most: two gigabytes, three months.
const CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const CACHE_DAYS: i32 = 90;
/// How often a deadline's watcher looks at the clock: a twentieth of a second, well under what a
/// person notices and far above the cost of looking.
const DEADLINE_LOOK: Duration = Duration::from_millis(50);

/// The object name of the workbench's action.
const ACTION: &str = "onlineData";

/// Opens File > Project Coordinate System, suggesting systems for a longitude and latitude when
/// there is one; true when the project's coordinate system changed.
pub type ChooseProjectCrs = Box<dyn FnMut(&mut dyn Host, Option<(f64, f64)>) -> bool>;

/// What the workbench asks of the window besides the `Host` each call is lent.
#[derive(Default)]
pub struct OnlineServices {
    /// The version the User-Agent names.
    pub version: String,
    pub choose_project_crs: Option<ChooseProjectCrs>,
}

/// What a job's end changes in the workbench, taken at its next look (see the module comment).
enum OnlineEvent {
    /// The dialog's status line.
    Status(String, bool),
    /// What a catalogue search found, for the dialog to list.
    Results(Vec<CkanResource>),
    /// The catalogue read again from its files, and the dialog's tree with it.
    Reload,
    /// The dialog's tree rebuilt: a key was stored or removed.
    Refresh,
    /// A layer chosen in the dialog's tree.
    Select(String, String),
}

type Events = Rc<RefCell<Vec<OnlineEvent>>>;

fn error_line(verb: OnlineVerb, message: &str) -> String {
    format!(
        "error verb={} message={}\n",
        to_string(verb),
        reply_value(message)
    )
}

/// A reply as the log takes it: without the line breaks it ends on.
fn reply(host: &mut dyn Host, text: &str, is_error: bool) {
    host.log(text.trim_end_matches('\n'), is_error);
}

fn environment_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn key_setting(name: &str) -> String {
    format!("online/keys/{name}")
}

fn has_key(settings: &UserSettings, name: &str) -> bool {
    settings
        .text(&key_setting(name))
        .is_some_and(|value| !value.is_empty())
}

fn save_key(settings: &mut UserSettings, name: &str, value: &str) {
    if value.is_empty() {
        settings.remove(&key_setting(name));
    } else {
        settings.set_text(&key_setting(name), value);
    }
}

/// Where the user catalogue is kept: beside the person's settings, unless
/// KATANA_ONLINE_CATALOGUE names another file.
fn user_catalogue_path() -> PathBuf {
    if let Some(overridden) = environment_path("KATANA_ONLINE_CATALOGUE") {
        return overridden;
    }
    UserSettings::default_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default()
        .join("online_sources.json")
}

/// Where answers are cached: the person's cache folder, unless KATANA_ONLINE_CACHE names
/// another.
fn cache_directory() -> PathBuf {
    if let Some(overridden) = environment_path("KATANA_ONLINE_CACHE") {
        return overridden;
    }
    let base = if cfg!(windows) {
        environment_path("LOCALAPPDATA").map(|folder| folder.join("Katana").join("cache"))
    } else {
        environment_path("XDG_CACHE_HOME")
            .filter(|folder| folder.is_absolute())
            .or_else(|| environment_path("HOME").map(|home| home.join(".cache")))
            .map(|folder| folder.join("katana"))
    };
    base.unwrap_or_default().join("online")
}

/// The workbench without its dialog: what the dialog's context and the verbs both run.
struct Core {
    services: OnlineServices,
    catalogue: OnlineCatalogue,
    events: Events,
}

impl Core {
    /// The built-in catalogue with the user's merged over it, read again.
    fn reload_catalogue(&mut self, host: &mut dyn Host) {
        self.catalogue = match built_in_catalogue() {
            Ok(built_in) => built_in,
            Err(error) => {
                reply(
                    host,
                    &format!(
                        "the built-in online catalogue could not be read: {}",
                        error.describe()
                    ),
                    true,
                );
                OnlineCatalogue::default()
            }
        };
        let user = user_catalogue_path();
        if user.is_file() {
            let parsed = std::fs::read(&user)
                .map_err(|error| {
                    make_error(
                        ErrorCode::FileImportFailure,
                        format!("{error}"),
                        String::new(),
                    )
                })
                .and_then(|bytes| parse_catalogue(bytes, true));
            match parsed {
                Err(error) => reply(
                    host,
                    &format!(
                        "the online catalogue {} was not read: {}",
                        user.to_string_lossy(),
                        error.describe()
                    ),
                    true,
                ),
                Ok(parsed) => {
                    self.catalogue =
                        merge_catalogues(std::mem::take(&mut self.catalogue), &parsed);
                }
            }
        }
    }

    fn environment(&self, host: &mut dyn Host) -> OnlineEnvironment {
        let mut environment = OnlineEnvironment {
            cache_directory: cache_directory(),
            user_agent: online_user_agent(&self.services.version),
            ..OnlineEnvironment::default()
        };
        let settings = host.settings();
        for name in settings.child_keys("online/keys") {
            let value = String::from(settings.text(&key_setting(&name)).unwrap_or(""));
            environment.keys.insert(name, value);
        }
        environment
    }

    fn area_of(host: &mut dyn Host, command: &OnlineCommand) -> Result<CrsBox> {
        let mut area = Box2::default();
        match command.area {
            OnlineAreaKind::Box => return Ok(command.r#box),
            OnlineAreaKind::View => {
                let view = host.views().and_then(|views| {
                    let plan = views.active_plan_view()?;
                    let set = views.view_set();
                    let set = set.borrow();
                    set.find(plan).map(|state| state.plan)
                });
                let Some(view) = view else {
                    return Err(make_error(
                        ErrorCode::InvalidState,
                        "there is no plan view to take the area from",
                        String::new(),
                    ));
                };
                let half_width = 0.5 * view.width_pixels / view.scale;
                let half_height = 0.5 * view.height_pixels / view.scale;
                return Ok(CrsBox {
                    min_x: view.center.x - half_width,
                    min_y: view.center.y - half_height,
                    max_x: view.center.x + half_width,
                    max_y: view.center.y + half_height,
                });
            }
            OnlineAreaKind::Drawing => {
                area = host.document().model().entities.bounds();
                if area.empty() {
                    return Err(make_error(
                        ErrorCode::InvalidState,
                        "the drawing is empty, so it has no extents; use area=view or a box",
                        String::new(),
                    ));
                }
            }
            OnlineAreaKind::Selection => {
                let document = host.document();
                let entities = &document.model().entities;
                for id in document.selection().ids() {
                    if let Some(entity) = entities.find(id) {
                        area.expand_box(bounding_box(&entity.geometry));
                    }
                }
                if area.empty() {
                    return Err(make_error(
                        ErrorCode::InvalidState,
                        "nothing is selected to take the area from",
                        String::new(),
                    ));
                }
            }
        }
        // A single point or a straight line has no area; give it a margin of a tenth of its
        // size, and at least ten metres, so a selected lot or a selected point asks for its
        // surroundings rather than for nothing.
        let margin = f64::max(
            10.0,
            0.1 * f64::max(area.max.x - area.min.x, area.max.y - area.min.y),
        );
        Ok(CrsBox {
            min_x: area.min.x - margin,
            min_y: area.min.y - margin,
            max_x: area.max.x + margin,
            max_y: area.max.y + margin,
        })
    }

    /// Runs one command. An Import starts a background job (and in a headless session waits for
    /// it); the others answer at once. Fails only when the command cannot start; how it ended is
    /// logged.
    fn run(&mut self, host: &mut dyn Host, command: &OnlineCommand) -> Status {
        match command.verb {
            OnlineVerb::Providers => {
                let text = format_providers(&self.catalogue.filter(&command.filter));
                reply(host, &text, false);
                Ok(())
            }
            OnlineVerb::Layers => self.layers(host, command),
            OnlineVerb::Info => {
                let layer = self
                    .catalogue
                    .find_provider(&command.provider)
                    .and_then(|provider| provider.find_layer(&command.layer));
                let Some(layer) = layer else {
                    return Err(make_error(
                        ErrorCode::NotFound,
                        format!(
                            "no layer {} {}; ONLINE LAYERS <provider> lists them",
                            command.provider, command.layer
                        ),
                        String::new(),
                    ));
                };
                let text = format_info(layer);
                reply(host, &text, false);
                Ok(())
            }
            OnlineVerb::Key => {
                save_key(host.settings(), &command.key_name, &command.key_value);
                // The key itself is never repeated, not even here.
                let stored = if command.key_value.is_empty() {
                    String::from(" stored=no")
                } else {
                    format!(" stored=yes length={}", command.key_value.len())
                };
                reply(
                    host,
                    &format!(
                        "key name={}{stored} where=settings\n",
                        reply_value(&command.key_name)
                    ),
                    false,
                );
                self.events.borrow_mut().push(OnlineEvent::Refresh);
                Ok(())
            }
            OnlineVerb::Custom => self.start_discovery(host, &command.url),
            OnlineVerb::Import => self.start_import(host, command),
        }
    }

    /// ONLINE LAYERS: a provider's layers, or - for a catalogue with words - a search.
    fn layers(&mut self, host: &mut dyn Host, command: &OnlineCommand) -> Status {
        let Some(provider) = self.catalogue.find_provider(&command.provider) else {
            return Err(make_error(
                ErrorCode::NotFound,
                format!(
                    "no provider {}; ONLINE PROVIDERS lists them",
                    command.provider
                ),
                String::new(),
            ));
        };
        let layers = provider.layers();
        let searched = layers
            .first()
            .filter(|first| !command.filter.is_empty() && first.r#type == OnlineServiceType::Ckan);
        let Some(first) = searched else {
            let text = format_layers(provider);
            reply(host, &text, false);
            return Ok(());
        };
        // A catalogue search is a request, so it runs off the interface thread like every
        // other.
        let layer: OnlineLayer = (*first).clone();
        let title = format!("Searching {}", layer.provider_title);
        let words = command.filter.clone();
        let environment = self.environment(host);
        let found: Arc<Mutex<Vec<CkanResource>>> = Arc::new(Mutex::new(Vec::new()));
        let found_by_work = Arc::clone(&found);
        let work = job_work(move |control: &JobControl| {
            control.set_stage("Searching");
            let resources =
                search_catalogue(&layer, &words, &environment, Some(control.stop_flag()))?;
            *found_by_work.lock().unwrap_or_else(PoisonError::into_inner) = resources;
            Ok(None)
        });
        let events = Rc::clone(&self.events);
        let finished = job_finished(move |report: &JobReport, host: &mut dyn Host| {
            if report.outcome == JobOutcome::Finished {
                let found = found.lock().unwrap_or_else(PoisonError::into_inner).clone();
                reply(host, &format_catalogue_search(&found), false);
                // The dialog lists them to add from, whoever asked.
                events.borrow_mut().push(OnlineEvent::Results(found));
            } else {
                let why = if report.outcome == JobOutcome::Cancelled {
                    String::from("cancelled")
                } else {
                    report.error.clone()
                };
                reply(host, &error_line(OnlineVerb::Layers, &why), true);
                events.borrow_mut().push(OnlineEvent::Status(why, true));
            }
        });
        let id = host.jobs().start(&title, work, Some(finished));
        if host.headless() {
            host.wait_for(id);
        }
        Ok(())
    }

    /// Discovers the service at `url` and adds it to the user catalogue, in a background job.
    fn start_discovery(&mut self, host: &mut dyn Host, url: &str) -> Status {
        let environment = self.environment(host);
        let found: Arc<Mutex<OnlineProvider>> = Arc::new(Mutex::new(OnlineProvider::default()));
        let found_by_work = Arc::clone(&found);
        let address = String::from(url);
        let work = job_work(move |control: &JobControl| {
            control.set_stage("Reading the service's description");
            let provider = discover_online(&address, &environment, Some(control.stop_flag()))?;
            *found_by_work.lock().unwrap_or_else(PoisonError::into_inner) = provider;
            Ok(None)
        });
        let events = Rc::clone(&self.events);
        let finished = job_finished(move |report: &JobReport, host: &mut dyn Host| {
            if report.outcome != JobOutcome::Finished {
                let why = if report.outcome == JobOutcome::Cancelled {
                    String::from("cancelled")
                } else {
                    report.error.clone()
                };
                reply(host, &error_line(OnlineVerb::Custom, &why), true);
                events.borrow_mut().push(OnlineEvent::Status(why, true));
                return;
            }
            let mut found = found.lock().unwrap_or_else(PoisonError::into_inner).clone();
            if let Err(why) = add_to_user_catalogue(host, &mut found) {
                reply(host, &error_line(OnlineVerb::Custom, &why), true);
                return;
            }
            reply(host, &format_layers(&found), false);
            let mut events = events.borrow_mut();
            events.push(OnlineEvent::Reload);
            events.push(OnlineEvent::Status(
                format!("Added {} under Custom.", found.title),
                false,
            ));
            if let Some(first) = found.layers().first() {
                events.push(OnlineEvent::Select(found.id.clone(), first.id.clone()));
            }
        });
        let title = format!("Asking {}", redact_url(url));
        let id = host.jobs().start(&title, work, Some(finished));
        if host.headless() {
            host.wait_for(id);
        }
        Ok(())
    }

    fn start_import(&mut self, host: &mut dyn Host, command: &OnlineCommand) -> Status {
        let found = self
            .catalogue
            .find_provider(&command.provider)
            .and_then(|provider| provider.find_layer(&command.layer));
        let Some(found) = found else {
            return Err(make_error(
                ErrorCode::NotFound,
                format!(
                    "no layer {} {}; ONLINE LAYERS <provider> lists them",
                    command.provider, command.layer
                ),
                String::new(),
            ));
        };
        if found.kind == OnlineLayerKind::Catalogue {
            return Err(make_error(
                ErrorCode::Unsupported,
                format!(
                    "a catalogue is searched, not imported: ONLINE LAYERS {} <words>",
                    found.provider_id
                ),
                String::new(),
            ));
        }
        // Everything the job reads is copied here, on the interface thread.
        let layer: OnlineLayer = found.clone();
        // The project's CRS, or the one given for a project that has none - which then becomes
        // the project's, so the next import agrees with this one.
        let mut crs = host.document().metadata().coordinate_system.clone();
        // Set on the project only when the import succeeds (in its apply): a failed or
        // cancelled import changes nothing.
        let mut crs_to_set = String::new();
        if !command.crs.is_empty() {
            crs_to_wkt(&command.crs)?;
            if crs.is_empty() {
                crs.clone_from(&command.crs);
                crs_to_set.clone_from(&command.crs);
            } else {
                // The same system however it is spelt: epsg:7856 is EPSG:7856.
                let given = crs_epsg_code(&command.crs);
                let project = crs_epsg_code(&crs);
                if given.is_none() || project.is_none() || given != project {
                    return Err(make_error(
                        ErrorCode::InvalidCrs,
                        format!("the project is in {crs}; crs= is for a project that has none"),
                        String::new(),
                    ));
                }
            }
        }
        if crs.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidCrs,
                "the project has no coordinate system, so web data has nowhere to go; set one \
                 with File > Project Coordinate System or CRS SET EPSG:<code> (CRS SUGGEST \
                 lon,lat names the zone; Sydney in GDA2020 is EPSG:7856), or add \
                 crs=EPSG:<code> to this import",
                String::new(),
            ));
        }
        let area = Self::area_of(host, command)?;

        let options = OnlineRequestOptions {
            area,
            area_crs: if command.area == OnlineAreaKind::Box && command.box_is_lon_lat {
                String::from("EPSG:4326")
            } else {
                crs.clone()
            },
            target_crs: crs,
            resolution: command.resolution,
            target_layer: command.target_layer.clone(),
            from_date: command.from_date.clone(),
            to_date: command.to_date.clone(),
            max_cloud: command.max_cloud,
            filter: command.tag.clone(),
            time: command.time.clone(),
        };
        let target_layer = if options.target_layer.is_empty() {
            format!("online/{}/{}", layer.provider_id, layer.id)
        } else {
            options.target_layer.clone()
        };
        let environment = self.environment(host);
        // What arrives is framed unless it was asked for the view already on screen: a typed box
        // or the drawing's extent is usually somewhere else, and web data in a real coordinate
        // system lands kilometres - often thousands - from a drawing in local coordinates. Left
        // unframed, a successful import looked like one that had done nothing.
        let frame = command.area != OnlineAreaKind::View;
        // A deadline asked for (timeout=, the dialog's Give up after): the job is cancelled if
        // it is still running then, as the status bar's Cancel would. Without one, a person
        // cancels when they choose - and headless, where nobody can, a deadline does it instead.
        let deadline = if host.headless() {
            Some(command.timeout_seconds.unwrap_or(HEADLESS_DEADLINE_SECONDS))
        } else {
            command.timeout_seconds
        };
        let timed_out = Arc::new(AtomicBool::new(false));
        let timed_out_by_work = Arc::clone(&timed_out);
        let title = format!("Online: {} - {}", layer.provider_title, layer.title);
        let work = job_work(move |control: &JobControl| {
            let _ = prune_cache(&environment.cache_directory, CACHE_DAYS, CACHE_BYTES);
            let progress = |fraction: f64, stage: &str| {
                if fraction >= 0.0 {
                    control.set_progress(fraction);
                }
                control.set_stage(stage);
            };
            let progress: &(dyn Fn(f64, &str) + Sync) = &progress;
            let stop = control.stop_flag();
            let ended = AtomicBool::new(false);
            let fetched = std::thread::scope(|scope| {
                if let Some(seconds) = deadline {
                    let until =
                        Instant::now() + Duration::from_secs(u64::try_from(seconds).unwrap_or(0));
                    let ended = &ended;
                    let timed_out = &timed_out_by_work;
                    scope.spawn(move || {
                        while !ended.load(Ordering::Acquire) {
                            if Instant::now() >= until {
                                stop.store(true, Ordering::Release);
                                timed_out.store(true, Ordering::Release);
                                break;
                            }
                            std::thread::sleep(DEADLINE_LOOK);
                        }
                    });
                }
                let fetched =
                    fetch_online_layer(&layer, &options, &environment, Some(stop), Some(progress));
                ended.store(true, Ordering::Release);
                fetched
            })?;
            Ok(Some(job_apply(move |host: &mut dyn Host| {
                apply_import(host, &layer, fetched, &target_layer, &crs_to_set, frame);
            })))
        });
        let events = Rc::clone(&self.events);
        let finished = job_finished(move |report: &JobReport, host: &mut dyn Host| {
            if timed_out.load(Ordering::Acquire)
                && let Some(seconds) = deadline
            {
                reply(
                    host,
                    &error_line(
                        OnlineVerb::Import,
                        &format!("no answer within {seconds} s; cancelled"),
                    ),
                    true,
                );
            }
            if report.outcome == JobOutcome::Cancelled {
                reply(host, &error_line(OnlineVerb::Import, "cancelled"), true);
            } else if report.outcome == JobOutcome::Failed {
                reply(host, &error_line(OnlineVerb::Import, &report.error), true);
            }
            let said = match report.outcome {
                JobOutcome::Finished => {
                    String::from("Imported - the command log has what and from whom.")
                }
                JobOutcome::Cancelled => String::from("Cancelled."),
                JobOutcome::Failed => report.error.clone(),
            };
            events.borrow_mut().push(OnlineEvent::Status(
                said,
                report.outcome != JobOutcome::Finished,
            ));
        });
        let id = host.jobs().start(&title, work, Some(finished));
        if host.headless() {
            host.wait_for(id);
        }
        Ok(())
    }
}

/// A discovered provider written to the user catalogue, merged by id, so the service is there
/// next time - and a key it needs put in the settings. Why it could not be, otherwise.
fn add_to_user_catalogue(
    host: &mut dyn Host,
    found: &mut OnlineProvider,
) -> std::result::Result<(), String> {
    let path = user_catalogue_path();
    let shown = path.to_string_lossy().into_owned();
    let mut user = OnlineCatalogue::default();
    if path.is_file() {
        let parsed = std::fs::read(&path)
            .map_err(|error| format!("{error}"))
            .and_then(|bytes| parse_catalogue(bytes, true).map_err(|error| error.describe()));
        match parsed {
            Ok(parsed) => user = parsed,
            // Writing now would replace the person's catalogue with this one provider.
            Err(why) => {
                return Err(format!(
                    "the user catalogue {shown} could not be read, so nothing was added to it: \
                     {why}"
                ));
            }
        }
    }
    // A key in a discovered address (a WMTS template's ?api=, a token) goes to the settings, and
    // the address keeps {key}, so the catalogue on disk never holds it.
    let provider_id = found.id.clone();
    for service in &mut found.services {
        for layer in &mut service.layers {
            let (endpoint, secret) = extract_key(&layer.endpoint);
            if !secret.is_empty() {
                layer.endpoint = endpoint;
                layer.key_required = true;
                layer.key_name.clone_from(&provider_id);
                save_key(host.settings(), &provider_id, &secret);
            }
        }
    }
    let added = OnlineCatalogue {
        providers: vec![found.clone()],
    };
    let user = merge_catalogues(user, &added);
    let not_written = || format!("could not write the user catalogue {shown}");
    if let Some(folder) = path.parent() {
        // A folder that cannot be made is found out by the write below.
        let _ = std::fs::create_dir_all(folder);
    }
    let mut partial = path.clone().into_os_string();
    partial.push(".part");
    let partial = PathBuf::from(partial);
    std::fs::write(&partial, catalogue_to_json(&user)).map_err(|_| not_written())?;
    std::fs::rename(&partial, &path).map_err(|_| not_written())?;
    Ok(())
}

/// The third phase of an import, on the interface thread: the project's system set when it had
/// none, the raster added to the reference data or the entities as ONE command, the reply
/// logged, and what arrived framed.
fn apply_import(
    host: &mut dyn Host,
    layer: &OnlineLayer,
    mut result: OnlineImport,
    target_layer: &str,
    crs_to_set: &str,
    frame: bool,
) {
    if !crs_to_set.is_empty() && host.document().metadata().coordinate_system.is_empty() {
        let mut metadata = host.document().metadata().clone();
        metadata.coordinate_system = String::from(crs_to_set);
        host.document_mut().set_metadata(metadata);
        reply(
            host,
            &format!(
                "project coordinate_system={} set=yes\n",
                reply_value(crs_to_set)
            ),
            false,
        );
    }
    let mut added: usize = 0;
    let mut arrived = Box2::default();
    if let Some(mut raster) = result.raster.take() {
        arrived = raster.world_bounds();
        // The pixels go to the reference data; the reply needs only the name, the size and the
        // id, which are kept.
        let pixels = std::mem::take(&mut raster.rgba);
        let mut kept = raster.clone();
        raster.rgba = pixels;
        kept.id = host.reference_mut().add(raster);
        result.raster = Some(kept);
        if let Some(views) = host.views() {
            views.refresh_all();
        }
    } else if let Some(vectors) = result.vectors.as_mut() {
        let mut transaction = Transaction::new("ONLINE IMPORT");
        for name in &vectors.layers_needed {
            if !host.document().model().layers.contains(name) {
                transaction.add(create_layer(Layer {
                    name: name.clone(),
                    ..Layer::default()
                }));
            }
        }
        added = vectors.entities.len();
        arrived = vectors.bounds;
        if added != 0 {
            transaction.add(create_entities(std::mem::take(&mut vectors.entities)));
        }
        // One command, one undo; an import of nothing adds nothing.
        if transaction.size() != 0
            && let Err(error) = host.document_mut().execute(Box::new(transaction))
        {
            reply(host, &format_error(OnlineVerb::Import, &error), true);
            return;
        }
    }
    reply(
        host,
        &format_import(layer, &result, added, target_layer),
        false,
    );
    if frame
        && !arrived.empty()
        && let Some(views) = host.views()
    {
        views.zoom_to(&arrived);
    }
}

/// The workbench as the dialog asks it: its catalogue and executor, and the window's drawing and
/// settings.
struct WorkbenchContext<'a, 'h> {
    core: &'a mut Core,
    host: &'a mut (dyn Host + 'h),
}

impl OnlineDialogContext for WorkbenchContext<'_, '_> {
    fn catalogue(&self) -> &OnlineCatalogue {
        &self.core.catalogue
    }

    fn project_crs(&self) -> String {
        self.host.document().metadata().coordinate_system.clone()
    }

    fn has_key(&mut self, key_name: &str) -> bool {
        has_key(self.host.settings(), key_name)
    }

    fn save_key(&mut self, key_name: &str, value: &str) {
        save_key(self.host.settings(), key_name, value);
    }

    fn run(&mut self, command: &OnlineCommand) -> Status {
        self.core.run(&mut *self.host, command)
    }

    fn add_custom(&mut self, url: &str) -> Status {
        self.core.start_discovery(&mut *self.host, url)
    }

    fn can_choose_project_crs(&self) -> bool {
        self.core.services.choose_project_crs.is_some()
    }

    fn choose_project_crs(&mut self, place: Option<(f64, f64)>) -> bool {
        match self.core.services.choose_project_crs.as_mut() {
            Some(choose) => choose(&mut *self.host, place),
            None => false,
        }
    }
}

/// GIS > Online - Web Services (see the module comment).
pub struct OnlineDataWorkbench {
    core: Core,
    /// The dialog, made the first time it is asked for.
    dialog: Option<OnlineDataDialog>,
}

impl OnlineDataWorkbench {
    /// The workbench over the window, its catalogue read.
    pub fn new(services: OnlineServices, host: &mut dyn Host) -> Self {
        let mut core = Core {
            services,
            catalogue: OnlineCatalogue::default(),
            events: Rc::new(RefCell::new(Vec::new())),
        };
        core.reload_catalogue(host);
        Self { core, dialog: None }
    }

    /// The workbench's menu action, onlineData: it opens the dialog onlineDataDialog.
    pub fn action() -> Action {
        Action::new(ACTION, "O&nline Data...")
            .icon("ImportRaster")
            .tip(
                "Import imagery, elevation and features from public web services - Australian \
                 state and national, and global - into the project's coordinate system",
            )
            .opens("onlineDataDialog")
    }

    pub fn catalogue(&self) -> &OnlineCatalogue {
        &self.core.catalogue
    }

    /// `line` as the command log may show it: an ONLINE KEY's value replaced by ***, everything
    /// else as typed. The masking has one home, the command log's.
    pub fn logged_line(line: &str) -> String {
        crate::command_log::online_logged_line(line)
    }

    /// The built-in catalogue with the user's merged over it, read again.
    pub fn reload_catalogue(&mut self, host: &mut dyn Host) {
        self.core.reload_catalogue(&mut *host);
        let Self { core, dialog } = self;
        if let Some(dialog) = dialog {
            dialog.refresh_catalogue(&WorkbenchContext { core, host });
        }
    }

    /// The dialog, made the first time it is asked for.
    pub fn dialog(&mut self, host: &mut dyn Host) -> &mut OnlineDataDialog {
        let Self { core, dialog } = self;
        dialog.get_or_insert_with(|| OnlineDataDialog::new(&WorkbenchContext { core, host }))
    }

    /// Takes what the jobs' ends have changed since the workbench last looked.
    fn deliver(&mut self, host: &mut dyn Host) {
        loop {
            let events: Vec<OnlineEvent> = std::mem::take(&mut *self.core.events.borrow_mut());
            if events.is_empty() {
                return;
            }
            for event in events {
                if matches!(event, OnlineEvent::Reload) {
                    self.core.reload_catalogue(&mut *host);
                }
                let Self { core, dialog } = self;
                let Some(dialog) = dialog else {
                    continue;
                };
                match event {
                    OnlineEvent::Status(text, is_error) => dialog.set_status(&text, is_error),
                    OnlineEvent::Results(found) => dialog.show_catalogue_results(&found),
                    OnlineEvent::Reload | OnlineEvent::Refresh => {
                        dialog.refresh_catalogue(&WorkbenchContext {
                            core,
                            host: &mut *host,
                        });
                    }
                    OnlineEvent::Select(provider, layer) => {
                        dialog.select_layer(&provider, &layer);
                    }
                }
            }
        }
    }

    /// Runs one command. An Import starts a background job (and in a headless session waits for
    /// it); the others answer at once. Fails only when the command cannot start; how it ended is
    /// logged.
    pub fn run(&mut self, host: &mut dyn Host, command: &OnlineCommand) -> Status {
        self.deliver(&mut *host);
        let started = self.core.run(&mut *host, command);
        self.deliver(host);
        started
    }
}

impl Workbench for OnlineDataWorkbench {
    fn menus(&self) -> Vec<Menu> {
        vec![
            Menu::new("gisMenu", "&GIS")
                .section("Online - Web Services")
                .action(Self::action()),
        ]
    }

    fn tool_bars(&self) -> Vec<ToolBar> {
        Vec::new()
    }

    fn docks(&self) -> Vec<DockSpec> {
        Vec::new()
    }

    /// True when `line` is an ONLINE verb, which it then runs, replying to the log; false leaves
    /// the line to whoever asked.
    fn run_line(&mut self, host: &mut dyn Host, line: &str) -> bool {
        let first = line.trim().split(' ').next().unwrap_or("");
        if !first.eq_ignore_ascii_case("ONLINE") {
            return false;
        }
        let command = match parse_online_command(line) {
            Ok(command) => command,
            Err(error) => {
                let context = if error.context.is_empty() {
                    String::new()
                } else {
                    format!(" context={}", reply_value(&error.context))
                };
                reply(
                    host,
                    &format!(
                        "error verb=ONLINE code={} message={}{context}",
                        error.code,
                        reply_value(&error.message)
                    ),
                    true,
                );
                return true;
            }
        };
        if let Err(error) = self.run(&mut *host, &command) {
            reply(host, &format_error(command.verb, &error), true);
        }
        true
    }

    fn trigger(&mut self, host: &mut dyn Host, name: &str) -> bool {
        if name != ACTION {
            return false;
        }
        self.deliver(&mut *host);
        let Self { core, dialog } = self;
        let context = WorkbenchContext { core, host };
        let shown = dialog.get_or_insert_with(|| OnlineDataDialog::new(&context));
        shown.refresh_catalogue(&context);
        shown.show();
        true
    }

    fn ui(&mut self, ui: &mut Ui, host: &mut dyn Host) {
        self.deliver(&mut *host);
        {
            let Self { core, dialog } = self;
            if let Some(dialog) = dialog {
                dialog.ui(ui, &mut WorkbenchContext { core, host: &mut *host });
            }
        }
        // What a line the dialog ran ended on, when it ended inside the call (a headless wait).
        self.deliver(host);
    }
}
