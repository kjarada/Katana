//! GDAL's algorithms and their arguments as JSON (docs/geoprocessing.md,
//! "Schemas"), the port of src/katana_app/geo/schema.hpp/.cpp: what GDAL HELP
//! ... JSON prints, what katana_gdal_catalogue and katana_gdal_describe
//! return, and the JSON Schema an agent fills katana_gdal_run's arguments by.
//! Generated from the argument specs GDAL declares at run time, never written
//! by hand, so a GDAL upgrade that adds an argument or moves a bound changes
//! the schema with it.
//!
//! The JSON is katana_core's nlohmann translation (`Json`, objects sorted by
//! key), each value of the kind nlohmann makes from the C++ value: an `int` a
//! signed integer, a `double` a float, a list of strings an array of them.

use katana_core::json::{Json, JsonErrorKind};
use katana_gis::processing::{
    self as gp, AlgorithmInfo, AlgorithmSpec, ArgSpec, ArgType, Bound, DatasetKind, Scalar,
};

use super::replies::sources_for;

fn strings(items: &[String]) -> Json {
    Json::array_of(items.iter().map(Json::from))
}

fn words(items: &[&str]) -> Json {
    Json::array_of(items.iter().map(|item| Json::from(*item)))
}

/// `std::visit([](const auto& held) { return Json(held); }, value)`.
fn scalar_json(value: &Scalar) -> Json {
    match value {
        Scalar::Boolean(held) => Json::from(*held),
        Scalar::Text(held) => Json::from(held),
        Scalar::Integer(held) => Json::from(*held),
        Scalar::Real(held) => Json::from(*held),
        Scalar::TextList(held) => strings(held),
        Scalar::IntegerList(held) => Json::array_of(held.iter().map(|each| Json::from(*each))),
        Scalar::RealList(held) => Json::array_of(held.iter().map(|each| Json::from(*each))),
    }
}

fn null_if_empty(text: &str) -> Json {
    if text.is_empty() {
        Json::Null
    } else {
        Json::from(text)
    }
}

fn bound_json(bound: Option<Bound>) -> Json {
    match bound {
        None => Json::Null,
        Some(bound) => Json::object_of([
            ("value", Json::from(bound.value)),
            ("inclusive", Json::from(bound.inclusive)),
        ]),
    }
}

fn accepts(arg: &ArgSpec) -> Vec<&'static str> {
    let mut ways = Vec::new();
    if arg.accepts_name {
        ways.push("name");
    }
    if arg.accepts_object {
        ways.push("object");
    }
    ways
}

/// "a,b,c" as its non-empty parts.
fn split(text: &str) -> Vec<String> {
    text.split(',')
        .filter(|part| !part.is_empty())
        .map(String::from)
        .collect()
}

fn is_number(r#type: ArgType) -> bool {
    matches!(
        r#type,
        ArgType::Integer | ArgType::IntegerList | ArgType::Real | ArgType::RealList
    )
}

/// One value of the argument's type: a list's items, or the scalar itself.
fn element_schema(arg: &ArgSpec, r#type: ArgType) -> Json {
    let mut schema = Json::Null;
    match r#type {
        ArgType::Boolean => schema["type"] = Json::from("boolean"),
        ArgType::Integer | ArgType::IntegerList => schema["type"] = Json::from("integer"),
        ArgType::Real | ArgType::RealList => schema["type"] = Json::from("number"),
        ArgType::String | ArgType::StringList | ArgType::Dataset | ArgType::DatasetList => {
            schema["type"] = Json::from("string");
            if !arg.choices.is_empty() {
                schema["enum"] = strings(&arg.choices);
            }
        }
    }
    if is_number(r#type) {
        if let Some(min) = arg.min {
            schema[if min.inclusive {
                "minimum"
            } else {
                "exclusiveMinimum"
            }] = Json::from(min.value);
        }
        if let Some(max) = arg.max {
            schema[if max.inclusive {
                "maximum"
            } else {
                "exclusiveMaximum"
            }] = Json::from(max.value);
        }
    }
    schema
}

fn is_list(r#type: ArgType) -> bool {
    matches!(
        r#type,
        ArgType::StringList | ArgType::IntegerList | ArgType::RealList
    )
}

/// `{path:[...], name, description, aliases, policy, url, container}`
pub fn algorithm_json(info: &AlgorithmInfo) -> Json {
    Json::object_of([
        ("path", strings(&info.path)),
        ("name", Json::from(gp::path_text(&info.path))),
        ("description", Json::from(&info.description)),
        ("aliases", strings(&info.aliases)),
        ("policy", Json::from(gp::to_string(info.policy))),
        ("url", Json::from(&info.help_url)),
        ("container", Json::from(info.container)),
    ])
}

