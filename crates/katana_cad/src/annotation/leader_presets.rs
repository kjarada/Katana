//! What a leader reads beyond the entity its tip is on, and the leaders made
//! for the questions a drawing is asked most
//! (include/katana/cad/annotation/leader_presets.hpp; docs/annotation.md,
//! "What a leader can say" and "Presets").
//!
//! leader_context is the drawing's surfaces and alignments as the entity
//! layer's LeaderContext (entity's leader_values: {surface.NAME},
//! {dsurface.NAME}, {cut.NAME}, {fill.NAME}, {station.NAME}, {offset.NAME},
//! {profile.NAME}, {dprofile.NAME}). Every caller that shows what a leader
//! says - the plan painter and so plots and sheets, LIST and LEADER, the
//! property panel, the Leaders manager, the tools, the DXF export and the IFC
//! export - asks it of the one Document, so they cannot say different things.
//!
//! A PRESET is a named template: the level of a point, its height above a
//! surface, the cut or the fill to one, its name, its code, its details, its
//! station and offset. LEADER preset=, LEADER FOR preset=, the Annotate
//! menu's preset leader tools and the Leaders manager all make one through
//! resolve_leader_preset, so a tool clicked and a line typed make the same
//! leader, and what the note says stays live like any smart leader's.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use katana_core::error::make_error;
use katana_core::json::Json;
use katana_core::text::lowered;
use katana_core::{Error, ErrorCode, Result};
use katana_entity::anchor::describe;
use katana_entity::entity::to_string;
use katana_entity::leader_values::{AlignmentPlaceFunction, SurfaceLevel, is_smart};
use katana_entity::tables::AlignmentDatabase;
use katana_entity::text_block::text_lines;
use katana_entity::{Alignment, AlignmentPlace, EntityId, Geometry, LeaderContext, Model};
use katana_geometry::alignment::solve_alignment;
use katana_geometry::profile::solve_profile;
use katana_geometry::{Point2, SolvedAlignment, SolvedProfile};
use katana_terrain::SurfaceStore;

use crate::annotation::leader_edit::leader_says;
use crate::document::Document;
use crate::scope_verbs::{ScopeMatch, ScopeViewProvider, ScopeWords, match_scope};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LeaderPreset {
    #[default]
    Level,
    Surface,
    CutFill,
    Name,
    Code,
    Details,
    Station,
}

/// What a preset reads besides its target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PresetReads {
    #[default]
    Target,
    Surface,
    Alignment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaderPresetInfo {
    pub preset: LeaderPreset,
    /// As preset= takes it: "level".
    pub word: &'static str,
    /// As the window shows it: "Level".
    pub title: &'static str,
    pub reads: PresetReads,
    /// The template, NAME standing for the surface or the alignment.
    pub pattern: &'static str,
    /// The catalogue tool that makes it (interactive_tool.rs), empty for none.
    pub tool: &'static str,
    /// One sentence: what the note says.
    pub says: &'static str,
}

