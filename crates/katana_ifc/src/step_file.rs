//! The DATA section of an ISO 10303-21 file, built one instance at a time.
//! Internal to katana_ifc.
//!
//! An instance is written the moment it is added, so everything it refers to
//! must already exist: a reference is always to a lower number, and nothing is
//! patched afterwards. The builders are written in that order (a point before
//! the curve through it), which is also the order a person reading the file
//! expects.

use crate::step::{step_real, step_string};

pub(crate) type Id = u32;

/// An attribute list, built left to right in the schema's order. Every
/// method appends one attribute.
#[derive(Clone, Debug, Default)]
pub(crate) struct Args {
    text: String,
    non_finite: bool,
}

impl Args {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn separate(&mut self) {
        if !self.text.is_empty() {
            self.text.push(',');
        }
    }

    pub(crate) fn reference(&mut self, id: Id) -> &mut Self {
        self.separate();
        self.text.push('#');
        self.text.push_str(&format!("{id}"));
        self
    }

    /// 0 is "no instance": $
    pub(crate) fn reference_or_null(&mut self, id: Id) -> &mut Self {
        if id == 0 {
            self.null()
        } else {
            self.reference(id)
        }
    }

    /// $: an optional attribute left out
    pub(crate) fn null(&mut self) -> &mut Self {
        self.separate();
        self.text.push('$');
        self
    }

    /// *: an attribute the schema derives
    pub(crate) fn derived(&mut self) -> &mut Self {
        self.separate();
        self.text.push('*');
        self
    }

    pub(crate) fn string(&mut self, utf8: &str) -> &mut Self {
        self.separate();
        self.text.push_str(&step_string(utf8));
        self
    }

    /// $ for empty
    pub(crate) fn string_or_null(&mut self, utf8: &str) -> &mut Self {
        if utf8.is_empty() {
            self.null()
        } else {
            self.string(utf8)
        }
    }

    pub(crate) fn real(&mut self, value: f64) -> &mut Self {
        self.non_finite = self.non_finite || !value.is_finite();
        self.separate();
        self.text.push_str(&step_real(value));
        self
    }

    pub(crate) fn integer(&mut self, value: i64) -> &mut Self {
        self.separate();
        self.text.push_str(&format!("{value}"));
        self
    }

    /// .VALUE.
    pub(crate) fn enumeration(&mut self, value: &str) -> &mut Self {
        self.separate();
        self.text.push('.');
        self.text.push_str(value);
        self.text.push('.');
        self
    }

    pub(crate) fn enumeration_or_null(&mut self, value: &str) -> &mut Self {
        if value.is_empty() {
            self.null()
        } else {
            self.enumeration(value)
        }
    }

    /// .T. / .F.
    pub(crate) fn boolean(&mut self, value: bool) -> &mut Self {
        self.separate();
        self.text.push_str(if value { ".T." } else { ".F." });
        self
    }

    /// (#1,#2)
    pub(crate) fn refs(&mut self, ids: &[Id]) -> &mut Self {
        self.separate();
        self.text.push('(');
        for (i, id) in ids.iter().enumerate() {
            if i > 0 {
                self.text.push(',');
            }
            self.text.push('#');
            self.text.push_str(&format!("{id}"));
        }
        self.text.push(')');
        self
    }

    pub(crate) fn reals(&mut self, values: &[f64]) -> &mut Self {
        self.separate();
        self.text.push('(');
        for (i, &value) in values.iter().enumerate() {
            self.non_finite = self.non_finite || !value.is_finite();
            if i > 0 {
                self.text.push(',');
            }
            self.text.push_str(&step_real(value));
        }
        self.text.push(')');
        self
    }

    /// A typed value of a SELECT: IFCLENGTHMEASURE(1.5)
    pub(crate) fn typed(&mut self, entity_type: &str, encoded: &str) -> &mut Self {
        self.separate();
        self.text.push_str(&typed_value(entity_type, encoded));
        self
    }

    /// already encoded
    pub(crate) fn raw(&mut self, encoded: &str) -> &mut Self {
        self.separate();
        self.text.push_str(encoded);
        self
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn non_finite(&self) -> bool {
        self.non_finite
    }
}

/// A list of already-encoded items: (a,b,c).
pub(crate) fn list_of(items: &[String]) -> String {
    let mut out = String::from("(");
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(item);
    }
    out.push(')');
    out
}

/// A value typed by its IFC defined type: IFCLABEL('x').
pub(crate) fn typed_value(entity_type: &str, encoded: &str) -> String {
    let mut out = String::with_capacity(entity_type.len() + encoded.len() + 2);
    out.push_str(&entity_type.to_ascii_uppercase());
    out.push('(');
    out.push_str(encoded);
    out.push(')');
    out
}

/// The C++ StepFile kept a count of the instances of each entity type that
/// nothing read; it is not carried over.
#[derive(Clone, Debug)]
pub(crate) struct StepFile {
    data: String,
    next: Id,
    non_finite: bool,
}

impl Default for StepFile {
    fn default() -> Self {
        Self {
            data: String::new(),
            next: 1,
            non_finite: false,
        }
    }
}

impl StepFile {
    /// `entity` is the schema's spelling ("IfcPipeSegment"); it is written in
    /// upper case, as ISO 10303-21 requires.
    pub(crate) fn add(&mut self, entity: &str, args: &Args) -> Id {
        let id = self.next;
        self.next += 1;
        self.non_finite = self.non_finite || args.non_finite();
        self.data.push('#');
        self.data.push_str(&format!("{id}"));
        self.data.push('=');
        self.data.push_str(&entity.to_ascii_uppercase());
        self.data.push('(');
        self.data.push_str(args.text());
        self.data.push_str(");\n");
        id
    }

    pub(crate) fn size(&self) -> usize {
        (self.next - 1) as usize
    }

    pub(crate) fn data(&self) -> &str {
        &self.data
    }

    pub(crate) fn non_finite(&self) -> bool {
        self.non_finite
    }
}
