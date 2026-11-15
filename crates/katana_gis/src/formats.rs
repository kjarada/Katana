//! The formats this build of GDAL reads and writes, read from GDAL's own
//! driver manager rather than from tables of extensions kept by hand
//! (include/katana/gis/formats.hpp, src/katana_io/geo/formats.cpp;
//! docs/interop.md, "Formats"). Three hand-kept tables had drifted apart (a
//! CSV could be written and not read) and offered ECW, for which the
//! toolchain has no driver, while FlatGeobuf, GeoParquet, KMZ, GPX and a
//! zipped shapefile, all of which GDAL has, could not be opened at all.
//!
//! What GDAL knows comes from the registry; what Katana prefers is a small
//! curated overlay below, each entry with its reason: drivers that are no
//! format at all are hidden; where two writers claim an extension, GDAL's own
//! choice for the name (GDALGetOutputDriversForDatasetName, what `gdal vector
//! convert` uses) decides, and the overlay only where Katana's differs; a
//! writer that needs another program (GPSBabel) is not offered. And a file is
//! routed by what it HOLDS, found by GDAL, not by the letters after its dot.
//!
//! `formats()` is read once, into a OnceLock, as the C++'s function-local
//! static is.

#![allow(
    unsafe_code,
    reason = "Reads GDAL's driver manager and identifies files through it; every call states why it is sound."
)]

use std::collections::BTreeSet;
use std::ffi::{CStr, c_int};
use std::path::Path;
use std::ptr;
use std::sync::OnceLock;

use katana_core::error::make_error;
use katana_core::path_text::path_exists;
use katana_core::text::parse_finite_double;
use katana_core::{ErrorCode, Result, Status};

use crate::cpl_error_collector::quiet_errors;
use crate::ffi::{cpl, gdal, minixml};
use crate::formats_detail::{extension_of_file_name, file_name_of, open_names};
use crate::gdal_adapter::{description_of, driver_by_name, metadata_item};
use crate::gdal_registry::ensure_registered;

// ---- the curated overlay -------------------------------------------------------------------
//
// Everything else is GDAL's. Each entry says why Katana differs.

/// Drivers that are no format a person opens or saves:
///
/// ```text
/// MEM       memory only, what the bridge builds its inputs in;
/// DERIVED   a syntax for VRT pixel functions over another dataset;
/// HTTP      GDAL's wrapper that fetches a URL for another driver - a URL
///           reaches it without being named;
/// AIVector  sends the data to a remote AI service to interpret it;
/// GPSBabel  runs the gpsbabel program, which Katana does not ship, for
///           both reading and writing.
/// ```
///
/// A driver of neither rasters nor vectors (GNM networks, CPHD) is left out
/// by the rule in formats(), not by name.
const HIDDEN: [&str; 5] = ["MEM", "DERIVED", "HTTP", "AIVector", "GPSBabel"];

/// Where Katana's writer for an extension is not GDAL's first choice: for
/// .xml GDAL picks PDS4 (NASA's planetary data); in a survey or GIS office an
/// .xml of features is GML.
const WRITER_FOR: [(&str, &str); 1] = [("xml", "GML")];

/// Extensions a file dialog does not offer as data: a shapefile's or a
/// raster's sidecars and headers, which are opened through the file they go
/// with, and generic ones only a rare format claims (VDV's .txt, ZMap's .dat,
/// NSIDC's .bin, and the many .xml), which would fill a filter with files that
/// are not data.
const NOT_OFFERED: [&str; 14] = [
    "dbf", "shx", "prj", "cpg", "mid", "hdr", "aux", "ovr", "tfw", "wld", "txt", "dat", "bin",
    "xml",
];

/// The formats a save dialog lists first, in this order: the ones a survey or
/// CAD office exchanges every day; everything else follows by name.
const SAVE_FIRST: [&str; 13] = [
    "ESRI Shapefile",
    "GeoJSON",
    "GPKG",
    "LIBKML",
    "DXF",
    "GML",
    "CSV",
    "FlatGeobuf",
    "Parquet",
    "GPX",
    "MapInfo File",
    "SQLite",
    "OpenFileGDB",
];

