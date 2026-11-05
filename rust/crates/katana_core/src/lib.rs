//! The `core` layer: errors and results, text and its encodings, logging, the task pool, SHA-256, the strict XML reader.
//!
//! The port of src/katana_core and include/katana/core.

pub mod cpu_features;
pub mod error;
pub mod library_data;
pub mod log;
pub mod path_text;
pub mod sha256;
pub mod simd;
pub mod task_pool;
pub mod text;
pub mod text_encoding;
pub mod text_kernels;
pub mod xml;

pub use error::{Error, ErrorCode, Result, Status};
