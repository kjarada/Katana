//! A tree GDAL's XML parser made (cpl_minixml.h), as an owner and views:
//! moved here from formats.rs and web_access.rs so that their reading of a
//! driver's option list and of a capabilities document is safe code.
//!
//! `XmlDocument` owns the tree CPLParseXMLString made and destroys it on drop
//! (CPLDestroyXMLNode); an `XmlNode` is one node of it, borrowed for as long
//! as the document is, and never changed: nothing here writes to a tree.

#![allow(
    unsafe_code,
    reason = "Reads and frees a tree GDAL's XML parser made; every call states why it is sound."
)]

use std::ffi::CStr;
use std::ptr::{self, NonNull};

use super::{cpl, minixml};

/// A parsed tree, destroyed when it goes.
pub(crate) struct XmlDocument(NonNull<minixml::CplXmlNode>);

impl XmlDocument {
    /// `CPLParseXMLString(text)`: None when it is not XML (GDAL's reason left
    /// as this thread's last error).
    pub(crate) fn parse(text: &CStr) -> Option<Self> {
        // SAFETY: a NUL-terminated document; the tree GDAL returns (or NULL)
        // is owned by the value made from it.
        NonNull::new(unsafe { minixml::CPLParseXMLString(text.as_ptr()) }).map(Self)
    }

    /// The first node of the tree's top level (an `<?xml?>` declaration, a
    /// comment or the document element).
    pub(crate) fn first(&self) -> XmlNode<'_> {
        // SAFETY: the tree's first node, alive and unchanged until the
        // document is dropped, which the borrow of `self` outlasts.
        XmlNode(unsafe { self.0.as_ref() })
    }
}

impl Drop for XmlDocument {
    fn drop(&mut self) {
        // SAFETY: the tree GDAL made, destroyed once, after the last borrow
        // of a node of it.
        unsafe { minixml::CPLDestroyXMLNode(self.0.as_ptr()) };
    }
}

/// One node of a live tree.
#[derive(Clone, Copy)]
pub(crate) struct XmlNode<'a>(&'a minixml::CplXmlNode);

impl<'a> XmlNode<'a> {
    /// CXT_ELEMENT, CXT_TEXT, CXT_ATTRIBUTE ...
    pub(crate) fn node_type(self) -> minixml::CplXmlNodeType {
        self.0.e_type
    }

    /// The node's value (an element's or attribute's name, a text's text) as
    /// GDAL's bytes; None for NULL.
    pub(crate) fn value(self) -> Option<&'a CStr> {
        let value = self.0.psz_value;
        // SAFETY: NULL or a NUL-terminated string of the live tree.
        (!value.is_null()).then(|| unsafe { CStr::from_ptr(value) })
    }

    /// The value decoded lossily; empty for NULL.
    pub(crate) fn text(self) -> String {
        self.value()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// The next sibling.
    pub(crate) fn next(self) -> Option<XmlNode<'a>> {
        // SAFETY: a sibling link of a live node is NULL or a live node of the
        // same tree.
        unsafe { self.0.ps_next.cast_const().as_ref() }.map(XmlNode)
    }

    /// The first child.
    pub(crate) fn child(self) -> Option<XmlNode<'a>> {
        // SAFETY: a child link of a live node is NULL or a live node of the
        // same tree.
        unsafe { self.0.ps_child.cast_const().as_ref() }.map(XmlNode)
    }

    /// This node and the siblings after it, in order.
    pub(crate) fn and_siblings(self) -> impl Iterator<Item = XmlNode<'a>> {
        std::iter::successors(Some(self), |node| node.next())
    }

    /// The children, in order.
    pub(crate) fn children(self) -> impl Iterator<Item = XmlNode<'a>> {
        std::iter::successors(self.child(), |node| node.next())
    }

    /// `CPLGetXMLValue(node, path, "")`, copied.
    pub(crate) fn xml_value(self, path: Option<&CStr>) -> String {
        // SAFETY: a live node of a live tree, NULL or a NUL-terminated path
        // and an empty default; the value is the tree's own, copied at once.
        unsafe {
            cpl::decoded(minixml::CPLGetXMLValue(
                self.0,
                path.map_or(ptr::null(), CStr::as_ptr),
                c"".as_ptr(),
            ))
        }
    }
}