/// A save dialog entry for more than a format's first extension, where the
/// second is a different kind of file to its reader: a KMZ is a zipped KML,
/// and a zipped shapefile is one file to hand over instead of four.
struct SaveAs {
    driver: &'static str,
    extension: &'static str,
    description: &'static str,
}

const SAVE_AS: [SaveAs; 6] = [
    SaveAs {
        driver: "ESRI Shapefile",
        extension: "shp",
        description: "ESRI Shapefile",
    },
    SaveAs {
        driver: "ESRI Shapefile",
        extension: "shp.zip",
        description: "ESRI Shapefile, zipped",
    },
    SaveAs {
        driver: "GeoJSON",
        extension: "geojson",
        description: "GeoJSON",
    },
    SaveAs {
        driver: "LIBKML",
        extension: "kml",
        description: "KML",
    },
    SaveAs {
        driver: "LIBKML",
        extension: "kmz",
        description: "KMZ (zipped KML)",
    },
    SaveAs {
        driver: "MapInfo File",
        extension: "tab",
        description: "MapInfo TAB",
    },
];

/// The URL schemes and /vsi prefixes that reach a server.
const REMOTE_PREFIXES: [&str; 9] = [
    "/vsicurl",
    "/vsis3",
    "/vsigs",
    "/vsiaz",
    "/vsiadls",
    "/vsioss",
    "/vsiswift",
    "/vsiwebhdfs",
    "/vsihdfs",
];

// ---- the types ------------------------------------------------------------------------------

/// One of GDAL's drivers, as Katana offers it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Format {
    /// GDAL's short name: "GPKG", "ESRI Shapefile".
    pub driver: String,
    /// GDAL's long name: "GeoPackage".
    pub description: String,
    /// Holds rasters.
    pub raster: bool,
    /// Holds vector layers.
    pub vector: bool,
    pub read_raster: bool,
    pub read_vector: bool,
    /// Writes a raster: creates one, or copies a finished one (PNG, AAIGrid),
    /// which GdalDataset::write_raster does by building it in memory first.
    pub write_raster: bool,
    /// Writes vector layers: creates a dataset and layers in it. A driver
    /// that can only copy a whole dataset (GMLAS) cannot take Katana's tables.
    pub write_vector: bool,
    /// Opens a path inside an archive or on the web (/vsizip, /vsicurl).
    pub virtual_io: bool,
    /// Lower case, without the dot, in GDAL's order; compound ones whole
    /// ("shp.zip"). Empty for a database or a web service.
    pub extensions: Vec<String>,
    /// The prefix of a connection string the driver opens ("PG:"), for one
    /// that is no file.
    pub connection_prefix: String,
    /// GDAL's page for it; empty when it names none.
    pub help_url: String,
}

impl Format {
    pub fn reads(&self) -> bool {
        self.read_raster || self.read_vector
    }

    pub fn writes(&self) -> bool {
        self.write_raster || self.write_vector
    }
}

/// One option a driver takes, as GDAL declares it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormatOption {
    pub name: String,
    /// GDAL's own: "boolean", "int", "integer", "float", "string",
    /// "string-select" ... as the driver spells it.
    pub r#type: String,
    pub description: String,
    /// Empty when GDAL declares none.
    pub default_value: String,
    /// "raster", "vector" or "raster,vector" where the driver says which kind
    /// of data the option is for; empty when it says nothing.
    pub scope: String,
    /// A string-select's values.
    pub choices: Vec<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

/// A driver's options for opening a dataset (IMPORT's oo=), creating one
/// (EXPORT's co=) and creating a layer in one (lco=): what an option given to
/// GDAL is checked against, and what a person is shown.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormatOptions {
    pub open: Vec<FormatOption>,
    pub creation: Vec<FormatOption>,
    pub layer_creation: Vec<FormatOption>,
}

