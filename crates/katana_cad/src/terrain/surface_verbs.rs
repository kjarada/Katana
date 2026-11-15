//! The drawing's surfaces and their display styles on the command line
//! (include/katana/cad/terrain/surface_verbs.hpp; docs/terrain.md, "Surfaces
//! in the drawing"): the verbs a person, a script, katana_cli and an agent
//! type to rename, copy, show, hide and restyle a surface, and to make and
//! edit the styles it is drawn in.
//!
//! ```text
//!   SURFACE RENAME <name> <new>
//!   SURFACE STYLE <name> <style>          ("" for the drawing's default style)
//!   SURFACE SHOW <name>... | ALL
//!   SURFACE HIDE <name>... | ALL
//!   SURFACE COPY <name> <new>
//!   SURFACESTYLE LIST [JSON] | INFO <style> | NEW <style> [FROM <style>] [key=value ...]
//!              | SET <style> key=value ... | DELETE <style> | RENAME <style> <new>
//!              | DEFAULT <style>
//! ```
//!
//! Words are case-insensitive; a name with blanks is quoted. Every change is
//! ONE undo step through Document::set_surfaces / set_surface_styles /
//! set_surfaces_and_styles, a refused one changes nothing, and a reply is
//! key=value records, one a line. The geoprocessing executor keeps the other
//! SURFACE words that are its (LIST, INFO, REMOVE, FROM, EXPORT; docs/
//! geoprocessing.md), and the surface edits and volumes theirs; this module
//! claims only the words above (is_surface_verb).

use katana_commands::{CommandPtr, Transaction};
use katana_core::error::{make_error, to_string as code_name};
use katana_core::json::OrderedJson;
use katana_core::text::{
    equals_ignoring_case, format_exact_real, format_fixed, lowered, parse_finite_double,
    parse_integer, unescape_typed,
};
use katana_core::{Error, ErrorCode, Result, Status};
use katana_entity::colour_names::resolve_colour;
use katana_entity::{Color, ColourTable};
use katana_terrain::{NamedSurface, SurfaceStore};

use crate::annotation::leader_edit::{LeaderReadsRenamed, ReadKind, rename_leader_reads};
use crate::document::Document;
use crate::road::corridor_model::CorridorSet;
use crate::road::road_objects::RoadObjectSet;
use crate::scope_verbs::record_value;
use crate::terrain::surface_style::{
    ColourStop, SurfaceDisplayStyle, SurfaceFaces, SurfaceStyleSet, parse_surface_faces,
    parse_surface_shading, style_of, surface_faces_word, surface_ramp_names, surface_shading_word,
    surface_styles_to_json,
};

const SURFACE_USAGE: &str = "SURFACE RENAME <name> <new> | STYLE <name> <style> | SHOW <name>...|ALL | \
     HIDE <name>...|ALL | COPY <name> <new>";
const STYLE_USAGE: &str = "SURFACESTYLE LIST [JSON] | INFO <style> | NEW <style> [FROM <style>] [key=value ...] | \
     SET <style> key=value ... | DELETE <style> | RENAME <style> <new> | DEFAULT <style>";

fn is(tokens: &[String], at: usize, word: &str) -> bool {
    tokens
        .get(at)
        .is_some_and(|token| equals_ignoring_case(token.as_bytes(), word.as_bytes()))
}

// core's trimmed over text: it drops ASCII bytes only, so the rest is UTF-8.
fn trimmed(text: &str) -> &str {
    std::str::from_utf8(katana_core::text::trimmed(text.as_bytes())).unwrap_or_default()
}

