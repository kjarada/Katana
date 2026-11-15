//! An entity's properties read as the tree their names describe, one level at
//! a time: what the Properties panel shows and what PROP TREE replies
//! (include/katana/cad/property_outline.hpp, src/katana_cad/property_outline.cpp).
//!
//! The model keeps one flat map of properties per entity; an archive import
//! flattens its attribute groups into it with '/' - "Asset/Dimensions/Size" -
//! and a string's per-vertex attributes as "vertex/3/Name" (docs/desktop.md,
//! "The attribute manager"). A surveyed string of a thousand vertices with
//! thirty attributes each is thirty thousand keys: shown flat, the panel was a
//! list nobody could read, and built all at once it was thirty thousand rows
//! made before anything could be seen. The owner's request of 2026-09-26 was a
//! tree "because some lines are very long and contain lots of attributes in
//! the vertices, so need a smart way to load all these information".
//!
//! So an outline is asked for ONE LEVEL: the entries directly beneath a path,
//! each saying how much lies beneath it, and a front end asks for the next
//! level only when a person opens it - or an agent names it (PROP TREE ...
//! UNDER path). Nothing is stored: the tree is a reading of the names, as the
//! layer tree is a reading of '/'-separated layer paths.
//!
//! Several entities are read together as the attribute manager reads them: a
//! value shows only where every one of them holds the same value at that
//! name; one that only some hold, or that they hold differently, VARIES,
//! because showing the first entity's value would put words in the others'
//! mouths.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;

use katana_core::ErrorCode;
use katana_core::Result;
use katana_core::error::make_error;
use katana_core::text::{ascii_lower, parse_integer};
use katana_entity::entity::{property_to_string, type_name};
use katana_entity::{PropertyMap, PropertyValue};

use crate::document::Document;
use crate::scope_verbs::{
    ScopeViewProvider, match_scope, parse_scope_words, record_value, scope_record,
};

/// One entry directly beneath a path.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PropertyOutlineEntry {
    /// The part of the name after the parent's path: "Dimensions", "3".
    pub name: String,
    /// The whole name up to here: "Asset/Dimensions". A key when a value is
    /// stored under it (`holders` > 0), and a prefix when names go on beneath
    /// it (`children` > 0); it can be both.
    pub path: String,
    /// How many of the entities read store a value under exactly `path`.
    pub holders: usize,
    /// That value, when every entity read holds one and they all agree.
    pub value: Option<PropertyValue>,
    /// Some hold it and some do not, or they hold different values.
    pub varies: bool,
    /// The distinct names directly beneath `path`, and every value beneath it
    /// at any depth (counted once per entity that holds it).
    pub children: usize,
    pub values: usize,
}

/// The most entries one PROP TREE reply lists unless LIMIT says otherwise: a
/// page an agent reads at once, as the panel shows a page at a time.
pub const PROPERTY_TREE_PAGE: usize = 200;

// Less, equal or greater, comparing runs of digits as numbers and, with
// `fold_case`, letters without regard to case.
fn natural_compare(a: &[u8], b: &[u8], fold_case: bool) -> Ordering {
    let mut i = 0;
    let mut j = 0;
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            // A number of any length: past its leading zeros, the longer run
            // is the larger number, and runs of one length compare as text.
            let mut end_a = i;
            while end_a < a.len() && a[end_a].is_ascii_digit() {
                end_a += 1;
            }
            let mut end_b = j;
            while end_b < b.len() && b[end_b].is_ascii_digit() {
                end_b += 1;
            }
            let mut start_a = i;
            while start_a + 1 < end_a && a[start_a] == b'0' {
                start_a += 1;
            }
            let mut start_b = j;
            while start_b + 1 < end_b && b[start_b] == b'0' {
                start_b += 1;
            }
            let length_a = end_a - start_a;
            let length_b = end_b - start_b;
            if length_a != length_b {
                return length_a.cmp(&length_b);
            }
            let digits = a[start_a..end_a].cmp(&b[start_b..end_b]);
            if digits != Ordering::Equal {
                return digits;
            }
            i = end_a;
            j = end_b;
            continue;
        }
        let x = if fold_case { ascii_lower(a[i]) } else { a[i] };
        let y = if fold_case { ascii_lower(b[j]) } else { b[j] };
        if x != y {
            return x.cmp(&y);
        }
        i += 1;
        j += 1;
    }
    if i < a.len() {
        return Ordering::Greater;
    }
    if j < b.len() {
        Ordering::Less
    } else {
        Ordering::Equal
    }
}

