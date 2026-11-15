//! What the UPDATE verbs (update_verbs.rs) need of a session: the part of
//! session.cpp that acts on the PROGRAM that runs the session, not on its
//! drawing. katana_cli runs every UPDATE line, with UPDATE APPLY CONFIRM as
//! the last line of its run (Session::take_pending_apply); katana_mcp runs
//! none, and says which of its tools checks for an update
//! (Session::set_update_surface).

use std::cell::RefCell;

use katana_update::ApplyRequest;

use crate::console::Console;
use crate::update_verbs::{
    UpdateContext, UpdateSurface, abandon_apply, make_update_context, run_update_line,
};

/// Where a session's UPDATE lines get their context when it is not the
/// running program's (Session::set_update_context_source): the C++'s
/// `std::function<UpdateContext(UpdateSurface)>`. A trait and not a closure,
/// because the context it hands over borrows from the source - a fake feed, a
/// credential store, a launcher - and a closure cannot lend what it holds. It
/// is asked for every line with the session's surface, as make_update_context
/// is, so a line sees what the line before it changed.
pub trait UpdateContextSource {
    fn context(&self, surface: UpdateSurface) -> UpdateContext<'_>;
}

#[derive(Default)]
pub(crate) struct UpdatePart {
    // A program's session: there is a program for UPDATE to act on.
    pub(crate) program: bool,
    // Whose lines these are: katana_cli's, unless katana_mcp's server said Mcp.
    pub(crate) surface: UpdateSurface,
    // What an UPDATE APPLY CONFIRM left for the front end to launch once the
    // session has ended (Session::take_pending_apply).
    pub(crate) pending_apply: Option<ApplyRequest>,
    // Where a line's context comes from when it is not the running program's
    // (Session::set_update_context_source). None: make_update_context.
    pub(crate) source: Option<Box<dyn UpdateContextSource>>,
}

// This process, which the update helper waits for before it swaps a file.
fn own_process_id() -> u64 {
    u64::from(std::process::id())
}

// An UPDATE line: the verbs' reply on `out`, a refusal on `err`. Returns false
// when the line was refused or failed.
pub(crate) fn run_update(updates: &mut UpdatePart, line: &str, console: &mut dyn Console) -> bool {
    if updates.surface == UpdateSurface::Mcp {
        // Refused by name, as the window's own verbs are, and whatever the
        // line says: katana_run_commands promises its client that it reaches
        // no network (docs/mcp.md), and an agent that could download or apply
        // would replace the very program its client holds open.
        console.err(
            "error: Unsupported: UPDATE lines are not run by katana_mcp: its tool \
             katana_check_update checks for a newer Katana and reports this copy's update status, \
             and it downloads and installs nothing; the Katana window or katana_cli does that\n",
        );
        return false;
    }
    if !updates.program {
        console.err(
            "error: Unsupported: UPDATE acts on the program that runs it, and this session is no \
             program's (session.hpp)\n",
        );
        return false;
    }
    // A download is watched, so its progress is printed as it happens; the
    // same lines are in the reply, and are not printed a second time. The
    // printer is a Fn the context lends the engine, so what it writes to is
    // in cells.
    let console = RefCell::new(console);
    let shown: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let reply = {
        let print: &dyn Fn(&str) = &|progress: &str| {
            console.borrow_mut().out(&format!("{progress}\n"));
            shown.borrow_mut().push(String::from(progress));
        };
        // Made for each line: it reads the install's records and the stored
        // token as they are now, so a line sees what the line before it
        // changed.
        let mut context = match &updates.source {
            Some(source) => source.context(updates.surface),
            None => make_update_context(updates.surface),
        };
        context.pid = own_process_id();
        // `relaunch` is left as it came, which from make_update_context is
        // empty: katana_cli starts nothing again after an update.
        context.progress_line = Some(print);
        run_update_line(&context, line)
    };
    let console = console.into_inner();
    let shown = shown.into_inner();
    let reply = match reply {
        Ok(reply) => reply,
        Err(error) => {
            console.err(&format!("error: {}\n", error.describe()));
            return false;
        }
    };
    let mut matched = 0;
    let mut rest = reply.text.as_str();
    while !rest.is_empty() {
        let end = rest.find('\n');
        let one = &rest[..end.unwrap_or(rest.len())];
        if matched < shown.len() && one == shown[matched] {
            matched += 1;
        } else {
            console.out(&format!("{one}\n"));
        }
        match end {
            Some(end) => rest = &rest[end + 1..],
            None => break,
        }
    }
    if let Some(apply) = reply.apply {
        // One apply is prepared at a time. An earlier one that nobody took is
        // nobody's now: its helper copy goes to the sweep (abandon_apply).
        if let Some(pending) = &updates.pending_apply {
            let _ = abandon_apply(pending);
        }
        updates.pending_apply = Some(apply);
    }
    true
}
