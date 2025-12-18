use kurbo::{Affine, Shape, Size};
use peniko::color::{AlphaColor, Oklab, Srgb};

struct RenderStore {
  font:   parley::FontContext,
  layout: parley::LayoutContext,

  render: vello::Renderer,
}

pub struct Render<'a> {
  store: &'a RenderStore,
  scene: vello::Scene,

  size: Size,
}

mod blitter;
mod window;

struct App {
  store: RenderStore,
  state: super::State,

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
      state: super::State::default(),

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
  fn resize(&mut self, device: &wgpu::Device, surface: &wgpu::SurfaceConfiguration) {
    self.texture = device.create_texture(&wgpu::TextureDescriptor {
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
    self.texture_view = self.texture.create_view(&wgpu::TextureViewDescriptor::default());
  }

  fn render(
    &mut self,
    surface: &wgpu::SurfaceTexture,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    scale: f64,
  ) {
    let mut render = Render {
      store: &self.store,
      scene: vello::Scene::new(),
      size:  Size::new(surface.texture.width() as f64, surface.texture.height() as f64),
    };

    self.state.draw(&mut render);

    self
      .store
      .render
      .render_to_texture(
        &device,
        &queue,
        &render.scene,
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

    queue.submit(std::iter::once(encoder.finish()));
  }
}

impl Render<'_> {
  pub fn size(&self) -> Size { self.size }

  pub fn fill(&mut self, shape: &impl Shape, color: Color) {
    self.scene.fill(
      peniko::Fill::NonZero,
      Affine::IDENTITY,
      peniko::Brush::Solid(encode_color(color)),
      None,
      shape,
    );
  }
}
