//! What the geoprocessing dialogs share (geo/geo_dialog_support.hpp; docs/geoprocessing.md, "The
//! window"; CLAUDE.md section 1): the Grid Points to DEM dialog, the DEM tools and the GDAL
//! Toolbox. None of them does the work: each builds the verb line its fields describe - shown, as
//! it will run, in its Command field - and Run hands that line to the window's one executor
//! (`Host::run`) exactly as if it had been typed. So it is logged, kept in the history and undone
//! like a typed line, and the line the dialog shows is the line an agent types.
//!
//! A geoprocessing line runs as a background job. Headless the executor waits for it, so the
//! outcome is the reply; interactively it answers `job id=<n> ... state=started` at once and the
//! reply arrives when the job ends (`Host::await_job`). `GeoRunPanel` hides the difference: it
//! shows "running" and then the reply, whichever way it came.
//!
//! What the C++'s GeoDialogContext carried - the executor, the jobs' listener, whether the session
//! is headless, the drawing, the reference rasters, the surfaces and the views - is the `Host` a
//! dialog is drawn with, so it has no Rust of its own (nor has geoDialogContext, which made it
//! from the workbench). showKeptDialog, which found a dialog by its object name among the
//! window's children, is the window's keeping of its dialogs' models between frames.

use std::cell::RefCell;
use std::rc::Rc;

use egui::{RichText, Ui};
use katana_app::geo::replies;
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result};

use super::geo_workbench::started_job;
use crate::drive::{LineOptions, TextOptions, button, label_as, line_with, text_with};
use crate::host::{Host, JobId, NO_JOB, VerbOutcome};
use crate::theme::palette;

/// `text` as one word of a line, as the command line reads it: double-quoted when it holds a blank
/// (geo::replies::line_word, the one quoting rule - a keyword as a name is quoted too).
/// InvalidArgument naming `field` for a double quote or a line break, which no word of a line can
/// carry, and for an empty word.
pub fn line_word(text: &str, field: &str) -> Result<String> {
    if text.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{field} is empty"),
            String::new(),
        ));
    }
    replies::line_word(text).ok_or_else(|| {
        make_error(
            ErrorCode::InvalidArgument,
            format!(
                "{field} holds a double quote or a line break, which a command line cannot carry"
            ),
            String::new(),
        )
    })
}

/// A reply's first line that is a record of `kind` ("output", "scope" ...): what a dialog reads its
/// status from; empty when there is none.
pub fn first_record(reply: &str, kind: &str) -> String {
    reply
        .split('\n')
        .find(|record| *record == kind || record.starts_with(&format!("{kind} ")))
        .map(String::from)
        .unwrap_or_default()
}

/// The Command, Preview, Run, status and Reply of a geoprocessing dialog, or of one tab of one. Its
/// object names are the prefix given, then:
///
/// ```text
/// <prefix>Panel     the panel itself
/// <prefix>Command   the exact line Run runs (read-only)
/// <prefix>Preview   run the line with PREVIEW: what it would take, checked by GDAL, nothing
///                   changed (absent when not offered)
/// <prefix>Run       run it
/// <prefix>Status    what happened last, one line
/// <prefix>Reply     the reply records, or why the line failed
/// ```
///
/// The dialog gives the panel its line - `Ok` the line, `Err` why its fields write none - each
/// frame it draws it (the C++'s `line` function, asked by refresh and by Run).
pub struct GeoRunPanel {
    prefix: String,
    preview: bool,
    command: String,
    placeholder: String,
    ready: bool,
    status: String,
    status_is_error: bool,
    reply: String,
    /// A job this panel started and has not heard the end of.
    pending: JobId,
    /// Where the job's end is told (`Host::await_job`'s done), for the next frame to take.
    heard: Rc<RefCell<Option<VerbOutcome>>>,
}

impl GeoRunPanel {
    pub fn new(prefix: &str, preview: bool) -> Self {
        Self {
            prefix: String::from(prefix),
            preview,
            command: String::new(),
            placeholder: String::new(),
            ready: false,
            status: String::new(),
            status_is_error: false,
            reply: String::new(),
            pending: NO_JOB,
            heard: Rc::new(RefCell::new(None)),
        }
    }

    fn name(&self, part: &str) -> String {
        format!("{}{part}", self.prefix)
    }

    /// Reads the line again into the Command field; Run and Preview are enabled only when there is
    /// a line.
    pub fn refresh(&mut self, made: &Result<String>) {
        self.ready = matches!(made, Ok(line) if !line.is_empty());
        self.command = match made {
            Ok(line) if self.ready => line.clone(),
            _ => String::new(),
        };
        self.placeholder = if self.ready {
            String::new()
        } else {
            format!(
                "nothing to run yet: {}",
                match made {
                    Ok(_) => String::from("the fields say no line"),
                    Err(error) => error.message.clone(),
                }
            )
        };
    }

