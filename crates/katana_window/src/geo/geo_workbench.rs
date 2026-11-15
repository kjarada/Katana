//! The geoprocessing verbs in the window (geo/geo_workbench.hpp; docs/desktop.md, "Geoprocessing
//! jobs"; docs/geoprocessing.md): GDAL and the families after it, run by the ONE executor
//! katana_cli and katana_mcp run (`katana_app::geo`).
//!
//! THE THREE PHASES ON THE WINDOW'S JOBS (the plan's section 4.2):
//!
//! 1. On the interface thread, `geo::handles(line)`; when it takes the line, `geo::prepare` runs
//!    with the `Context` the window lends for that call (`Host::geo`). What answers at once - GDAL
//!    LIST, HELP, VERSION, a PREVIEW, a refusal - is logged there.
//! 2. Anything that runs is a job: the `Work` on a job thread with the job's stop flag and a
//!    progress closure that writes the job's progress, which the status bar reads at each frame.
//! 3. The job's end comes back on the interface thread at the start of a frame; the `Apply` runs
//!    with a fresh Context from `Host::geo`, as one undo step, and its reply is logged.
//!
//! A headless run (--command, --run-line, a script) waits for the job, so its outcome is logged,
//! and testable, before the next line; an interactive one logs `job id=<n> title="..."
//! state=started` and goes on. When the job ends, every finished listener is told, which is how a
//! dialog learns what its line did (`await_job`, the window's MainWindow::awaitJob).
//!
//! THE FRONT END'S EFFECTS (decided here). The executor's front-end hooks (`FrontEnd`: frame,
//! changed, far_apart, imported) are called inside a phase, while the window's workspace is
//! borrowed elsewhere - the phase holds the drawing. So the window's hooks do nothing but queue
//! what they were asked (`GeoEffects`): frame this box, the reference rasters changed, show these
//! meshes. The window applies the queue after each phase, when it has its workspace back
//! (`GeoEffects::take`). The far-apart question is a question, not an effect, and egui cannot stop
//! in the middle of an apply to ask it: headless the hook keeps the data's coordinates, as the C++
//! does (nobody to ask); interactively it queues the question and answers Cancel, so nothing is
//! imported, and the window shows the question; the answer is set as the hook's answer for the
//! next time it is asked (`GeoEffects::answer_far_apart`) and the line run again. What differs
//! from the C++, which asked inside the apply: the first run's refusal "cancelled: nothing was
//! imported" is logged before the question is shown.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};

use katana_app::geo::replies::{parse_records, value};
use katana_app::geo::{FarApartChoice, FrontEnd, ImportShown, handles, prepare};
use katana_core::ErrorCode;
use katana_core::error::make_error;
use katana_core::text::parse_integer;
use katana_geometry::Box2;

use crate::drive::{Action, Menu, MenuItem};
use crate::host::{
    Host, JobAwait, JobControl, JobId, JobOutcome, JobReport, Jobs, NO_JOB, VerbOutcome, job_apply,
    job_finished, job_work, with_geo,
};

/// Told when a geoprocessing job ends: its id and what it said, with the window.
pub type FinishedListener = Box<dyn FnMut(JobId, &VerbOutcome, &mut dyn Host)>;

/// A listener's place: empty while it is being told, so a listener whose call runs another line -
/// which ends another job, headless, inside the call - is not told again from within itself.
type ListenerSlot = Rc<RefCell<Option<FinishedListener>>>;

/// The listeners of a workbench's jobs, shared with each job's finish (which has the window, not
/// the workbench) and with whoever listens.
#[derive(Default)]
struct ListenerTable {
    next: i32,
    finished: Option<ListenerSlot>,
    listening: BTreeMap<i32, ListenerSlot>,
}

/// A handle on a workbench's finished listeners: what a dialog keeps to listen and to stop.
#[derive(Clone, Default)]
pub struct GeoListeners {
    table: Rc<RefCell<ListenerTable>>,
}

