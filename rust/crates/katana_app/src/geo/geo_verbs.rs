//! The ONE geoprocessing executor (docs/geoprocessing.md, "The executor"),
//! the port of src/katana_app/geo/geo_verbs.hpp/.cpp: every geoprocessing
//! verb, GDAL and the curated families the lanes add, runs through here,
//! whether it was typed at katana_cli, sent by an agent to katana_mcp or run
//! on the window's command line.
//!
//! A line runs in three phases, so a headless session can run it inline while
//! the window runs it as a background job:
//!
//! ```text
//! prepare  on the calling thread (the GUI thread in the window): the line is
//!          read, the algorithm resolved and its policy checked, the scopes
//!          resolved by the shared scope parser and matcher, and everything
//!          the work needs COPIED out of the drawing - a feature set, a
//!          surface held by an Arc, paths. LIST, HELP, VERSION and PREVIEW
//!          have nothing to run and answer here.
//! work     on a worker, or inline: the algorithm and the conversions only,
//!          never the Document or the reference data. It returns an Apply.
//! apply    on the calling thread again: ONE command on the drawing (one
//!          undo step), reference rasters added, and the reply records.
//! ```
//!
//! Nothing a run computes is visible until it is applied, so a cancelled or
//! failed job leaves no trace (src/katana_qt/jobs.hpp's rule).
//!
//! The surfaces are the DRAWING's (Document::surfaces). A verb reads and
//! changes `Context::surfaces`, a staging copy the executor makes of the
//! drawing's as prepare and as apply begin; what the phase changed is
//! committed to the drawing when it ends, as ONE undo step named after the
//! verb ("SURFACE FROM", "SURFACE REMOVE"). A phase that fails commits
//! nothing. An apply that makes a step of its own (an IMPORT whose entities
//! and surfaces are one step) commits the staging store inside it
//! (Document::surfaces_command), and the executor then finds nothing left to
//! commit.
//!
//! The Rust shape (docs/rust.md, step 6; the plan's section 4.3). The C++
//! Context holds the drawing, the interpreter and the reference data by
//! reference for the life of its front end; in Rust that would be
//! self-referential inside the session and would lock the drawing away from
//! the window's painting. So a [`Context`] is a bundle of borrows built for
//! each phase, as a tool's ToolContext is (cad's interactive_tool.rs), and the
//! phases are values: [`Work`] is `Send`, so the window runs it on a job
//! thread, and the [`Apply`] it returns is `Send`, so it comes back. The
//! front end's hooks are lent with the Context and never enter the work, so
//! they need not be `Send`. Rejected: `Rc<RefCell<Document>>` (every reader a
//! run-time borrow that can panic), an `Arc<Mutex<..>>` (the drawing made
//! shareable for one front end's sake), one long-lived Context in the session
//! (self-referential).

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use katana_archive12d::ImportedMesh;
use katana_cad::{CommandInterpreter, Document};
use katana_core::error::make_error;
use katana_core::text::is_ascii_space;
use katana_core::{ErrorCode, Result, Status};
use katana_geometry::Box2;
use katana_interop::ReferenceData;
use katana_terrain::SurfaceStore;

use super::bindings::{Tokens, tokenize};
use super::verb_table::{VerbEntry, verb_table};

/// The answer to the window's question when an IMPORT that keeps its own
/// coordinates lands far from the drawing (docs/interop.md, "Placing an
/// import"): keep them, move the data alongside the drawing, or import nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FarApartChoice {
    Keep,
    Alongside,
    Cancel,
}

/// What an IMPORT brought that only a front end with 3D views shows: an
/// archive's meshes (the .12da format's), which only the window keeps, and how
/// many surfaces it added to the drawing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportShown {
    pub meshes: Vec<ImportedMesh>,
    pub surfaces: usize,
}

/// Frames what an apply added (the window's views).
pub type FrameHook = Box<dyn Fn(&Box2)>;
/// The reference rasters changed. (The surfaces are the drawing's: a change of
/// them is the Document's to notify.)
pub type ChangedHook = Box<dyn Fn()>;
/// Asked on the calling thread, with the drawing's box, the incoming data's
/// and the advice, when an IMPORT that keeps its own coordinates lands far
/// from the drawing.
pub type FarApartHook = Box<dyn Fn(&Box2, &Box2, &str) -> FarApartChoice>;
/// Shows what an IMPORT brought beyond the drawing, the reference data and the
/// surfaces.
pub type ImportedHook = Box<dyn Fn(ImportShown)>;