// A length or a level to the millimetre in a record: the one formatter
// (core's format_fixed), which holds any finite double and signs no zero
// (zmin=-0.000 reads as meant).
fn fixed3(number: f64) -> String {
    format_fixed(number, 3)
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn usage(why: &str, grammar: &str, word: &str) -> Error {
    make_error(
        ErrorCode::InvalidArgument,
        format!("{why}: {grammar}"),
        word,
    )
}

fn no_surface(name: &str) -> Error {
    make_error(
        ErrorCode::NotFound,
        "no surface has that name; SURFACE LIST names them",
        name,
    )
}

fn no_style(name: &str) -> Error {
    make_error(
        ErrorCode::NotFound,
        "no surface style has that name; SURFACESTYLE LIST names them",
        name,
    )
}

fn joined(records: &[String]) -> String {
    records.join("\n")
}

// The record of the surface `name` of the drawing, which the step that made
// or changed it has just put there.
fn record_of(document: &Document, name: &str) -> String {
    document
        .surfaces()
        .find(name)
        .map(|named| surface_record(named, document.surface_styles()))
        .unwrap_or_default()
}

// ---- SURFACE RENAME, STYLE, SHOW, HIDE, COPY ------------------------------------------------

fn rename_surface(document: &mut Document, tokens: &[String]) -> Result<String> {
    if tokens.len() != 4 {
        return Err(usage(
            "SURFACE RENAME names a surface and its new name",
            "SURFACE RENAME <name> <new>",
            "",
        ));
    }
    let Some(named) = document.surfaces().find(&tokens[2]) else {
        return Err(no_surface(&tokens[2]));
    };
    let old = named.name.clone();
    let mut next = document.surfaces().clone();
    next.rename(&old, &tokens[3])?;
    // A corridor names the surface its batters daylight on and a road object
    // the surface it stands on: left behind by a rename they would find no
    // surface, and lose their earthworks and their level, quietly (the next
    // build lists the problem). They follow it in the same step, as the
    // corridors of a renamed road template do (ROADTEMPLATE RENAME).
    if let Err(error) = document.corridors_status() {
        return Err(make_error(
            ErrorCode::InvalidState,
            "the drawing's corridors cannot be read: nothing is changed",
            error.describe(),
        ));
    }
    if let Err(error) = document.road_objects_status() {
        return Err(make_error(
            ErrorCode::InvalidState,
            "the drawing's road objects cannot be read: nothing is changed",
            error.describe(),
        ));
    }
    let mut corridors = CorridorSet::clone(&document.corridors());
    let mut following_corridors = 0;
    for corridor in &mut corridors.corridors {
        if equals_ignoring_case(corridor.surface.as_bytes(), old.as_bytes()) {
            corridor.surface = tokens[3].clone();
            following_corridors += 1;
        }
    }
    let mut objects = RoadObjectSet::clone(&document.road_objects());
    let mut following_objects = 0;
    for object in &mut objects.objects {
        if equals_ignoring_case(object.placement.surface.as_bytes(), old.as_bytes()) {
            object.placement.surface = tokens[3].clone();
            following_objects += 1;
        }
    }
    // A leader's note names the surface it reads the same way
    // ({dsurface.NAME}), and a note left reading the old name says nothing.
    let LeaderReadsRenamed {
        command: leaders_step,
        followed,
        unfollowed,
    } = rename_leader_reads(document.model(), ReadKind::Surface, &old, &tokens[3]);
    let surfaces_step = document.surfaces_command(next, "SURFACE RENAME")?;
    let corridors_step = document.corridors_command(&corridors, "SURFACE RENAME")?;
    let objects_step = document.road_objects_command(&objects, "SURFACE RENAME")?;
    let mut transaction = Transaction::new("SURFACE RENAME");
    for step in [surfaces_step, corridors_step, objects_step, leaders_step]
        .into_iter()
        .flatten()
    {
        transaction.add(step);
    }
    if transaction.size() > 0 {
        document.execute(Box::new(transaction))?;
    }
    let mut reply = format!(
        "renamed surface={} name={} corridors={following_corridors} \
         roadobjects={following_objects} leaders={followed}\n",
        record_value(&old),
        record_value(&tokens[3])
    );
    if unfollowed > 0 {
        // Said, not refused: the rename is what was asked for, and these
        // notes now read nothing, as they would had the surface been removed.
        reply.push_str(&format!(
            "warning leaders={unfollowed} still read surface {}: the new name cannot be written \
             in a note (empty, or holding ':', '{{' or '}}') or the leader is on a locked layer\n",
            record_value(&old)
        ));
    }
    Ok(reply + &record_of(document, &tokens[3]))
}

fn style_surface(document: &mut Document, tokens: &[String]) -> Result<String> {
    if tokens.len() != 4 {
        return Err(usage(
            "SURFACE STYLE names a surface and a style (\"\" for the drawing's default)",
            "SURFACE STYLE <name> <style>",
            "",
        ));
    }
    let Some(named) = document.surfaces().find(&tokens[2]) else {
        return Err(no_surface(&tokens[2]));
    };
    let name = named.name.clone();
    // Kept as the style spells itself, so a surface names its style one way.
    let mut style = String::new();
    if !tokens[3].is_empty() {
        let Some(found) = document.surface_styles().find(&tokens[3]) else {
            return Err(no_style(&tokens[3]));
        };
        style = found.name.clone();
    }
    let mut next = document.surfaces().clone();
    next.set_style(&name, &style)?;
    document.set_surfaces(next, "SURFACE STYLE")?;
    Ok(record_of(document, &name))
}

fn show_surfaces(document: &mut Document, tokens: &[String], visible: bool) -> Result<String> {
    let verb = if visible { "SHOW" } else { "HIDE" };
    if tokens.len() < 3 {
        return Err(usage(
            &format!("SURFACE {verb} names the surfaces, or ALL"),
            &format!("SURFACE {verb} <name>... | ALL"),
            "",
        ));
    }
    let mut names = Vec::new();
    // ALL is the keyword unless a surface has that name: the tokenizer drops
    // the quotes, so SURFACE HIDE "ALL" - the line the Surfaces panel builds
    // for the check box of a surface called ALL - reaches here as the bare
    // word, and every surface would go with it. A surface's own name wins; to
    // reach all of them in such a drawing, list their names.
    if tokens.len() == 3 && is(tokens, 2, "ALL") && document.surfaces().find(&tokens[2]).is_none() {
        for named in document.surfaces().all() {
            names.push(named.name.clone());
        }
    } else {
        // Every name found before anything is changed: one unknown refuses
        // the whole line, as a half-done SHOW would surprise.
        for token in &tokens[2..] {
            let Some(named) = document.surfaces().find(token) else {
                return Err(no_surface(token));
            };
            names.push(named.name.clone());
        }
    }
    let mut next = document.surfaces().clone();
    let mut changed = 0;
    for name in &names {
        if next
            .find(name)
            .is_some_and(|named| named.visible != visible)
        {
            let _ = next.set_visible(name, visible);
            changed += 1;
        }
    }
    if changed != 0 {
        document.set_surfaces(next, format!("SURFACE {verb}"))?;
    }
    let mut records: Vec<String> = names.iter().map(|name| record_of(document, name)).collect();
    records.push(format!(
        "{} surfaces={} changed={changed}",
        if visible { "shown" } else { "hidden" },
        names.len()
    ));
    Ok(joined(&records))
}

fn copy_surface(document: &mut Document, tokens: &[String]) -> Result<String> {
    if tokens.len() != 4 {
        return Err(usage(
            "SURFACE COPY names a surface and the copy's name",
            "SURFACE COPY <name> <new>",
            "",
        ));
    }
    let Some(named) = document.surfaces().find(&tokens[2]) else {
        return Err(no_surface(&tokens[2]));
    };
    // The copy SHARES the triangulation: a surface is immutable, so a copy
    // costs a pointer, and an edit of either makes a new one of its own.
    let mut copy = named.clone();
    copy.name = tokens[3].clone();
    copy.source = format!("copy of {}", named.name);
    let from = named.name.clone();
    let mut next = document.surfaces().clone();
    next.add(copy.clone())?;
    document.set_surfaces(next, "SURFACE COPY")?;
    Ok(format!(
        "copied surface={} name={}\n{}",
        record_value(&from),
        record_value(&copy.name),
        record_of(document, &copy.name)
    ))
}

// ---- SURFACESTYLE ---------------------------------------------------------------------------

fn surfaces_in(document: &Document, style: &SurfaceDisplayStyle) -> usize {
    let styles = document.surface_styles();
    // By identity, as the C++ compares the addresses: the style the surface
    // is drawn in IS this one of the set.
    document
        .surfaces()
        .all()
        .iter()
        .filter(|named| std::ptr::eq(style_of(styles, named), style))
        .count()
}

fn style_record(document: &Document, style: &SurfaceDisplayStyle) -> String {
    let styles = document.surface_styles();
    format!(
        "style name={} default={} surfaces={} faces={} triangles={} boundary={} contours={} \
         shading={} faces3d={} description={}",
        record_value(&style.name),
        yes_no(equals_ignoring_case(
            styles.default_style.as_bytes(),
            style.name.as_bytes()
        )),
        surfaces_in(document, style),
        surface_faces_word(style.plan_faces),
        if style.show_triangles { "on" } else { "off" },
        if style.show_boundary { "on" } else { "off" },
        if style.contours.show { "on" } else { "off" },
        surface_shading_word(style.shading),
        surface_faces_word(style.faces3d),
        record_value(&style.description)
    )
}

fn options_record(style: &SurfaceDisplayStyle) -> String {
    let mut record = String::from("options");
    for (key, value) in surface_style_options(style) {
        record.push_str(&format!(" {key}={}", record_value(&value)));
    }
    record
}

// What a style's change refuses: styles a newer Katana wrote, which are kept.
fn styles_editable(document: &Document) -> Status {
    if let Err(error) = document.surface_styles_status()
        && error.code == ErrorCode::Unsupported
    {
        return Err(make_error(
            ErrorCode::CommandRejected,
            "the surface styles were written by a newer version of Katana; they are kept as they \
             are",
            error.describe(),
        ));
    }
    Ok(())
}

// The key=value words from `at` on into `style`.
fn apply_options(
    document: &Document,
    style: &mut SurfaceDisplayStyle,
    tokens: &[String],
    at: usize,
) -> Status {
    for word in tokens.iter().skip(at) {
        let Some(equals) = word.find('=').filter(|&equals| equals != 0) else {
            return Err(usage("a style's fields are key=value", STYLE_USAGE, word));
        };
        let key = String::from_utf8_lossy(&lowered(&word[..equals])).into_owned();
        set_surface_style_option(
            style,
            &key,
            &word[equals + 1..],
            &document.customisation_state().colours,
        )?;
    }
    Ok(())
}

fn list_styles(document: &Document, tokens: &[String]) -> Result<String> {
    let json = tokens.len() == 3 && is(tokens, 2, "JSON");
    if tokens.len() > 3 || (tokens.len() == 3 && !json) {
        return Err(usage(
            "SURFACESTYLE LIST takes JSON or nothing",
            "SURFACESTYLE LIST [JSON]",
            "",
        ));
    }
    if json {
        return Ok(surface_styles_report(document));
    }
    let styles = document.surface_styles();
    let mut records: Vec<String> = styles
        .styles
        .iter()
        .map(|style| style_record(document, style))
        .collect();
    records.push(format!(
        "listed styles={} default={}",
        styles.styles.len(),
        record_value(&styles.default_style)
    ));
    if let Err(error) = document.surface_styles_status() {
        records.push(format!(
            "warning text={}",
            record_value(&format!(
                "the project's own styles cannot be read, so the built-in ones stand in: {}",
                error.describe()
            ))
        ));
    }
    Ok(joined(&records))
}

fn style_info(document: &Document, tokens: &[String]) -> Result<String> {
    if tokens.len() != 3 {
        return Err(usage(
            "SURFACESTYLE INFO names one style",
            "SURFACESTYLE INFO <style>",
            "",
        ));
    }
    let Some(style) = document.surface_styles().find(&tokens[2]) else {
        return Err(no_style(&tokens[2]));
    };
    Ok(format!(
        "{}\n{}",
        style_record(document, style),
        options_record(style)
    ))
}

fn new_style(document: &mut Document, tokens: &[String]) -> Result<String> {
    if tokens.len() < 3 || tokens[2].is_empty() {
        return Err(usage("SURFACESTYLE NEW names the style", STYLE_USAGE, ""));
    }
    styles_editable(document)?;
    if document.surface_styles().find(&tokens[2]).is_some() {
        return Err(make_error(
            ErrorCode::AlreadyExists,
            "a surface style already has that name",
            tokens[2].as_str(),
        ));
    }
    let mut style = SurfaceDisplayStyle::default();
    let mut at = 3;
    let mut from = String::new();
    if is(tokens, at, "FROM") {
        if at + 1 >= tokens.len() {
            return Err(usage("FROM names the style to start from", STYLE_USAGE, ""));
        }
        let Some(base) = document.surface_styles().find(&tokens[at + 1]) else {
            return Err(no_style(&tokens[at + 1]));
        };
        style = base.clone();
        from = base.name.clone();
        at += 2;
    }
    style.name = tokens[2].clone();
    if from.is_empty() {
        style.description.clear();
    }
    apply_options(document, &mut style, tokens, at)?;
    let mut styles = document.surface_styles().clone();
    styles.styles.push(style.clone());
    document.set_surface_styles(styles, "SURFACESTYLE NEW")?;
    let Some(made) = document.surface_styles().find(&style.name) else {
        return Err(no_style(&style.name));
    };
    Ok(format!(
        "created style={} from={}\n{}\n{}",
        record_value(&made.name),
        record_value(&from),
        style_record(document, made),
        options_record(made)
    ))
}

fn set_style(document: &mut Document, tokens: &[String]) -> Result<String> {
    if tokens.len() < 4 {
        return Err(usage(
            "SURFACESTYLE SET names the style and at least one key=value",
            STYLE_USAGE,
            "",
        ));
    }
    styles_editable(document)?;
    let Some(found) = document.surface_styles().find(&tokens[2]) else {
        return Err(no_style(&tokens[2]));
    };
    let name = found.name.clone();
    let mut styles = document.surface_styles().clone();
    let Some(index) = styles.styles.iter().position(|each| each.name == name) else {
        return Err(no_style(&tokens[2]));
    };
    let before = styles.styles[index].clone();
    apply_options(document, &mut styles.styles[index], tokens, 3)?;
    // The built-ins standing in for a set that could not be read are replaced
    // by the first set given (Document::set_surfaces_and_styles), whatever it
    // holds: that is a change, with an undo step, even when no field of this
    // style moved - so the reply says so rather than "changed=no" over a step
    // that was recorded and a kept text that is gone.
    let replacing = document.surface_styles_status().is_err();
    let changed = styles.styles[index] != before || replacing;
    document.set_surface_styles(styles, "SURFACESTYLE SET")?;
    let Some(now) = document.surface_styles().find(&name) else {
        return Err(no_style(&name));
    };
    Ok(format!(
        "set style={} changed={}{}\n{}\n{}",
        record_value(&name),
        yes_no(changed),
        if replacing {
            " replaced=unreadable-styles"
        } else {
            ""
        },
        style_record(document, now),
        options_record(now)
    ))
}

fn delete_style(document: &mut Document, tokens: &[String]) -> Result<String> {
    if tokens.len() != 3 {
        return Err(usage(
            "SURFACESTYLE DELETE names one style",
            "SURFACESTYLE DELETE <style>",
            "",
        ));
    }
    styles_editable(document)?;
    let Some(found) = document.surface_styles().find(&tokens[2]) else {
        return Err(no_style(&tokens[2]));
    };
    let name = found.name.clone();
    if equals_ignoring_case(
        document.surface_styles().default_style.as_bytes(),
        name.as_bytes(),
    ) {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "it is the default style; make another the default first (SURFACESTYLE DEFAULT \
             <style>)",
            name,
        ));
    }
    let mut styles = document.surface_styles().clone();
    styles.styles.retain(|each| each.name != name);
    // A surface drawn in it is the Document's refusal (set_surface_styles),
    // with the surface named.
    document.set_surface_styles(styles, "SURFACESTYLE DELETE")?;
    Ok(format!(
        "deleted style={} styles={}",
        record_value(&name),
        document.surface_styles().styles.len()
    ))
}

