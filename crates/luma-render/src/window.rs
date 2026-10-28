//! A window with a swap chain, driven by winit.

use std::sync::Arc;
use std::time::{Duration, Instant};

use luma_formats::Rgba;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::draw::DrawList;
use crate::error::{Error, Result};
use crate::gpu::Gpu;
use crate::renderer::{Renderer, Target};

/// How to open the window.
#[derive(Debug, Clone)]
pub struct WindowConfig {
    /// Title bar text.
    pub title: String,
    /// Initial size of the drawable area, in logical pixels (scaled by
    /// the display's factor on HiDPI screens).
    pub width: u32,
    /// See `width`.
    pub height: u32,
    /// Whether the user may resize the window.
    pub resizable: bool,
    /// Colour of the padding and of the cleared frame.
    pub clear: Rgba,
}

impl Default for WindowConfig {
    fn default() -> Self {
        WindowConfig {
            title: "Luma Engine".into(),
            width: 1024,
            height: 768,
            resizable: true,
            clear: Rgba {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
        }
    }
}

/// What a frame hands to the application.
pub struct Frame<'a> {
    /// The renderer, to create textures.
    pub renderer: &'a Renderer,
    /// Time since the previous frame.
    pub dt: Duration,
    /// Size of the drawable area in physical pixels.
    pub size: PhysicalSize<u32>,
    /// Pixels per logical pixel.
    pub scale_factor: f64,
    /// Frames drawn so far.
    pub frame_index: u64,
}

/// What to do after an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Keep running.
    Continue,
    /// Close the window and return from [`run`].
    Exit,
}

/// The application driven by the window.
pub trait App {
    /// Called once the device exists, before the first frame: create
    /// textures here.
    fn init(&mut self, renderer: &Renderer) -> Result<()>;

    /// Called for every window event. The window is closed when the
    /// user asks unless this returns [`Flow::Exit`] earlier.
    fn event(&mut self, _event: &WindowEvent) -> Flow {
        Flow::Continue
    }

    /// Fills the draw list for a frame. Return [`Flow::Exit`] to close
    /// the window after this frame.
    fn draw<'a>(&'a mut self, frame: &Frame<'a>, list: &mut DrawList<'a>) -> Flow;
}

/// Opens a window and runs `app` until it exits or the window closes.
pub fn run<A: App>(config: WindowConfig, app: A) -> Result<()> {
    let event_loop = EventLoop::new().map_err(|err| Error::Window {
        reason: err.to_string(),
    })?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut runner = Runner {
        config,
        app,
        state: None,
        last_frame: Instant::now(),
        frame_index: 0,
        result: Ok(()),
    };
    event_loop
        .run_app(&mut runner)
        .map_err(|err| Error::Window {
            reason: err.to_string(),
        })?;
    runner.result
}

struct State {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
}

struct Runner<A: App> {
    config: WindowConfig,
    app: A,
    state: Option<State>,
    last_frame: Instant,
    frame_index: u64,
    result: Result<()>,
}

impl<A: App> Runner<A> {
    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let attributes = Window::default_attributes()
            .with_title(&self.config.title)
            .with_inner_size(LogicalSize::new(self.config.width, self.config.height))
            .with_resizable(self.config.resizable);
        let window =
            Arc::new(
                event_loop
                    .create_window(attributes)
                    .map_err(|err| Error::Window {
                        reason: err.to_string(),
                    })?,
            );
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(window.clone())),
        );
        let surface = instance
            .create_surface(window.clone())
            .map_err(|err| Error::Window {
                reason: err.to_string(),
            })?;
        let gpu = Gpu::with_surface(instance, &surface)?;
        let size = window.inner_size();
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or_else(|| Error::Window {
                reason: "the surface supports no texture format".into(),
            })?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        surface.configure(&gpu.device, &config);
        let renderer = Renderer::new(gpu);
        self.app.init(&renderer)?;
        self.state = Some(State {
            window,
            surface,
            config,
            renderer,
        });
        self.last_frame = Instant::now();
        Ok(())
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        if let Some(state) = &mut self.state {
            state.config.width = size.width.max(1);
            state.config.height = size.height.max(1);
            state
                .surface
                .configure(&state.renderer.gpu().device, &state.config);
        }
    }

    fn redraw(&mut self) -> Result<Flow> {
        let Some(state) = &mut self.state else {
            return Ok(Flow::Continue);
        };
        let texture = match state.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(Flow::Continue);
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                state
                    .surface
                    .configure(&state.renderer.gpu().device, &state.config);
                return Ok(Flow::Continue);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(Error::Render {
                    reason: "surface validation error".into(),
                });
            }
        };
        let now = Instant::now();
        let frame = Frame {
            renderer: &state.renderer,
            dt: now - self.last_frame,
            size: PhysicalSize::new(state.config.width, state.config.height),
            scale_factor: state.window.scale_factor(),
            frame_index: self.frame_index,
        };
        self.last_frame = now;
        self.frame_index += 1;
        let mut list = DrawList::new(
            frame.size.width as f32 / frame.scale_factor as f32,
            frame.size.height as f32 / frame.scale_factor as f32,
        );
        let flow = self.app.draw(&frame, &mut list);
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        state.renderer.render(
            Target {
                view: &view,
                format: state.config.format,
                width: state.config.width,
                height: state.config.height,
            },
            self.config.clear,
            &list,
        )?;
        drop(list);
        state.window.pre_present_notify();
        state.renderer.gpu().queue.present(texture);
        Ok(flow)
    }
}

impl<A: App> ApplicationHandler for Runner<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none()
            && let Err(err) = self.open(event_loop)
        {
            self.result = Err(err);
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.app.event(&event) == Flow::Exit {
            event_loop.exit();
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => self.resize(size),
            WindowEvent::RedrawRequested => match self.redraw() {
                Ok(Flow::Continue) => {}
                Ok(Flow::Exit) => event_loop.exit(),
                Err(err) => {
                    self.result = Err(err);
                    event_loop.exit();
                }
            },
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }
}
