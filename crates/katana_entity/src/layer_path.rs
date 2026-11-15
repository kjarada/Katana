//! Nested layer names.
//!
//! A layer name is a PATH: "design/surface/tin1", "asbuilt/road/kerb". The
//! separator is '/' and the segments form a tree, exactly as a survey or civil
//! job is actually organised.
//!
//! WHY THE TREE IS DERIVED, NOT STORED. Entities reference a layer by its full
//! name, and that string is what a project database, a DXF export and an undo
//! record all carry. If the tree were the primary structure, every node would
//! need an identity of its own, a rename of "design" would have to rewrite the
//! node, every descendant AND every entity, and any of those going wrong would
//! leave entities pointing at a layer that no longer exists. Keeping the full
//! path as the single identity means the tree is a VIEW of the names.
//!
//! An ordered map over full paths puts every parent before its children (a
//! proper prefix sorts first), and everything under "a" is one contiguous
//! range, the keys from "a/" to "a0" - so listing a subtree is a range scan.
//! It is NOT a strict pre-order walk: a sibling whose name continues with a
//! character below '/' ("a 2", "a-old", "a.bak") sorts between "a" and "a/b",
//! so "the keys straight after a layer" are not its children (see
//! LayerDatabase::has_children).

use katana_core::error::{Error, ErrorCode, Status, make_error};
use katana_core::text::is_ascii_space;

pub const LAYER_SEPARATOR: char = '/';

/// Bounded so that a tree widget, a recursive walk and a database column all
/// have something finite to work with, and so that a generated name cannot
/// grow without limit. Sixteen levels is far past any real drawing
/// organisation.
pub const MAXIMUM_LAYER_DEPTH: usize = 16;
pub const MAXIMUM_LAYER_NAME_LENGTH: usize = 512;

fn is_blank(segment: &str) -> bool {
    segment.bytes().all(is_ascii_space)
}

/// Checks `name` as a layer path. Fails with InvalidArgument and says which
/// rule was broken - an empty segment, a leading or trailing separator, a
/// segment that is only whitespace, "." or "..", a control character, or a
/// path past the depth or length limit.
///
/// "." and ".." are refused because layer names end up in file names (a plot
/// per layer, an export per layer) and a name that means "the parent
/// directory" there is a path traversal waiting to happen.
///
/// The C++ also refused a name that is not UTF-8 (audit MOD-12); a `&str`
/// cannot be one.
pub fn validate_layer_path(name: &str) -> Status {
    if name.is_empty() {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "layer name is empty",
        ));
    }
    if name.len() > MAXIMUM_LAYER_NAME_LENGTH {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "layer name is too long",
            format!("{} > {}", name.len(), MAXIMUM_LAYER_NAME_LENGTH),
        ));
    }
    if name.starts_with(LAYER_SEPARATOR) || name.ends_with(LAYER_SEPARATOR) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "layer name must not start or end with '/'",
            name,
        ));
    }

    let segments = layer_segments(name);
    if segments.len() > MAXIMUM_LAYER_DEPTH {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "layer name is nested too deeply",
            format!("{} > {}", segments.len(), MAXIMUM_LAYER_DEPTH),
        ));
    }
    for segment in segments {
        if segment.is_empty() {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "layer name has an empty level",
                name,
            ));
        }
        if is_blank(segment) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "layer level is only whitespace",
                name,
            ));
        }
        if segment.ends_with([' ', '\t']) || segment.starts_with([' ', '\t']) {
            // Surrounding spaces make two layers that look identical in a tree
            // and compare different everywhere else.
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "layer level has leading or trailing whitespace",
                name,
            ));
        }
        if segment == "." || segment == ".." {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "layer level must not be '.' or '..'",
                name,
            ));
        }
        if segment.bytes().any(|byte| byte < 0x20 || byte == 0x7F) {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "layer name contains a control character",
                name,
            ));
        }
    }
    Ok(())
}

/// Splits on the separator. No validation: use on a path that already passed
/// validate_layer_path, or to report what is wrong with one that did not.
pub fn layer_segments(name: &str) -> Vec<&str> {
    name.split(LAYER_SEPARATOR).collect()
}

/// "design/surface/tin1" -> "tin1". The whole name when it has no separator.
pub fn layer_leaf(name: &str) -> &str {
    match name.rfind(LAYER_SEPARATOR) {
        Some(separator) => &name[separator + 1..],
        None => name,
    }
}

/// "design/surface/tin1" -> "design/surface"; empty for a root layer.
pub fn layer_parent(name: &str) -> &str {
    match name.rfind(LAYER_SEPARATOR) {
        Some(separator) => &name[..separator],
        None => "",
    }
}

/// Every proper ancestor, outermost first: "design", "design/surface".
pub fn layer_ancestors(name: &str) -> Vec<String> {
    // The name itself is not one of its ancestors.
    name.match_indices(LAYER_SEPARATOR)
        .map(|(separator, _)| name[..separator].to_owned())
        .collect()
}

/// Number of separators plus one. "0" is depth 1.
pub fn layer_depth(name: &str) -> usize {
    if name.is_empty() {
        return 0;
    }
    name.matches(LAYER_SEPARATOR).count() + 1
}

/// True when `name` is `ancestor` itself or lies beneath it. Compares whole
/// segments, so "designs/x" is NOT under "design" - a plain prefix test would
/// say it was, and would hide or lock the wrong layers.
pub fn is_layer_under(name: &str, ancestor: &str) -> bool {
    if ancestor.is_empty() {
        return true; // the empty path is the root of everything
    }
    if !name.as_bytes().starts_with(ancestor.as_bytes()) {
        return false;
    }
    // Whole segments only: "designs/x" starts with "design" as TEXT but is a
    // different tree, and treating it as a descendant would hide or lock layers
    // the user never touched.
    name.len() == ancestor.len() || name.as_bytes()[ancestor.len()] == b'/'
}

/// Joins segments with the separator, skipping empty ones.
pub fn join_layer_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        return child.to_owned();
    }
    if child.is_empty() {
        return parent.to_owned();
    }
    let mut joined = String::with_capacity(parent.len() + 1 + child.len());
    joined.push_str(parent);
    joined.push(LAYER_SEPARATOR);
    joined.push_str(child);
    joined
}

/// `name` with the `from` prefix replaced by `to`, for renaming a subtree.
/// Returns `name` unchanged when it is not under `from`.
pub fn rewrite_layer_prefix(name: &str, from: &str, to: &str) -> String {
    if !is_layer_under(name, from) {
        return name.to_owned();
    }
    if name.len() == from.len() {
        return to.to_owned();
    }
    // Byte-wise, as the C++ substr is: with an empty `from` the C++ drops the
    // name's first byte (a defect kept as it is, docs/rust.md "Not done"), and
    // a byte that splits a character must not panic here.
    let tail = String::from_utf8_lossy(&name.as_bytes()[from.len() + 1..]);
    let mut rewritten = String::with_capacity(to.len() + 1 + tail.len());
    rewritten.push_str(to);
    rewritten.push(LAYER_SEPARATOR);
    rewritten.push_str(&tail);
    rewritten
}