fn rename_style(document: &mut Document, tokens: &[String]) -> Result<String> {
    if tokens.len() != 4 {
        return Err(usage(
            "SURFACESTYLE RENAME names a style and its new name",
            "SURFACESTYLE RENAME <style> <new>",
            "",
        ));
    }
    styles_editable(document)?;
    let Some(found) = document.surface_styles().find(&tokens[2]) else {
        return Err(no_style(&tokens[2]));
    };
    let old = found.name.clone();
    let name = &tokens[3];
    if let Some(other) = document.surface_styles().find(name)
        && !std::ptr::eq(other, found)
    {
        return Err(make_error(
            ErrorCode::AlreadyExists,
            "a surface style already has that name",
            name.as_str(),
        ));
    }
    let mut styles: SurfaceStyleSet = document.surface_styles().clone();
    for style in &mut styles.styles {
        if style.name == old {
            style.name = name.clone();
        }
    }
    if equals_ignoring_case(styles.default_style.as_bytes(), old.as_bytes()) {
        styles.default_style = name.clone();
    }
    // Every surface drawn in it follows, in the same step.
    let mut surfaces: SurfaceStore = document.surfaces().clone();
    let mut moved = 0;
    for named in document.surfaces().all() {
        if equals_ignoring_case(named.style.as_bytes(), old.as_bytes()) {
            let _ = surfaces.set_style(&named.name, name);
            moved += 1;
        }
    }
    document.set_surfaces_and_styles(surfaces, styles, "SURFACESTYLE RENAME")?;
    Ok(format!(
        "renamed style={} name={} surfaces={moved}",
        record_value(&old),
        record_value(name)
    ))
}

