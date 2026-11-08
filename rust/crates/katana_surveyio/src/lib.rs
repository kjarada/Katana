//! The `surveyio` layer: instrument field files read into survey values.
//!
//! The port of src/katana_surveyio and include/katana/surveyio (docs/rust.md).

mod delimited_layout;
pub mod delimited_points;
mod delimited_proposal;
mod delimited_records;
pub mod detect;
pub mod format;
pub mod leica;
mod leica_dbx;
mod leica_gsi;
mod opcode_field_file;
pub mod reader;
pub mod rinex;
mod rinex_compact;
#[doc(hidden)]
pub mod rinex_internal;
mod rinex_navigation;
mod sokkia_sdr;
mod topcon_gts;
mod topcon_raw_builder;
mod topcon_rw5;
mod trimble_dc;
mod trimble_jobxml;
mod xml_pull;

pub use delimited_points::{
    ColumnPreset, ColumnRole, DELIMITED_HEADER_CONFIDENCE, DELIMITED_HEADERLESS_CONFIDENCE,
    DELIMITED_POINTS_FORMAT_ID, DelimitedExportOptions, DelimitedImportOptions, DelimitedLayout,
    Delimiter, LayoutCandidate, LayoutProposal, LayoutProposalOutcome, MAXIMUM_EXPORT_DECIMALS,
    PROPOSAL_SAMPLE_LINES, Quoting, all_column_presets, column_preset_named,
    delimited_points_format, header_name, layout_template, parse_delimited_points,
    parse_layout_template, preset_columns, preset_layout, probe_delimited_points, propose_layout,
    template_letter, validate_layout, write_delimited_points,
};
pub use detect::{
    Detection, DetectionOutcome, FormatCandidate, IDENTIFICATION_MARGIN, IDENTIFIED_CONFIDENCE,
    PROBE_BYTES, detect_file, detect_file_with, detect_format, detect_format_with, looks_like_text,
    probe_lines, probe_of, without_byte_order_mark,
};
pub use format::{
    FormatContent, FormatDescriptor, FormatProbe, FormatReader, FormatRegistration, FormatRegistry,
    FormatSignature, ImportResult, Manufacturer, Named, ProbeInput, ProbeResult, ReadOptions,
    ReadResult, ReadWarning, SiblingFile, SiblingLookup, describe, describe_format,
    format_registry, ruled_out, to_string,
};
pub use leica::{LEICA_DBX_FORMAT_ID, LEICA_GSI_FORMAT_ID};
pub use reader::{
    MAX_SIBLING_FILES, MAX_SURVEY_FILE_BYTES, read_survey, report_input_for, siblings_in_folder,
    siblings_in_memory, to_import_result,
};
pub use rinex::{RINEX_APPROXIMATE_POSITION_SIGMA, RINEX_OBSERVATION_FORMAT_ID};
