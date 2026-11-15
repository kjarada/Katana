//! Loading customisations ON TOP of the one a session has
//! (include/katana/cad/customisation_merge.hpp;
//! src/katana_cad/customisation/customisation_merge.cpp).
//!
//! A session starts with a customisation and a person then loads their own
//! beside it: a few symbols, a table of codes for one client. That load is
//! MERGED into what is there, and Replace is the other choice, said in so many
//! words:
//!
//! ```text
//!   Merge    A loaded definition takes the place of the one of its name. The
//!            rules a load gives a key in a section take the place of that
//!            key's rules in that section; everything else is kept.
//!   Replace  The load takes the place of each KIND it brought: its definitions
//!            become the whole library, its rules the whole survey map. A kind
//!            it did not bring is kept - a file of symbols alone never empties
//!            the survey codes, in either mode.
//! ```
//!
//! What "the same rule" is, where rules have no names, is A KEY IN A SECTION.
//! A customisation says one aspect of a code per section - where it goes, its
//! symbol, its attributes - so a personal file that gives `WM*` a new colour
//! replaces the `WM*` feature rules and leaves the `WM*` symbol alone.
//! Appending instead would do nothing at all: among rules of one key the
//! EARLIER wins a field (entity's SurveyMap::add), so the rules already loaded
//! would outrank every rule the person had just loaded.
//!
//! WHERE the loaded rules go matters among rules of their own key, and in
//! EVERY section, because some fields are filled by more than one: the
//! string's attributes by the pipe and the attribute sections both. So every
//! rule the load gives a key goes in AHEAD of every current rule of that key
//! it leaves standing - at the first of them. A key the session does not have
//! follows, in the order it was loaded.
//!
//! The rest of a customisation is merged by what it is:
//!
//! ```text
//!   colours     by name, compared as colour names are (fold_colour_name): a
//!               loaded name takes the place of the session's, in both modes.
//!   linework,   taken only when the loaded customisation SAYS them. A file of
//!   automation  symbols for a colleague must not reset their control codes.
//!   sources     recorded (customisation_record.rs): each loaded customisation
//!               is a source, by name, with what it brought and its notice.
//!               One that LISTS its sources - every customisation a session
//!               writes does - brings those instead, each with its own notice;
//!               its own notice, which it says at its top level, goes with
//!               the source of its name, or with every source it lists when
//!               none has its name (written under another name than the
//!               session had, nothing says which of them the notice came
//!               with). An author's notice is never dropped by a merge.
//!   name,       the session's own - a load is added to it. They are the
//!   description first loaded customisation's when the session has no name yet,
//!   notice,     and when a Replace brought both kinds, after which nothing of
//!   basedOn     the session's definitions or rules is left to be named by it.
//! ```
//!
//! This is the merge the older style libraries and survey code files were
//! loaded through, moved onto the one customisation type; the rule is the
//! same. It reads no file: read_customisation_file (customisation_host) does
//! that.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use katana_entity::colour_names::fold_colour_name;
use katana_entity::customisation::validate_customisation_name;
use katana_entity::style_library::add_or_replace;
use katana_entity::{
    ColourTableEntry, Customisation, EnumName, StyleLibrary, SurveyRule, SurveySection,
};

use crate::customisation_record::{CustomisationSource, record_customisation_load};
use crate::customisation_state::customisation_faults;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LoadMode {
    #[default]
    Merge,
    Replace,
}

/// "merge" or "replace".
impl EnumName for LoadMode {
    fn name(self) -> &'static str {
        match self {
            LoadMode::Merge => "merge",
            LoadMode::Replace => "replace",
        }
    }
}

/// What one loaded customisation did to what was there before the load.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CustomisationLoad {
    pub name: String,
    pub definitions: bool, // it brought linestyle or symbol definitions
    pub rules: bool,       // it brought survey code rules
    /// Definitions by name. Two loaded customisations defining one name leave
    /// the later's, and so the later is the one that brought it.
    pub added_definitions: Vec<String>,
    pub replaced_definitions: Vec<String>,
    /// Rule keys, once for each section the customisation gives the key rules
    /// in: one giving `WM*` three feature rules replaced one thing, not three,
    /// and one giving it a feature and a symbol rule lists it twice.
    pub added_keys: Vec<String>,
    pub replaced_keys: Vec<String>,
    /// Colour names, as the customisation writes them.
    pub added_colours: Vec<String>,
    pub replaced_colours: Vec<String>,
    pub linework: bool,   // it says the linework codes, which were taken
    pub automation: bool, // it says the automation switches, which were taken
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomisationMerge {
    /// What to install - the session untouched when there is a problem.
    pub merged: Customisation,
    /// Whether the load brought any of each kind; a kind it did not bring is
    /// the session's own in `merged`.
    pub definitions_loaded: bool,
    pub rules_loaded: bool,
    /// One entry per loaded customisation, in load order - one that changed
    /// nothing included: "loaded, and it added nothing" is an answer a person
    /// wants.
    pub loads: Vec<CustomisationLoad>,
    /// Replace only: what the load did not bring and so is gone.
    pub removed_definitions: Vec<String>,
    pub removed_keys: Vec<String>,
    /// Everything that stops the load, each naming the customisation it is
    /// about and then the part of it (customisation_faults is what is looked
    /// for). A load is ALL OR NOTHING: with any problem `merged` is the
    /// session as it was, so that half a load is never installed.
    pub problems: Vec<String>,
}

