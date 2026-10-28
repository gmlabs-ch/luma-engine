//! Access to the graphics device.

use crate::error::{Error, Result};

/// A graphics device and its queue, with the instance and adapter they
/// came from.
#[derive(Debug)]
pub struct Gpu {
    /// The wgpu instance.
    pub instance: wgpu::Instance,
    /// The adapter the device was created from.
    pub adapter: wgpu::Adapter,
    /// The device.
    pub device: wgpu::Device,
    /// The queue of the device.
    pub queue: wgpu::Queue,
}

impl Gpu {
    /// Opens the best adapter that can draw to `surface` (or any adapter
    /// when there is no surface) and creates a device on it.
    pub fn new(surface: Option<&wgpu::Surface<'_>>) -> Result<Gpu> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        Gpu::with_instance(instance, surface)
    }

    /// Like [`Gpu::new`] without a surface: for rendering to textures only
    /// (tests, screenshots, tools).
    pub fn headless() -> Result<Gpu> {
        Gpu::new(None)
    }

    /// Creates a device on an existing instance, able to draw to `surface`.
    pub fn with_surface(instance: wgpu::Instance, surface: &wgpu::Surface<'_>) -> Result<Gpu> {
        Gpu::with_instance(instance, Some(surface))
    }

    fn with_instance(instance: wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<Gpu> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: surface,
            apply_limit_buckets: false,
        }))
        .map_err(|err| Error::NoAdapter {
            reason: err.to_string(),
        })?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("luma device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|err| Error::Device {
            reason: err.to_string(),
        })?;
        Ok(Gpu {
            instance,
            adapter,
            device,
            queue,
        })
    }

    /// A one-line description of the adapter: name and backend.
    pub fn describe(&self) -> String {
        let info = self.adapter.get_info();
        format!("{} ({:?})", info.name, info.backend)
    }

    /// Waits until the GPU has finished all submitted work.
    pub fn wait(&self) -> Result<()> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map(|_| ())
            .map_err(|err| Error::Render {
                reason: err.to_string(),
            })
    }
}