// natural_less as an ordering, for a sort.
fn natural_order(a: &str, b: &str) -> Ordering {
    // Equal but for case or leading zeros: the plain order, which is total.
    natural_compare(a.as_bytes(), b.as_bytes(), true).then_with(|| a.cmp(b))
}

/// The order entries are listed in: digits compare as whole numbers, so
/// "vertex/2" comes before "vertex/10" - an archive numbers vertices from 1 and
/// the map's own order would put 10 to 19 between 1 and 2 - and letters
/// without regard to case, a tie then broken by case, so the order is total.
pub fn natural_less(a: &str, b: &str) -> bool {
    natural_order(a, b) == Ordering::Less
}

// What one level gathers for one name beneath the path.
#[derive(Default)]
struct Gathered<'a> {
    holders: usize,
    first: Option<&'a PropertyValue>,
    differs: bool,
    // Slices of the maps' keys, which outlive the call.
    children: BTreeSet<&'a str>,
    values: usize,
}

// The entries of `properties` whose keys begin with `prefix` ("" for all of
// them): from the first key not below it, while they share it.
fn beneath<'a>(
    properties: &'a PropertyMap,
    prefix: &'a str,
) -> impl Iterator<Item = (&'a String, &'a PropertyValue)> + 'a {
    properties
        .range::<str, _>((Bound::Included(prefix), Bound::Unbounded))
        .take_while(move |(key, _)| key.starts_with(prefix))
}

/// The entries directly beneath `path` in `properties`, in natural_less order.
/// `path` is "" for the top, or a name without a trailing '/'. A path nothing
/// is stored beneath gives no entries; asking costs the keys beneath `path`,
/// never the whole map.
pub fn property_outline(properties: &PropertyMap, path: &str) -> Vec<PropertyOutlineEntry> {
    property_outline_of(&[properties], path)
}

/// The same over several maps read as one (see the top of this file for what
/// varies) - the C++ overload over a span of maps. Empty `maps`: no entries.
pub fn property_outline_of(maps: &[&PropertyMap], path: &str) -> Vec<PropertyOutlineEntry> {
    let prefix = if path.is_empty() {
        String::new()
    } else {
        format!("{path}/")
    };
    let mut gathered: BTreeMap<&str, Gathered<'_>> = BTreeMap::new();
    for &properties in maps {
        for (key, value) in beneath(properties, &prefix) {
            let rest = &key[prefix.len()..];
            match rest.find('/') {
                None => {
                    let entry = gathered.entry(rest).or_default();
                    entry.holders += 1;
                    match entry.first {
                        None => entry.first = Some(value),
                        Some(first) if first != value => entry.differs = true,
                        Some(_) => {}
                    }
                }
                Some(slash) => {
                    let entry = gathered.entry(&rest[..slash]).or_default();
                    let below = &rest[slash + 1..];
                    entry
                        .children
                        .insert(&below[..below.find('/').unwrap_or(below.len())]);
                    entry.values += 1;
                }
            }
        }
    }

    let mut entries = Vec::with_capacity(gathered.len());
    for (name, entry) in &gathered {
        let varies = entry.holders > 0 && (entry.differs || entry.holders < maps.len());
        entries.push(PropertyOutlineEntry {
            name: String::from(*name),
            path: format!("{prefix}{name}"),
            holders: entry.holders,
            value: if entry.holders > 0 && !varies {
                entry.first.cloned()
            } else {
                None
            },
            varies,
            children: entry.children.len(),
            values: entry.values,
        });
    }
    entries.sort_by(|a, b| natural_order(&a.name, &b.name));
    entries
}

