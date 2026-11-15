//! Archive files - .12da, and the zipped .12daz - in and out
//! (include/katana/interop/archive12d.hpp, src/katana_interop/archive12d.cpp).
//!
//! The format itself lives in katana_archive12d, which needs no third-party
//! library and so can be built and sanitized without GDAL. This is only the
//! part that touches a FILE: reading its bytes, opening the ZIP container of
//! a .12daz (katana_gis::zip_container), and turning the point clouds an
//! archive can carry into reference layers, which katana_archive12d cannot
//! see.
//!
//! An archive is not a vector file like the others. One import can bring
//! entities, the layers they sit on, named alignments, surfaces and point
//! clouds, and they do not all live in the same place: entities, layers and
//! alignments go through commands and are undoable; surfaces and clouds are
//! session data the front end owns. So this returns them separately and the
//! caller places each.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use katana_archive12d::{
    ElementTally, ExportMesh, ExportOptions, ExportSurface, ImportOptions, ImportedCloud,
    ImportedMesh, ImportedSurface, ReadOptions, WriteOptions, from_domain, read_archive, to_domain,
    write_archive,
};
use katana_core::error::make_error;
use katana_core::text_encoding::{self, decode_text, encode_utf16_little_endian};
use katana_core::{ErrorCode, Result};
use katana_entity::{Alignment, ColourTable, Entity, EntityId, Layer, Model, Style};
use katana_geometry::{Box2, Vec2};
use katana_gis::zip_container::{list_zip, read_zip_member, write_zip};
use katana_math::cpp;
use katana_pointcloud::PointCloudPoint;

use crate::import::{PointCloudImportOptions, import_point_cloud};
use crate::reference_data::PointCloudLayer;

fn lower_extension(path: &Path) -> String {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// A path as UTF-8. What comes out of here goes into entity metadata, where
/// bytes that are not UTF-8 are refused for the whole import, and to GDAL,
/// which expects UTF-8 too.
fn utf8(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `path.string()`, for an error's context.
fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn file_name(path: &Path) -> &Path {
    path.file_name().map_or(Path::new(""), Path::new)
}

fn read_whole_file(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    // std::filesystem::file_size, which fails for what is not a regular file.
    let size = match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => metadata.len(),
        _ => {
            return Err(make_error(
                ErrorCode::FileImportFailure,
                "the file cannot be read",
                path_string(path),
            ));
        }
    };
    if size > max_bytes {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            format!("the file is larger than the limit of {max_bytes} bytes"),
            path_string(path),
        ));
    }
    let Ok(file) = File::open(path) else {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "the file cannot be opened",
            path_string(path),
        ));
    };
    let mut bytes = Vec::with_capacity(size as usize);
    match file.take(size).read_to_end(&mut bytes) {
        Ok(_) if bytes.len() as u64 == size => Ok(bytes),
        _ => Err(make_error(
            ErrorCode::FileImportFailure,
            "the file could not be read to its end",
            path_string(path),
        )),
    }
}

/// The .12da inside a .12daz. The archive's own program names it after the
/// project, NOT after the zip - "UT5527 Appin Rd V5.12daz" holds "Appin Rd
/// V5.12da" - so the name cannot be predicted: it is the one member that is
/// a .12da.
fn read_zipped_archive(path: &Path, max_bytes: u64, member_name: &mut String) -> Result<Vec<u8>> {
    let members = list_zip(path)?;
    let candidates: Vec<String> = members
        .iter()
        .filter(|member| lower_extension(Path::new(&member.name)) == "12da")
        .map(|member| member.name.clone())
        .collect();
    if candidates.is_empty() {
        return Err(make_error(
            ErrorCode::FileImportFailure,
            "the archive holds no .12da file",
            path_string(path),
        ));
    }
    if candidates.len() > 1 {
        // Which one was meant is not ours to guess; importing the first and
        // saying nothing would silently drop the rest.
        return Err(make_error(
            ErrorCode::Unsupported,
            format!(
                "the archive holds {} .12da files; unzip it and import them one at a time",
                candidates.len()
            ),
            path_string(path),
        ));
    }
    member_name.clone_from(&candidates[0]);
    read_zip_member(path, member_name, max_bytes)
}

fn to_layer(cloud: &ImportedCloud, source: &Path) -> PointCloudLayer {
    let mut layer = PointCloudLayer {
        name: cloud.name.clone(),
        source: source.to_path_buf(),
        ..PointCloudLayer::default()
    };
    layer.points.reserve(cloud.points.len());
    for point in &cloud.points {
        layer.points.push(PointCloudPoint {
            x: point.x,
            y: point.y,
            z: point.z,
            intensity: f64::from(point.intensity),
            classification: point.classification,
            ..PointCloudPoint::default()
        });
        let bounds = &mut layer.bounds;
        bounds.min_x = cpp::min(bounds.min_x, point.x);
        bounds.min_y = cpp::min(bounds.min_y, point.y);
        bounds.min_z = cpp::min(bounds.min_z, point.z);
        bounds.max_x = cpp::max(bounds.max_x, point.x);
        bounds.max_y = cpp::max(bounds.max_y, point.y);
        bounds.max_z = cpp::max(bounds.max_z, point.z);
    }
    layer.source_point_count = cloud.points.len() as u64;
    layer
}

