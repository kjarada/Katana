//! Which renderer a 3D view uses (renderer_choice): every rule, in order, as a
//! pure function - no GPU, window or environment variable. The twin of
//! tests/gpu/test_renderer_choice.cpp; the C++'s Direct3D 11 build is the
//! port's Direct3D 12 one, the default backend of a RendererEnvironment as
//! Direct3D 11 was the C++'s.

use katana_gpu::renderer_choice::{GpuBackend, RendererEnvironment, RendererKind, choose_renderer};

// Everything in favour of the GPU; each test takes one thing away.
fn desktop() -> RendererEnvironment {
    RendererEnvironment {
        gpu_built: true,
        platform_name: String::from("windows"),
        ..RendererEnvironment::default()
    }
}

mod renderer_choice {
    use super::*;

    #[test]
    fn the_windows_desktop_with_nothing_against_it_gets_the_gpu() {
        assert_eq!(choose_renderer(&desktop()).kind, RendererKind::Gpu);
        let mut asked = desktop();
        asked.renderer_override = String::from("gpu");
        assert_eq!(choose_renderer(&asked).kind, RendererKind::Gpu);
    }

    #[test]
    fn headless_platforms_get_the_software_rasteriser() {
        for platform in ["offscreen", "minimal", "", "xcb"] {
            let mut environment = desktop();
            environment.platform_name = String::from(platform);
            let decision = choose_renderer(&environment);
            assert_eq!(decision.kind, RendererKind::Software, "{platform}");
            assert!(decision.reason.contains("platform"), "{}", decision.reason);
        }
    }

    #[test]
    fn the_environment_variable_forces_software_in_any_case() {
        let mut environment = desktop();
        environment.renderer_override = String::from("Software");
        let decision = choose_renderer(&environment);
        assert_eq!(decision.kind, RendererKind::Software);
        assert!(decision.reason.contains("KATANA_RENDERER"));
    }

    #[test]
    fn asking_for_the_gpu_cannot_override_a_platform_that_cannot_show_it() {
        let mut environment = desktop();
        environment.platform_name = String::from("offscreen");
        environment.renderer_override = String::from("gpu");
        assert_eq!(choose_renderer(&environment).kind, RendererKind::Software);
    }

    #[test]
    fn the_users_setting_and_an_earlier_failure_each_choose_software() {
        let mut preferred = desktop();
        preferred.user_prefers_software = true;
        assert_eq!(choose_renderer(&preferred).kind, RendererKind::Software);

        let mut failed = desktop();
        failed.gpu_failed_this_session = true;
        let decision = choose_renderer(&failed);
        assert_eq!(decision.kind, RendererKind::Software);
        assert!(decision.reason.contains("failed"));
    }

    #[test]
    fn a_copy_built_without_the_gpu_renderer_always_uses_software() {
        let mut environment = desktop();
        environment.gpu_built = false;
        environment.renderer_override = String::from("gpu");
        assert_eq!(choose_renderer(&environment).kind, RendererKind::Software);
    }

    #[test]
    fn an_unknown_override_is_ignored_and_said_so() {
        let mut environment = desktop();
        environment.renderer_override = String::from("vulcan");
        let decision = choose_renderer(&environment);
        assert_eq!(decision.kind, RendererKind::Gpu);
        assert!(decision.reason.contains("vulcan"), "{}", decision.reason);
    }

    // The Vulkan build (Linux) is shown by the X11 and Wayland platforms, and
    // by nothing the Direct3D build is shown by.
    #[test]
    fn the_vulkan_build_is_shown_on_xcb_and_wayland_only() {
        for platform in ["xcb", "wayland", "XCB"] {
            let mut environment = desktop();
            environment.backend = GpuBackend::Vulkan;
            environment.platform_name = String::from(platform);
            let decision = choose_renderer(&environment);
            assert_eq!(decision.kind, RendererKind::Gpu, "{platform}");
            assert!(decision.reason.contains("Vulkan"), "{}", decision.reason);
        }
        for platform in ["windows", "offscreen", "minimal", "vnc", "eglfs", ""] {
            let mut environment = desktop();
            environment.backend = GpuBackend::Vulkan;
            environment.platform_name = String::from(platform);
            let decision = choose_renderer(&environment);
            assert_eq!(decision.kind, RendererKind::Software, "{platform}");
            assert!(decision.reason.contains("Vulkan"), "{}", decision.reason);
        }
    }

    // The Metal build (macOS) is shown by the cocoa platform, and by nothing
    // the Direct3D or Vulkan builds are shown by.
    #[test]
    fn the_metal_build_is_shown_on_cocoa_only() {
        for platform in ["cocoa", "Cocoa"] {
            let mut environment = desktop();
            environment.backend = GpuBackend::Metal;
            environment.platform_name = String::from(platform);
            let decision = choose_renderer(&environment);
            assert_eq!(decision.kind, RendererKind::Gpu, "{platform}");
            assert!(decision.reason.contains("Metal"), "{}", decision.reason);
        }
        for platform in ["windows", "xcb", "wayland", "offscreen", "minimal", ""] {
            let mut environment = desktop();
            environment.backend = GpuBackend::Metal;
            environment.platform_name = String::from(platform);
            let decision = choose_renderer(&environment);
            assert_eq!(decision.kind, RendererKind::Software, "{platform}");
            assert!(decision.reason.contains("Metal"), "{}", decision.reason);
        }
    }

    // Only an explicit KATANA_RENDERER=gpu lets the view draw on a software
    // device; the default refuses one, and the software choice never carries
    // it.
    #[test]
    fn only_asking_for_the_gpu_allows_a_software_device() {
        assert!(!choose_renderer(&desktop()).software_device_allowed);
        let mut asked = desktop();
        asked.renderer_override = String::from("GPU");
        assert!(choose_renderer(&asked).software_device_allowed);
        asked.platform_name = String::from("offscreen");
        assert!(!choose_renderer(&asked).software_device_allowed);
    }
}