// The presets. Each template is a choice, written once here: a level to the
// millimetre as a survey plan writes one ("RL 12.345"); a difference with its
// sign always written, so above and below read apart at a glance; cut and fill
// on two lines of which only the one that holds is said; a station in chainage
// form and its offset signed, left positive (entity's leader_values).
static PRESETS: [LeaderPresetInfo; 7] = [
    LeaderPresetInfo {
        preset: LeaderPreset::Level,
        word: "level",
        title: "Level",
        reads: PresetReads::Target,
        pattern: "RL {rl:.3f}",
        tool: "annotate.leader.level",
        says: "the level (z) of the point the tip is on",
    },
    LeaderPresetInfo {
        preset: LeaderPreset::Surface,
        word: "surface",
        title: "Surface Difference",
        reads: PresetReads::Surface,
        pattern: "{dsurface.NAME:+.3f}",
        tool: "annotate.leader.surface",
        says: "how far the point is above (+) or below (-) the surface",
    },
    LeaderPresetInfo {
        preset: LeaderPreset::CutFill,
        word: "cutfill",
        title: "Cut/Fill",
        reads: PresetReads::Surface,
        pattern: "CUT {cut.NAME:.3f}\nFILL {fill.NAME:.3f}",
        tool: "annotate.leader.cutfill",
        says: "the cut down to the point where it is below the surface, else the fill up to it",
    },
    LeaderPresetInfo {
        preset: LeaderPreset::Name,
        word: "name",
        title: "Point Name",
        reads: PresetReads::Target,
        pattern: "{point}",
        tool: "annotate.leader.name",
        says: "the point's name (its point number)",
    },
    LeaderPresetInfo {
        preset: LeaderPreset::Code,
        word: "code",
        title: "Code",
        reads: PresetReads::Target,
        pattern: "{code}",
        tool: "annotate.leader.code",
        says: "the survey code of what the tip is on",
    },
    LeaderPresetInfo {
        preset: LeaderPreset::Details,
        word: "details",
        title: "Point Details",
        reads: PresetReads::Target,
        pattern: "{point}\n{code}\nRL {rl:.3f}",
        tool: "",
        says: "the point's name, code and level, each line only where it has one",
    },
    LeaderPresetInfo {
        preset: LeaderPreset::Station,
        word: "station",
        title: "Station",
        reads: PresetReads::Alignment,
        pattern: "CH {station.NAME:ch}\nOS {offset.NAME:+.3f}",
        tool: "annotate.leader.station",
        says: "the station along the alignment and the offset from it, left positive",
    },
];

const NAME: &str = "NAME";

// `pattern` with each NAME replaced by `name`.
fn instantiate(pattern: &str, name: &str) -> String {
    pattern.replace(NAME, name)
}

// std::string_view::find(needle, from) on the bytes: None when `from` is past
// the end, `from` itself for an empty needle.
fn find_from(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    let bytes = haystack.as_bytes();
    if from > bytes.len() {
        return None;
    }
    let needle = needle.as_bytes();
    if needle.is_empty() {
        return Some(from);
    }
    bytes[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| at + from)
}

// One alignment of a model, solved.
struct Solved {
    horizontal: SolvedAlignment,
    profile: Option<SolvedProfile>,
}

// The alignments of a model, each solved once - when a leader first asks for
// it - and kept. Shared by every copy of the context the one function is
// copied into, and locked: a painter may read leaders on more than one thread
// (the C++'s std::mutex; the port's cad plan, section 7, risk 4).
struct AlignmentReader {
    definitions: BTreeMap<String, Alignment>,
    solved: Mutex<BTreeMap<String, Option<Arc<Solved>>>>,
}

impl AlignmentReader {
    fn new(table: &AlignmentDatabase) -> Self {
        let mut definitions = BTreeMap::new();
        table.for_each(|alignment| {
            definitions
                .entry(alignment.name.clone())
                .or_insert_with(|| alignment.clone());
        });
        Self {
            definitions,
            solved: Mutex::new(BTreeMap::new()),
        }
    }

    fn place(&self, name: &str, at: Point2) -> Option<AlignmentPlace> {
        let solved = self.solve(name)?;
        let where_ = solved.horizontal.station_offset(at)?;
        let mut place = AlignmentPlace {
            station: where_.station,
            offset: where_.offset,
            profile: None,
        };
        if let Some(profile) = &solved.profile {
            place.profile = profile.elevation_at(where_.station);
        }
        Some(place)
    }

    // None for no alignment of that name, or one that does not solve (a
    // stored one always does: the table validates it on the way in).
    fn solve(&self, name: &str) -> Option<Arc<Solved>> {
        let mut solved = self.solved.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = solved.get(name) {
            return found.clone();
        }
        let mut made = None;
        if let Some(definition) = self.definitions.get(name)
            && let Ok(horizontal) = solve_alignment(&definition.horizontal)
        {
            let profile = definition
                .vertical
                .as_ref()
                .and_then(|vertical| solve_profile(vertical).ok());
            made = Some(Arc::new(Solved {
                horizontal,
                profile,
            }));
        }
        solved.insert(String::from(name), made.clone());
        made
    }
}