/// The front end's optional hooks, the C++ Context's std::function members.
/// Absent, as in a session or a headless window: nothing is framed or
/// notified, a far-apart IMPORT keeps its coordinates and the reply warns, and
/// an archive's meshes are counted and said to be held nowhere.
#[derive(Default)]
pub struct FrontEnd {
    pub frame: Option<FrameHook>,
    pub changed: Option<ChangedHook>,
    pub far_apart: Option<FarApartHook>,
    pub imported: Option<ImportedHook>,
}

impl fmt::Debug for FrontEnd {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FrontEnd")
            .field("frame", &self.frame.is_some())
            .field("changed", &self.changed.is_some())
            .field("far_apart", &self.far_apart.is_some())
            .field("imported", &self.imported.is_some())
            .finish()
    }
}

/// What a geoprocessing line runs against, lent for one phase: the front
/// end's drawing (with its surfaces), its interpreter (whose scope context
/// answers VIEW in the window and refuses it headless), its reference rasters
/// and point clouds, where derived rasters go when the drawing has no
/// project, and the front end's hooks.
pub struct Context<'a> {
    pub document: &'a mut Document,
    pub interpreter: &'a CommandInterpreter,
    /// The front end's reference layers: a run's derived rasters are added
    /// here as it applies, and a RASTER source is found here.
    pub reference: &'a mut ReferenceData,
    /// Where derived rasters go when the drawing has no project directory.
    pub scratch: &'a Path,
    pub front_end: &'a FrontEnd,
    /// The phase's STAGING copy of the drawing's surfaces (see the top of this
    /// file): made by the executor from Document::surfaces as prepare and
    /// apply begin, read and changed by the verbs, committed by the executor.
    /// Never a second store: nothing keeps it between phases, and a front end
    /// never fills it.
    pub surfaces: SurfaceStore,
}

impl<'a> Context<'a> {
    /// A phase's context, with no staged surfaces: the executor stages them.
    pub fn new(
        document: &'a mut Document,
        interpreter: &'a CommandInterpreter,
        reference: &'a mut ReferenceData,
        scratch: &'a Path,
        front_end: &'a FrontEnd,
    ) -> Self {
        Self {
            document,
            interpreter,
            reference,
            scratch,
            front_end,
            surfaces: SurfaceStore::default(),
        }
    }
}

/// How far a run has got, 0 to 1, possibly called from a GDAL thread: the
/// bridge's own type (katana_gis::processing::run).
pub type Progress<'p> = Option<&'p (dyn Fn(f64) + Sync)>;

