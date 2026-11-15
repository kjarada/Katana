//! The GPU renderer drawing into a texture instead of a window (docs/gpu.md,
//! "Testing"; offscreen_gpu.hpp).
//!
//! wgpu renders into a texture with no window and no surface: on Linux through
//! Vulkan, on the real GPU or on Mesa's lavapipe (mesa-vulkan-drivers); on
//! Windows through Direct3D 12, on the real GPU or on WARP, Windows' software
//! device; on macOS through Metal. The C++'s Linux tests needed an X server
//! because Qt's offscreen platform could not make a Vulkan instance; these
//! need no display at all. An OffscreenGpu is a multisampled colour target
//! resolved into a texture, a 32-bit float depth texture, and a synchronous
//! frame with an optional read-back and the GPU's own timestamps for the
//! frame. The tests compare its pixels with the software rasteriser's; the 3D
//! view can show its texture.
//!
//! A GpuContext is the device it draws on. The C++ made a QRhi for every
//! target; a wgpu device can be shared, so a context is made once - by
//! GpuContext::request, or from the window's own device (from_parts) - and
//! any number of targets are made on it (create_on). The tests make one
//! context of each kind per process (tests/support/gpu_support.rs).

use crate::gpu_renderer::{
    FrameSettings, GpuFrameStats, GpuRenderer, RenderTarget, TargetFormat, guarded,
};
use crate::names::{EnumName, to_string};
use crate::renderer_choice::GpuBackend;
use katana_core::{Error, ErrorCode, Result, Status};
use katana_render::{Camera, Rgba, rgba_with_alpha};

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum GpuDevice {
    /// The machine's GPU.
    #[default]
    Hardware,
    /// The platform's software device: WARP on Windows, which every install
    /// has; lavapipe on Linux, Mesa's Vulkan on the CPU (mesa-vulkan-drivers).
    /// Slow, but a GPU-less machine can still run the tests on it. macOS has
    /// none.
    Software,
}

impl EnumName for GpuDevice {
    fn name(self) -> &'static str {
        match self {
            GpuDevice::Hardware => "hardware",
            GpuDevice::Software => "software",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OffscreenOptions {
    pub device: GpuDevice,
    /// 4 unless the device cannot; 1 turns multisampling off. WebGPU promises
    /// 1 and 4 for the colour and depth formats used here, and wgpu allows
    /// other counts only behind an adapter-specific feature, so any other
    /// count is taken as 1, as the C++ took a count the device did not list.
    pub sample_count: u32,
    /// Ask the GPU to time each frame (on a device with timestamp queries).
    pub timestamps: bool,
}

impl Default for OffscreenOptions {
    fn default() -> Self {
        Self {
            device: GpuDevice::Hardware,
            sample_count: 4,
            timestamps: false,
        }
    }
}

// QRhiTexture::RGBA8, the C++'s colour: unsigned normalised bytes with no sRGB
// conversion, so a read-back pixel is the byte the shader wrote, as the
// software rasteriser's is.
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
// QRhiTexture::D32F: reversed Z needs the float's precision near zero.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// A device to draw with, and the adapter it was made from. Cloning shares
/// the device: wgpu's handles are counted references.
#[derive(Clone, Debug)]
pub struct GpuContext {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

// Asked for the GPU and given a software device, say so rather than measure
// the wrong thing; asked for a software device and given hardware (a
// platform whose software device is not installed), say so rather than repeat
// the hardware run under the other name.
fn check_kind(info: &wgpu::AdapterInfo, kind: GpuDevice, api: &str) -> Status {
    let cpu_device = info.device_type == wgpu::DeviceType::Cpu;
    if kind == GpuDevice::Hardware && cpu_device {
        return Err(Error::new(
            ErrorCode::Unsupported,
            format!("only a software {api} device is available"),
        ));
    }
    if kind == GpuDevice::Software && !cpu_device {
        return Err(Error::new(
            ErrorCode::Unsupported,
            format!("no software {api} device ({} is hardware)", info.name),
        ));
    }
    Ok(())
}

impl GpuContext {
    /// A device of the kind asked for, on this build's backend: the machine's
    /// most capable GPU, or its software device (wgpu's fallback adapter,
    /// which is a CPU device: lavapipe, WARP). Fails with Unsupported when
    /// there is none - the tests SKIP on that, rather than fail.
    pub fn request(kind: GpuDevice) -> Result<GpuContext> {
        let backend = GpuBackend::built();
        let api = to_string(backend);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: backend.backends(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: kind == GpuDevice::Software,
            compatible_surface: None,
            apply_limit_buckets: false,
        };
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&options)) else {
            return Err(Error::new(
                ErrorCode::Unsupported,
                format!("no {api} device ({})", to_string(kind)),
            ));
        };
        check_kind(&adapter.get_info(), kind, api)?;
        // Timestamps where the adapter has them, for OffscreenOptions::timestamps.
        let features = if adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            wgpu::Features::TIMESTAMP_QUERY
        } else {
            wgpu::Features::empty()
        };
        let descriptor = wgpu::DeviceDescriptor {
            label: Some("katana"),
            required_features: features,
            // All the adapter offers, so a large view (up to the C++'s 16384
            // pixels) is limited by the device rather than by WebGPU's
            // defaults.
            required_limits: adapter.limits(),
            ..wgpu::DeviceDescriptor::default()
        };
        match pollster::block_on(adapter.request_device(&descriptor)) {
            Ok((device, queue)) => Ok(GpuContext {
                adapter,
                device,
                queue,
            }),
            Err(error) => Err(Error::new(
                ErrorCode::Unsupported,
                format!("no {api} device ({}): {error}", to_string(kind)),
            )),
        }
    }