/// The drawing's surfaces and alignments as a leader reads them, from the
/// model and a surface store (the C++ `leaderContext(model, surfaces)`
/// overload). The context keeps a copy of the surface store - a pointer a
/// surface, each shared and immutable - and of the alignments' definitions,
/// each solved once, when a leader first reads it, so it reads what the
/// drawing was when it was made: make it where it is used, not once for a
/// session.
pub fn leader_context_of(model: &Model, surfaces: &SurfaceStore) -> LeaderContext {
    // A copy: what the context reads cannot change or go under it.
    let store = surfaces.clone();
    let surface_level: SurfaceLevel = Arc::new(move |name: &str, at: Point2| {
        let found = store.find(name)?;
        found.surface.as_ref()?.elevation_at(at)
    });
    let reader = Arc::new(AlignmentReader::new(&model.alignments));
    let alignment_place: AlignmentPlaceFunction =
        Arc::new(move |name: &str, at: Point2| reader.place(name, at));
    LeaderContext {
        surfaces: surfaces
            .all()
            .iter()
            .map(|named| named.name.clone())
            .collect(),
        alignments: model.alignments.names(),
        surface_level: Some(surface_level),
        alignment_place: Some(alignment_place),
    }
}

/// The drawing's surfaces (Document::surfaces) and alignments (the model's
/// table) as a leader reads them.
pub fn leader_context(document: &Document) -> LeaderContext {
    leader_context_of(document.model(), document.surfaces())
}

/// Every preset, in the order HELP, LEADER PRESETS and the window list them.
pub fn leader_presets() -> &'static [LeaderPresetInfo] {
    &PRESETS
}

pub fn preset_info(preset: LeaderPreset) -> &'static LeaderPresetInfo {
    PRESETS
        .iter()
        .find(|info| info.preset == preset)
        .unwrap_or(&PRESETS[0])
}

/// By its word, ignoring case; InvalidArgument naming the words for another.
pub fn leader_preset_named(word: &str) -> Result<LeaderPreset> {
    let lower = lowered(word);
    let mut words = String::new();
    for info in &PRESETS {
        if info.word.as_bytes() == lower.as_slice() {
            return Ok(info.preset);
        }
        if !words.is_empty() {
            words.push('|');
        }
        words.push_str(info.word);
    }
    Err(make_error(
        ErrorCode::InvalidArgument,
        format!("no such leader preset (preset={words})"),
        word,
    ))
}

/// The template of `preset` reading `name` - the surface or the alignment;
/// ignored (the C++ default is empty) for a preset that reads only its
/// target. InvalidArgument for a name the template language cannot write:
/// empty, or holding ':', '{' or '}' (entity's leader_values).
pub fn preset_template(preset: LeaderPreset, name: &str) -> Result<String> {
    let info = preset_info(preset);
    if info.reads == PresetReads::Target {
        return Ok(String::from(info.pattern));
    }
    if name.is_empty() || name.contains([':', '{', '}']) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            format!(
                "a template cannot name a {} that is unnamed or whose name holds ':', '{{' or \
                 '}}': rename it",
                if info.reads == PresetReads::Surface {
                    "surface"
                } else {
                    "alignment"
                }
            ),
            name,
        ));
    }
    Ok(instantiate(info.pattern, name))
}

/// A preset made for a drawing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedPreset {
    pub preset: LeaderPreset,
    pub template_text: String,
    /// The surface or the alignment it reads, as the drawing names it; empty
    /// for a preset that reads only its target.
    pub reads: String,
    /// Set when none was named and the first visible surface, or the first
    /// alignment by name, was taken: "first visible surface". Said in the
    /// reply, so a leader is never quietly made against a surface nobody
    /// chose.
    pub chosen: String,
}

