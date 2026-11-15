//! The spellings of the linework control codes.
//!
//! A point's field code may carry control codes after its string name: "KB1
//! ST", "FL CL", "KB1 BC", "FN3 JPN 105". What each control DOES is the
//! linework processing in the cad layer (cad/linework.hpp); how it is SPELLED
//! is here, because a spelling is part of a customisation - a survey team
//! writes "ST" or "S" as its field book does - and a customisation is read and
//! written in this layer, below cad (customisation.rs); cad names it
//! `cad::LineworkCodes`.

use katana_core::error::{ErrorCode, Status, make_error};
use katana_core::text::{equals_ignoring_case, is_ascii_space};

use crate::entity_geometry::Validate;

/// The defaults are spellings common in field practice, chosen so that an
/// untouched table does something sensible; no product's file was copied for
/// them:
///
/// ```text
///   ST   start   - "start", the usual two-letter field abbreviation
///   END  end     - written out in most code sets
///   CL   close   - "close", as in "FL CL"
///   BC   arcStart, EC arcEnd - "begin curve" / "end curve", the road-design
///                  abbreviations surveyors already use for tangent points
///   JPN  join    - "join point number": also draw a line from this point to
///                  the point whose number follows ("JPN 105")
///   RECT rectangle - "rectangle": three points make a rectangle
/// ```
///
/// An empty spelling switches that control off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineworkCodes {
    pub start: String,
    pub end: String,
    pub close: String,
    pub arc_start: String,
    pub arc_end: String,
    pub join: String,
    pub rectangle: String,
}

impl Default for LineworkCodes {
    fn default() -> Self {
        Self {
            start: String::from("ST"),
            end: String::from("END"),
            close: String::from("CL"),
            arc_start: String::from("BC"),
            arc_end: String::from("EC"),
            join: String::from("JPN"),
            rectangle: String::from("RECT"),
        }
    }
}

/// Which spelling a member reaches: the C++ pointer to a member of
/// LineworkCodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Spelling {
    Start,
    End,
    Close,
    ArcStart,
    ArcEnd,
    Join,
    Rectangle,
}

/// One control: the word it is known by and where its spelling is kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineworkCodeMember {
    pub name: &'static str, // "arcStart"
    spelling: Spelling,
}

impl LineworkCodeMember {
    /// `codes.*member.spelling`.
    pub fn spelling(self, codes: &LineworkCodes) -> &str {
        match self.spelling {
            Spelling::Start => codes.start.as_str(),
            Spelling::End => codes.end.as_str(),
            Spelling::Close => codes.close.as_str(),
            Spelling::ArcStart => codes.arc_start.as_str(),
            Spelling::ArcEnd => codes.arc_end.as_str(),
            Spelling::Join => codes.join.as_str(),
            Spelling::Rectangle => codes.rectangle.as_str(),
        }
    }

    /// The same, to change it.
    pub fn spelling_mut(self, codes: &mut LineworkCodes) -> &mut String {
        match self.spelling {
            Spelling::Start => &mut codes.start,
            Spelling::End => &mut codes.end,
            Spelling::Close => &mut codes.close,
            Spelling::ArcStart => &mut codes.arc_start,
            Spelling::ArcEnd => &mut codes.arc_end,
            Spelling::Join => &mut codes.join,
            Spelling::Rectangle => &mut codes.rectangle,
        }
    }
}

static MEMBERS: [LineworkCodeMember; 7] = [
    LineworkCodeMember {
        name: "start",
        spelling: Spelling::Start,
    },
    LineworkCodeMember {
        name: "end",
        spelling: Spelling::End,
    },
    LineworkCodeMember {
        name: "close",
        spelling: Spelling::Close,
    },
    LineworkCodeMember {
        name: "arcStart",
        spelling: Spelling::ArcStart,
    },
    LineworkCodeMember {
        name: "arcEnd",
        spelling: Spelling::ArcEnd,
    },
    LineworkCodeMember {
        name: "join",
        spelling: Spelling::Join,
    },
    LineworkCodeMember {
        name: "rectangle",
        spelling: Spelling::Rectangle,
    },
];

/// The seven controls in the order the struct declares them, by the words a
/// refusal, the Katana customisation format and a verb name them with:
/// "start", "end", "close", "arcStart", "arcEnd", "join", "rectangle". One
/// list, so that the three cannot come to disagree about a name.
pub fn linework_code_members() -> &'static [LineworkCodeMember] {
    &MEMBERS
}

/// Refuses a spelling containing a blank (a code is split on blanks, so it
/// could never be matched) and two controls spelled alike (a token that means
/// two things means neither). Spellings are compared ignoring ASCII case,
/// because tokens are matched that way - see cad::parseFieldCode.
impl Validate for LineworkCodes {
    fn validate(&self) -> Status {
        let all = linework_code_members();
        for (i, member) in all.iter().enumerate() {
            let spelling: &str = member.spelling(self);
            if spelling.bytes().any(is_ascii_space) {
                return Err(make_error(
                    ErrorCode::InvalidArgument,
                    "a linework control code cannot contain a blank: codes are split on blanks, so it could never be matched",
                    format!("{}=\"{spelling}\"", member.name),
                ));
            }
            if spelling.is_empty() {
                continue;
            }
            for other in &all[i + 1..] {
                if equals_ignoring_case(spelling.as_bytes(), other.spelling(self).as_bytes()) {
                    return Err(make_error(
                        ErrorCode::InvalidArgument,
                        "two linework controls are spelled alike, so the token would mean both",
                        format!("{} and {} are \"{spelling}\"", member.name, other.name),
                    ));
                }
            }
        }
        Ok(())
    }
}
