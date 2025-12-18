use winit::{
  event::WindowEvent,
  event_loop::{self, ActiveEventLoop},
  keyboard::Key,
};

struct App {
  init: Option<Init>,
}

struct Init {
  instance: wgpu::Instance,
  surface:  wgpu::Surface<'static>,
  device:   wgpu::Device,
  queue:    wgpu::Queue,

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
      pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()));
    let (device, queue) =
      pollster::block_on(adapter.unwrap().request_device(&Default::default())).unwrap();

    let surface_desc = wgpu::SurfaceConfiguration {
      usage:                         wgpu::TextureUsages::RENDER_ATTACHMENT,
      format:                        wgpu::TextureFormat::Bgra8UnormSrgb,
      width:                         800,
      height:                        600,
      alpha_mode:                    wgpu::CompositeAlphaMode::Auto,
      view_formats:                  vec![],
      present_mode:                  wgpu::PresentMode::AutoVsync,
      desired_maximum_frame_latency: 2,
    };

    surface.configure(&device, &surface_desc);

    self.init = Some(Init { instance, surface, device, queue, window });
  }

  fn window_event(
    &mut self,
    event_loop: &ActiveEventLoop,
    _: winit::window::WindowId,
    event: WindowEvent,
  ) {
    match event {
      WindowEvent::KeyboardInput {
        event: winit::event::KeyEvent { logical_key: Key::Character(c), .. },
        ..
      } if c == "q" => {
        event_loop.exit();
      }
      WindowEvent::CloseRequested => {
        event_loop.exit();
      }

      WindowEvent::RedrawRequested => {
        if let Some(init) = &mut self.init {
          let texture = init.surface.get_current_texture().unwrap();

          texture.present();
        }
      }

      _ => (),
    }
  }
}

pub fn run() {
  let event_loop = winit::event_loop::EventLoop::new().unwrap();
  event_loop.set_control_flow(event_loop::ControlFlow::Wait);

  let mut app = App { init: None };
  event_loop.run_app(&mut app).unwrap();
}
