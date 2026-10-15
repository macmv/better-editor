use be_input::{Key, KeyStroke};
use kurbo::{Point, Vec2};
use winit::{
  event::WindowEvent,
  event_loop::{self, ActiveEventLoop},
  window::Window,
};

use crate::{Event, MouseEvent, render::cursor::CursorKind};

type AppBuilder = Box<
  dyn FnMut(
    &wgpu::Device,
    &wgpu::SurfaceConfiguration,
    &Window,
    event_loop::EventLoopProxy<Event>,
  ) -> super::App,
>;

struct App {
  builder: AppBuilder,
  init:    Option<Init>,
  proxy:   event_loop::EventLoopProxy<Event>,
}

struct Init {
  app:  super::App,
  keys: KeyState,

  cursor:      Option<Point>,
  cursor_kind: CursorKind,

  surface: wgpu::Surface<'static>,
  device:  wgpu::Device,
  queue:   wgpu::Queue,
  config:  wgpu::SurfaceConfiguration,
  scale:   f64,

  // SAFETY: Keep this field last so we don't segfault on exit.
  window: winit::window::Window,
}

#[derive(Default)]
struct KeyState {
  mods: winit::keyboard::ModifiersState,
}

impl winit::application::ApplicationHandler<Event> for App {
  fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    let window = event_loop
      .create_window(winit::window::WindowAttributes::default().with_title("Better Editor"))
      .unwrap();

    let mut descriptor = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(
      event_loop.owned_display_handle(),
    ));
    descriptor.flags = wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL; // disable validation.
    let instance = wgpu::Instance::new(descriptor);
    let surface = instance.create_surface(&window).unwrap();

    // SAFETY: `window` is kept alive for the duration of `surface`.
    let surface =
      unsafe { std::mem::transmute::<wgpu::Surface<'_>, wgpu::Surface<'static>>(surface) };

    let adapter =
      pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();

    let surface_caps = surface.get_capabilities(&adapter);
    let surface_format = surface_caps
      .formats
      .iter()
      .find(|f| f.is_srgb())
      .copied()
      .expect("could not find sRGB surface");

    let config = wgpu::SurfaceConfiguration {
      usage:                         wgpu::TextureUsages::RENDER_ATTACHMENT,
      format:                        surface_format,
      color_space:                   wgpu::SurfaceColorSpace::Auto,
      width:                         window.inner_size().width,
      height:                        window.inner_size().height,
      alpha_mode:                    wgpu::CompositeAlphaMode::Auto,
      view_formats:                  vec![],
      present_mode:                  wgpu::PresentMode::AutoVsync,
      desired_maximum_frame_latency: 2,
    };

    surface.configure(&device, &config);

    self.init = Some(Init {
      app: (self.builder)(&device, &config, &window, self.proxy.clone()),
      keys: Default::default(),
      cursor: None,
      cursor_kind: CursorKind::Default,
      surface,
      device,
      queue,
      config,
      scale: window.scale_factor(),
      window,
    });
  }

  fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Event) {
    if let Some(init) = &mut self.init {
      if init.app.state.on_event(event, &mut init.app.store) {
        event_loop.exit();
      }
      init.window.request_redraw();
    }
  }

  fn window_event(
    &mut self,
    event_loop: &ActiveEventLoop,
    _: winit::window::WindowId,
    event: WindowEvent,
  ) {
    match event {
      WindowEvent::CloseRequested => {
        event_loop.exit();
      }

      // The release of a modifier can go to another window (such as when switching workspaces), so
      // drop all modifiers when focus is lost.
      WindowEvent::Focused(false) => {
        if let Some(init) = &mut self.init {
          init.keys = KeyState::default();
        }
      }

      WindowEvent::ModifiersChanged(mods) => {
        if let Some(init) = &mut self.init {
          init.keys.mods = mods.state();
        }
      }

      WindowEvent::KeyboardInput {
        event:
          winit::event::KeyEvent {
            logical_key: key, state: winit::event::ElementState::Pressed, ..
          },
        ..
      } => {
        if let Some(init) = &mut self.init
          && let Some(key) = init.keys.parse_key(key)
        {
          init.app.state.on_key(key, &mut init.app.store);
          init.window.request_redraw();
        }
      }

      WindowEvent::CursorMoved { position, .. } => {
        if let Some(init) = &mut self.init {
          let pos = Point::new(position.x / init.scale, position.y / init.scale);
          init.cursor = Some(pos);
          let size = kurbo::Size::new(
            init.app.texture.width() as f64 / init.scale,
            init.app.texture.height() as f64 / init.scale,
          );

          let new_cursor = init.app.state.on_mouse(MouseEvent::Move { pos }, size, &init.app.store);
          if new_cursor != init.cursor_kind {
            super::cursor::set_cursor(&init.window, new_cursor);
            init.cursor_kind = new_cursor;
          }
          init.window.request_redraw();
        }
      }

      WindowEvent::CursorLeft { .. } => {
        if let Some(init) = &mut self.init {
          init.cursor = None;
          let size = kurbo::Size::new(
            init.app.texture.width() as f64 / init.scale,
            init.app.texture.height() as f64 / init.scale,
          );
          init.app.state.on_mouse(MouseEvent::Leave, size, &init.app.store);
          init.window.request_redraw();
        }
      }

      WindowEvent::MouseInput { state, button, .. } => {
        if let Some(init) = &mut self.init
          && let Some(cursor) = init.cursor
          && let Some(button) = match button {
            winit::event::MouseButton::Left => Some(crate::MouseButton::Left),
            winit::event::MouseButton::Right => Some(crate::MouseButton::Right),
            winit::event::MouseButton::Middle => Some(crate::MouseButton::Middle),
            _ => None,
          }
        {
          let size = kurbo::Size::new(
            init.app.texture.width() as f64 / init.scale,
            init.app.texture.height() as f64 / init.scale,
          );

          init.app.state.on_mouse(
            MouseEvent::Button {
              pos: cursor,
              pressed: match state {
                winit::event::ElementState::Pressed => true,
                winit::event::ElementState::Released => false,
              },
              button,
            },
            size,
            &init.app.store,
          );
          init.window.request_redraw();
        }
      }

      WindowEvent::MouseWheel { delta, .. } => {
        if let Some(init) = &mut self.init
          && let Some(cursor) = init.cursor
        {
          let size = kurbo::Size::new(
            init.app.texture.width() as f64 / init.scale,
            init.app.texture.height() as f64 / init.scale,
          );

          // This is 3x the line height (approximately).
          const SCROLL_MULTIPLIER: f64 = 60.0;

          init.app.state.on_mouse(
            MouseEvent::Scroll {
              pos:   cursor,
              delta: match delta {
                winit::event::MouseScrollDelta::LineDelta(x, y) => {
                  Vec2::new(x as f64 * SCROLL_MULTIPLIER, y as f64 * SCROLL_MULTIPLIER)
                }
                winit::event::MouseScrollDelta::PixelDelta(pos) => Vec2::new(pos.x, pos.y),
              },
            },
            size,
            &init.app.store,
          );
          init.window.request_redraw();
        }
      }

      WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
        if let Some(init) = &mut self.init {
          init.scale = scale_factor;
        }
      }

      WindowEvent::Resized(size) => {
        if let Some(init) = &mut self.init {
          init.config.width = size.width;
          init.config.height = size.height;

          init.surface.configure(&init.device, &init.config);
          init.app.resize(&init.device, &init.config);
        }
      }

      WindowEvent::RedrawRequested => {
        if let Some(init) = &mut self.init {
          use wgpu::CurrentSurfaceTexture::*;
          let texture = match init.surface.get_current_texture() {
            Success(texture) | Suboptimal(texture) => texture,
            Outdated | Lost => {
              init.surface.configure(&init.device, &init.config);
              init.window.request_redraw();
              return;
            }
            Timeout | Occluded => return,
            Validation => panic!("surface validation error"),
          };

          puffin::GlobalProfiler::lock().new_frame();
          init.app.render(&texture, &init.device, &init.queue, init.scale);

          init.window.pre_present_notify();
          init.queue.present(texture);

          // We'd like to stay up to date with events, so lets avoid buffering a
          // bunch of frames.
          init.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();

          if init.app.state.should_exit() {
            event_loop.exit();
          } else if init.app.state.animated() {
            init.window.request_redraw();
            event_loop.set_control_flow(event_loop::ControlFlow::Poll);
          } else {
            event_loop.set_control_flow(event_loop::ControlFlow::Wait);
          }
        }
      }

      _ => (),
    }
  }
}

