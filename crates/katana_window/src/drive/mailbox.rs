//! The mailbox: the acts a driver posts to a named control - a fill, a press, a scroll into view -
//! which the control takes the next time a frame draws it and applies as a person's act would
//! (drive::controls). A disabled or hidden control refuses the act with the C++'s sentence, as
//! `--fill` and `--press` refuse (main.cpp's fillField and its --press step); the sentences are
//! here, one home, for every control that takes acts.
//!
//! THE RULE FOR AN ACT NO CONTROL TOOK. At the end of every frame that counts, each act still
//! waiting is settled by its target, as the C++'s `findChild` failed:
//!
//! ```text
//! target drawn, settled      refused: the target has no such control ("--fill: the dialog has no
//!                            field NAME that takes 'TEXT'", "--press: TARGET has no button NAME")
//! target drawn in egui's     kept for the next frame: a new dialog is measured invisibly first,
//! measuring pass             and its controls are not yet there to take anything
//! target not drawn           refused: "KIND TEXT: TARGET has closed", main.cpp's sentence for a
//!                            target gone between two steps
//! ```
//!
//! So a step never waits on a control that is not there: one frame decides it.

use std::collections::BTreeMap;

use katana_core::text::format_general;

use super::registry::Snapshot;

pub type ActId = u64;

/// What a driver does to a control.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// `--fill NAME=TEXT`: the text written, chosen or ticked as the control's kind takes it.
    Fill(String),
    /// `--press NAME`: a click.
    Press,
    /// Brought into view, as QScrollArea::ensureWidgetVisible scrolls to a control.
    ScrollTo,
}

/// An act addressed to a control of a target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Posted {
    /// The target's object name, as the registry records it (`registry::WINDOW` for the window).
    pub target: String,
    /// The target as the step named it, which the press refusals say: `--dialog surveyInverse`
    /// opens `surveyInverseDialog`, and a press there is refused naming `surveyInverse`.
    pub said_as: String,
    /// The control's object name.
    pub name: String,
    pub act: Act,
}

impl Posted {
    fn new(target: &str, name: &str, act: Act) -> Self {
        Self {
            target: String::from(target),
            said_as: String::from(target),
            name: String::from(name),
            act,
        }
    }

    pub fn fill(target: &str, name: &str, text: &str) -> Self {
        Self::new(target, name, Act::Fill(String::from(text)))
    }

    pub fn press(target: &str, name: &str) -> Self {
        Self::new(target, name, Act::Press)
    }

    pub fn scroll_to(target: &str, name: &str) -> Self {
        Self::new(target, name, Act::ScrollTo)
    }

    /// The same act, its target named as the step named it.
    pub fn said_as(mut self, said_as: &str) -> Self {
        self.said_as = String::from(said_as);
        self
    }

    /// The step as main.cpp prints it: its switch and its text.
    fn step(&self) -> (&'static str, String) {
        match &self.act {
            Act::Fill(text) => ("--fill", format!("{}={text}", self.name)),
            Act::Press => ("--press", self.name.clone()),
            Act::ScrollTo => ("--scroll-to", self.name.clone()),
        }
    }
}

/// An act a control took, for it to answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Taken {
    pub id: ActId,
    pub posted: Posted,
}

/// What became of a posted act.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActOutcome {
    /// No frame has decided it yet.
    Waiting,
    Done,
    /// Refused, with the C++'s sentence.
    Refused(String),
}

/// The acts waiting for their controls, and what became of those decided.
#[derive(Debug, Default)]
pub struct Mailbox {
    next: ActId,
    waiting: Vec<(ActId, Posted)>,
    outcomes: BTreeMap<ActId, ActOutcome>,
}

impl Mailbox {
    pub fn post(&mut self, posted: Posted) -> ActId {
        self.next += 1;
        self.waiting.push((self.next, posted));
        self.next
    }

    /// The first act waiting for control `name` of `target`, taken out: done unless the control
    /// says otherwise (`settle`).
    pub fn take(&mut self, target: &str, name: &str) -> Option<Taken> {
        let at = self
            .waiting
            .iter()
            .position(|(_, posted)| posted.target == target && posted.name == name)?;
        let (id, posted) = self.waiting.remove(at);
        self.outcomes.insert(id, ActOutcome::Done);
        Some(Taken { id, posted })
    }