impl GeoListeners {
    /// GeoWorkbench::addFinishedListener: told when any of the workbench's jobs ends, after the
    /// window's own `finished`; the key returned stops it.
    pub fn add(&self, listener: FinishedListener) -> i32 {
        let mut table = self.table.borrow_mut();
        table.next += 1;
        let key = table.next;
        table
            .listening
            .insert(key, Rc::new(RefCell::new(Some(listener))));
        key
    }

    pub fn remove(&self, key: i32) {
        self.table.borrow_mut().listening.remove(&key);
    }

    /// Tells the window's `finished`, then every listener, of job `id`'s end. A copy of the
    /// listeners is told, so one may remove itself.
    fn notify(&self, id: JobId, outcome: &VerbOutcome, host: &mut dyn Host) {
        let (finished, listening) = {
            let table = self.table.borrow();
            let listening: Vec<ListenerSlot> = table.listening.values().cloned().collect();
            (table.finished.clone(), listening)
        };
        for slot in finished.into_iter().chain(listening) {
            let taken = slot.borrow_mut().take();
            if let Some(mut listener) = taken {
                listener(id, outcome, &mut *host);
                let mut place = slot.borrow_mut();
                if place.is_none() {
                    *place = Some(listener);
                }
            }
        }
    }
}

/// The geoprocessing verbs in the window (see the module comment). Built by the window, which
/// lends itself as the host for each call.
#[derive(Default)]
pub struct GeoWorkbench {
    last_job: JobId,
    listeners: GeoListeners,
    effects: Option<GeoEffects>,
}

impl GeoWorkbench {
    pub fn new() -> Self {
        Self::default()
    }

    /// The window's front-end effects, so a question the IMPORT hook queued is given the line it
    /// was asked for (`GeoEffect::AskFarApart`'s `line`).
    pub fn set_effects(&mut self, effects: Option<GeoEffects>) {
        self.effects = effects;
    }

    /// GeoServices::finished: told of every job's end before the listeners; the window's own.
    pub fn set_finished(&mut self, finished: Option<FinishedListener>) {
        self.listeners.table.borrow_mut().finished =
            finished.map(|listener| Rc::new(RefCell::new(Some(listener))));
    }

    pub fn add_finished_listener(&self, listener: FinishedListener) -> i32 {
        self.listeners.add(listener)
    }

    pub fn remove_finished_listener(&self, key: i32) {
        self.listeners.remove(key);
    }

    /// The listeners, for whoever listens without the workbench at hand.
    pub fn listeners(&self) -> GeoListeners {
        self.listeners.clone()
    }

    /// The job the last line started; NO_JOB when it started none.
    pub fn last_job(&self) -> JobId {
        self.last_job
    }