fn default_style(document: &mut Document, tokens: &[String]) -> Result<String> {
    if tokens.len() != 3 {
        return Err(usage(
            "SURFACESTYLE DEFAULT names one style",
            "SURFACESTYLE DEFAULT <style>",
            "",
        ));
    }
    styles_editable(document)?;
    let Some(found) = document.surface_styles().find(&tokens[2]) else {
        return Err(no_style(&tokens[2]));
    };
    let mut styles = document.surface_styles().clone();
    styles.default_style = found.name.clone();
    document.set_surface_styles(styles, "SURFACESTYLE DEFAULT")?;
    Ok(format!(
        "default style={}",
        record_value(&document.surface_styles().default_style)
    ))
}

// ---- the option words -----------------------------------------------------------------------

fn on_off(key: &str, value: &str) -> Result<bool> {
    for on in ["on", "yes", "true", "1"] {
        if equals_ignoring_case(value.as_bytes(), on.as_bytes()) {
            return Ok(true);
        }
    }
    for off in ["off", "no", "false", "0"] {
        if equals_ignoring_case(value.as_bytes(), off.as_bytes()) {
            return Ok(false);
        }
    }
    Err(make_error(
        ErrorCode::InvalidArgument,
        format!("{key} is on or off"),
        format!("{key}={value}"),
    ))
}

