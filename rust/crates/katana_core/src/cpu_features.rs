use crate::{Error, ErrorCode, Result};
use std::fmt;
use std::io::Write;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(u8)]
pub enum SimdLevel {
    #[default]
    Scalar,
    Avx2,
    Neon,
}

pub const fn to_string(level: SimdLevel) -> &'static str {
    match level {
        SimdLevel::Scalar => "scalar",
        SimdLevel::Avx2 => "avx2",
        SimdLevel::Neon => "neon",
    }
}

impl fmt::Display for SimdLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(to_string(*self))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SimdSelection {
    pub detected: SimdLevel,
    pub active: SimdLevel,
    pub note: String,
}

fn probe_processor() -> SimdLevel {
    #[cfg(all(
        feature = "simd-kernels",
        any(target_arch = "x86", target_arch = "x86_64")
    ))]
    {
        if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
            SimdLevel::Avx2
        } else {
            SimdLevel::Scalar
        }
    }
    #[cfg(all(feature = "simd-kernels", target_arch = "aarch64"))]
    {
        SimdLevel::Neon
    }
    #[cfg(not(all(
        feature = "simd-kernels",
        any(target_arch = "x86", target_arch = "x86_64", target_arch = "aarch64")
    )))]
    {
        SimdLevel::Scalar
    }
}

pub fn choose_simd_level(requested: &str, detected: SimdLevel) -> Result<SimdLevel> {
    let level = match requested {
        "" => return Ok(detected),
        "scalar" => SimdLevel::Scalar,
        "avx2" => SimdLevel::Avx2,
        "neon" => SimdLevel::Neon,
        _ => {
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                format!("'{requested}' is not a SIMD level; use scalar, avx2 or neon"),
            ));
        }
    };
    if level != SimdLevel::Scalar && level != detected {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            format!("{level} was asked for but this processor supports only {detected}"),
        ));
    }
    Ok(level)
}

pub fn simd_selection() -> &'static SimdSelection {
    static SELECTION: OnceLock<SimdSelection> = OnceLock::new();
    SELECTION.get_or_init(|| {
        let detected = probe_processor();
        let mut selection = SimdSelection {
            detected,
            active: detected,
            note: String::new(),
        };
        let requested = std::env::var_os("KATANA_SIMD").unwrap_or_default();
        let requested = requested.to_string_lossy();
        if requested.is_empty() {
            return selection;
        }
        match choose_simd_level(&requested, detected) {
            Ok(chosen) => {
                selection.active = chosen;
                if chosen != detected {
                    selection.note = format!(
                        "KATANA_SIMD={requested} in force; the processor supports {detected}"
                    );
                }
            }
            Err(error) => {
                selection.note = format!("KATANA_SIMD ignored: {}", error.message);
                let _ = writeln!(std::io::stderr().lock(), "katana: {}", selection.note);
            }
        }
        selection
    })
}

pub fn detected_simd_level() -> SimdLevel {
    simd_selection().detected
}

fn active_level() -> &'static AtomicU8 {
    static ACTIVE: OnceLock<AtomicU8> = OnceLock::new();
    ACTIVE.get_or_init(|| AtomicU8::new(simd_selection().active as u8))
}

fn from_byte(level: u8) -> SimdLevel {
    match level {
        1 => SimdLevel::Avx2,
        2 => SimdLevel::Neon,
        _ => SimdLevel::Scalar,
    }
}

pub fn active_simd_level() -> SimdLevel {
    from_byte(active_level().load(Ordering::Relaxed))
}

pub fn set_simd_level(level: SimdLevel) -> Result<SimdLevel> {
    let allowed = choose_simd_level(to_string(level), detected_simd_level())?;
    Ok(from_byte(
        active_level().swap(allowed as u8, Ordering::Relaxed),
    ))
}