/// `preset` for `document`, reading `surface` (for a preset that reads a
/// surface) or `alignment` (one that reads an alignment). The name must be
/// the drawing's (a surface ignoring case, an alignment exactly, as each is
/// found); none given takes the first visible surface, or the first
/// alignment by name, and says so. Refused: a name the preset does not read
/// (surface= for preset=level), a surface or alignment the drawing has not,
/// and a drawing with none to take (or every surface hidden).
pub fn resolve_leader_preset(
    document: &Document,
    preset: LeaderPreset,
    surface: Option<&str>,
    alignment: Option<&str>,
) -> Result<ResolvedPreset> {
    let info = preset_info(preset);
    let word = info.word;
    if surface.is_some() && info.reads != PresetReads::Surface {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "surface= is for preset=surface and preset=cutfill",
            format!("preset={word}"),
        ));
    }
    if alignment.is_some() && info.reads != PresetReads::Alignment {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "alignment= is for preset=station",
            format!("preset={word}"),
        ));
    }
    let mut resolved = ResolvedPreset {
        preset,
        ..ResolvedPreset::default()
    };
    if info.reads == PresetReads::Surface {
        let store = document.surfaces();
        if let Some(surface) = surface {
            let Some(found) = store.find(surface) else {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "the drawing has no surface of that name (SURFACE LIST lists them)",
                    surface,
                ));
            };
            resolved.reads = found.name.clone();
        } else {
            let all = store.all();
            let Some(visible) = all.iter().find(|named| named.visible) else {
                return Err(Error::new(
                    ErrorCode::InvalidState,
                    if all.is_empty() {
                        format!(
                            "preset={word} reads a surface and the drawing has none (SURFACE \
                             FROM makes one)"
                        )
                    } else {
                        format!(
                            "preset={word} reads a surface and every surface is hidden: name \
                             one with surface="
                        )
                    },
                ));
            };
            resolved.reads = visible.name.clone();
            resolved.chosen = String::from("first visible surface");
        }
    } else if info.reads == PresetReads::Alignment {
        let table = &document.model().alignments;
        if let Some(alignment) = alignment {
            if !table.contains(alignment) {
                return Err(make_error(
                    ErrorCode::NotFound,
                    "the drawing has no alignment of that name (ALIGN LIST lists them)",
                    alignment,
                ));
            }
            resolved.reads = String::from(alignment);
        } else {
            let names = table.names();
            let Some(first) = names.into_iter().next() else {
                return Err(Error::new(
                    ErrorCode::InvalidState,
                    "preset=station reads an alignment and the drawing has none",
                ));
            };
            resolved.reads = first;
            resolved.chosen = String::from("first alignment");
        }
    }
    resolved.template_text = preset_template(preset, &resolved.reads)?;
    Ok(resolved)
}

/// The preset a template is, and the name it reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecognisedPreset {
    pub preset: LeaderPreset,
    pub reads: String,
}

/// The preset `template_text` is, and the name it reads; None for a template
/// no preset makes. What LEADER LIST and the Leaders manager say a leader is.
pub fn recognise_preset(template_text: &str) -> Option<RecognisedPreset> {
    for info in &PRESETS {
        if info.reads == PresetReads::Target {
            if template_text == info.pattern {
                return Some(RecognisedPreset {
                    preset: info.preset,
                    reads: String::new(),
                });
            }
            continue;
        }
        // The name between what comes before the first NAME and what comes
        // after it, checked by making the template again from it: a pattern
        // naming it twice (cut and fill) must name the same one twice.
        let Some(at) = info.pattern.find(NAME) else {
            continue;
        };
        let before = &info.pattern[..at];
        let after = &info.pattern[at + NAME.len()..];
        let first_line = after.split('\n').next().unwrap_or(after);
        let end = find_from(template_text, first_line, before.len());
        let Some(end) = end.filter(|&end| template_text.starts_with(before) && end > before.len())
        else {
            continue;
        };
        let Some(name) = template_text.get(before.len()..end) else {
            continue;
        };
        if instantiate(info.pattern, name) == template_text {
            return Some(RecognisedPreset {
                preset: info.preset,
                reads: String::from(name),
            });
        }
    }
    None
}

/// The leaders a scope and filter take (LEADER LIST, VALUES, FREEZE ... and
/// MCP's katana_leaders over `scope`): scope_verbs' match_scope - the one
/// scope grammar and matcher, so a view, layers, an area, the selection or
/// the drawing, and WHERE, mean here what they mean in Global Modify - with
/// what it matched narrowed to the leaders among it, so the `matched=` of the
/// scope's record counts leaders. A scope that takes none is no failure.
pub fn match_leaders(
    document: &Document,
    words: &ScopeWords,
    views: ScopeViewProvider<'_>,
) -> Result<ScopeMatch> {
    let mut found = match_scope(document, words, views)?;
    let entities = &document.model().entities;
    found.matched.retain(|&id| {
        entities
            .find(id)
            .is_some_and(|entity| matches!(entity.geometry, Geometry::Leader(_)))
    });
    Ok(found)
}

