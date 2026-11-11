//! A window with a swap chain, driven by winit.

use std::sync::Arc;
use std::time::{Duration, Instant};

use luma_formats::Rgba;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Fullscreen, Window, WindowId};

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
    /// Whether the system cursor shows over the window (a game draws
    /// its own).
    pub cursor_visible: bool,
    /// Whether the window covers the whole screen, without borders, at
    /// the desktop's resolution (`width` and `height` are then unused).
    pub fullscreen: bool,
    /// Where to put the window's top-left corner, in physical pixels of
    /// the desktop; `None` lets the system choose. Ignored in full
    /// screen, and when the window would be on no monitor.
    pub position: Option<(i32, i32)>,
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
            cursor_visible: true,
            fullscreen: false,
            position: None,
        }
    }
}

/// Whether a rectangle of the desktop (`x`, `y`, `width`, `height`)
/// shows on one of the monitors, given as rectangles too: a window
/// remembered from a screen that is gone must not open out of sight.
pub fn on_a_monitor(rect: (i32, i32, u32, u32), monitors: &[(i32, i32, u32, u32)]) -> bool {
    let (x, y, w, h) = rect;
    let (right, bottom) = (i64::from(x) + i64::from(w), i64::from(y) + i64::from(h));
    monitors.iter().any(|&(mx, my, mw, mh)| {
        let (m_right, m_bottom) = (i64::from(mx) + i64::from(mw), i64::from(my) + i64::from(mh));
        i64::from(x) < m_right
            && right > i64::from(mx)
            && i64::from(y) < m_bottom
            && bottom > i64::from(my)
    })
}

/// What a frame hands to the application.
pub struct Frame<'a> {
    /// The renderer, to create textures.
    pub renderer: &'a Renderer,
    /// The window, to move its cursor or read where it is.
    pub window: &'a Window,
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
    /// Keep running, but do not present this frame: the draw list is
    /// dropped and the window keeps showing the previous frame.
    Skip,
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

    /// Whether the application wants frames. While `false` the window
    /// asks for no redraw and the event loop sleeps until the next
    /// event (a game whose window is not active); a redraw the system
    /// asks for still calls [`App::draw`].
    fn active(&self) -> bool {
        true
    }

    /// Called when the user asks to close the window. [`Flow::Exit`],
    /// the default, closes it; [`Flow::Continue`] keeps it open, for an
    /// app that asks the user first.
    fn close_requested(&mut self) -> Flow {
        Flow::Exit
    }

    /// Fills the draw list for a frame. Return [`Flow::Exit`] to close
    /// the window after this frame, [`Flow::Skip`] to leave the window
    /// as it is (the list is not drawn).
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
        let mut attributes = Window::default_attributes()
            .with_title(&self.config.title)
            .with_inner_size(LogicalSize::new(self.config.width, self.config.height))
            .with_resizable(self.config.resizable);
        if self.config.fullscreen {
            attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(None)));
        } else if let Some((x, y)) = self.config.position {
            attributes = attributes.with_position(PhysicalPosition::new(x, y));
        }
        let window =
            Arc::new(
                event_loop
                    .create_window(attributes)
                    .map_err(|err| Error::Window {
                        reason: err.to_string(),
                    })?,
            );
        window.set_cursor_visible(self.config.cursor_visible);
        if let (false, Some((x, y))) = (self.config.fullscreen, self.config.position) {
            // A remembered position may belong to a screen that is gone:
            // then the window goes to the corner of the first one.
            let size = window.outer_size();
            let monitors: Vec<_> = window
                .available_monitors()
                .map(|m| {
                    let (p, s) = (m.position(), m.size());
                    (p.x, p.y, s.width, s.height)
                })
                .collect();
            if !monitors.is_empty() && !on_a_monitor((x, y, size.width, size.height), &monitors) {
                let (mx, my, ..) = monitors[0];
                window.set_outer_position(PhysicalPosition::new(mx, my));
            }
        }
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
            window: &state.window,
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
        if flow == Flow::Skip {
            // The surface texture goes back unpresented: the window keeps
            // its last image.
            return Ok(Flow::Continue);
        }
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
        if self.state.is_some() {
            return;
        }
        if let Err(err) = self.open(event_loop) {
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
            WindowEvent::CloseRequested => {
                if self.app.close_requested() == Flow::Exit {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => self.resize(size),
            WindowEvent::RedrawRequested => match self.redraw() {
                Ok(Flow::Continue | Flow::Skip) => {}
                Ok(Flow::Exit) => event_loop.exit(),
                Err(err) => {
                    self.result = Err(err);
                    event_loop.exit();
                }
            },
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if !self.app.active() {
            // Nothing to do until an event wakes the loop.
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        event_loop.set_control_flow(ControlFlow::Poll);
        if let Some(state) = &self.state {
            state.window.request_redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_is_on_a_monitor_when_it_overlaps_one() {
        let monitors = [(0, 0, 1920, 1080), (1920, 0, 1280, 1024)];
        assert!(on_a_monitor((100, 100, 800, 600), &monitors));
        assert!(on_a_monitor((2000, 500, 800, 600), &monitors));
        assert!(on_a_monitor((-700, 100, 800, 600), &monitors), "partly");
        assert!(!on_a_monitor((-800, 100, 800, 600), &monitors));
        assert!(!on_a_monitor((4000, 100, 800, 600), &monitors));
        assert!(!on_a_monitor((100, 2000, 800, 600), &monitors));
        assert!(!on_a_monitor(
            (i32::MAX, i32::MAX, u32::MAX, u32::MAX),
            &monitors
        ));
        assert!(!on_a_monitor((0, 0, 800, 600), &[]));
    }
}