impl CustomisationMerge {
    pub fn ok(&self) -> bool {
        self.problems.is_empty()
    }
}

/// A rule's identity where rules have no names: its section and its key.
type RuleGroup<'a> = (SurveySection, &'a str);

fn group_of(rule: &SurveyRule) -> RuleGroup<'_> {
    (rule.section, rule.key.as_str())
}

/// A loaded rule, with the customisation of the load it came from.
#[derive(Clone, Copy)]
struct LoadedRule<'a> {
    rule: &'a SurveyRule,
    load: usize,
}

struct Merger<'a> {
    current: &'a Customisation,
    loaded: &'a [Customisation],
    mode: LoadMode,
    out: CustomisationMerge,
}

impl<'a> Merger<'a> {
    fn run(mut self) -> CustomisationMerge {
        if self.loaded.is_empty() {
            // Nothing was loaded, so nothing changes - not even the session's
            // list of sources, which a load would otherwise have completed.
            self.out.merged = self.current.clone();
            return self.out;
        }
        self.check_loads();
        self.merge_library();
        self.merge_map();
        self.merge_colours();
        self.merge_settings();
        self.merge_header();
        if !self.out.problems.is_empty() {
            // All or nothing: half a load is not something to install.
            self.out.merged = self.current.clone();
        }
        self.out
    }

    /// How a problem names the customisation it is about.
    fn label_of(&self, index: usize) -> String {
        let name = &self.loaded[index].name;
        if validate_customisation_name(name).is_ok() {
            return format!("\"{name}\"");
        }
        format!("customisation {} of this load", index + 1)
    }

    fn problem(&mut self, index: usize, what: &str) {
        let line = format!("{}: {what}", self.label_of(index));
        self.out.problems.push(line);
    }

    /// One entry per loaded customisation, in order, and everything about one
    /// that would make the session unfit to be kept or recorded - the very
    /// list Document::install_customisation refuses on (customisation_faults),
    /// so that what a load is told here and what an install would then refuse
    /// cannot come to differ. A customisation read from a file has passed all
    /// of this already; one built in code has not.
    fn check_loads(&mut self) {
        let loaded = self.loaded;
        for (index, load) in loaded.iter().enumerate() {
            self.out.loads.push(CustomisationLoad {
                name: load.name.clone(),
                definitions: !load.library.empty(),
                rules: !load.map.empty(),
                ..CustomisationLoad::default()
            });
            for fault in customisation_faults(load) {
                let what = if fault.context.is_empty() {
                    fault.message
                } else {
                    format!("{} [{}]", fault.message, fault.context)
                };
                self.problem(index, &what);
            }
        }
    }

    fn merge_library(&mut self) {
        let current = self.current;
        let loaded = self.loaded;
        // Two loaded customisations defining one name leave the later's
        // definition, and so the later is the one that brought it.
        let mut owner: BTreeMap<String, usize> = BTreeMap::new();
        for (index, load) in loaded.iter().enumerate() {
            load.library.for_each(|definition| {
                owner.insert(definition.name.clone(), index);
            });
        }
        self.out.definitions_loaded = !owner.is_empty();
        if !self.out.definitions_loaded {
            // A load of survey codes alone. Installing its empty library
            // would make every linestyle and symbol stop drawing.
            self.out.merged.library = current.library.clone();
            return;
        }
        self.out.merged.library = if self.mode == LoadMode::Merge {
            current.library.clone()
        } else {
            StyleLibrary::default()
        };
        for (name, &index) in &owner {
            let Some(definition) = loaded[index].library.find(name) else {
                continue; // the owner was taken from this very library
            };
            let load = &mut self.out.loads[index];
            if current.library.contains(name) {
                load.replaced_definitions.push(name.clone());
            } else {
                load.added_definitions.push(name.clone());
            }
            // Cannot fail: the definition was validated when it entered the
            // loaded library, a table that refuses an invalid one. Reported
            // rather than assumed, so that if that ever changes the definition
            // stops the load instead of going missing without a word.
            if let Err(error) = add_or_replace(&mut self.out.merged.library, definition.clone()) {
                self.problem(index, &error.describe());
            }
        }
        if self.mode == LoadMode::Replace {
            let removed = &mut self.out.removed_definitions;
            current.library.for_each(|definition| {
                if !owner.contains_key(&definition.name) {
                    removed.push(definition.name.clone());
                }
            });
        }
    }