/// The third phase: one undo step at most; the reply records.
pub type Apply = Box<dyn FnOnce(&mut Context<'_>) -> Result<String> + Send>;

/// The second phase. Pure: never the Document or the reference data. The stop
/// is asked between and inside the steps; "cancelled" when it was requested.
pub type Work =
    Box<dyn FnOnce(Option<&AtomicBool>, Option<&(dyn Fn(f64) + Sync)>) -> Result<Apply> + Send>;

/// An Apply from a closure, its context's lifetimes left open as Apply's are.
pub fn make_apply<F>(apply: F) -> Apply
where
    F: FnOnce(&mut Context<'_>) -> Result<String> + Send + 'static,
{
    Box::new(apply)
}

/// A Work from a closure, its stop's and progress's lifetimes left open as
/// Work's are.
pub fn make_work<F>(work: F) -> Work
where
    F: FnOnce(Option<&AtomicBool>, Progress<'_>) -> Result<Apply> + Send + 'static,
{
    Box::new(work)
}

/// The C++'s `stop.stop_requested()`: a stop given and set.
pub fn stop_requested(stop: Option<&AtomicBool>) -> bool {
    stop.is_some_and(|stop| stop.load(Ordering::Acquire))
}

/// What prepare made of a line.
#[derive(Default)]
pub struct Prepared {
    /// For a job's status-bar line: "GDAL raster hillshade".
    pub title: String,
    /// Set when there is nothing to run: LIST, HELP, VERSION, PREVIEW.
    pub reply: Option<String>,
    /// Set when there is: the C++'s non-empty std::function.
    pub work: Option<Work>,
}

impl fmt::Debug for Prepared {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Prepared")
            .field("title", &self.title)
            .field("reply", &self.reply)
            .field("work", &self.work.is_some())
            .finish()
    }
}

/// A line's text without the blanks around it: core's `trimmed` on a `&str`.
pub(crate) fn trimmed_text(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_ascii() && is_ascii_space(c as u8))
}

/// The C++'s `std::error_code::message()` for a failed file operation: the
/// system's own sentence, which Rust's io::Error shows followed by
/// " (os error N)". The app's one copy (storage, gis and cad keep their own;
/// docs/rust.md "Not done").
pub(crate) fn system_message(error: &std::io::Error) -> String {
    let text = format!("{error}");
    if let Some(code) = error.raw_os_error()
        && let Some(sentence) = text.strip_suffix(&format!(" (os error {code})"))
    {
        return String::from(sentence);
    }
    text
}

/// The row that takes a line of these words: one whose second word matches
/// before one that takes any.
fn entry_for(words: &[String]) -> Option<&'static VerbEntry> {
    let verb = words.first()?.to_ascii_uppercase();
    let second = words
        .get(1)
        .map(|word| word.to_ascii_uppercase())
        .unwrap_or_default();
    let mut any = None;
    for entry in verb_table() {
        if verb != entry.verb {
            continue;
        }
        if entry.subverb.is_empty() {
            any.get_or_insert(entry);
        } else if second == entry.subverb {
            return Some(entry);
        }
    }
    any
}

/// The row that takes this line: its verb's, unless that row leaves the line
/// to the interpreter (INFO <id>).
fn entry_taking(tokens: &Tokens) -> Option<&'static VerbEntry> {
    let entry = entry_for(&tokens.words)?;
    match entry.takes {
        Some(takes) if !takes(tokens) => None,
        _ => Some(entry),
    }
}

// ---- the drawing's surfaces, staged -------------------------------------------------------------

/// The undo step a line's change of the surfaces is named: its verb, and its
/// second word when that is a word ("SURFACE FROM", "GDAL RASTER") and not a
/// path or a name.
fn step_name_of(tokens: &Tokens) -> String {
    let mut name = tokens.words[0].to_ascii_uppercase();
    if tokens.size() > 1
        && !tokens.quoted[1]
        && !tokens[1].is_empty()
        && tokens[1].bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        name.push(' ');
        name += &tokens[1].to_ascii_uppercase();
    }
    name
}

/// A copy of the drawing's surfaces for the phase about to run; its revision,
/// to tell afterwards whether the phase changed it. `unstage` drops it when
/// the phase ends, on every path out of it ("nothing keeps it between
/// phases"): the copy holds an Arc of every surface.
fn stage(context: &mut Context<'_>) -> u64 {
    context.surfaces = context.document.surfaces().clone();
    context.surfaces.revision()
}

fn unstage(context: &mut Context<'_>) {
    context.surfaces = SurfaceStore::default();
}

/// What the phase changed, committed as one step. Nothing when it changed
/// nothing, or when the drawing's surfaces already are the staged ones (an
/// apply that committed them in a step of its own): Document::set_surfaces
/// records no step for an equal store.
fn commit(context: &mut Context<'_>, staged: u64, step: &str) -> Status {
    if context.surfaces.revision() == staged {
        return Ok(());
    }
    context
        .document
        .set_surfaces(context.surfaces.clone(), step)
}

/// Whether a verb of the executor's begins `line`.
pub fn handles(line: &str) -> bool {
    match tokenize(line) {
        Ok(tokens) => {
            !tokens.words.is_empty() && !tokens.quoted[0] && entry_taking(&tokens).is_some()
        }
        Err(_) => {
            // A quote left open still says which verb it is; the verb refuses
            // it.
            let body = trimmed_text(line);
            let blank = body.find([' ', '\t']).unwrap_or(body.len());
            entry_for(&[String::from(&body[..blank])]).is_some()
        }
    }
}

