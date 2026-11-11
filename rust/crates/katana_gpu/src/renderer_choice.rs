//! Which renderer a 3D view should use: the GPU one or the software rasteriser
//! (docs/gpu.md, "Fallback"; renderer_choice.hpp).
//!
//! The software rasteriser is not going away. It is the only renderer of a
//! headless run, where every test and every headless screenshot runs; it is
//! the bit-exact reference the GPU is tested against; and it is what a machine
//! whose GPU or driver misbehaves falls back to. So a view that can host both
//! asks this function, once when it is built and again whenever something
//! below changes, and follows the answer.
//!
//! THE RULES, first match wins:
//!
//! ```text
//! 1. The GPU renderer was not built                       -> software
//! 2. KATANA_RENDERER=software in the environment          -> software
//! 3. The user's setting asks for software                 -> software
//! 4. The GPU view already failed in this session          -> software
//!    (the view could not render, or GpuRenderer::initialise failed): a
//!    renderer that failed once is not retried behind the user's back.
//! 5. The platform cannot show the build's GPU view        -> software
//!    (offscreen and minimal - headless runs - and, for the Direct3D 12
//!    build, anything but the windows platform; for the Vulkan build,
//!    anything but xcb and wayland; for the Metal build, anything but cocoa)
//! 6. Otherwise                                            -> GPU
//!    (KATANA_RENDERER=gpu cannot make rules 1-5 go away, and the GPU is
//!    already the default. What it does add is permission to draw on a
//!    software device - WARP, lavapipe - which the view otherwise refuses:
//!    RendererDecision::software_device_allowed.)
//! ```
//!
//! Any other value of KATANA_RENDERER is ignored and reported in the reason,
//! so a typo is visible in the log rather than silently meaning "gpu".
//!
//! The decision is a PURE function of RendererEnvironment, so every rule is
//! tested without a GPU, a window or an environment variable;
//! current_renderer_environment() is the one place that reads the real ones.
//!
//! THE PLATFORM NAMES are Qt's, kept as the vocabulary of the rule: "windows",
//! "xcb" (X11), "wayland" and "cocoa" for the desktops, "offscreen" (and Qt's
//! "minimal") for a headless run. The window names its platform in these
//! words: a headless run is "offscreen", the desktop the one winit runs on.

use crate::names::EnumName;
use katana_core::text::trimmed;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum RendererKind {
    Gpu,
    #[default]
    Software,
}

impl EnumName for RendererKind {
    fn name(self) -> &'static str {
        match self {
            RendererKind::Gpu => "GPU",
            RendererKind::Software => "software",
        }
    }
}

/// The graphics API the GPU renderer draws with in this build: wgpu's native
/// backend for the platform, which decides which platforms can show it. The
/// C++ drew with Direct3D 11 on Windows (QRhi's); wgpu's Windows backend is
/// Direct3D 12, whose software device is WARP as Direct3D 11's was. GLES is
/// not built (docs/rust.md, "Third-party libraries").
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum GpuBackend {
    #[default]
    Direct3D12,
    Vulkan,
    Metal,
}

impl EnumName for GpuBackend {
    fn name(self) -> &'static str {
        match self {
            GpuBackend::Direct3D12 => "Direct3D 12",
            GpuBackend::Vulkan => "Vulkan",
            GpuBackend::Metal => "Metal",
        }
    }
}

impl GpuBackend {
    /// The backend this copy was built to draw with.
    pub const fn built() -> GpuBackend {
        if cfg!(windows) {
            GpuBackend::Direct3D12
        } else if cfg!(target_vendor = "apple") {
            GpuBackend::Metal
        } else {
            GpuBackend::Vulkan
        }
    }

