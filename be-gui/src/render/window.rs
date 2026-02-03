use be_input::{Key, KeyStroke};
use kurbo::Point;
use winit::{
  event::WindowEvent,
  event_loop::{self, ActiveEventLoop},
  keyboard::NamedKey,
};

use crate::{Event, MouseEvent, render::cursor::CursorKind};

type AppBuilder =
  fn(&wgpu::Device, &wgpu::SurfaceConfiguration, event_loop::EventLoopProxy<Event>) -> super::App;

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
  left_control:  bool,
  right_control: bool,
  left_alt:      bool,
  right_alt:     bool,
}

impl winit::application::ApplicationHandler<Event> for App {
  fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    let window = event_loop
      .create_window(winit::window::WindowAttributes::default().with_title("Better Editor"))
      .unwrap();

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
      flags: wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL, // disable validation.
      ..Default::default()
    });
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
      width:                         window.inner_size().width,
      height:                        window.inner_size().height,
      alpha_mode:                    wgpu::CompositeAlphaMode::Auto,
      view_formats:                  vec![],
      present_mode:                  wgpu::PresentMode::AutoVsync,
      desired_maximum_frame_latency: 2,
    };

    surface.configure(&device, &config);

    self.init = Some(Init {
      app: (self.builder)(&device, &config, self.proxy.clone()),
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
      if init.app.state.on_event(event) {
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

      WindowEvent::KeyboardInput {
        event:
          winit::event::KeyEvent {
            logical_key: winit::keyboard::Key::Named(key @ (NamedKey::Control | NamedKey::Alt)),
            location,
            state,
            ..
          },
        ..
      } => {
        if let Some(init) = &mut self.init {
          let key = match (key, location) {
            (NamedKey::Control, winit::keyboard::KeyLocation::Left) => &mut init.keys.left_control,
            (NamedKey::Control, winit::keyboard::KeyLocation::Right) => {
              &mut init.keys.right_control
            }
            (NamedKey::Alt, winit::keyboard::KeyLocation::Left) => &mut init.keys.left_alt,
            (NamedKey::Alt, winit::keyboard::KeyLocation::Right) => &mut init.keys.right_alt,

            _ => unreachable!(),
          };

          match state {
            winit::event::ElementState::Pressed => *key = true,
            winit::event::ElementState::Released => *key = false,
          }
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
          init.app.state.on_key(key);
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

          let new_cursor = init.app.state.on_mouse(MouseEvent::Move { pos }, size, init.scale);
          if new_cursor != init.cursor_kind {
            super::cursor::set_cursor(&init.window, new_cursor);
            init.cursor_kind = new_cursor;
          }
        }
      }

      WindowEvent::CursorLeft { .. } => {
        if let Some(init) = &mut self.init {
          init.cursor = None;
          let size = kurbo::Size::new(
            init.app.texture.width() as f64 / init.scale,
            init.app.texture.height() as f64 / init.scale,
          );
          init.app.state.on_mouse(MouseEvent::Left, size, init.scale);
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
            init.scale,
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
          let texture = init.surface.get_current_texture().unwrap();

          puffin::GlobalProfiler::lock().new_frame();
          init.app.render(&texture, &init.device, &init.queue, init.scale);

          init.window.pre_present_notify();
          texture.present();

          // We'd like to stay up to date with events, so lets avoid buffering a bunch of
          // frames.
          init.device.poll(wgpu::PollType::wait()).unwrap();

          if init.app.state.animated() {
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
      WKey::Named(Tab) => None, // TODO
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
      control: self.left_control || self.right_control,
      alt: self.left_alt || self.right_alt,
    })
  }
}