/// Which of a driver's lists an option is checked against: IMPORT's oo=,
/// EXPORT's co= and lco=.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OptionList {
    Open,
    Creation,
    LayerCreation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DataKind {
    Raster,
    Vector,
}

/// What a save dialog offers for vector data: one entry per format and file
/// kind a person saves as (KML and KMZ are two, a shapefile and a zipped one
/// two), the common formats first. `driver` is what EXPORT writes that
/// extension with (vector_writer_for).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveChoice {
    pub description: String,
    /// Without the dot.
    pub extension: String,
    pub driver: String,
}

/// What a dataset holds, found by GDAL (identify_content).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Content {
    pub driver: String,
    /// What GDAL opens: the path, or /vsizip/<path>[/<member>].
    pub name: String,
    pub raster: bool,
    pub vector: bool,
}

// ---- helpers --------------------------------------------------------------------------------

/// The C++'s `text += (text.empty() ? "" : ", ") + item`: an empty item adds
/// nothing, not even its comma, while the text is still empty.
pub(crate) fn comma_joined<'a>(items: impl Iterator<Item = &'a str>) -> String {
    let mut text = String::new();
    for item in items {
        if !text.is_empty() {
            text.push_str(", ");
        }
        text.push_str(item);
    }
    text
}

/// Whether a dialog's filter leaves the extension out (NOT_OFFERED).
pub(crate) fn is_not_offered(extension: &str) -> bool {
    NOT_OFFERED.contains(&extension)
}