    /// The wgpu backends an instance of this backend is made with.
    pub fn backends(self) -> wgpu::Backends {
        match self {
            GpuBackend::Direct3D12 => wgpu::Backends::DX12,
            GpuBackend::Vulkan => wgpu::Backends::VULKAN,
            GpuBackend::Metal => wgpu::Backends::METAL,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RendererEnvironment {
    /// False when the GPU renderer is not compiled in.
    pub gpu_built: bool,
    pub backend: GpuBackend,
    /// The platform's name, in the module's vocabulary: "windows",
    /// "offscreen", "minimal", ...
    pub platform_name: String,
    /// The value of KATANA_RENDERER, empty when unset.
    pub renderer_override: String,
    /// The user's own choice (a preference), true to force the software path.
    pub user_prefers_software: bool,
    /// A GPU view has failed to render in this session.
    pub gpu_failed_this_session: bool,
}

impl Default for RendererEnvironment {
    fn default() -> Self {
        Self {
            gpu_built: true,
            backend: GpuBackend::default(),
            platform_name: String::new(),
            renderer_override: String::new(),
            user_prefers_software: false,
            gpu_failed_this_session: false,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RendererDecision {
    pub kind: RendererKind,
    /// One sentence for the log and the status line: why this renderer.
    pub reason: String,
    /// The GPU view may draw on a software device (WARP, lavapipe) - only when
    /// KATANA_RENDERER=gpu asks for the GPU path whatever it runs on. Without
    /// that such a device is refused and the rasteriser draws instead: it was
    /// written to do the CPU's work well, and a software GPU device was not.
    pub software_device_allowed: bool,
}

fn software(reason: String) -> RendererDecision {
    RendererDecision {
        kind: RendererKind::Software,
        reason,
        software_device_allowed: false,
    }
}

pub fn choose_renderer(environment: &RendererEnvironment) -> RendererDecision {
    let requested = environment.renderer_override.to_ascii_lowercase();
    let request_known = requested.is_empty() || requested == "gpu" || requested == "software";
    // Appended to whatever reason wins, so a mistyped value is never silent.
    let ignored = if request_known {
        String::new()
    } else {
        format!(
            " (KATANA_RENDERER={} is not 'gpu' or 'software' and was ignored)",
            environment.renderer_override
        )
    };

    if !environment.gpu_built {
        return software(format!(
            "the GPU renderer is not built into this copy{ignored}"
        ));
    }
    if requested == "software" {
        return software(String::from(
            "KATANA_RENDERER=software asks for the software renderer",
        ));
    }
    if environment.user_prefers_software {
        return software(format!(
            "the software renderer is chosen in the settings{ignored}"
        ));
    }
    if environment.gpu_failed_this_session {
        return software(format!(
            "the GPU renderer failed earlier in this session{ignored}"
        ));
    }
    let platform = environment.platform_name.to_ascii_lowercase();
    let shown = match environment.backend {
        GpuBackend::Direct3D12 => platform == "windows",
        GpuBackend::Vulkan => platform == "xcb" || platform == "wayland",
        GpuBackend::Metal => platform == "cocoa",
    };
    let api = environment.backend.name();
    if !shown {
        let name = if platform.is_empty() {
            "none"
        } else {
            platform.as_str()
        };
        return software(format!(
            "the '{name}' platform cannot show the {api} view{ignored}"
        ));
    }
    RendererDecision {
        kind: RendererKind::Gpu,
        reason: format!("{api} on the {platform} platform{ignored}"),
        software_device_allowed: requested == "gpu",
    }
}

/// The environment as it is now: the backend this copy was built for, the
/// platform the caller names (the C++ read QGuiApplication's; the gpu layer
/// cannot see the window, so the window says), KATANA_RENDERER from the
/// process environment, with surrounding white space trimmed. The two flags
/// are the caller's to know.
pub fn current_renderer_environment(
    user_prefers_software: bool,
    gpu_failed_this_session: bool,
    platform_name: &str,
) -> RendererEnvironment {
    let value = std::env::var_os("KATANA_RENDERER").unwrap_or_default();
    let value = value.to_string_lossy();
    RendererEnvironment {
        // This module is only compiled into the GPU layer.
        gpu_built: true,
        backend: GpuBackend::built(),
        platform_name: String::from(platform_name),
        renderer_override: String::from_utf8_lossy(trimmed(value.as_bytes())).into_owned(),
        user_prefers_software,
        gpu_failed_this_session,
    }
}