fn colour_of(key: &str, value: &str, colours: &ColourTable) -> Result<Color> {
    if value.starts_with('#') {
        if let Ok(hex) = Color::from_hex(value) {
            return Ok(hex);
        }
    } else if let Some(named) = resolve_colour(colours, value) {
        return Ok(named);
    }
    Err(make_error(
        ErrorCode::InvalidArgument,
        format!("{key} is a colour: #RRGGBB, #RRGGBBAA or a colour name"),
        format!("{key}={value}"),
    ))
}

fn number_of(key: &str, value: &str) -> Result<f64> {
    parse_finite_double(value).ok_or_else(|| {
        make_error(
            ErrorCode::InvalidArgument,
            format!("{key} is a number"),
            format!("{key}={value}"),
        )
    })
}

fn count_of(key: &str, value: &str) -> Result<i32> {
    match parse_integer(value) {
        Some(number) if (0..=1_000_000).contains(&number) => Ok(number as i32),
        _ => Err(make_error(
            ErrorCode::InvalidArgument,
            format!("{key} is a whole number"),
            format!("{key}={value}"),
        )),
    }
}

fn stops_of(value: &str, colours: &ColourTable) -> Result<Vec<ColourStop>> {
    let mut stops = Vec::new();
    if trimmed(value).is_empty() {
        return Ok(stops);
    }
    let mut start = 0;
    while start <= value.len() {
        let end = value[start..]
            .find(';')
            .map_or(value.len(), |at| start + at);
        let item = trimmed(&value[start..end]);
        let Some(colon) = item.find(':') else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                "stops are value:colour pairs separated by ';': stops=0:#1A9850;2:#91CF60",
                format!("stops={value}"),
            ));
        };
        let number = number_of("stops", &item[..colon])?;
        let colour = colour_of("stops", &item[colon + 1..], colours)?;
        stops.push(ColourStop {
            value: number,
            colour,
        });
        if end == value.len() {
            break;
        }
        start = end + 1;
    }
    Ok(stops)
}

fn stops_text(stops: &[ColourStop]) -> String {
    stops
        .iter()
        .map(|stop| format!("{}:{}", format_exact_real(stop.value), stop.colour.to_hex()))
        .collect::<Vec<String>>()
        .join(";")
}

// ---- the module's entry points ----------------------------------------------------------------

/// The drawing's style set as JSON for a reader that is not a person
/// (SURFACESTYLE LIST JSON, the katana://surface-styles resource): the set as
/// surface_styles_to_json writes it and, when the project's own styles could
/// not be read (Document::surface_styles_status), two members that say the
/// set is a stand-in: "standing_in": {"code", "message"} and "editable"
/// (false for styles a newer Katana wrote, which every edit refuses). Never
/// fails.
pub fn surface_styles_report(document: &Document) -> String {
    let set = surface_styles_to_json(document.surface_styles());
    let Err(why) = document.surface_styles_status() else {
        return set;
    };
    // The built-ins stand in for a set this build cannot read, and the plain
    // SURFACESTYLE LIST says so in a warning record. The JSON must as well, or
    // an agent reading it is told the drawing has the built-ins at version 1
    // while the project keeps a set it cannot see - and its next edit is then
    // refused. Added here and not in surface_styles_to_json, which is the form
    // a save writes into the project. The text is this build's own output,
    // and a parse that somehow failed leaves the set without the note.
    let Ok(mut root) = OrderedJson::parse(&set) else {
        return set;
    };
    if !root.is_object() {
        return set;
    }
    root["standing_in"] = OrderedJson::object_of([
        ("code", OrderedJson::from(code_name(why.code))),
        ("message", OrderedJson::from(why.describe())),
    ]);
    // A set a newer Katana wrote is kept as it is and every edit refused; a
    // damaged one is replaced by the first set given (docs/terrain.md).
    root["editable"] = OrderedJson::from(why.code != ErrorCode::Unsupported);
    root.dump_with(-1, ' ', false)
}