fn same_driver(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// A driver's space-separated list, each word lower case.
fn words(text: Option<String>) -> Vec<String> {
    text.map_or_else(Vec::new, |text| {
        text.split(' ')
            .filter(|word| !word.is_empty())
            .map(str::to_ascii_lowercase)
            .collect()
    })
}

/// Whether a driver's capability is set (CPLTestBool of its metadata item).
fn flag(driver: gdal::GdalDriverH, key: &CStr) -> bool {
    // SAFETY: a registered driver and NUL-terminated key and domain; the
    // value is the driver's own, tested at once.
    unsafe {
        let value = gdal::GDALGetMetadataItem(driver.cast(), key.as_ptr(), c"".as_ptr());
        !value.is_null() && cpl::CPLTestBool(value)
    }
}

/// The extension of a name, compound ones whole where a format claims them:
/// "lots.shp.zip" is "shp.zip" to a registry that lists it, "zip" otherwise.
pub(crate) fn extension_of_name(name: &[u8]) -> String {
    let file = file_name_of(name).to_ascii_lowercase();
    let mut best = String::new();
    for format in formats() {
        for extension in &format.extensions {
            if extension.len() > best.len() && file.len() > extension.len() + 1 {
                let tail = &file[file.len() - extension.len() - 1..];
                if tail[0] == b'.' && &tail[1..] == extension.as_bytes() {
                    best.clone_from(extension);
                }
            }
        }
    }
    if !best.is_empty() {
        return best;
    }
    let plain = extension_of_file_name(file_name_of(name));
    match plain.strip_prefix(b".") {
        Some(rest) => String::from_utf8_lossy(rest).to_ascii_lowercase(),
        None => String::new(),
    }
}

fn extension_of(path: &Path) -> String {
    extension_of_name(&cpl::native_path(path))
}

fn read_format(driver: gdal::GdalDriverH) -> Format {
    let name = description_of(driver.cast());
    let description =
        metadata_item(driver.cast(), gdal::GDAL_DMD_LONGNAME).unwrap_or_else(|| name.clone());
    let raster = flag(driver, gdal::GDAL_DCAP_RASTER);
    let vector = flag(driver, gdal::GDAL_DCAP_VECTOR);
    let open = flag(driver, gdal::GDAL_DCAP_OPEN);
    let create = flag(driver, gdal::GDAL_DCAP_CREATE);
    let copy = flag(driver, gdal::GDAL_DCAP_CREATECOPY);
    // GDAL has no per-kind "create" for a driver of both kinds. A vector
    // writer is one that creates layers and their fields, which is what
    // write_tables does (MBTiles declares the fields and not the layers;
    // S-57, whose writer needs a catalogue of its own, neither). A driver of
    // both kinds writes rasters when it declares the data types it creates
    // them in - a file geodatabase declares none, since it reads a
    // geodatabase's rasters and writes only its tables. A driver of rasters
    // alone writes what it creates or copies, declared types or not (XYZ).
    let write_vector = vector
        && create
        && (flag(driver, gdal::GDAL_DCAP_CREATE_LAYER)
            || flag(driver, gdal::GDAL_DCAP_CREATE_FIELD));
    let declares_types = metadata_item(driver.cast(), gdal::GDAL_DMD_CREATIONDATATYPES)
        .is_some_and(|types| !types.is_empty());
    let write_raster = raster && (create || copy) && (!vector || declares_types);
    let virtual_io = flag(driver, gdal::GDAL_DCAP_VIRTUALIO);
    let mut extensions = words(metadata_item(driver.cast(), gdal::GDAL_DMD_EXTENSIONS));
    if extensions.is_empty() {
        extensions = words(metadata_item(driver.cast(), gdal::GDAL_DMD_EXTENSION));
    }
    let connection_prefix =
        metadata_item(driver.cast(), gdal::GDAL_DMD_CONNECTION_PREFIX).unwrap_or_default();
    // GDAL gives a page of its site ("drivers/vector/gpkg.html").
    let help_url = metadata_item(driver.cast(), gdal::GDAL_DMD_HELPTOPIC)
        .map(|page| {
            if page.starts_with("http") {
                page
            } else {
                format!("https://gdal.org/{page}")
            }
        })
        .unwrap_or_default();
    Format {
        driver: name,
        description,
        raster,
        vector,
        read_raster: raster && open,
        read_vector: vector && open,
        write_raster,
        write_vector,
        virtual_io,
        extensions,
        connection_prefix,
        help_url,
    }
}

/// The nodes of a sibling chain of a parsed tree, from `first`.
///
/// # Safety
///
/// `first` is NULL or a node of a tree that stays alive, and unchanged,
/// while the nodes are used.
unsafe fn siblings<'a>(
    first: *const minixml::CplXmlNode,
) -> impl Iterator<Item = &'a minixml::CplXmlNode> {
    // SAFETY: NULL or a live node (the caller's contract).
    let first = unsafe { first.as_ref() };
    // SAFETY: a sibling link of a live node is NULL or a live node of the
    // same tree.
    std::iter::successors(first, |node| unsafe { node.ps_next.cast_const().as_ref() })
}

/// `EQUAL(node->pszValue, word)`: the node's name, ignoring ASCII case.
fn named(node: &minixml::CplXmlNode, word: &str) -> bool {
    // SAFETY: an element's value is its name, a live NUL-terminated string
    // of the live tree (or NULL).
    !node.psz_value.is_null()
        && unsafe { CStr::from_ptr(node.psz_value) }
            .to_bytes()
            .eq_ignore_ascii_case(word.as_bytes())
}

/// `CPLGetXMLValue(node, path, "")`.
fn xml_value(node: &minixml::CplXmlNode, path: Option<&CStr>) -> String {
    // SAFETY: a live node of a live tree, NULL or a NUL-terminated path and
    // an empty default; the value is the tree's own, copied at once.
    unsafe {
        cpl::decoded(minixml::CPLGetXMLValue(
            node,
            path.map_or(ptr::null(), CStr::as_ptr),
            c"".as_ptr(),
        ))
    }
}