// ---- the JSON ------------------------------------------------------------------------------
//
// The report is plain data and the writer a function taking it. In the C++
// that let a test hand the writer a byte that is not UTF-8, which it must
// write as U+FFFD instead of throwing; a Rust String holds no such byte, so
// here it is the shape alone.

/// One leader as the JSON says it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LeaderReport {
    pub id: EntityId,
    pub layer: String,
    /// The first vertex; absent for a leader with none.
    pub tip: Option<Point2>,
    pub vertices: usize,
    /// The entity the tip is on and where on it, absent for a plain leader.
    pub target: Option<EntityId>,
    pub anchor: Option<String>,
    pub smart: bool,
    /// Absent for a leader whose note is plain text.
    pub template_text: Option<String>,
    pub label_style: Option<String>,
    /// The preset the template is, and the surface or alignment it reads.
    pub preset: Option<String>,
    pub reads: Option<String>,
    pub arrow: String,
    pub callout: String,
    /// What the note says now, whole and line by line.
    pub text: String,
    pub lines: Vec<String>,
    pub landing_angle: Option<f64>,
    pub points: Vec<Point2>,
    pub text_style: String,
    pub paper_height: f64,
    pub arrow_size: f64,
    pub landing: f64,
}

/// The scope the leaders were chosen by: its record (scope_verbs'
/// scope_record, as the verbs reply it) and how many leaders it took.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LeadersScopeReport {
    pub record: String,
    pub matched: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LeadersReport {
    pub leaders: Vec<LeaderReport>,
    pub surfaces: Vec<String>,
    pub alignments: Vec<String>,
    pub scope: Option<LeadersScopeReport>,
}

/// The drawing's leaders as a report: `only` the leaders of those ids when
/// set - and then NONE when it is empty, which is what a scope that took
/// nothing must give - and, when `target` is given, only those on it.
pub fn leaders_report(
    document: &Document,
    only: Option<&[EntityId]>,
    target: Option<EntityId>,
) -> LeadersReport {
    let model = document.model();
    let context = leader_context(document);
    let mut report = LeadersReport::default();
    model.entities.for_each(|entity| {
        let Geometry::Leader(leader) = &entity.geometry else {
            return;
        };
        if only.is_some_and(|only| !only.contains(&entity.id))
            || target.is_some_and(|target| leader.tip_ref.entity != target)
        {
            return;
        }
        let mut record = LeaderReport {
            id: entity.id,
            layer: entity.layer.clone(),
            tip: leader.vertices.first().copied(),
            vertices: leader.vertices.len(),
            smart: is_smart(leader),
            ..LeaderReport::default()
        };
        if leader.tip_ref.associated() {
            record.target = Some(leader.tip_ref.entity);
            record.anchor = Some(describe(&leader.tip_ref));
        }
        if leader.fields {
            record.template_text = Some(leader.text.clone());
        }
        if !leader.label_style.is_empty() {
            record.label_style = Some(leader.label_style.clone());
        }
        let recognised = if leader.fields {
            recognise_preset(&leader.text)
        } else {
            None
        };
        if let Some(recognised) = recognised {
            record.preset = Some(String::from(preset_info(recognised.preset).word));
            if !recognised.reads.is_empty() {
                record.reads = Some(recognised.reads);
            }
        }
        record.arrow = String::from(to_string(leader.arrow));
        record.callout = String::from(to_string(leader.callout));
        record.landing_angle = leader.landing_angle;
        record.points = leader.vertices.clone();
        record.text_style = leader.style.clone();
        record.paper_height = leader.paper_height;
        record.arrow_size = leader.arrow_size;
        record.landing = leader.landing;
        record.text = leader_says(model, leader, &context);
        if !record.text.is_empty() {
            record.lines = text_lines(&record.text)
                .into_iter()
                .map(String::from)
                .collect();
        }
        report.leaders.push(record);
    });
    report.surfaces = context.surfaces.clone();
    report.alignments = context.alignments.clone();
    report
}

