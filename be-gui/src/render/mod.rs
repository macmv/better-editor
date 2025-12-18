use vello::peniko::color::{AlphaColor, Oklab, Oklch, Srgb};

struct RenderStore {
  font:   parley::FontContext,
  layout: parley::LayoutContext,

  render: vello::Renderer,
}

pub struct Render<'a> {
  store: &'a RenderStore,
  scene: vello::Scene,
}

mod blitter;
mod window;

struct App {
  store: RenderStore,

  texture:      wgpu::Texture,
  texture_view: wgpu::TextureView,

  blitter: blitter::TextureBlitterConvert,
}

pub type Color = AlphaColor<Oklab>;

/// Converts things to sRGB, so that vello uses OkLAB for everything, and then
/// we undo this conversion in the blitter.
pub fn encode_color(color: Color) -> AlphaColor<Srgb> {
  let [l, a, b, alpha] = color.components;

  AlphaColor::new([l, a + 0.5, b + 0.5, alpha])
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub fn run() {
  window::run(|device, surface| {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
      label:           None,
      size:            wgpu::Extent3d {
        width:                 surface.width,
        height:                surface.height,
        depth_or_array_layers: 1,
      },
      mip_level_count: 1,
      sample_count:    1,
      dimension:       wgpu::TextureDimension::D2,
      format:          FORMAT,
      usage:           wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
      view_formats:    &[],
    });
    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    App {
      store: RenderStore {
        font:   parley::FontContext::new(),
        layout: parley::LayoutContext::new(),
        render: vello::Renderer::new(&device, vello::RendererOptions::default()).unwrap(),
      },

      texture,
      texture_view,
      blitter: blitter::TextureBlitterConvert::new(&device, surface.format),
    }
  });
}

impl App {
  fn render(&mut self, surface: &wgpu::SurfaceTexture, device: &wgpu::Device, queue: &wgpu::Queue) {
    let scene = vello::Scene::new();

    self
      .store
      .render
      .render_to_texture(
        &device,
        &queue,
        &scene,
        &self.texture_view,
        &vello::RenderParams {
          base_color:          encode_color(Color::WHITE),
          width:               surface.texture.width(),
          height:              surface.texture.height(),
          antialiasing_method: vello::AaConfig::Msaa16,
        },
      )
      .unwrap();

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    self.blitter.copy(
      device,
      &mut encoder,
      &self.texture_view,
      &surface.texture.create_view(&wgpu::TextureViewDescriptor::default()),
    );

    // submit will accept anything that implements IntoIter
    queue.submit(std::iter::once(encoder.finish()));
  }
}