    /// A device made elsewhere - the window's, which egui-wgpu made - with the
    /// adapter it came from.
    pub fn from_parts(adapter: wgpu::Adapter, device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self {
            adapter,
            device,
            queue,
        }
    }

    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// The adapter's name, for test and benchmark output.
    pub fn device_name(&self) -> String {
        self.adapter.get_info().name
    }

    /// Whether the device runs on the CPU (lavapipe, WARP).
    pub fn is_software(&self) -> bool {
        self.adapter.get_info().device_type == wgpu::DeviceType::Cpu
    }

    // Whether both of the target's formats take `count` samples a pixel.
    fn sample_count_supported(&self, count: u32) -> bool {
        (count == 1 || count == 4)
            && self
                .adapter
                .get_texture_format_features(COLOR_FORMAT)
                .flags
                .sample_count_supported(count)
            && self
                .adapter
                .get_texture_format_features(DEPTH_FORMAT)
                .flags
                .sample_count_supported(count)
    }
}

// The GPU's start and end times of each frame's pass.
#[derive(Debug)]
struct Timer {
    queries: wgpu::QuerySet,
    resolved: wgpu::Buffer,
    read: wgpu::Buffer,
}

const TIMESTAMP_BYTES: wgpu::BufferAddress = 2 * wgpu::QUERY_SIZE as wgpu::BufferAddress;

#[derive(Debug)]
pub struct OffscreenGpu {
    context: GpuContext,
    width: i32,
    height: i32,
    sample_count: u32,
    // What is read back, and what the view shows.
    resolved: wgpu::Texture,
    resolved_view: wgpu::TextureView,
    // What is drawn into, when multisampled.
    msaa_view: Option<wgpu::TextureView>,
    // Depth32Float, reversed Z.
    depth_view: wgpu::TextureView,
    // Rows of the colour texture, each padded to wgpu's 256-byte copy pitch.
    readback: wgpu::Buffer,
    row_pitch: u32,
    timer: Option<Timer>,
    renderer: GpuRenderer,
    last_gpu_ms: f64,
}

fn check_size(width: i32, height: i32) -> Status {
    if width <= 0 || height <= 0 || width > 16384 || height > 16384 {
        return Err(Error::new(
            ErrorCode::InvalidArgument,
            "offscreen target size out of range",
        ));
    }
    Ok(())
}

fn failure(message: &str) -> Error {
    Error::new(ErrorCode::RenderingFailure, message)
}

// Maps `buffer` for reading, waits for the GPU, and copies its bytes out.
fn read_back(device: &wgpu::Device, buffer: &wgpu::Buffer) -> std::result::Result<Vec<u8>, String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            // The receiver waits below; a send can fail only once it is gone.
            let _ = sender.send(result);
        });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| format!("{error}"))?;
    match receiver.recv() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Err(format!("{error}")),
        Err(_) => return Err(String::from("the buffer was never mapped")),
    }
    let bytes = match buffer.slice(..).get_mapped_range() {
        Ok(view) => view.to_vec(),
        Err(error) => {
            buffer.unmap();
            return Err(format!("{error}"));
        }
    };
    buffer.unmap();
    Ok(bytes)
}