/// `{name, short_name, aliases, type, required, positional, category,
/// default, choices, min:{value,inclusive}, max:{...}, count:{min,max},
/// dataset:{kinds, accepts, sources}, depends_on, exclusion_group,
/// dependency_group, input, output, description}`; absent members are null.
pub fn argument_json(arg: &ArgSpec) -> Json {
    let mut json = Json::object_of([
        ("name", Json::from(&arg.name)),
        ("short_name", null_if_empty(&arg.short_name)),
        ("aliases", strings(&arg.aliases)),
        ("type", Json::from(gp::to_string(arg.r#type))),
        ("required", Json::from(arg.required)),
        ("positional", Json::from(arg.positional)),
        ("category", Json::from(&arg.category)),
        (
            "default",
            arg.default_value.as_ref().map_or(Json::Null, scalar_json),
        ),
        ("choices", strings(&arg.choices)),
        ("min", bound_json(arg.min)),
        ("max", bound_json(arg.max)),
        (
            "count",
            Json::object_of([
                ("min", Json::from(arg.min_count)),
                (
                    "max",
                    if arg.max_count < 0 {
                        Json::Null
                    } else {
                        Json::from(arg.max_count)
                    },
                ),
            ]),
        ),
        ("depends_on", strings(&arg.depends_on)),
        ("exclusion_group", null_if_empty(&arg.exclusion_group)),
        ("dependency_group", null_if_empty(&arg.dependency_group)),
        ("input", Json::from(arg.is_input)),
        ("output", Json::from(arg.is_output)),
        ("description", Json::from(&arg.description)),
    ]);
    json["dataset"] = if arg.is_dataset() {
        Json::object_of([
            (
                "kinds",
                strings(&split(&gp::dataset_kinds_text(arg.dataset_kinds))),
            ),
            ("accepts", words(&accepts(arg))),
            ("update", Json::from(arg.dataset_update)),
            ("sources", strings(&split(&sources_for(arg)))),
        ])
    } else {
        Json::Null
    };
    json
}

/// The JSON Schema of katana_gdal_run's `arguments`: one property per
/// argument that is not a dataset, typed, with GDAL's choices as an enum, its
/// bounds as minimum/exclusiveMinimum and maximum/exclusiveMaximum, list
/// counts as minItems/maxItems, and its default.
pub fn arguments_schema(spec: &AlgorithmSpec) -> Json {
    let mut properties = Json::object();
    for arg in &spec.args {
        if arg.is_dataset() {
            continue;
        }
        let mut schema = if is_list(arg.r#type) {
            let mut list = Json::Null;
            list["type"] = Json::from("array");
            list["items"] = element_schema(arg, arg.r#type);
            if arg.min_count > 0 {
                list["minItems"] = Json::from(arg.min_count);
            }
            if arg.max_count >= 0 {
                list["maxItems"] = Json::from(arg.max_count);
            }
            list
        } else {
            element_schema(arg, arg.r#type)
        };
        if let Some(default) = &arg.default_value {
            schema["default"] = scalar_json(default);
        }
        schema["description"] = Json::from(&arg.description);
        properties[arg.name.as_str()] = schema;
    }
    Json::object_of([
        ("type", Json::from("object")),
        ("properties", properties),
        ("additionalProperties", Json::from(false)),
    ])
}

/// The JSON Schema of one Source an argument of `kinds` (processing's
/// DatasetKind bits) reads: {scope, area, layers, only, where} for vector,
/// {raster} | {surface, cell} for raster, and {file, layer} for any; 0 is
/// every source.
pub fn source_schema(kinds: u32) -> Json {
    // Only the sources an argument of these kinds reads (sources_for): a
    // raster input offered the drawing's scope took drawing data the binder
    // then had to refuse. A file is offered to every kind.
    let vector = kinds == 0 || (kinds & DatasetKind::Vector as u32) != 0;
    let raster = kinds == 0 || (kinds & DatasetKind::Raster as u32) != 0;
    let mut properties = Json::object();
    let mut forms: Vec<&str> = Vec::new();
    if vector {
        forms.push("drawing data by scope ({scope, area?, layers?, only?, where?})");
        properties["scope"] = Json::object_of([
            ("type", Json::from("string")),
            ("enum", words(&["selection", "drawing", "area", "layers"])),
            (
                "description",
                Json::from(
                    "Which drawing data: the selection, the whole drawing, a window (area) or \
                     named layers (layers).",
                ),
            ),
        ]);
        properties["area"] = Json::object_of([
            ("type", Json::from("array")),
            ("items", Json::object_of([("type", Json::from("number"))])),
            ("minItems", Json::from(4_i32)),
            ("maxItems", Json::from(4_i32)),
            (
                "description",
                Json::from("x0, y0, x1, y1, with scope area."),
            ),
        ]);
        properties["layers"] = Json::object_of([
            ("type", Json::from("array")),
            ("items", Json::object_of([("type", Json::from("string"))])),
            ("description", Json::from("Layer paths, with scope layers.")),
        ]);
        properties["only"] = Json::object_of([
            ("type", Json::from("boolean")),
            (
                "description",
                Json::from("With scope layers: those layers only, not the layers beneath."),
            ),
        ]);
        properties["where"] = Json::object_of([
            ("type", Json::from("array")),
            ("items", Json::object_of([("type", Json::from("string"))])),
            (
                "description",
                Json::from(
                    "Filter conditions, as WHERE takes them: TYPE=polyline, LAYER=pattern, \
                     STYLE=, COLOUR=, PROP=key[:pattern], TEXT=, DRAWN. Without scope they \
                     filter the selection, as WHERE alone does.",
                ),
            ),
        ]);
    }
    if raster {
        forms.push("a reference raster ({raster})");
        forms.push("a surface ({surface, cell?})");
        properties["raster"] = Json::object_of([
            ("type", words(&["integer", "string"])),
            (
                "description",
                Json::from("A reference raster's id or name (REFS lists them)."),
            ),
        ]);
        properties["surface"] = Json::object_of([
            ("type", Json::from("string")),
            ("description", Json::from("A surface's name.")),
        ]);
        properties["cell"] = Json::object_of([
            ("type", Json::from("number")),
            ("exclusiveMinimum", Json::from(0_i32)),
            (
                "description",
                Json::from("With surface: the grid cell; suggested from its extent when absent."),
            ),
        ]);
    }
    forms.push("a file ({file, layer?})");
    properties["file"] = Json::object_of([
        ("type", Json::from("string")),
        ("description", Json::from("A file, a /vsi path or a URL.")),
    ]);
    properties["layer"] = Json::object_of([
        ("type", Json::from("string")),
        ("description", Json::from("With file: the file's layer.")),
    ]);
    let mut description = String::from("One source: ");
    for (i, form) in forms.iter().enumerate() {
        description += if i == 0 {
            ""
        } else if i + 1 == forms.len() {
            " or "
        } else {
            ", "
        };
        description += *form;
    }
    description.push('.');
    Json::object_of([
        ("type", Json::from("object")),
        ("description", Json::from(description)),
        ("properties", properties),
        ("additionalProperties", Json::from(false)),
    ])
}

/// The JSON Schema of katana_gdal_run's `inputs`: one property per input
/// dataset argument, a Source object or, for a list, an array of them.
pub fn inputs_schema(spec: &AlgorithmSpec) -> Json {
    let mut properties = Json::object();
    let mut required: Vec<String> = Vec::new();
    for arg in &spec.args {
        if !arg.is_dataset() || !arg.is_input || arg.is_output {
            continue;
        }
        let mut schema = if arg.r#type == ArgType::DatasetList && arg.max_count != 1 {
            Json::object_of([(
                "oneOf",
                Json::array_of([
                    source_schema(arg.dataset_kinds),
                    Json::object_of([
                        ("type", Json::from("array")),
                        ("items", source_schema(arg.dataset_kinds)),
                    ]),
                ]),
            )])
        } else {
            source_schema(arg.dataset_kinds)
        };
        schema["description"] =
            Json::from(format!("{} Accepts {}.", arg.description, sources_for(arg)));
        properties[arg.name.as_str()] = schema;
        if arg.required {
            required.push(arg.name.clone());
        }
    }
    let mut schema = Json::object_of([
        ("type", Json::from("object")),
        ("properties", properties),
        ("additionalProperties", Json::from(false)),
    ]);
    if !required.is_empty() {
        schema["x-required"] = strings(&required); // bound by FROM or by GDAL's own words
    }
    schema
}

/// `{algorithm:{...}, arguments:[...], arguments_schema, inputs_schema,
/// gdal_usage}`: katana_gdal_describe's answer and GDAL HELP ... JSON.
pub fn describe_json(spec: &AlgorithmSpec) -> Json {
    let mut algorithm = algorithm_json(&spec.info);
    algorithm["long_description"] = Json::from(&spec.long_description);
    let arguments = Json::array_of(spec.args.iter().map(argument_json));
    // GDAL's usage as the JSON it is, or as its text when it is none (GDAL
    // writes a default of Infinity bare): the C++ catches nlohmann's
    // parse_error alone, and anything else it throws goes on up, as here.
    let usage = match Json::parse(&spec.usage_json) {
        Ok(usage) => usage,
        Err(error) if error.kind == JsonErrorKind::ParseError => Json::from(&spec.usage_json),
        Err(error) => panic!("{}", error.what),
    };
    Json::object_of([
        ("algorithm", algorithm),
        ("arguments", arguments),
        ("arguments_schema", arguments_schema(spec)),
        ("inputs_schema", inputs_schema(spec)),
        ("gdal_usage", usage),
    ])
}