    fn add(&mut self, rule: &SurveyRule, index: usize) {
        // As for definitions: every rule came from a map that validated it.
        if let Err(status) = self.out.merged.map.add(rule.clone()) {
            self.problem(index, &status.describe());
        }
    }

    fn merge_map(&mut self) {
        let current = self.current;
        let loaded = self.loaded;
        // The loaded rules in the order they were loaded, each with the
        // customisation it came from.
        let mut rules: Vec<LoadedRule<'a>> = Vec::new();
        for (index, load) in loaded.iter().enumerate() {
            for rule in load.map.rules() {
                rules.push(LoadedRule { rule, load: index });
            }
        }
        self.out.rules_loaded = !rules.is_empty();
        if !self.out.rules_loaded {
            // A load of symbols alone. Installing its empty map would leave
            // nothing to code a survey with.
            self.out.merged.map = current.map.clone();
            return;
        }

        // Sets asked only whether they hold a group, never walked, so their
        // order is never seen.
        let current_groups: HashSet<RuleGroup<'a>> =
            current.map.rules().iter().map(group_of).collect();
        let mut loaded_groups: HashSet<RuleGroup<'a>> = HashSet::new();
        let mut reported: HashSet<(usize, RuleGroup<'a>)> = HashSet::new();
        for entry in &rules {
            let group = group_of(entry.rule);
            loaded_groups.insert(group);
            // Once per customisation, key and section: one giving `WM*` three
            // feature rules replaced one thing, not three.
            if reported.insert((entry.load, group)) {
                let load = &mut self.out.loads[entry.load];
                if current_groups.contains(&group) {
                    load.replaced_keys.push(String::from(group.1));
                } else {
                    load.added_keys.push(String::from(group.1));
                }
            }
        }

        if self.mode == LoadMode::Replace {
            let mut kept: BTreeSet<&str> = BTreeSet::new();
            for entry in &rules {
                self.add(entry.rule, entry.load);
                kept.insert(entry.rule.key.as_str());
            }
            for key in current.map.keys() {
                if !kept.contains(key.as_str()) {
                    self.out.removed_keys.push(key);
                }
            }
            return;
        }

        // Merge: at the first current rule of a key the load gives rules for,
        // ALL the loaded rules of that key go in, in their order; after them,
        // the current rules of that key in sections the load did not give it.
        // The current rules of a loaded (section, key) group are dropped -
        // they are what was replaced.
        let mut loaded_by_key: BTreeMap<&str, Vec<LoadedRule<'a>>> = BTreeMap::new();
        for entry in &rules {
            loaded_by_key
                .entry(entry.rule.key.as_str())
                .or_default()
                .push(*entry);
        }
        let mut placed: BTreeSet<&str> = BTreeSet::new();
        for rule in current.map.rules() {
            if let Some(found) = loaded_by_key.get(rule.key.as_str())
                && placed.insert(rule.key.as_str())
            {
                for entry in found {
                    self.add(entry.rule, entry.load);
                }
            }
            if !loaded_groups.contains(&group_of(rule)) {
                // A current rule is the session's own; were it refused, the
                // fault would lie with no customisation of this load.
                if let Err(status) = self.out.merged.map.add(rule.clone()) {
                    self.out
                        .problems
                        .push(format!("the session's own rules: {}", status.describe()));
                }
            }
        }
        for entry in &rules {
            if !placed.contains(entry.rule.key.as_str()) {
                self.add(entry.rule, entry.load);
            }
        }
    }

    fn merge_colours(&mut self) {
        let current = self.current;
        let loaded = self.loaded;
        // By the fold, which is a colour name's identity: the loaded "SUI Gas"
        // takes the place of the session's "sui_gas".
        let mut by_fold: BTreeMap<String, ColourTableEntry> = BTreeMap::new();
        for entry in current.colours.entries() {
            by_fold.insert(fold_colour_name(&entry.name), entry);
        }
        // Whose entry each fold is, for a problem to name: none for the
        // session's own.
        let mut owner: BTreeMap<String, usize> = BTreeMap::new();
        for (index, load) in loaded.iter().enumerate() {
            for entry in load.colours.entries() {
                let record = &mut self.out.loads[index];
                if current.colours.find(&entry.name).is_some() {
                    record.replaced_colours.push(entry.name.clone());
                } else {
                    record.added_colours.push(entry.name.clone());
                }
                let fold = fold_colour_name(&entry.name);
                owner.insert(fold.clone(), index);
                by_fold.insert(fold, entry);
            }
        }
        for (fold, entry) in &by_fold {
            // Cannot fail, each entry having come out of a table that took
            // it and no two sharing a fold; reported if it ever does.
            if let Err(status) = self
                .out
                .merged
                .colours
                .add(entry.name.clone(), entry.colour)
            {
                match owner.get(fold) {
                    Some(&index) => self.problem(index, &status.describe()),
                    None => self
                        .out
                        .problems
                        .push(format!("the session's own colours: {}", status.describe())),
                }
            }
        }
    }

    fn merge_settings(&mut self) {
        let current = self.current;
        let loaded = self.loaded;
        self.out.merged.linework = current.linework.clone();
        self.out.merged.automation = current.automation;
        for (index, load) in loaded.iter().enumerate() {
            // Only what a customisation SAYS: one that is silent about the
            // control codes leaves the session's as they are.
            if load.linework.is_some() {
                self.out.merged.linework = load.linework.clone();
                self.out.loads[index].linework = true;
            }
            if load.automation.is_some() {
                self.out.merged.automation = load.automation;
                self.out.loads[index].automation = true;
            }
        }
    }

    fn merge_header(&mut self) {
        let current = self.current;
        let loaded = self.loaded;
        let replace = self.mode == LoadMode::Replace;
        // The session keeps its name: a load is added to it. Unless it has
        // none yet, or the load took the place of both kinds, after which
        // none of its definitions or rules is left for the name to be about.
        let takes_over = current.name.is_empty()
            || (replace && self.out.definitions_loaded && self.out.rules_loaded);
        let header = if takes_over { &loaded[0] } else { current };
        self.out.merged.name = header.name.clone();
        self.out.merged.description = header.description.clone();
        self.out.merged.notice = header.notice.clone();
        self.out.merged.based_on = header.based_on.clone();

        // A customisation that lists no sources is its own one source - the
        // session too, as Document::install_customisation has it.
        let mut sources = current.sources.clone();
        if sources.is_empty() && !current.name.is_empty() {
            sources.push(CustomisationSource {
                name: current.name.clone(),
                definitions: !current.library.empty(),
                rules: !current.map.empty(),
                notice: Vec::new(),
            });
        }
        let mut brought: Vec<CustomisationSource> = Vec::new();
        for (index, load) in loaded.iter().enumerate() {
            // Its notice goes with it as a source, except where it has just
            // become the session's own notice above.
            let is_header = takes_over && index == 0;
            let notice = if is_header {
                Vec::new()
            } else {
                load.notice.clone()
            };
            if load.sources.is_empty() {
                brought.push(CustomisationSource {
                    name: load.name.clone(),
                    definitions: !load.library.empty(),
                    rules: !load.map.empty(),
                    notice,
                });
                continue;
            }
            // It lists what went into it, and those are its sources, each
            // with its own notice. Its OWN notice is not among them: a
            // session writes it at the top level and leaves the entry of its
            // name without one (Document::customisation). Copying the list
            // and no more dropped it - an author's notice gone from every
            // customisation a session had written, the moment it was loaded
            // into another.
            let first = brought.len();
            brought.extend(load.sources.iter().cloned());
            if notice.is_empty() {
                continue;
            }
            if let Some(own) = brought[first..]
                .iter_mut()
                .find(|source| source.name == load.name)
            {
                carry(own, &notice);
                continue;
            }
            // None has its name: it was written under another than the
            // session had, and nothing says which of them the notice came
            // with. Every one carries it, rather than none.
            for source in &mut brought[first..] {
                carry(source, &notice);
            }
        }
        record_customisation_load(
            &mut sources,
            &brought,
            replace && self.out.definitions_loaded,
            replace && self.out.rules_loaded,
        );
        self.out.merged.sources = sources;
    }
}

/// `source` now also says `notice`: those lines first, then the lines it had
/// that are not among them, so that a notice said in both places is said
/// once.
fn carry(source: &mut CustomisationSource, notice: &[String]) {
    let mut lines = notice.to_vec();
    for line in std::mem::take(&mut source.notice) {
        if !notice.contains(&line) {
            lines.push(line);
        }
    }
    source.notice = lines;
}

/// `current` is the session (Document::customisation()), `loaded` the
/// customisations of one load in the order they were named. Install `merged`
/// with Document::install_customisation when the result is ok() - and heed
/// what that returns: the loads were judged here, the session's own
/// definitions and sources are judged there.
pub fn merge_customisation(
    current: &Customisation,
    loaded: &[Customisation],
    mode: LoadMode,
) -> CustomisationMerge {
    Merger {
        current,
        loaded,
        mode,
        out: CustomisationMerge::default(),
    }
    .run()
}