/// The first phase. Fails for a line the executor refuses: its grammar, an
/// unknown algorithm or argument, a Confirm algorithm without CONFIRM, a file
/// in the way without OVERWRITE, VIEW headless.
pub fn prepare(context: &mut Context<'_>, line: &str) -> Result<Prepared> {
    let tokens = tokenize(line)?;
    let Some(entry) = entry_taking(&tokens) else {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "not a geoprocessing verb",
            trimmed_text(line),
        ));
    };
    let step = step_name_of(&tokens);
    let staged = stage(context);
    let prepared = (entry.prepare)(&mut *context, &tokens, line).and_then(|prepared| {
        // A line answered here may have changed the surfaces (SURFACE REMOVE).
        commit(&mut *context, staged, &step)?;
        Ok(prepared)
    });
    unstage(context);
    let mut prepared = prepared?;
    if let Some(work) = prepared.work.take() {
        // The apply, wherever it runs - inline here, or on the window's GUI
        // thread when its job ends - stages the drawing's surfaces as they are
        // THEN, and commits what it changed. The work itself is left as it
        // was: it never sees the Context.
        prepared.work = Some(make_work(move |stop, progress| {
            let apply = work(stop, progress)?;
            Ok(make_apply(move |context| {
                let staged = stage(context);
                let reply = apply(&mut *context).and_then(|reply| {
                    commit(&mut *context, staged, &step)?;
                    Ok(reply)
                });
                unstage(context);
                reply
            }))
        }));
    }
    Ok(prepared)
}

/// The three phases on this thread, one after another: what katana_cli and
/// katana_mcp run. The reply records, or the refusal. The C++'s defaults are
/// `None`: no stop, no progress.
pub fn run_now(
    context: &mut Context<'_>,
    line: &str,
    stop: Option<&AtomicBool>,
    progress: Progress<'_>,
) -> Result<String> {
    let prepared = prepare(context, line)?;
    if let Some(reply) = prepared.reply {
        return Ok(reply);
    }
    // A prepared line that answers nothing has work: the C++ calls it
    // unchecked (an empty std::function would throw bad_function_call).
    let work = prepared
        .work
        .expect("a prepared line with no reply has work to run");
    let apply = work(stop, progress)?;
    apply(context)
}

/// The usage of every verb in the table, for HELP, the window's Command
/// Reference and katana_help alike.
pub fn help_text() -> String {
    let mut text = String::new();
    for entry in verb_table() {
        text += if text.is_empty() {
            "Geo       "
        } else {
            "          "
        };
        text += entry.usage;
        text.push('\n');
    }
    text
}

/// Where a front end keeps derived rasters for a drawing with no project:
/// `<temp>/katana-scratch/<process id>-<start>/<n>`, a new folder at every
/// call, so no front end - not another Session in this process, nor a later
/// process given an ended one's id - finds another's files there. Each front
/// end asks once and keeps what it was given. Nothing is created until a
/// raster is written there.
pub fn own_scratch() -> PathBuf {
    // The process's folder is named by the process id AND the time it was
    // first asked for: Windows hands an ended process's id to a later one,
    // which found the folder the earlier one left behind with its files still
    // in it, and a result never replaces a file (keep_derived), so its `west`
    // came back as `west-2`. Two processes with one id never run at once, so
    // the later one's clock reads later. Inside it each front end has a
    // folder of its own, for the same reason: two Sessions in one process
    // (the geo tests run dozens) shared one folder, and the second's `west`
    // came back as `west-2`.
    static PROCESS: OnceLock<PathBuf> = OnceLock::new();
    static SERVED: AtomicU64 = AtomicU64::new(0);
    let process = PROCESS.get_or_init(|| {
        // std::filesystem::temp_directory_path, which fails for a temporary
        // directory that is not one; the working directory then.
        let mut temp = std::env::temp_dir();
        if !temp.is_dir() {
            temp = std::env::current_dir().unwrap_or_default();
        }
        let id = std::process::id();
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        temp.join("katana-scratch").join(format!("{id}-{started}"))
    });
    let served = SERVED.fetch_add(1, Ordering::Relaxed) + 1;
    process.join(format!("{served}"))
}
