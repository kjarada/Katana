//! The Document's road-model members (document.rs;
//! src/katana_cad/road/road_document.cpp), kept here with the rest of the road
//! model as the sheet set's are kept with the sheets: the document carries
//! only the JSON; everything that knows what a template or a corridor is lives
//! in road/. Document::road_model_changed, which compares the road model's
//! metadata keys after every step, is document.rs's.
//!
//! The parsed sets are the document's road_template_cache and corridor_cache
//! (CacheSlot::parsed_key, the C++ `upToDate`), and a change is the document's
//! parsed_key_command (the C++ `changeCommand` and its RoadModelCommand): the
//! states before and after, swapped by undo and redo without parsing again.

use std::collections::BTreeSet;
use std::sync::Arc;

use katana_commands::CommandPtr;
use katana_core::error::make_error;
use katana_core::{ErrorCode, Result, Status};

use crate::document::{
    CORRIDORS_KEY, CacheSlot, Document, DocumentState, ParsedKey, ROAD_TEMPLATES_KEY,
};
use crate::road::corridor_model::{
    CorridorSet, check_corridor, corridors_from_json, corridors_to_json,
};
use crate::road::road_template::{
    RoadTemplateSet, check_template, road_templates_from_json, road_templates_to_json,
};

fn check_templates(set: &RoadTemplateSet) -> Status {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for each in &set.templates {
        check_template(each)?;
        if !names.insert(each.name.to_ascii_lowercase()) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "two road templates have one name",
                each.name.as_str(),
            ));
        }
    }
    Ok(())
}

fn check_corridors(set: &CorridorSet) -> Status {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for each in &set.corridors {
        check_corridor(each)?;
        if !names.insert(each.name.to_ascii_lowercase()) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "two corridors have one name",
                each.name.as_str(),
            ));
        }
    }
    Ok(())
}

// Which of the state's slots a step puts its parsed state back in.
fn road_template_slot(state: &DocumentState) -> &CacheSlot {
    &state.road_template_cache
}

fn corridor_slot(state: &DocumentState) -> &CacheSlot {
    &state.corridor_cache
}

impl Document {
    // ---- the road model (docs/road_design.md) -----------------------------------
    //
    // The drawing's road templates and corridors, each kept as versioned JSON
    // under a project metadata key of its own ("road_templates", "corridors"),
    // as the sheets are - the storage layer carries a key it does not read, so
    // no schema change - and parsed once per change. Empty when the project
    // has none, or when what it has cannot be read; the status then says why,
    // and the setters refuse to overwrite what a newer Katana wrote. Each is
    // replaced as ONE undoable step named `step_name` (the C++ defaults it to
    // Document::SET_ROAD_TEMPLATES and SET_CORRIDORS); InvalidArgument for a
    // set check_template or check_corridor refuses, or two of one name. A set
    // equal to the current one records nothing.
    //
    // Every change of the road model's keys - these two and the placed
    // objects' "road_objects" - is reported as DocumentChange::ROAD_MODEL with
    // the step that made it (Document::road_model_changed).

    // The templates' key, parsed when its text has changed since.
    fn road_template_key(&self) -> Arc<ParsedKey<RoadTemplateSet>> {
        self.state.road_template_cache.parsed_key(
            &self.state.metadata,
            ROAD_TEMPLATES_KEY,
            road_templates_from_json,
        )
    }

    fn corridor_key(&self) -> Arc<ParsedKey<CorridorSet>> {
        self.state.corridor_cache.parsed_key(
            &self.state.metadata,
            CORRIDORS_KEY,
            corridors_from_json,
        )
    }

    pub fn road_templates(&self) -> Arc<RoadTemplateSet> {
        Arc::clone(&self.road_template_key().set)
    }

    pub fn road_templates_status(&self) -> Status {
        self.road_template_key().status.clone()
    }

    pub fn set_road_templates(
        &mut self,
        templates: &RoadTemplateSet,
        step_name: impl Into<String>,
    ) -> Status {
        match self.road_templates_command(templates, step_name)? {
            None => Ok(()), // nothing changed, and nothing to undo
            Some(command) => self.execute(command),
        }
    }

    /// As a command, for a step that does more besides (a template renamed
    /// with the corridors that name it); None when nothing changes.
    pub fn road_templates_command(
        &self,
        templates: &RoadTemplateSet,
        step_name: impl Into<String>,
    ) -> Result<Option<CommandPtr>> {
        check_templates(templates)?;
        // Brought up to date with the metadata: it is the step's "before".
        let before = self.road_template_key();
        self.parsed_key_command(
            road_template_slot,
            ROAD_TEMPLATES_KEY,
            before,
            templates,
            road_templates_to_json,
            step_name,
            "road templates",
        )
    }

    pub fn corridors(&self) -> Arc<CorridorSet> {
        Arc::clone(&self.corridor_key().set)
    }

    pub fn corridors_status(&self) -> Status {
        self.corridor_key().status.clone()
    }

    pub fn set_corridors(
        &mut self,
        corridors: &CorridorSet,
        step_name: impl Into<String>,
    ) -> Status {
        match self.corridors_command(corridors, step_name)? {
            None => Ok(()), // nothing changed, and nothing to undo
            Some(command) => self.execute(command),
        }
    }

    /// The same change as a command, for a step that does more besides -
    /// CORRIDOR NEW makes the layers its objects are shown on in the same
    /// step. None when `corridors` equals the current set.
    pub fn corridors_command(
        &self,
        corridors: &CorridorSet,
        step_name: impl Into<String>,
    ) -> Result<Option<CommandPtr>> {
        check_corridors(corridors)?;
        let before = self.corridor_key();
        self.parsed_key_command(
            corridor_slot,
            CORRIDORS_KEY,
            before,
            corridors,
            corridors_to_json,
            step_name,
            "corridors",
        )
    }
}
