//! GDAL's XML parser (cpl_minixml.h): the node and the three calls the
//! formats' option lists are read with, written by hand from GDAL 3.13.2's
//! header (ffi/cpl.rs says how the ffi folder is laid out and checked).

#![allow(
    unsafe_code,
    reason = "The declarations of GDAL's XML parser; every call states why it is sound."
)]
#![allow(
    dead_code,
    reason = "The header's node and values are declared whole; native/ffi_check.c holds each."
)]

use std::ffi::{c_char, c_int};

/// `CPLXMLNodeType`.
pub(crate) type CplXmlNodeType = c_int;
pub(crate) const CXT_ELEMENT: CplXmlNodeType = 0;
pub(crate) const CXT_TEXT: CplXmlNodeType = 1;
pub(crate) const CXT_ATTRIBUTE: CplXmlNodeType = 2;
pub(crate) const CXT_COMMENT: CplXmlNodeType = 3;
pub(crate) const CXT_LITERAL: CplXmlNodeType = 4;

/// `CPLXMLNode`, every member in the header's order. Made by
/// `CPLParseXMLString`, read by Rust, freed by `CPLDestroyXMLNode`.
#[repr(C)]
pub(crate) struct CplXmlNode {
    pub(crate) e_type: CplXmlNodeType,
    pub(crate) psz_value: *mut c_char,
    pub(crate) ps_next: *mut CplXmlNode,
    pub(crate) ps_child: *mut CplXmlNode,
}

// An int (padded to a pointer's alignment) and three pointers.
const POINTER: usize = std::mem::size_of::<*const u8>();
const _: () = assert!(std::mem::size_of::<CplXmlNode>() == 4 * POINTER);
const _: () = assert!(std::mem::offset_of!(CplXmlNode, psz_value) == POINTER);
const _: () = assert!(std::mem::offset_of!(CplXmlNode, ps_next) == 2 * POINTER);
const _: () = assert!(std::mem::offset_of!(CplXmlNode, ps_child) == 3 * POINTER);

#[link(name = "gdal")]
unsafe extern "C" {
    pub(crate) fn CPLParseXMLString(text: *const c_char) -> *mut CplXmlNode;
    pub(crate) fn CPLDestroyXMLNode(node: *mut CplXmlNode);
    pub(crate) fn CPLGetXMLValue(
        root: *const CplXmlNode,
        path: *const c_char,
        default: *const c_char,
    ) -> *const c_char;
}