/// One surface as every reply that names one writes it - SURFACE LIST and
/// INFO, SURFACE FROM, an IMPORT that brings surfaces, and the verbs here -
/// with heights and bounds to the millimetre and the style it is drawn in
/// (style_of):
///
/// ```text
///   surface name=ground triangles=2 points=4 bounds=0.000,0.000,10.000,10.000
///           zmin=100.000 zmax=101.000 source="drawing (4 entities)" style=Standard
///           visible=yes
/// ```
pub fn surface_record(named: &NamedSurface, styles: &SurfaceStyleSet) -> String {
    // A surface in a drawing always has its triangulation; the C++ reads it
    // without asking.
    let surface = named
        .surface
        .as_deref()
        .expect("a drawing's surface has a triangulation");
    let bounds = surface.bounds();
    format!(
        "surface name={} triangles={} points={} bounds={},{},{},{} zmin={} zmax={} source={} \
         style={} visible={}",
        record_value(&named.name),
        surface.triangle_count(),
        surface.vertex_count(),
        fixed3(bounds.min.x),
        fixed3(bounds.min.y),
        fixed3(bounds.max.x),
        fixed3(bounds.max.y),
        fixed3(surface.min_elevation()),
        fixed3(surface.max_elevation()),
        if named.source.is_empty() {
            String::new()
        } else {
            record_value(&named.source)
        },
        record_value(&style_of(styles, named).name),
        yes_no(named.visible)
    )
}

/// For an importer that brings surfaces with its entities (an IFC file; the
/// geoprocessing IMPORT of an archive does the same through its staging
/// store): `command` - the import's entities, which may be None - and
/// `surfaces` as ONE undo step, named after the command, or `step_name` when
/// there is none. Each surface joins the drawing's under its own name, or
/// "name (2)" when that is taken. The surfaces' records (surface_record), in
/// their order; on a refusal nothing changes.
pub fn import_surfaces(
    document: &mut Document,
    command: Option<CommandPtr>,
    surfaces: Vec<NamedSurface>,
    step_name: String,
) -> Result<Vec<String>> {
    if surfaces.is_empty() {
        if let Some(command) = command {
            document.execute(command)?;
        }
        return Ok(Vec::new());
    }
    let mut next = document.surfaces().clone();
    let mut names = Vec::new();
    for mut surface in surfaces {
        surface.name = next.unique_name(surface.name.as_bytes());
        names.push(surface.name.clone());
        next.add(surface)?;
    }
    let step = match &command {
        Some(command) => String::from(command.name()),
        None => step_name,
    };
    let surfaces_step = document.surfaces_command(next, step.as_str())?;
    let mut transaction = Transaction::new(step);
    if let Some(command) = command {
        transaction.add(command);
    }
    if let Some(surfaces_step) = surfaces_step {
        transaction.add(surfaces_step);
    }
    if transaction.size() != 0 {
        document.execute(Box::new(transaction))?;
    }
    Ok(names.iter().map(|name| record_of(document, name)).collect())
}

/// True for a line this module runs: SURFACESTYLE, and SURFACE followed by
/// RENAME, STYLE, SHOW, HIDE or COPY. `tokens` is the whole line split into
/// words (as the interpreter's tokenize splits it), the verb first.
pub fn is_surface_verb(tokens: &[String]) -> bool {
    let Some(first) = tokens.first() else {
        return false;
    };
    if equals_ignoring_case(first.as_bytes(), b"SURFACESTYLE") {
        return true;
    }
    if !equals_ignoring_case(first.as_bytes(), b"SURFACE") || tokens.len() < 2 {
        return false;
    }
    ["RENAME", "STYLE", "SHOW", "HIDE", "COPY"]
        .iter()
        .any(|word| equals_ignoring_case(tokens[1].as_bytes(), word.as_bytes()))
}

/// Runs one of them on `document`. The reply, or an error saying what was
/// refused and why.
pub fn run_surface_verb(document: &mut Document, tokens: &[String]) -> Result<String> {
    if is(tokens, 0, "SURFACE") {
        if is(tokens, 1, "RENAME") {
            return rename_surface(document, tokens);
        }
        if is(tokens, 1, "STYLE") {
            return style_surface(document, tokens);
        }
        if is(tokens, 1, "SHOW") {
            return show_surfaces(document, tokens, true);
        }
        if is(tokens, 1, "HIDE") {
            return show_surfaces(document, tokens, false);
        }
        if is(tokens, 1, "COPY") {
            return copy_surface(document, tokens);
        }
        return Err(usage(
            "not a word of this SURFACE family",
            SURFACE_USAGE,
            tokens.get(1).map_or("", String::as_str),
        ));
    }
    if !is(tokens, 0, "SURFACESTYLE") {
        return Err(usage(
            "not a surface verb",
            STYLE_USAGE,
            tokens.first().map_or("", String::as_str),
        ));
    }
    if tokens.len() == 1 || is(tokens, 1, "LIST") {
        if tokens.len() == 1 {
            let listed = [String::from("SURFACESTYLE"), String::from("LIST")];
            return list_styles(document, &listed);
        }
        return list_styles(document, tokens);
    }
    if is(tokens, 1, "INFO") {
        return style_info(document, tokens);
    }
    if is(tokens, 1, "NEW") {
        return new_style(document, tokens);
    }
    if is(tokens, 1, "SET") {
        return set_style(document, tokens);
    }
    if is(tokens, 1, "DELETE") {
        return delete_style(document, tokens);
    }
    if is(tokens, 1, "RENAME") {
        return rename_style(document, tokens);
    }
    if is(tokens, 1, "DEFAULT") {
        return default_style(document, tokens);
    }
    Err(usage(
        "SURFACESTYLE takes LIST, INFO, NEW, SET, DELETE, RENAME or DEFAULT",
        STYLE_USAGE,
        &tokens[1],
    ))
}

