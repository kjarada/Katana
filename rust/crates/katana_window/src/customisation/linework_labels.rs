//! What a person reads each linework control code by. The port of customisation/linework_labels.
//!
//! The seven controls have two kinds of name. A file, a refusal and CUSTOMISE SET call one
//! `arcStart` (katana_entity's linework_code_members) - a word for a script. A person is shown
//! "Begin curve". THIS is the one list of the second kind, keyed by the first, so that a control
//! has one name in the window: File > Settings lists the seven spellings under it
//! (settings_dialog), and the controls' one editor, the Survey Code Manager's Linework tab, labels
//! its seven fields from it. SettingsDialog.TheLineworkControlsAreCalledWhatTheirOneEditorCallsThem
//! reads the tab's labels against Settings' page, so a field built without the list fails there.
//!
//! Nothing of the managers is in it: either dialog uses it without the other.

/// One control: the name a file gives it and the words a person reads it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineworkControlLabel {
    /// katana_entity's LineworkCodeMember::name: "arcStart".
    pub member: &'static str,
    /// "Begin curve"
    pub label: &'static str,
}

/// In the order katana_entity's linework_code_members lists the controls, which is the order both
/// dialogs show them in.
pub const LINEWORK_CONTROL_LABELS: [LineworkControlLabel; 7] = [
    LineworkControlLabel {
        member: "start",
        label: "Start",
    },
    LineworkControlLabel {
        member: "end",
        label: "End",
    },
    LineworkControlLabel {
        member: "close",
        label: "Close",
    },
    LineworkControlLabel {
        member: "arcStart",
        label: "Begin curve",
    },
    LineworkControlLabel {
        member: "arcEnd",
        label: "End curve",
    },
    LineworkControlLabel {
        member: "join",
        label: "Join to point",
    },
    LineworkControlLabel {
        member: "rectangle",
        label: "Rectangle",
    },
];

/// The label of the control a file calls `member`. A control this list does not know yet - one
/// added to katana_entity's linework_code_members and not here - is shown by the name a file gives
/// it, rather than left out or left blank.
pub fn linework_control_label(member: &str) -> String {
    LINEWORK_CONTROL_LABELS
        .iter()
        .find(|each| each.member == member)
        .map_or_else(|| String::from(member), |each| String::from(each.label))
}