impl OffscreenGpu {
    /// A target on a device of its own. Fails with Unsupported when there is
    /// no device of the kind asked for - the tests SKIP on that, rather than
    /// fail - with InvalidArgument for a size outside 1 to 16384, and with
    /// RenderingFailure when the device exists but a target or pipeline
    /// cannot be made.
    pub fn create(width: i32, height: i32, options: &OffscreenOptions) -> Result<OffscreenGpu> {
        check_size(width, height)?;
        let context = GpuContext::request(options.device)?;
        Self::create_on(&context, width, height, options)
    }

    /// A target on `context`'s device, which must be of the kind
    /// `options.device` asks for (Unsupported otherwise, as create()).
    pub fn create_on(
        context: &GpuContext,
        width: i32,
        height: i32,
        options: &OffscreenOptions,
    ) -> Result<OffscreenGpu> {
        check_size(width, height)?;
        check_kind(
            &context.adapter.get_info(),
            options.device,
            to_string(GpuBackend::built()),
        )?;
        let sample_count = if context.sample_count_supported(options.sample_count) {
            options.sample_count
        } else {
            1
        };
        if !context
            .adapter
            .get_texture_format_features(DEPTH_FORMAT)
            .allowed_usages
            .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
        {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "the device has no 32-bit float depth format",
            ));
        }

        let device = &context.device;
        // check_size bounded both to 1..=16384.
        let size = wgpu::Extent3d {
            width: width as u32,
            height: height as u32,
            depth_or_array_layers: 1,
        };
        let texture = |label: &'static str,
                       samples: u32,
                       format: wgpu::TextureFormat,
                       usage: wgpu::TextureUsages| {
            guarded(device, || {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
            })
        };
        // TEXTURE_BINDING so that the window can show it as an egui texture.
        let resolved = texture(
            "katana colour",
            1,
            COLOR_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
        )
        .map_err(|_| failure("could not create the colour texture"))?;
        let msaa = if sample_count > 1 {
            Some(
                texture(
                    "katana multisampled colour",
                    sample_count,
                    COLOR_FORMAT,
                    wgpu::TextureUsages::RENDER_ATTACHMENT,
                )
                .map_err(|_| failure("could not create the multisampled colour buffer"))?,
            )
        } else {
            None
        };
        let depth = texture(
            "katana depth",
            sample_count,
            DEPTH_FORMAT,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        )
        .map_err(|_| failure("could not create the depth texture"))?;

        let row_pitch = (size.width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let readback = guarded(device, || {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("katana read-back"),
                size: wgpu::BufferAddress::from(row_pitch) * wgpu::BufferAddress::from(size.height),
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        })
        .map_err(|_| failure("could not create the read-back buffer"))?;

        let timer =
            if options.timestamps && device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
                let made = guarded(device, || Timer {
                    queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                        label: Some("katana frame time"),
                        ty: wgpu::QueryType::Timestamp,
                        count: 2,
                    }),
                    resolved: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("katana frame time"),
                        size: TIMESTAMP_BYTES,
                        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                        mapped_at_creation: false,
                    }),
                    read: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("katana frame time read-back"),
                        size: TIMESTAMP_BYTES,
                        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }),
                });
                Some(made.map_err(|_| failure("could not create the frame timer"))?)
            } else {
                None
            };

        let mut renderer = GpuRenderer::new();
        renderer.initialise(
            device,
            &context.queue,
            TargetFormat {
                color: COLOR_FORMAT,
                depth: DEPTH_FORMAT,
                sample_count,
            },
        )?;
        let view =
            |texture: &wgpu::Texture| texture.create_view(&wgpu::TextureViewDescriptor::default());
        Ok(OffscreenGpu {
            context: context.clone(),
            width,
            height,
            sample_count,
            resolved_view: view(&resolved),
            resolved,
            msaa_view: msaa.as_ref().map(view),
            depth_view: view(&depth),
            readback,
            row_pitch,
            timer,
            renderer,
            last_gpu_ms: 0.0,
        })
    }

    pub fn renderer(&mut self) -> &mut GpuRenderer {
        &mut self.renderer
    }

    /// The device it draws on (the C++'s rhi()).
    pub fn context(&self) -> &GpuContext {
        &self.context
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn sample_count(&self) -> u32 {
        self.sample_count
    }

    /// The adapter's name, for test and benchmark output.
    pub fn device_name(&self) -> String {
        self.context.device_name()
    }

    /// The resolved colour of the last frame, RGBA8: what the window shows
    /// (it is made with TEXTURE_BINDING) and what render_frame reads back.
    pub fn color_texture(&self) -> &wgpu::Texture {
        &self.resolved
    }

    /// Draws one frame through `camera` and waits for the GPU to finish it.
    /// With `pixels`, the resolved colour is read back into it as 0xAARRGGBB,
    /// row-major, top row first - the software Framebuffer's layout - so the
    /// two paths' images compare directly.
    pub fn render_frame(
        &mut self,
        camera: &Camera,
        settings: &FrameSettings,
        pixels: Option<&mut Vec<Rgba>>,
    ) -> Result<GpuFrameStats> {
        let width = self.width as u32;
        let height = self.height as u32;
        let device = &self.context.device;
        let queue = &self.context.queue;
        let (color, resolve) = match &self.msaa_view {
            Some(msaa) => (msaa, Some(&self.resolved_view)),
            None => (&self.resolved_view, None),
        };
        let target = RenderTarget {
            color,
            resolve,
            depth: &self.depth_view,
            width,
            height,
            timestamp_writes: self
                .timer
                .as_ref()
                .map(|timer| wgpu::RenderPassTimestampWrites {
                    query_set: &timer.queries,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                }),
        };
        let read_pixels = pixels.is_some();
        let renderer = &mut self.renderer;
        let resolved = &self.resolved;
        let readback = &self.readback;
        let row_pitch = self.row_pitch;
        let timer = self.timer.as_ref();
        // One error scope over the frame: what wgpu finds wrong in the
        // recording is reported at the submission, not where it was recorded.
        let frame = guarded(device, || {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("katana offscreen frame"),
            });
            let stats = renderer.render(&mut encoder, target, camera, settings);
            if stats.is_ok() {
                if read_pixels {
                    encoder.copy_texture_to_buffer(
                        resolved.as_image_copy(),
                        wgpu::TexelCopyBufferInfo {
                            buffer: readback,
                            layout: wgpu::TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(row_pitch),
                                rows_per_image: Some(height),
                            },
                        },
                        wgpu::Extent3d {
                            width,
                            height,
                            depth_or_array_layers: 1,
                        },
                    );
                }
                if let Some(timer) = timer {
                    encoder.resolve_query_set(&timer.queries, 0..2, &timer.resolved, 0);
                    encoder.copy_buffer_to_buffer(
                        &timer.resolved,
                        0,
                        &timer.read,
                        0,
                        TIMESTAMP_BYTES,
                    );
                }
            }
            let index = queue.submit(Some(encoder.finish()));
            (stats, index)
        });
        let (stats, index) = frame.map_err(|message| {
            failure(&format!("the GPU could not finish the frame: {message}"))
        })?;
        // Synchronous: returns once the GPU has finished, so the read-back is
        // there to map.
        if device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: None,
            })
            .is_err()
        {
            return Err(failure("the GPU could not finish the frame"));
        }
        let stats = stats?;

        if let Some(timer) = timer {
            let bytes = read_back(device, &timer.read).map_err(|message| {
                failure(&format!("the frame time came back unread: {message}"))
            })?;
            let ticks = bytes.as_chunks::<8>().0;
            if ticks.len() >= 2 {
                let begin = u64::from_ne_bytes(ticks[0]);
                let end = u64::from_ne_bytes(ticks[1]);
                // Ticks to nanoseconds, then to milliseconds.
                self.last_gpu_ms = end.saturating_sub(begin) as f64
                    * f64::from(queue.get_timestamp_period())
                    / 1.0e6;
            }
        }

        if let Some(pixels) = pixels {
            let bytes = read_back(device, readback)
                .map_err(|message| failure(&format!("the read-back failed: {message}")))?;
            let row_bytes = width as usize * 4;
            let pitch = row_pitch as usize;
            if bytes.len() < pitch * (height as usize - 1) + row_bytes {
                return Err(failure("the read-back came back short"));
            }
            pixels.clear();
            pixels.reserve(width as usize * height as usize);
            // RGBA8 in memory order R, G, B, A -> 0xAARRGGBB.
            for row in bytes.chunks(pitch).take(height as usize) {
                for pixel in row[..row_bytes].as_chunks::<4>().0 {
                    pixels.push(rgba_with_alpha(pixel[0], pixel[1], pixel[2], pixel[3]));
                }
            }
        }
        Ok(stats)
    }

    /// What the GPU itself measured for the most recent frame, in
    /// milliseconds; 0 before any frame, or without timestamps.
    pub fn last_gpu_milliseconds(&self) -> f64 {
        self.last_gpu_ms
    }
}