/// The lines the general HELP gives these verbs.
pub fn surface_verb_summary() -> String {
    String::from(concat!(
        "Surfaces  SURFACE RENAME <name> <new> | STYLE <name> <style> | SHOW|HIDE <name>...|ALL\n",
        "          | COPY <name> <new>   the drawing's surfaces, saved with it, one undo step\n",
        "          each (SURFACE LIST|INFO|REMOVE|FROM|EXPORT: the geoprocessing verbs)\n",
        "          SURFACESTYLE LIST [JSON] | INFO | NEW <style> [FROM <style>] [key=value]\n",
        "          | SET <style> key=value ... | DELETE | RENAME | DEFAULT   the styles a\n",
        "          surface is drawn in (HELP SURFACESTYLE: every key)\n",
    ))
}

/// Every word and key, for HELP SURFACESTYLE.
pub fn surface_verb_help() -> String {
    [
        SURFACE_USAGE,
        "\n",
        STYLE_USAGE,
        concat!(
            "\n",
            "A surface is named in any case; a name with blanks is quoted. SURFACE STYLE <name> \"\"\n",
            "gives it back to the drawing's default style. SHOW and HIDE change what the views\n",
            "draw only: a hidden surface is still sampled, measured and exported.\n",
            "A style's keys, for NEW and SET (INFO prints them, to be typed back):\n",
            "  description=<text>\n",
            "  triangles=on|off trianglecolour=<c>   boundary=on|off boundarycolour=<c>\n",
            "  points=on|off pointcolour=<c>         breaklines=on|off breaklinecolour=<c>\n",
            "  faces=none|flat|elevation|slope|aspect opacity=0..1   (the plan view)\n",
            "  contours=on|off interval=<m> major=<every n> base=<level> minorcolour=<c>\n",
            "  majorcolour=<c> labels=on|off\n",
            "  ramp=<name> banded=on|off bands=<n> stops=<v>:<c>;<v>:<c>... (empty clears)\n",
            "  min=<v> max=<v> (empty: the surface's own range) flatcolour=<c>\n",
            "  shading=automatic|shaded|edges|wireframe|hidden faces3d=flat|elevation|slope|aspect\n",
            "  (the 3D view). A colour <c> is #RRGGBB, #RRGGBBAA or a colour name.\n",
            "Built in: Standard (the default), Triangles, Contours, Elevation Bands, Slope Bands,\n",
            "Cut Fill and Outline. A style a surface is drawn in, or the default, is not deleted.\n",
        ),
    ]
    .concat()
}

// ---- a style's fields as key=value, for the verbs and the window's style manager ----

