use be_input::Key;
use winit::{
  event::{self, WindowEvent},
  event_loop::{self, ActiveEventLoop},
};

type AppBuilder = fn(&wgpu::Device, &wgpu::SurfaceConfiguration) -> super::App;

struct App {
  builder: AppBuilder,
  init:    Option<Init>,
}

struct Init {
  app: super::App,

  surface: wgpu::Surface<'static>,
  device:  wgpu::Device,
  queue:   wgpu::Queue,
  config:  wgpu::SurfaceConfiguration,
  scale:   f64,

  // SAFETY: Keep this field last so we don't segfault on exit.
  window: winit::window::Window,
}

impl winit::application::ApplicationHandler for App {
  fn resumed(&mut self, event_loop: &ActiveEventLoop) {
    let window = event_loop
      .create_window(winit::window::WindowAttributes::default().with_title("Better Editor"))
      .unwrap();

    let instance = wgpu::Instance::new(&Default::default());
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
      app: (self.builder)(&device, &config),
      surface,
      device,
      queue,
      config,
      scale: window.scale_factor(),
      window,
    });
  }

  fn window_event(
    &mut self,
    event_loop: &ActiveEventLoop,
    _: winit::window::WindowId,
    event: WindowEvent,
  ) {
    match event {
      WindowEvent::KeyboardInput {
        event:
          winit::event::KeyEvent {
            logical_key: winit::keyboard::Key::Character(c),
            state: winit::event::ElementState::Pressed,
            ..
          },
        ..
      } if c == "q" => {
        event_loop.exit();
      }
      WindowEvent::CloseRequested => {
        event_loop.exit();
      }

      WindowEvent::KeyboardInput {
        event:
          winit::event::KeyEvent {
            logical_key: key, state: winit::event::ElementState::Pressed, ..
          },
        ..
      } => {
        if let Some(init) = &mut self.init
          && let Some(key) = parse_key(key)
        {
          init.app.state.on_key(key);
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

          init.app.render(&texture, &init.device, &init.queue, init.scale);

          texture.present();
        }
      }

      _ => (),
    }
  }
}

pub fn run(builder: AppBuilder) {
  let event_loop = winit::event_loop::EventLoop::new().unwrap();
  event_loop.set_control_flow(event_loop::ControlFlow::Wait);

  let mut app = App { builder, init: None };
  event_loop.run_app(&mut app).unwrap();
}

fn parse_key(key: winit::keyboard::Key) -> Option<Key> {
  use winit::keyboard::{Key as WKey, NamedKey::*};

  match key {
    WKey::Character(s) if s.len() == 1 => Some(Key::Char(s.chars().next()?)),
    WKey::Named(Escape) => Some(Key::Escape),

    _ => None,
  }
}