/// The LAS file a ref_data cloud names, if it sits beside the archive. Only
/// the file NAME is taken from the reference: an archive records something
/// like "..\..\scans\site.las", and following that out of the directory the
/// archive was found in would let a file choose what gets read.
fn referenced_cloud(archive: &Path, reference: &str) -> Option<PointCloudLayer> {
    if reference.is_empty() {
        return None;
    }
    // The name after the last separator of EITHER kind: the reference was
    // written on Windows, and a path on any other system reads a backslash as
    // part of the name, so "..\..\scans\site.las" would be one long file name
    // that is never beside the archive.
    let name = match reference.rfind(['/', '\\']) {
        Some(separator) => &reference[separator + 1..],
        None => reference,
    };
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    let beside = archive.parent().unwrap_or(Path::new("")).join(name);
    if !beside.is_file() {
        return None;
    }
    import_point_cloud(&beside, &PointCloudImportOptions::default()).ok()
}

/// Without the leading dot: 12da and 12daz. There is no ".12dz"; it was
/// accepted for a while by mistake.
pub fn archive12d_extensions() -> Vec<String> {
    vec![String::from("12da"), String::from("12daz")]
}

pub fn is_zipped_archive12d(path: &Path) -> bool {
    lower_extension(path) == "12daz"
}

#[derive(Clone, Debug, PartialEq)]
pub struct Archive12dImportOptions {
    /// Subtracted from every plan coordinate. See VectorImportOptions.
    pub origin_shift: Option<Vec2>,
    /// Parent layer for every model; empty puts models at the top level.
    pub layer_prefix: String,
    pub curve_tolerance: f64,
    /// The largest file, or zip member once inflated, that will be read. An
    /// archive is text and is held in memory whole; the biggest real export
    /// seen is 58 MB (29 MB of text once decoded). A gigabyte is room for
    /// anything plausible and a bound on what a hostile zip can ask for.
    pub max_bytes: u64,
    pub colours: ColourTable,
}