fn parse_options(xml: Option<String>) -> Vec<FormatOption> {
    let mut options = Vec::new();
    let Some(xml) = xml.filter(|xml| !xml.is_empty()) else {
        return options;
    };
    let text = cpl::c_string(&xml);
    // SAFETY: a NUL-terminated document; the tree returned is destroyed below.
    let root = unsafe { minixml::CPLParseXMLString(text.as_ptr()) };
    if root.is_null() {
        return options;
    }
    // SAFETY: the tree lives, unchanged, until it is destroyed below.
    for list in unsafe { siblings(root) } {
        if list.e_type != minixml::CXT_ELEMENT {
            continue;
        }
        // SAFETY: a child link of a node of the same live tree.
        for node in unsafe { siblings(list.ps_child) } {
            if node.e_type != minixml::CXT_ELEMENT || !named(node, "Option") {
                continue;
            }
            // SAFETY: a child link of a node of the same live tree.
            let choices = unsafe { siblings(node.ps_child) }
                .filter(|value| value.e_type == minixml::CXT_ELEMENT && named(value, "Value"))
                .map(|value| xml_value(value, None))
                .collect();
            let option = FormatOption {
                name: xml_value(node, Some(c"name")),
                r#type: xml_value(node, Some(c"type")),
                description: xml_value(node, Some(c"description")),
                default_value: xml_value(node, Some(c"default")),
                scope: xml_value(node, Some(c"scope")),
                choices,
                min: parse_finite_double(xml_value(node, Some(c"min"))),
                max: parse_finite_double(xml_value(node, Some(c"max"))),
            };
            if !option.name.is_empty() {
                options.push(option);
            }
        }
    }
    // SAFETY: the tree CPLParseXMLString made, destroyed once after its last use.
    unsafe { minixml::CPLDestroyXMLNode(root) };
    options
}

/// Whether the dataset holds rasters: bands, or raster subdatasets (a
/// GeoPackage of several tile tables has no bands of its own).
fn holds_raster(dataset: gdal::GdalDatasetH) -> bool {
    // SAFETY: a live dataset; its metadata list is its own, read at once.
    unsafe {
        if gdal::GDALGetRasterCount(dataset) > 0 {
            return true;
        }
        let subdatasets = gdal::GDALGetMetadata(dataset.cast(), c"SUBDATASETS".as_ptr());
        !subdatasets.is_null() && !(*subdatasets).is_null()
    }
}

fn last_error_message() -> String {
    // SAFETY: this thread's last error message, copied at once.
    unsafe { cpl::decoded(cpl::CPLGetLastErrorMsg()) }
}

// ---- the registry ---------------------------------------------------------------------------

/// Every format of this build that Katana offers, in the order of their
/// driver names ignoring case - stable from one run to the next, unlike
/// GDAL's registration order, which depends on the plugins found. Read once.
pub fn formats() -> &'static [Format] {
    static ALL: OnceLock<Vec<Format>> = OnceLock::new();
    ALL.get_or_init(|| {
        ensure_registered();
        let mut out = Vec::new();
        // SAFETY: the driver manager's count; registered above.
        let count = unsafe { gdal::GDALGetDriverCount() };
        for i in 0..count {
            // SAFETY: one of the registered drivers, which live as long as
            // the process.
            let driver = unsafe { gdal::GDALGetDriver(i) };
            if driver.is_null() || HIDDEN.contains(&description_of(driver.cast()).as_str()) {
                continue;
            }
            let format = read_format(driver);
            if !format.raster && !format.vector {
                continue; // neither kind of data: a network model, a phase history
            }
            out.push(format);
        }
        out.sort_by(|a, b| {
            a.driver
                .to_ascii_lowercase()
                .cmp(&b.driver.to_ascii_lowercase())
                .then_with(|| a.driver.cmp(&b.driver))
        });
        out
    })
}

/// One format by its driver name, ignoring case; None when this build has no
/// such driver, or Katana does not offer it.
pub fn find_format(driver: &str) -> Option<&'static Format> {
    formats()
        .iter()
        .find(|format| same_driver(&format.driver, driver))
}

