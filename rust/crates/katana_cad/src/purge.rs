//! Purge: delete the styles, linetypes and hatch patterns nothing uses
//! (include/katana/cad/purge.hpp).
//!
//! What "uses" means is entity::table_usage's answer - the one the delete
//! guards and the managers' "Used" column read - so purge can never offer an
//! item a delete would then refuse, nor keep one a manager shows as unused.
//!
//! The plan is iterated to a FIXPOINT: purging a style can free the linetype
//! and hatch pattern only it named, and those go in the same purge rather
//! than needing a second one. Protected items ("continuous", "none") are
//! never in it: the model resolves to them when nothing else is said.

use katana_commands::entity_commands::TableItems;
use katana_commands::{CommandPtr, purge_table_items};
use katana_core::{Error, ErrorCode, Result};
use katana_entity::table_usage::table_usage;
use katana_entity::tables::{HatchPatternPolicy, LinetypePolicy};
use katana_entity::{HatchPattern, Linetype, Model, NamedTablePolicy, UsageOptions, Users};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PurgeOptions {
    pub styles: bool,
    pub linetypes: bool,
    pub hatches: bool,
    /// Styles to keep although no entity wears them yet: the Document's
    /// current style, which new work is about to be drawn in. What such a
    /// style names is kept with it.
    pub keep_styles: Vec<String>,
}

impl Default for PurgeOptions {
    fn default() -> Self {
        Self {
            styles: true,
            linetypes: true,
            hatches: true,
            keep_styles: Vec::new(),
        }
    }
}

fn listed(sorted: &[String], name: &str) -> bool {
    sorted
        .binary_search_by(|probe| probe.as_str().cmp(name))
        .is_ok()
}

fn insert_sorted(sorted: &mut Vec<String>, name: &str) {
    // Where std::lower_bound puts it: before the first that is not less.
    let at = sorted.partition_point(|probe| probe.as_str() < name);
    sorted.insert(at, String::from(name));
}

// Free once the plan so far is gone: named by no layer, by no style that is
// staying, and reached by no entity except through what is going. Entities
// reach a linetype or hatch pattern only through a layer or a style, and a
// style is only planned once no entity wears it, so "no layer and no staying
// style" is the whole test for those two.
fn free_once_the_plan_is_gone(users: &Users, planned_styles: &[String]) -> bool {
    users.layers.is_empty()
        && users
            .styles
            .iter()
            .all(|style| listed(planned_styles, style))
}

/// The unused items, ascending within each table. Empty when nothing is.
/// (The C++ default options are PurgeOptions::default().)
pub fn plan_purge(model: &Model, options: &PurgeOptions) -> TableItems {
    let usage = table_usage(model, UsageOptions::default());
    let mut plan = TableItems::default();

    // Rounds until one adds nothing. Each kind is looked at every round, so
    // the order of the kinds below decides nothing, and an item freed by one
    // purged in the same round is found in the next.
    let mut added = true;
    while added {
        added = false;
        if options.styles {
            for (name, users) in &usage.styles {
                if !model.styles.contains(name)
                    || listed(&plan.styles, name)
                    || options.keep_styles.contains(name)
                {
                    continue;
                }
                // A style is held by the entities wearing it and nothing
                // else: no table names a style.
                if users.entities == 0 && free_once_the_plan_is_gone(users, &plan.styles) {
                    insert_sorted(&mut plan.styles, name);
                    added = true;
                }
            }
        }
        if options.linetypes {
            for (name, users) in &usage.linetypes {
                if !model.linetypes.contains(name)
                    || <LinetypePolicy as NamedTablePolicy<Linetype>>::is_protected(name)
                    || listed(&plan.linetypes, name)
                {
                    continue;
                }
                if free_once_the_plan_is_gone(users, &plan.styles) {
                    insert_sorted(&mut plan.linetypes, name);
                    added = true;
                }
            }
        }
        if options.hatches {
            for (name, users) in &usage.hatch_patterns {
                if !model.hatch_patterns.contains(name)
                    || <HatchPatternPolicy as NamedTablePolicy<HatchPattern>>::is_protected(name)
                    || listed(&plan.hatch_patterns, name)
                {
                    continue;
                }
                if free_once_the_plan_is_gone(users, &plan.styles) {
                    insert_sorted(&mut plan.hatch_patterns, name);
                    added = true;
                }
            }
        }
    }
    plan
}

/// ONE command deleting plan_purge's items, so one undo restores them all; or
/// None when nothing is unused - never an empty undo step (the shape of audit
/// QT-01). Fails with InvalidArgument when the options ask for nothing.
pub fn purge_command(model: &Model, options: &PurgeOptions) -> Result<Option<CommandPtr>> {
    if !options.styles && !options.linetypes && !options.hatches {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "a purge has to be asked for styles, linetypes or hatch patterns",
        ));
    }
    let plan = plan_purge(model, options);
    if plan.empty() {
        return Ok(None);
    }
    Ok(Some(purge_table_items(plan)))
}