    /// True when `line` is the executor's, which it then runs - answered in the log, or started
    /// as a job. False leaves the line to whoever asked.
    pub fn run_line(&mut self, host: &mut dyn Host, line: &str) -> bool {
        let text = line.trim();
        if !handles(text) {
            return false;
        }
        self.last_job = NO_JOB;
        let prepared = with_geo(&mut *host, |context| prepare(context, text));
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                host.log(&format!("error: {}", error.describe()), true);
                return true;
            }
        };
        if let Some(reply) = &prepared.reply {
            host.log(reply, false);
            return true;
        }

        // The job owns what the work reads - copied at prepare - and hands back the executor's
        // Apply, which runs on the interface thread with the window, as one step.
        let work = prepared.work;
        let reply = Arc::new(Mutex::new(VerbOutcome::default()));
        let applied = Arc::clone(&reply);
        let work_of_job = job_work(move |control: &JobControl| {
            // The C++ called the prepared std::function unchecked: an empty one threw
            // std::bad_function_call, which the runner caught and reported.
            let Some(work) = work else {
                return Err(make_error(
                    ErrorCode::Internal,
                    "the job failed: bad_function_call",
                    String::new(),
                ));
            };
            let progress = |fraction: f64| control.set_progress(fraction);
            let progress: &(dyn Fn(f64) + Sync) = &progress;
            let apply = work(Some(control.stop_flag()), Some(progress))?;
            Ok(Some(job_apply(move |host: &mut dyn Host| {
                let applied_reply = with_geo(&mut *host, |context| apply(context));
                let mut outcome = applied.lock().unwrap_or_else(PoisonError::into_inner);
                match applied_reply {
                    Err(error) => {
                        outcome.ok = false;
                        outcome.error = format!("error: {}", error.describe());
                        host.log(&outcome.error, true);
                    }
                    Ok(text) => {
                        outcome.ok = true;
                        outcome.reply = text;
                        if !outcome.reply.is_empty() {
                            host.log(&outcome.reply, false);
                        }
                    }
                }
            })))
        });
        let listeners = self.listeners.clone();
        let effects = self.effects.clone();
        let asked_line = String::from(text);
        let finished = job_finished(move |report: &JobReport, host: &mut dyn Host| {
            let mut outcome = reply.lock().unwrap_or_else(PoisonError::into_inner).clone();
            if report.outcome == JobOutcome::Cancelled {
                // The executor's word for it, as katana_cli says it.
                outcome.ok = false;
                outcome.error = String::from("error: InvalidState: cancelled");
                host.log(&outcome.error, true);
            } else if report.outcome == JobOutcome::Failed {
                outcome.ok = false;
                outcome.error = format!("error: {}", report.error);
                host.log(&outcome.error, true);
            }
            if let Some(effects) = &effects {
                effects.claim_question(&asked_line);
            }
            listeners.notify(report.id, &outcome, host);
        });
        let id = host
            .jobs()
            .start(&prepared.title, work_of_job, Some(finished));
        self.last_job = id;
        if host.headless() {
            // Nobody to wait for a result, and the next line may read it.
            host.wait_for(id);
        } else {
            host.log(&started_record(id, &prepared.title), false);
        }
        true
    }

    /// MainWindow::awaitJob, the one home of its rule: when `started` - what a line run through
    /// the executor answered - says a geoprocessing job was started and that job is still
    /// running, `done` is called with the job's outcome when it ends, once, and true answered.
    /// False otherwise: `started` is the answer. The window's `Host::await_job` calls this with
    /// its jobs.
    pub fn await_job(&self, jobs: &dyn Jobs, started: &VerbOutcome, done: JobAwait) -> bool {
        let Some(job) = started_job(&started.reply) else {
            return false;
        };
        if !jobs.is_active(job) {
            return false;
        }
        // Told once, then forgotten: the listener takes itself out by its own key, which it is
        // given once it has one. The listener is added before the next frame runs, so the job
        // cannot end unheard in between.
        let table = self.listeners.clone();
        let own_key = Rc::new(Cell::new(0));
        let key = Rc::clone(&own_key);
        let mut done = Some(done);
        let added = self.listeners.add(Box::new(
            move |ended: JobId, outcome: &VerbOutcome, host: &mut dyn Host| {
                if ended != job {
                    return;
                }
                table.remove(key.get());
                if let Some(done) = done.take() {
                    done(outcome, host);
                }
            },
        ));
        own_key.set(added);
        true
    }
}

/// The record an interactive run logs when its line became a job: `job id=<n> title="..."
/// state=started`, the title quoted as any record's value is (geo::replies::value), so a title
/// with a quote in it still reads back.
pub fn started_record(id: JobId, title: &str) -> String {
    format!("job id={id} title={} state=started", value(title))
}

/// The job a reply says it started: the id of its `job ... state=started` record, on any line of
/// it; None for a reply that started none - a headless run's, which waited for the job. The one
/// reader of that record, for the dialogs and the window's awaitJob alike.
pub fn started_job(reply: &str) -> Option<JobId> {
    for record in parse_records(reply) {
        if record.kind != "job" || record.get("state") != Some("started") {
            continue;
        }
        if let Some(id) = parse_integer(record.get("id").unwrap_or(""))
            && id > 0
        {
            return Some(id as JobId);
        }
    }
    None
}