/// NotFound for a driver find_format does not find.
pub fn format_options(driver: &str) -> Result<FormatOptions> {
    let not_found = || {
        make_error(
            ErrorCode::NotFound,
            "this GDAL build has no format of that name",
            driver,
        )
    };
    let format = find_format(driver).ok_or_else(not_found)?;
    let gdal_driver = driver_by_name(&format.driver);
    if gdal_driver.is_null() {
        return Err(not_found());
    }
    let list = |key: &CStr| parse_options(metadata_item(gdal_driver.cast(), key));
    Ok(FormatOptions {
        open: list(gdal::GDAL_DMD_OPENOPTIONLIST),
        creation: list(gdal::GDAL_DMD_CREATIONOPTIONLIST),
        layer_creation: list(gdal::GDAL_DS_LAYER_CREATIONOPTIONLIST),
    })
}

/// `given`, KEY=VALUE each, checked against the driver's own list of that
/// kind (format_options): InvalidArgument naming the first that is not
/// KEY=VALUE, whose key the driver does not declare - listing the keys it
/// does - or whose value is not one of a string-select's choices. GDAL itself
/// only warns of an option it does not know and carries on without it, so a
/// misspelt one (RFC7964=YES) was written as if it had worked. Keys and
/// choices ignore case, as GDAL's do. A driver that declares no list of that
/// kind is refused any option, saying so. NotFound for a driver find_format
/// does not find. Nothing given is always good.
pub fn check_options(driver: &str, which: OptionList, given: &[String]) -> Status {
    if given.is_empty() {
        return Ok(());
    }
    let options = format_options(driver)?;
    let (list, kind) = match which {
        OptionList::Open => (&options.open, "open"),
        OptionList::Creation => (&options.creation, "creation"),
        OptionList::LayerCreation => (&options.layer_creation, "layer creation"),
    };
    let known = comma_joined(list.iter().map(|option| option.name.as_str()));
    for word in given {
        let Some(equals) = word.find('=').filter(|&equals| equals != 0) else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("a {kind} option is KEY=VALUE"),
                word.clone(),
            ));
        };
        let key = &word[..equals];
        let value = &word[equals + 1..];
        let Some(found) = list
            .iter()
            .find(|option| option.name.eq_ignore_ascii_case(key))
        else {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                if list.is_empty() {
                    format!("{driver} takes no {kind} options")
                } else {
                    format!("{driver} has no {kind} option {key}; it has {known}")
                },
                word.clone(),
            ));
        };
        if !found.choices.is_empty()
            && !found
                .choices
                .iter()
                .any(|choice| choice.eq_ignore_ascii_case(value))
        {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!(
                    "{} is one of {}",
                    found.name,
                    comma_joined(found.choices.iter().map(String::as_str))
                ),
                word.clone(),
            ));
        }
    }
    Ok(())
}

/// The extensions of the formats that read data of `kind`, for a file
/// dialog's filter: sorted, each once, without the ones a sidecar or a header
/// file has (.dbf, .shx, .prj, .hdr ...) and without the generic ones only a
/// rare format claims (.txt, .dat, .bin), which would fill the filter with
/// files that are not data. A file of any extension is still opened by what
/// it holds when a person picks it through "All files".
pub fn readable_extensions(kind: DataKind) -> Vec<String> {
    let mut extensions = BTreeSet::new();
    for format in formats() {
        let reads = match kind {
            DataKind::Raster => format.read_raster,
            DataKind::Vector => format.read_vector,
        };
        if !reads {
            continue;
        }
        for extension in &format.extensions {
            if !is_not_offered(extension) {
                extensions.insert(extension.clone());
            }
        }
    }
    extensions.into_iter().collect()
}