/// Sets the field `key` names (lower case, as SURFACESTYLE NEW and SET take
/// it) from `value`: description=, triangles=on|off, trianglecolour=,
/// boundary=, boundarycolour=, points=, pointcolour=, breaklines=,
/// breaklinecolour=, faces=none|flat|elevation|slope|aspect, opacity=0..1,
/// contours=on|off, interval=, major=, base=, minorcolour=, majorcolour=,
/// labels=on|off, ramp=<name>, banded=on|off, bands=, stops=v:colour;v:colour
/// ... (empty clears), min=, max= (empty clears), flatcolour=,
/// shading=automatic|shaded|edges|wireframe|hidden,
/// faces3d=flat|elevation|slope|aspect. A colour is #RRGGBB, #RRGGBBAA or a
/// colour name `colours` or the standard names know (the C++ default is an
/// empty table). A description is read as a typed text (core's
/// unescape_typed: "\\" is one backslash, "\n" a line break, which a
/// description may not hold), so what INFO prints reads back as it was.
/// InvalidArgument naming the key for a key there is none of or a value it
/// cannot take; the style is left as it was. Whether the style as a whole
/// can be drawn is check_surface_style's to say.
pub fn set_surface_style_option(
    style: &mut SurfaceDisplayStyle,
    key: &str,
    value: &str,
    colours: &ColourTable,
) -> Status {
    // Worked on a copy and kept only whole: a refused value leaves the style
    // as it was.
    let mut next = style.clone();
    let flag = |field: &mut bool| -> Status {
        *field = on_off(key, value)?;
        Ok(())
    };
    let colour = |field: &mut Color| -> Status {
        *field = colour_of(key, value, colours)?;
        Ok(())
    };
    let real = |field: &mut f64| -> Status {
        *field = number_of(key, value)?;
        Ok(())
    };
    let count = |field: &mut i32| -> Status {
        *field = count_of(key, value)?;
        Ok(())
    };
    let faces = |field: &mut SurfaceFaces, none_allowed: bool| -> Status {
        match parse_surface_faces(value) {
            Some(parsed) if none_allowed || parsed != SurfaceFaces::None => {
                *field = parsed;
                Ok(())
            }
            _ => Err(make_error(
                ErrorCode::InvalidArgument,
                if none_allowed {
                    format!("{key} is none, flat, elevation, slope or aspect")
                } else {
                    format!("{key} is flat, elevation, slope or aspect")
                },
                format!("{key}={value}"),
            )),
        }
    };
    let optional = |field: &mut Option<f64>| -> Status {
        if trimmed(value).is_empty() {
            *field = None;
            return Ok(());
        }
        *field = Some(number_of(key, value)?);
        Ok(())
    };

    let status = match key {
        "description" => {
            // Read as a typed text, as the annotation and sheet verbs read
            // theirs (core's unescape_typed): two backslashes are one.
            // SURFACESTYLE INFO prints the description through record_value,
            // which doubles every backslash, so reading it plain doubled them
            // again on each trip.
            next.description = unescape_typed(value);
            if next.description.contains(['\r', '\n']) {
                Err(make_error(
                    ErrorCode::InvalidArgument,
                    "description holds a line break (a backslash before an n is one; type two \
                     backslashes for a backslash)",
                    format!("description={value}"),
                ))
            } else {
                Ok(())
            }
        }
        "triangles" => flag(&mut next.show_triangles),
        "trianglecolour" => colour(&mut next.triangle_colour),
        "boundary" => flag(&mut next.show_boundary),
        "boundarycolour" => colour(&mut next.boundary_colour),
        "points" => flag(&mut next.show_points),
        "pointcolour" => colour(&mut next.point_colour),
        "breaklines" => flag(&mut next.show_breaklines),
        "breaklinecolour" => colour(&mut next.breakline_colour),
        "faces" => faces(&mut next.plan_faces, true),
        "opacity" => real(&mut next.plan_opacity),
        "contours" => flag(&mut next.contours.show),
        "interval" => real(&mut next.contours.interval),
        "major" => count(&mut next.contours.major_every),
        "base" => real(&mut next.contours.base),
        "minorcolour" => colour(&mut next.contours.minor_colour),
        "majorcolour" => colour(&mut next.contours.major_colour),
        "labels" => flag(&mut next.contours.label_major),
        "ramp" => {
            // Kept as the ramp spells itself, so a style names a ramp one way.
            next.ramp = String::from(value);
            for &ramp in surface_ramp_names() {
                if equals_ignoring_case(ramp.as_bytes(), value.as_bytes()) {
                    next.ramp = String::from(ramp);
                }
            }
            Ok(())
        }
        "banded" => flag(&mut next.banded),
        "bands" => count(&mut next.bands),
        "stops" => stops_of(value, colours).map(|stops| next.stops = stops),
        "min" => optional(&mut next.range_min),
        "max" => optional(&mut next.range_max),
        "flatcolour" => colour(&mut next.flat_colour),
        "shading" => match parse_surface_shading(value) {
            Some(shading) => {
                next.shading = shading;
                Ok(())
            }
            None => Err(make_error(
                ErrorCode::InvalidArgument,
                "shading is automatic, shaded, edges, wireframe or hidden",
                format!("shading={value}"),
            )),
        },
        "faces3d" => faces(&mut next.faces3d, false),
        _ => Err(make_error(
            ErrorCode::InvalidArgument,
            "a surface style has no such key; HELP SURFACESTYLE lists them",
            key,
        )),
    };
    status?;
    *style = next;
    Ok(())
}

/// Every field of `style` as the key=value pairs set_surface_style_option
/// takes, in the order above: what SURFACESTYLE INFO prints, and typed back
/// after SURFACESTYLE SET <style> gives the same style.
pub fn surface_style_options(style: &SurfaceDisplayStyle) -> Vec<(String, String)> {
    let on_off_text = |value: bool| String::from(if value { "on" } else { "off" });
    let real = format_exact_real;
    let optional = |value: Option<f64>| value.map(real).unwrap_or_default();
    let pair = |key: &str, value: String| (String::from(key), value);
    vec![
        pair("description", style.description.clone()),
        pair("triangles", on_off_text(style.show_triangles)),
        pair("trianglecolour", style.triangle_colour.to_hex()),
        pair("boundary", on_off_text(style.show_boundary)),
        pair("boundarycolour", style.boundary_colour.to_hex()),
        pair("points", on_off_text(style.show_points)),
        pair("pointcolour", style.point_colour.to_hex()),
        pair("breaklines", on_off_text(style.show_breaklines)),
        pair("breaklinecolour", style.breakline_colour.to_hex()),
        pair("faces", String::from(surface_faces_word(style.plan_faces))),
        pair("opacity", real(style.plan_opacity)),
        pair("contours", on_off_text(style.contours.show)),
        pair("interval", real(style.contours.interval)),
        pair("major", format!("{}", style.contours.major_every)),
        pair("base", real(style.contours.base)),
        pair("minorcolour", style.contours.minor_colour.to_hex()),
        pair("majorcolour", style.contours.major_colour.to_hex()),
        pair("labels", on_off_text(style.contours.label_major)),
        pair("ramp", style.ramp.clone()),
        pair("banded", on_off_text(style.banded)),
        pair("bands", format!("{}", style.bands)),
        pair("stops", stops_text(&style.stops)),
        pair("min", optional(style.range_min)),
        pair("max", optional(style.range_max)),
        pair("flatcolour", style.flat_colour.to_hex()),
        pair("shading", String::from(surface_shading_word(style.shading))),
        pair("faces3d", String::from(surface_faces_word(style.faces3d))),
    ]
}