    /// A control's answer to the act it took.
    pub fn settle(&mut self, id: ActId, answer: Result<(), String>) {
        let outcome = match answer {
            Ok(()) => ActOutcome::Done,
            Err(sentence) => ActOutcome::Refused(sentence),
        };
        self.outcomes.insert(id, outcome);
    }

    pub fn outcome(&self, id: ActId) -> ActOutcome {
        self.outcomes
            .get(&id)
            .cloned()
            .unwrap_or(ActOutcome::Waiting)
    }

    /// How many acts are still waiting for their controls.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// The end of a frame that counts: each act still waiting is settled by its target (the rule
    /// in the module comment).
    pub fn end_frame(&mut self, frame: &Snapshot) {
        let mut kept = Vec::new();
        for (id, posted) in std::mem::take(&mut self.waiting) {
            match frame.target(&posted.target) {
                Some(target) if !target.settled => kept.push((id, posted)),
                Some(_) => {
                    self.outcomes
                        .insert(id, ActOutcome::Refused(untaken(&posted)));
                }
                None => {
                    self.outcomes
                        .insert(id, ActOutcome::Refused(has_closed(&posted)));
                }
            }
        }
        self.waiting = kept;
    }
}

// ---- the C++'s sentences ---------------------------------------------------------------------

/// No control of that name in a target that was drawn.
pub fn untaken(posted: &Posted) -> String {
    match &posted.act {
        Act::Fill(text) => no_field(&posted.name, text),
        Act::Press => no_button(posted),
        Act::ScrollTo => format!(
            "--scroll-to: {} has no control {}",
            posted.said_as, posted.name
        ),
    }
}

/// main.cpp: "%s %s: %s has closed" - the step, and the target gone.
pub fn has_closed(posted: &Posted) -> String {
    let (kind, text) = posted.step();
    format!("{kind} {text}: {} has closed", posted.said_as)
}

/// fillField: a field that cannot take the text, or none of that name.
pub fn no_field(name: &str, text: &str) -> String {
    format!("--fill: the dialog has no field {name} that takes '{text}'")
}

/// fillField: "a person cannot type into a disabled field" - named by the dialog's object name.
pub fn fill_disabled(posted: &Posted) -> String {
    format!(
        "--fill: {}'s field {} is disabled",
        posted.target, posted.name
    )
}

/// The --press step: no button of that name.
pub fn no_button(posted: &Posted) -> String {
    format!("--press: {} has no button {}", posted.said_as, posted.name)
}

/// The --press step: "a disabled button ignores a click: a test that pressed nothing must not
/// pass".
pub fn press_disabled(posted: &Posted) -> String {
    format!(
        "--press: {}'s button {} is disabled",
        posted.said_as, posted.name
    )
}

/// The --press step: "nor can a person click a button its owner has hidden".
pub fn press_hidden(posted: &Posted) -> String {
    format!(
        "--press: {}'s button {} is hidden",
        posted.said_as, posted.name
    )
}

/// fillField: a choice that lists no such item and takes no typed one.
pub fn no_choice(name: &str, text: &str) -> String {
    format!("--fill: {name} has no choice '{text}'")
}

/// fillField: a whole-number box given what is no whole number in its range.
pub fn whole_out_of_range(name: &str, minimum: i64, maximum: i64, text: &str) -> String {
    format!("--fill: {name} takes a whole number from {minimum} to {maximum}, not '{text}'")
}

/// fillField: a number box given what is no number in its range; the bounds as printf's %g.
pub fn number_out_of_range(name: &str, minimum: f64, maximum: f64, text: &str) -> String {
    format!(
        "--fill: {name} takes a number from {} to {}, not '{text}'",
        format_general(minimum, 6),
        format_general(maximum, 6)
    )
}

/// fillField: tabbed pages with no tab of that title.
pub fn no_tab(name: &str, text: &str) -> String {
    format!("--fill: {name} has no tab '{text}'")
}

/// fillField: a list, grid or tree with no row of that text.
pub fn lists_no(name: &str, text: &str) -> String {
    format!("--fill: {name} lists no '{text}'")
}