/// The driver EXPORT writes a vector path with: GDAL's own choice for the
/// name - its extension, compound ones whole (".shp.zip" is a zipped
/// shapefile, ".kml" LIBKML) - where Katana's overlay does not differ.
/// InvalidArgument for a path with no extension; Unsupported, naming the
/// extension, when no writer Katana offers claims it.
pub fn vector_writer_for(path: &Path) -> Result<String> {
    let extension = extension_of(path);
    if extension.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "cannot choose a vector driver: the path has no extension",
            cpl::path_text(path),
        ));
    }
    let offered = |driver: &str| find_format(driver).filter(|format| format.write_vector);
    for (claimed, driver) in WRITER_FOR {
        if extension == claimed
            && let Some(format) = offered(driver)
        {
            return Ok(format.driver.clone());
        }
    }
    // GDAL's own ranking of the writers that claim the name (LIBKML before
    // KML for a .kml, GeoJSON before JSON-FG for a .json), the first Katana
    // offers taken.
    let name = cpl::c_string(format!("katana.{extension}"));
    // SAFETY: a NUL-terminated name; the list GDAL returns is copied and
    // freed by take_string_list.
    let candidates = unsafe {
        cpl::take_string_list(gdal::GDALGetOutputDriversForDatasetName(
            name.as_ptr(),
            gdal::GDAL_OF_VECTOR as c_int,
            false,
            false,
        ))
    }
    .unwrap_or_default();
    for candidate in &candidates {
        if let Some(format) = offered(&String::from_utf8_lossy(candidate)) {
            return Ok(format.driver.clone());
        }
    }
    Err(make_error(
        ErrorCode::Unsupported,
        format!("no vector driver is registered for '.{extension}'"),
        cpl::path_text(path),
    ))
}

pub fn vector_save_choices() -> Vec<SaveChoice> {
    let mut choices = Vec::new();
    for format in formats() {
        if !format.write_vector {
            continue;
        }
        let writes_it = |extension: &str| {
            vector_writer_for(Path::new(&format!("katana.{extension}")))
                .is_ok_and(|writer| writer == format.driver)
        };
        let mut curated = false;
        for entry in &SAVE_AS {
            if format.driver == entry.driver && writes_it(entry.extension) {
                choices.push(SaveChoice {
                    description: String::from(entry.description),
                    extension: String::from(entry.extension),
                    driver: format.driver.clone(),
                });
                curated = true;
            }
        }
        if curated {
            continue;
        }
        if let Some(extension) = format
            .extensions
            .iter()
            .find(|extension| !is_not_offered(extension) && writes_it(extension.as_str()))
        {
            choices.push(SaveChoice {
                description: format.description.clone(),
                extension: extension.clone(),
                driver: format.driver.clone(),
            });
        }
    }
    let rank = |choice: &SaveChoice| {
        SAVE_FIRST
            .iter()
            .position(|driver| choice.driver == *driver)
            .unwrap_or(SAVE_FIRST.len())
    };
    choices.sort_by(|a, b| {
        rank(a).cmp(&rank(b)).then_with(|| {
            a.description
                .to_ascii_lowercase()
                .cmp(&b.description.to_ascii_lowercase())
        })
    });
    choices
}

// ---- paths GDAL opens -----------------------------------------------------------------------

/// True for a web address (http://, https://, ftp://, s3:// ...), or a
/// /vsicurl-style path to one: opening it costs a round trip to a server.
pub fn is_remote_path(path: &Path) -> bool {
    let text = cpl::native_path(path);
    // A URL's scheme (RFC 3986, 3.1): letters, digits, '+', '-', '.'; more
    // than one letter, so no drive ("C:") is taken for one.
    if let Some(scheme) = text.windows(3).position(|window| window == b"://")
        && scheme > 1
        && text[..scheme]
            .iter()
            .all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
    {
        return true;
    }
    REMOTE_PREFIXES
        .iter()
        .any(|prefix| text.starts_with(prefix.as_bytes()))
}