// ---- the menus ------------------------------------------------------------------------------

/// The menus the geoprocessing packages add their items to (geo_workbench.hpp's GeoMenus), each
/// part made the first time an item is added, so a menu shows no heading with nothing under it:
///
/// ```text
/// GIS      "Processing - GDAL", "Analysis - GDAL", "Check - GDAL" sections
/// Terrain  "Analysis" (terrainAnalysisMenu) and "DEM" (terrainDemMenu)
/// ```
///
/// A submenu is a menu item too, which the window's --check-menus holds to an icon and a status
/// tip; `drive::Menu` has neither (F0's frozen row), so they are kept here (`submenu_sign`) for the
/// window to read until the row has them.
#[derive(Clone, Debug, PartialEq)]
pub struct GeoMenus {
    gis: Menu,
    terrain: Menu,
    /// A GIS section's title and the name of its last action.
    gis_sections: BTreeMap<String, String>,
    /// A Terrain submenu's name, and its icon and status tip.
    terrain_signs: BTreeMap<String, (&'static str, String)>,
}

impl GeoMenus {
    pub fn new(gis: Menu, terrain: Menu) -> Self {
        Self {
            gis,
            terrain,
            gis_sections: BTreeMap::new(),
            terrain_signs: BTreeMap::new(),
        }
    }

    /// Adds `action` under the GIS menu's section `title`, made at the end of the menu the first
    /// time; after the section's last item otherwise, so a package's items stay together whatever
    /// order the packages add them in.
    pub fn add_to_gis(&mut self, title: &str, action: Action) {
        let name = action.name.clone();
        let Some(last) = self.gis_sections.get(title) else {
            self.gis.items.push(MenuItem::Section(String::from(title)));
            self.gis.items.push(MenuItem::Action(action));
            self.gis_sections.insert(String::from(title), name);
            return;
        };
        let at =
            self.gis.items.iter().position(
                |item| matches!(item, MenuItem::Action(existing) if existing.name == *last),
            );
        match at {
            Some(at) => self.gis.items.insert(at + 1, MenuItem::Action(action)),
            None => self.gis.items.push(MenuItem::Action(action)),
        }
        self.gis_sections.insert(String::from(title), name);
    }

    /// Adds `action` at the end of the Terrain submenu `name`, made on first use with `title`,
    /// `icon` and the status tip `tip`.
    pub fn add_to_terrain(
        &mut self,
        title: &str,
        name: &str,
        icon: &'static str,
        tip: &str,
        action: Action,
    ) {
        let at = self
            .terrain
            .items
            .iter()
            .position(|item| matches!(item, MenuItem::Menu(menu) if menu.name == name));
        match at {
            Some(at) => {
                if let MenuItem::Menu(menu) = &mut self.terrain.items[at] {
                    menu.items.push(MenuItem::Action(action));
                }
            }
            None => {
                self.terrain
                    .items
                    .push(MenuItem::Menu(Menu::new(name, title).action(action)));
                self.terrain_signs
                    .insert(String::from(name), (icon, String::from(tip)));
            }
        }
    }

    pub fn gis(&self) -> &Menu {
        &self.gis
    }

    pub fn terrain(&self) -> &Menu {
        &self.terrain
    }

    /// The icon and the status tip of the Terrain submenu `name`.
    pub fn submenu_sign(&self, name: &str) -> Option<(&'static str, &str)> {
        self.terrain_signs
            .get(name)
            .map(|(icon, tip)| (*icon, tip.as_str()))
    }

    /// The GIS and the Terrain menus as the packages left them.
    pub fn into_menus(self) -> (Menu, Menu) {
        (self.gis, self.terrain)
    }
}

// ---- the front end's effects ------------------------------------------------------------------

/// The window's question when an IMPORT that keeps its own coordinates lands far from the drawing:
/// what the hook was asked, and the line that asked it, to run again with the answer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FarApartQuestion {
    pub drawing: Box2,
    pub incoming: Box2,
    /// interop's advice: why the two are far apart.
    pub advice: String,
    /// The line whose apply asked; empty until its job's end is told.
    pub line: String,
}