    /// What Run (`preview` false) and Preview do: the line handed to the executor and its reply
    /// shown when it comes. A line that cannot be written runs nothing, and the status says why.
    /// The outcome is answered when it came at once (the dialog's onFinished); a job's comes later,
    /// from `poll`.
    pub fn run(
        &mut self,
        host: &mut dyn Host,
        made: &Result<String>,
        preview: bool,
    ) -> Option<VerbOutcome> {
        let line = match made {
            Ok(line) if !line.is_empty() => line.clone(),
            Ok(_) => {
                self.set_status("There is nothing to run yet.", true);
                return None;
            }
            Err(error) => {
                self.set_status(&error.message, true);
                return None;
            }
        };
        let text = if preview {
            format!("{line} PREVIEW")
        } else {
            line
        };
        let outcome = host.run(&text);
        let job = if outcome.ok {
            started_job(&outcome.reply)
        } else {
            None
        };
        let Some(job) = job else {
            // Answered at once - a PREVIEW, a refusal - or a headless run, whose job ended inside
            // the call.
            return Some(self.finish(outcome));
        };
        let heard = Rc::clone(&self.heard);
        *heard.borrow_mut() = None;
        // Without a listener (a host that hears no job's end) the panel waits as the C++'s did
        // with none: running, never told.
        let _ = host.await_job(
            &outcome,
            Box::new(move |done: &VerbOutcome, _host: &mut dyn Host| {
                *heard.borrow_mut() = Some(done.clone());
            }),
        );
        self.pending = job;
        self.set_status(
            &format!("Running as job {job} - its progress and Cancel are in the status bar."),
            false,
        );
        None
    }

    /// The end of the job this panel waits for, when it has been heard: shown, and answered (the
    /// dialog's onFinished).
    pub fn poll(&mut self) -> Option<VerbOutcome> {
        if self.pending == NO_JOB {
            return None;
        }
        let heard = self.heard.borrow_mut().take();
        let outcome = heard?;
        self.pending = NO_JOB;
        Some(self.finish(outcome))
    }

    fn finish(&mut self, outcome: VerbOutcome) -> VerbOutcome {
        let mut text = if outcome.ok {
            outcome.reply.clone()
        } else {
            outcome.error.clone()
        };
        if !outcome.ok && !outcome.reply.is_empty() {
            text = format!("{}\n{}", outcome.reply, outcome.error);
        }
        while text.ends_with('\n') {
            text.pop();
        }
        self.reply = text;
        if !outcome.ok {
            let first = String::from(outcome.error.split('\n').next().unwrap_or(""));
            self.set_status(&first, true);
        } else if first_record(&outcome.reply, "preview").contains("valid=yes") {
            self.set_status(
                "Previewed: GDAL accepts the line, and nothing was changed.",
                false,
            );
        } else if outcome.reply.contains(" ran=no") {
            self.set_status(
                "Nothing ran: the scope took nothing to work on (the reply says what it took).",
                false,
            );
        } else {
            // Said from the reply's own output record: a raster kept as a reference raster is no
            // step of the drawing's to undo.
            let output = first_record(&outcome.reply, "output");
            if output.contains(" target=layer ") {
                self.set_status(
                    "Done, as one undo step - Undo takes it back. The reply is below and in the \
                     command log.",
                    false,
                );
            } else if output.contains(" target=reference ") {
                self.set_status(
                    "Done: the result is a reference raster (REFS lists it). The reply is below \
                     and in the command log.",
                    false,
                );
            } else if output.contains(" target=file ") {
                self.set_status("Written. The reply is below and in the command log.", false);
            } else {
                self.set_status("Done. The reply is below and in the command log.", false);
            }
        }
        outcome
    }

    /// A job this panel started and has not heard the end of.
    pub fn running(&self) -> bool {
        self.pending != NO_JOB
    }

    pub fn reply(&self) -> &str {
        &self.reply
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn command(&self) -> &str {
        &self.command
    }

    pub fn set_status(&mut self, text: &str, is_error: bool) {
        self.status = String::from(text);
        self.status_is_error = is_error;
    }

    /// Draws the panel with the line `made`, running it when Run or Preview is pressed; answers
    /// the outcome of a run that ended this frame (the dialog's onFinished).
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        host: &mut dyn Host,
        made: &Result<String>,
    ) -> Option<VerbOutcome> {
        self.refresh(made);
        let mut finished = self.poll();
        let command_name = self.name("Command");
        let placeholder = self.placeholder.clone();
        ui.horizontal(|ui| {
            ui.label("Command:");
            line_with(
                ui,
                &command_name,
                &mut self.command,
                LineOptions {
                    placeholder: &placeholder,
                    read_only: true,
                    ..LineOptions::default()
                },
            )
            .on_hover_text(
                "The line Run hands to the command line - type it there, give it to katana_cli \
                 or send it to katana_mcp, and it does the same",
            );
        });
        let status_name = self.name("Status");
        let preview_name = self.name("Preview");
        let run_name = self.name("Run");
        let status = self.status.clone();
        let colour = if self.status_is_error {
            Some(palette::error().to_egui())
        } else {
            None
        };
        let ready = self.ready;
        let offers_preview = self.preview;
        let (preview_clicked, run_clicked) = ui
            .horizontal(|ui| {
                let mut text = RichText::new(status.as_str());
                if let Some(colour) = colour {
                    text = text.color(colour);
                }
                label_as(ui, &status_name, &status, egui::Label::new(text).wrap());
                ui.add_enabled_ui(ready, |ui| {
                    let preview = offers_preview
                        && button(ui, &preview_name, "Preview")
                            .on_hover_text(
                                "Run the line with PREVIEW: what it would take, checked by \
                                 GDAL, and nothing changed",
                            )
                            .clicked();
                    let run = button(ui, &run_name, "Run").clicked();
                    (preview, run)
                })
                .inner
            })
            .inner;
        let reply_name = self.name("Reply");
        text_with(
            ui,
            &reply_name,
            &mut self.reply,
            TextOptions {
                read_only: true,
                rows: 6,
                monospace: true,
            },
        );
        for (clicked, preview) in [(preview_clicked, true), (run_clicked, false)] {
            if clicked {
                let ran = self.run(host, made, preview);
                if finished.is_none() {
                    finished = ran;
                }
            }
        }
        finished
    }
}