pub fn run(builder: AppBuilder) {
  let event_loop = winit::event_loop::EventLoop::<Event>::with_user_event().build().unwrap();
  event_loop.set_control_flow(event_loop::ControlFlow::Wait);

  let mut app = App { builder, proxy: event_loop.create_proxy(), init: None };
  event_loop.run_app(&mut app).unwrap();
}

impl KeyState {
  fn parse_key(&self, key: winit::keyboard::Key) -> Option<KeyStroke> {
    use winit::keyboard::{Key as WKey, NamedKey::*};

    let key = match key {
      WKey::Character(s) if s.len() == 1 => Some(Key::Char(s.chars().next()?)),
      WKey::Named(Escape) => Some(Key::Escape),
      WKey::Named(Tab) => Some(Key::Tab),
      WKey::Named(ContextMenu) => Some(Key::Menu),
      WKey::Named(Enter) => Some(Key::Char('\n')),
      WKey::Named(Space) => Some(Key::Char(' ')),
      WKey::Named(Backspace) => Some(Key::Backspace),
      WKey::Named(Delete) => Some(Key::Delete),
      WKey::Named(ArrowUp) => Some(Key::ArrowUp),
      WKey::Named(ArrowDown) => Some(Key::ArrowDown),
      WKey::Named(ArrowLeft) => Some(Key::ArrowLeft),
      WKey::Named(ArrowRight) => Some(Key::ArrowRight),

      _ => None,
    };

    key.map(|key| KeyStroke {
      key,
      sup: self.mods.super_key(),
      shift: self.mods.shift_key(),
      control: self.mods.control_key(),
      alt: self.mods.alt_key(),
    })
  }
}