/// What a geo phase asked of the window, applied by the window after the phase.
#[derive(Clone, Debug, PartialEq)]
pub enum GeoEffect {
    /// Frame this box in the plan views (GeoServices::frame).
    Frame(Box2),
    /// The reference rasters changed: redraw them (GeoServices::changed).
    Changed,
    /// What an IMPORT brought that only the window shows: an archive's meshes, its surfaces.
    Imported(ImportShown),
    /// Ask where data that lands far from the drawing goes (see the module comment).
    AskFarApart(FarApartQuestion),
}

#[derive(Debug, Default)]
struct EffectsState {
    queue: Vec<GeoEffect>,
    headless: bool,
    /// The answer for the next far-apart question, given by the person to the question shown.
    answer: Option<FarApartChoice>,
}

/// The front-end effects queue (see the module comment): the window's `FrontEnd` hooks push here,
/// and the window takes what they pushed after each phase.
#[derive(Clone, Debug, Default)]
pub struct GeoEffects {
    state: Rc<RefCell<EffectsState>>,
}

impl GeoEffects {
    pub fn new() -> Self {
        Self::default()
    }

    /// A headless window keeps survey coordinates where the data lands far away, and says so (the
    /// executor's reply warns): nobody is asked.
    pub fn set_headless(&self, headless: bool) {
        self.state.borrow_mut().headless = headless;
    }

    /// The window's front end: hooks that queue what they are asked here. The window lends it in
    /// every Context (`Host::geo`).
    pub fn front_end(&self) -> FrontEnd {
        let frame = self.clone();
        let changed = self.clone();
        let far_apart = self.clone();
        let imported = self.clone();
        FrontEnd {
            frame: Some(Box::new(move |area: &Box2| {
                frame.push(GeoEffect::Frame(*area));
            })),
            changed: Some(Box::new(move || changed.push(GeoEffect::Changed))),
            far_apart: Some(Box::new(
                move |drawing: &Box2, incoming: &Box2, advice: &str| {
                    far_apart.far_apart(drawing, incoming, advice)
                },
            )),
            imported: Some(Box::new(move |shown: ImportShown| {
                imported.push(GeoEffect::Imported(shown));
            })),
        }
    }

    fn push(&self, effect: GeoEffect) {
        self.state.borrow_mut().queue.push(effect);
    }

    /// The far-apart hook: Keep headless; the answer set for it when there is one; otherwise the
    /// question queued and Cancel, so nothing is imported before the person has answered.
    fn far_apart(&self, drawing: &Box2, incoming: &Box2, advice: &str) -> FarApartChoice {
        let mut state = self.state.borrow_mut();
        if state.headless {
            return FarApartChoice::Keep;
        }
        if let Some(choice) = state.answer.take() {
            return choice;
        }
        state.queue.push(GeoEffect::AskFarApart(FarApartQuestion {
            drawing: *drawing,
            incoming: *incoming,
            advice: String::from(advice),
            line: String::new(),
        }));
        FarApartChoice::Cancel
    }

    /// The person's answer to the question shown, for the hook to give when the line runs again.
    pub fn answer_far_apart(&self, choice: FarApartChoice) {
        self.state.borrow_mut().answer = Some(choice);
    }

    /// Gives every queued question still without its line the line `line`.
    pub fn claim_question(&self, line: &str) {
        for effect in &mut self.state.borrow_mut().queue {
            if let GeoEffect::AskFarApart(question) = effect
                && question.line.is_empty()
            {
                question.line = String::from(line);
            }
        }
    }

    /// What the phases asked since the last call, in order.
    pub fn take(&self) -> Vec<GeoEffect> {
        std::mem::take(&mut self.state.borrow_mut().queue)
    }

    pub fn is_empty(&self) -> bool {
        self.state.borrow().queue.is_empty()
    }
}