/// leaders_report then leaders_json_of, for the drawing's leaders `only`
/// those of the ids when not empty (an empty list is all of them, as it
/// always was) and only those on `target` when given.
pub fn leaders_json(document: &Document, only: &[EntityId], target: Option<EntityId>) -> String {
    leaders_json_of(&leaders_report(
        document,
        if only.is_empty() { None } else { Some(only) },
        target,
    ))
}

fn optional_text(text: &Option<String>) -> Json {
    match text {
        Some(text) => Json::from(text),
        None => Json::Null,
    }
}

fn point_json(point: Point2) -> Json {
    Json::array_of([Json::from(point.x), Json::from(point.y)])
}

fn strings_json(items: &[String]) -> Json {
    Json::array_of(items.iter().map(Json::from))
}

/// The report as JSON (LEADER LIST JSON, MCP's katana_leaders):
/// {"leaders": [{id, layer, tip, vertices, target, anchor, smart, template,
/// labelstyle, preset, reads, arrow, callout, text, lines}], "presets":
/// [{name, title, reads, template, tool, says}], "surfaces": [...],
/// "alignments": [...]} and, when the leaders were chosen by a scope,
/// "scope": {"record": "scope=drawing matched=3", "matched": 3}. Absent
/// members are null. Indented by two, keys sorted, as nlohmann's `json`
/// writes them.
pub fn leaders_json_of(report: &LeadersReport) -> String {
    let mut leaders = Vec::new();
    for leader in &report.leaders {
        let mut record = Json::object();
        record["id"] = Json::from(leader.id);
        record["layer"] = Json::from(&leader.layer);
        record["tip"] = leader.tip.map_or(Json::Null, point_json);
        record["vertices"] = Json::from(leader.vertices);
        record["target"] = leader.target.map_or(Json::Null, Json::from);
        record["anchor"] = optional_text(&leader.anchor);
        record["smart"] = Json::from(leader.smart);
        record["template"] = optional_text(&leader.template_text);
        record["labelstyle"] = optional_text(&leader.label_style);
        record["preset"] = optional_text(&leader.preset);
        record["reads"] = optional_text(&leader.reads);
        record["arrow"] = Json::from(&leader.arrow);
        record["callout"] = Json::from(&leader.callout);
        record["text"] = Json::from(&leader.text);
        record["lines"] = strings_json(&leader.lines);
        record["points"] = Json::array_of(leader.points.iter().copied().map(point_json));
        record["text_style"] = Json::from(&leader.text_style);
        record["paper_height"] = Json::from(leader.paper_height);
        record["arrow_size"] = Json::from(leader.arrow_size);
        record["landing"] = Json::from(leader.landing);
        if let Some(angle) = leader.landing_angle {
            record["landing_angle"] = Json::from(angle);
        }
        leaders.push(record);
    }
    let presets = Json::array_of(PRESETS.iter().map(|info| {
        Json::object_of([
            ("name", Json::from(info.word)),
            ("title", Json::from(info.title)),
            (
                "reads",
                Json::from(match info.reads {
                    PresetReads::Target => "target",
                    PresetReads::Surface => "surface",
                    PresetReads::Alignment => "alignment",
                }),
            ),
            ("template", Json::from(info.pattern)),
            (
                "tool",
                if info.tool.is_empty() {
                    Json::Null
                } else {
                    Json::from(info.tool)
                },
            ),
            ("says", Json::from(info.says)),
        ])
    }));
    let mut result = Json::object();
    result["leaders"] = Json::from(leaders);
    result["presets"] = presets;
    result["surfaces"] = strings_json(&report.surfaces);
    result["alignments"] = strings_json(&report.alignments);
    if let Some(scope) = &report.scope {
        result["scope"] = Json::object_of([
            ("record", Json::from(&scope.record)),
            ("matched", Json::from(scope.matched)),
        ]);
    }
    // A reply, not a stored value: nlohmann's replace handler, which a
    // String's text never needs (core's json, dump_with).
    result.dump_with(2, ' ', false)
}