/// Whether anything is stored under `path` or beneath it: what tells a front
/// end to draw an expander before it reads the level. O(log n).
pub fn has_properties_under(properties: &PropertyMap, path: &str) -> bool {
    if path.is_empty() {
        return !properties.is_empty();
    }
    if properties.contains_key(path) {
        return true;
    }
    let prefix = format!("{path}/");
    beneath(properties, &prefix).next().is_some()
}

fn whole_number(word: &str, what: &str, least: usize) -> Result<usize> {
    match parse_integer(word) {
        Some(value) if value >= least as i64 => Ok(value as usize),
        _ => Err(make_error(
            ErrorCode::ParseFailure,
            format!(
                "{what} takes a whole number{}",
                if least > 0 {
                    format!(" of at least {least}")
                } else {
                    String::new()
                }
            ),
            word,
        )),
    }
}

/// PROP TREE [scope] [WHERE k=v ...] [UNDER path] [FROM n] [LIMIT n] - `words`
/// are those after TREE. The scope is the shared grammar (scope_verbs.rs), the
/// selection when none is given; `views` answers VIEW. The reply is one record
/// for the level, then one per entry:
///
/// ```text
/// scope=selection matched=1 under="" entries=3 from=0 shown=3
/// path=Asset children=2 values=5
/// path=Name value=KERB type=text
/// path=vertex children=1250 values=37500
/// ```
///
/// An entry holding a value says value= and type=, or varies=yes with how many
/// hold it (holders=); one with names beneath it says children= and values=. A
/// scope that takes nothing is reported (matched=0 entries=0), not refused.
/// ParseFailure for a word that is none of these, FROM or LIMIT that is not a
/// whole number (LIMIT at least 1), or UNDER without a path.
pub fn property_tree_reply(
    document: &Document,
    words: &[String],
    views: ScopeViewProvider<'_>,
) -> Result<String> {
    let mut at = 0;
    let scope = parse_scope_words(words, &mut at)?;
    let mut under = String::new();
    let mut from = 0_usize;
    let mut limit = PROPERTY_TREE_PAGE;
    while at < words.len() {
        let word = words[at].to_ascii_lowercase();
        let last = at + 1 >= words.len();
        if word == "under" || word == "from" || word == "limit" {
            if last {
                return Err(make_error(
                    ErrorCode::ParseFailure,
                    if word == "under" {
                        "UNDER needs a path after it"
                    } else {
                        "FROM and LIMIT need a number after them"
                    },
                    words[at].as_str(),
                ));
            }
            at += 1;
            let value = &words[at];
            if word == "under" {
                under.clone_from(value);
                // "vertex/3/" and "vertex/3" name one branch.
                while under.ends_with('/') {
                    under.pop();
                }
            } else if word == "from" {
                from = whole_number(value, "FROM", 0)?;
            } else {
                limit = whole_number(value, "LIMIT", 1)?;
            }
            at += 1;
            continue;
        }
        return Err(make_error(
            ErrorCode::ParseFailure,
            "not a PROP TREE word; PROP TREE [scope] [WHERE k=v ...] [UNDER path] [FROM n] \
             [LIMIT n]",
            words[at].as_str(),
        ));
    }

    let found = match_scope(document, &scope, views)?;
    let model = document.model();
    let maps: Vec<&PropertyMap> = found
        .matched
        .iter()
        .filter_map(|&id| model.entities.find(id))
        .map(|entity| &entity.properties)
        .collect();
    let entries = property_outline_of(&maps, &under);
    let first = from.min(entries.len());
    let shown = limit.min(entries.len() - first);

    let mut reply = format!(
        "{} under={} entries={} from={first} shown={shown}",
        scope_record(&found),
        record_value(&under),
        entries.len()
    );
    for entry in &entries[first..first + shown] {
        reply += &format!("\npath={}", record_value(&entry.path));
        if let Some(value) = &entry.value {
            reply += &format!(
                " value={} type={}",
                record_value(&property_to_string(value)),
                type_name(value)
            );
        } else if entry.varies {
            reply += &format!(" varies=yes holders={}", entry.holders);
        }
        if entry.children > 0 {
            reply += &format!(" children={} values={}", entry.children, entry.values);
        }
    }
    Ok(reply)
}