impl Default for Archive12dImportOptions {
    fn default() -> Self {
        Self {
            origin_shift: None,
            layer_prefix: String::new(),
            curve_tolerance: 0.001,
            max_bytes: 1_u64 << 30,
            colours: ColourTable::default(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Archive12dImportResult {
    pub entities: Vec<Entity>,
    pub layers_needed: Vec<Layer>,
    /// One per distinct linestyle the archive names; the caller creates those
    /// the drawing lacks, in the same transaction as the layers.
    pub styles_needed: Vec<Style>,
    /// Names are unique within the import; the caller must still rename any
    /// that the drawing already has, or create_alignment will refuse it.
    pub alignments: Vec<Alignment>,
    pub surfaces: Vec<ImportedSurface>,
    /// Trimeshes: session data like the surfaces, held by the caller.
    pub meshes: Vec<ImportedMesh>,
    pub clouds: Vec<PointCloudLayer>,
    pub tally: Vec<ElementTally>,

    pub bounds: Box2,
    /// As detected: "UTF-16 little-endian" ...
    pub encoding: String,
    /// The writing program's archive_version, when the file says.
    pub archive_version: String,
    /// The .12da inside a .12daz; empty for a plain file.
    pub member_name: String,
    pub warnings: Vec<String>,
}

pub fn import_archive12d(
    path: &Path,
    options: &Archive12dImportOptions,
) -> Result<Archive12dImportResult> {
    let mut result = Archive12dImportResult::default();
    let bytes = if is_zipped_archive12d(path) {
        read_zipped_archive(path, options.max_bytes, &mut result.member_name)
    } else {
        read_whole_file(path, options.max_bytes)
    }?;
    let decoded = decode_text(&bytes)
        .map_err(|error| make_error(error.code, error.message, path_string(path)))?;
    // A 58 MB UTF-16 file is not needed beside its decoded text.
    drop(bytes);
    result.encoding = String::from(text_encoding::to_string(decoded.encoding));

    let archive = read_archive(&decoded.text, &ReadOptions::default()).map_err(|error| {
        let at = if error.context.is_empty() {
            String::new()
        } else {
            format!(" [{}]", error.context)
        };
        make_error(
            error.code,
            error.message,
            format!("{}{at}", file_name(path).to_string_lossy()),
        )
    })?;
    for setting in &archive.header_settings {
        if setting.key == "archive_version" {
            result.archive_version.clone_from(&setting.value);
        }
    }

    let mapping = ImportOptions {
        colours: options.colours.clone(),
        origin_shift: options.origin_shift,
        layer_prefix: options.layer_prefix.clone(),
        curve_tolerance: options.curve_tolerance,
        source_name: utf8(file_name(path)),
        ..ImportOptions::default()
    };
    let domain = to_domain(&archive, &mapping)?;

    result.entities = domain.entities;
    result.layers_needed = domain.layers_needed;
    result.styles_needed = domain.styles_needed;
    result.alignments = domain.alignments;
    result.surfaces = domain.surfaces;
    result.meshes = domain.meshes;
    result.tally = domain.tally;
    result.bounds = domain.bounds;
    result.warnings = domain.warnings;
    if decoded.guessed {
        result.warnings.insert(
            0,
            format!(
                "the file has no byte order mark; it was read as {}",
                result.encoding
            ),
        );
    }
    for cloud in &domain.clouds {
        if cloud.points.is_empty() {
            // A ref_data cloud names a LAS file rather than holding points,
            // by a path relative to a project this archive has left behind.
            // The file is LOOKED FOR beside the archive - where an archive and
            // its scans travel together when they are sent on - and imported
            // when it is there. It is not chased any further than that: a
            // relative path out of a project directory that no longer exists
            // would have this reading arbitrary files from wherever the
            // archive happens to sit.
            if let Some(mut found) = referenced_cloud(path, &cloud.reference_file) {
                if !cloud.name.is_empty() {
                    found.name.clone_from(&cloud.name);
                }
                result.warnings.push(format!(
                    "point cloud \"{}\" was read from '{}', found beside the archive",
                    cloud.name, cloud.reference_file
                ));
                result.clouds.push(found);
                continue;
            }
            result.warnings.push(format!(
                "point cloud \"{}\" refers to the file '{}', which is not beside the archive; \
                 import it separately",
                cloud.name, cloud.reference_file
            ));
            continue;
        }
        result.clouds.push(to_layer(cloud, path));
    }
    for super_tin in &domain.super_tins {
        let members = super_tin.tins.join(", ");
        result.warnings.push(format!(
            "super tin \"{}\" ranks the tins {members}; they are imported as separate surfaces",
            super_tin.name
        ));
    }
    Ok(result)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Archive12dExportOptions {
    /// Empty: the whole model.
    pub entities: Vec<EntityId>,
    pub origin_shift: Option<Vec2>,
    /// UTF-16 little-endian with a byte order mark is what the archive's own
    /// program writes, and so what it is certain to read. UTF-8 is for a file
    /// meant to be read, or diffed, by people and other tools.
    pub utf16: bool,
    pub decimal_places: i32,
}

impl Default for Archive12dExportOptions {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            origin_shift: None,
            utf16: true,
            decimal_places: 8,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Archive12dExportResult {
    pub entities_written: usize,
    pub entities_skipped: usize,
    pub alignments_written: usize,
    pub surfaces_written: usize,
    pub bytes_written: u64,
    pub warnings: Vec<String>,
}

/// The extension decides the container: .12daz is zipped, holding one member
/// named after the file with the extension .12da, as the archive's own
/// program's are.
pub fn export_archive12d(
    model: &Model,
    surfaces: &[ExportSurface<'_>],
    path: &Path,
    options: &Archive12dExportOptions,
    meshes: &[ExportMesh<'_>],
) -> Result<Archive12dExportResult> {
    let extension = lower_extension(path);
    if !archive12d_extensions().contains(&extension) {
        // The vendor's name in a message, kept as the C++ has it until the
        // C++ line changes with it (CLAUDE.md section 9).
        return Err(make_error(
            ErrorCode::InvalidArgument,
            "an archive must be named .12da or .12daz",
            path_string(path),
        ));
    }

    let mapping = ExportOptions {
        entities: options.entities.clone(),
        origin_shift: options.origin_shift,
        ..ExportOptions::default()
    };
    let domain = from_domain(model, surfaces, &mapping, meshes)?;

    let writing = WriteOptions {
        decimal_places: options.decimal_places,
        banner: format!(
            "Written by Katana\nexport_file_name : {}",
            utf8(file_name(path))
        ),
        ..WriteOptions::default()
    };
    let text = write_archive(&domain.archive, &writing);
    let bytes = if options.utf16 {
        encode_utf16_little_endian(text.as_bytes()).map_err(|error| {
            make_error(
                ErrorCode::FileExportFailure,
                error.message,
                path_string(path),
            )
        })?
    } else {
        text.into_bytes()
    };

    if is_zipped_archive12d(path) {
        let member = file_name(path).with_extension("12da");
        write_zip(path, &utf8(&member), &bytes)?;
    } else if std::fs::write(path, &bytes).is_err() {
        return Err(make_error(
            ErrorCode::FileExportFailure,
            "the file could not be written",
            path_string(path),
        ));
    }

    Ok(Archive12dExportResult {
        entities_written: domain.entities_written,
        entities_skipped: domain.entities_skipped,
        alignments_written: domain.alignments_written,
        surfaces_written: domain.surfaces_written,
        bytes_written: bytes.len() as u64,
        warnings: domain.warnings,
    })
}