/// True for a name GDAL opens that is no file on this disk: a /vsi path
/// (/vsizip/, /vsicurl/ ...), a URL (https://, s3:// ...), or a connection
/// string one of GDAL's drivers takes ("PG:dbname=..."). Whether one exists
/// is GDAL's to find out.
pub fn is_virtual_path(path: &Path) -> bool {
    let text = cpl::native_path(path);
    if text.starts_with(b"/vsi") || is_remote_path(path) {
        return true;
    }
    // A connection string: a prefix of two letters or more, so a Windows
    // drive ("C:") is never taken for one.
    formats().iter().any(|format| {
        let prefix = format.connection_prefix.as_bytes();
        prefix.len() > 2
            && text.len() >= prefix.len()
            && text[..prefix.len()].eq_ignore_ascii_case(prefix)
    })
}

/// What a dataset holds, found by GDAL: the driver that recognises it, and
/// whether it holds rasters (bands, or raster subdatasets), vector layers or
/// both. The driver's identification alone decides where the driver holds
/// one kind only; a driver of both kinds (GeoPackage, MBTiles, PDF, a file
/// geodatabase) has the dataset opened to see which it holds. An archive GDAL
/// does not open as it is (a .zip of a shapefile) is looked into, and `name`
/// says how it is opened. NotFound for a local path that does not exist;
/// FileImportFailure, with GDAL's message, for a file no driver recognises.
pub fn identify_content(path: &Path) -> Result<Content> {
    ensure_registered();
    if !is_virtual_path(path) && !path_exists(path).unwrap_or(false) {
        return Err(make_error(
            ErrorCode::NotFound,
            "file does not exist",
            cpl::path_text(path),
        ));
    }
    let names = open_names(path);
    let _quiet = quiet_errors();
    let mut reason = String::new();
    for name in &names.names {
        let name_c = cpl::c_string(name);
        // SAFETY: a NUL-terminated name and no lists; the driver returned
        // lives as long as the process.
        let identified = unsafe {
            gdal::GDALIdentifyDriverEx(
                name_c.as_ptr(),
                gdal::GDAL_OF_RASTER | gdal::GDAL_OF_VECTOR,
                ptr::null(),
                ptr::null(),
            )
        };
        if identified.is_null() {
            if reason.is_empty() {
                reason = last_error_message();
            }
            continue;
        }
        let mut content = Content {
            driver: description_of(identified.cast()),
            name: String::from_utf8_lossy(name).into_owned(),
            raster: false,
            vector: false,
        };
        let raster = flag(identified, gdal::GDAL_DCAP_RASTER);
        let vector = flag(identified, gdal::GDAL_DCAP_VECTOR);
        if raster != vector {
            // A driver of one kind: its word is enough, and a large file is
            // not read twice (GeoJSON reads a whole file to open it).
            content.raster = raster;
            content.vector = vector;
            return Ok(content);
        }
        // SAFETY: a NUL-terminated name and no lists; the dataset returned
        // is closed below.
        let dataset = unsafe {
            gdal::GDALOpenEx(
                name_c.as_ptr(),
                gdal::GDAL_OF_READONLY | gdal::GDAL_OF_RASTER | gdal::GDAL_OF_VECTOR,
                ptr::null(),
                ptr::null(),
                ptr::null(),
            )
        };
        if dataset.is_null() {
            let message = last_error_message();
            if !message.is_empty() {
                reason = message;
            }
            continue;
        }
        // SAFETY: a live dataset.
        let driver = unsafe { gdal::GDALGetDatasetDriver(dataset) };
        if !driver.is_null() {
            content.driver = description_of(driver.cast());
        }
        content.raster = holds_raster(dataset);
        // SAFETY: a live dataset, then closed once.
        unsafe {
            content.vector = gdal::GDALDatasetGetLayerCount(dataset) > 0;
            gdal::GDALClose(dataset);
        }
        return Ok(content);
    }
    if !names.ambiguity.is_empty() {
        return Err(make_error(
            ErrorCode::InvalidArgument,
            names.ambiguity,
            cpl::path_text(path),
        ));
    }
    Err(make_error(
        ErrorCode::FileImportFailure,
        format!("GDAL recognises no format in '{}'", cpl::path_text(path)),
        reason,
    ))
}
