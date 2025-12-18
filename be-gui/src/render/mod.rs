use kurbo::{Affine, Point, Rect, Shape, Size, Stroke};
use peniko::{
  Fill,
  color::{AlphaColor, Oklab, Oklch, Srgb},
};

mod blitter;
mod window;

struct RenderStore {
  font:   parley::FontContext,
  layout: parley::LayoutContext<peniko::Brush>,

  render: vello::Renderer,
}

pub struct Render<'a> {
  store: &'a mut RenderStore,
  scene: vello::Scene,

  scale: f64,
  size:  Size,
}

struct App {
  store: RenderStore,
  state: super::State,

  texture:      wgpu::Texture,
  texture_view: wgpu::TextureView,

  blitter: blitter::TextureBlitterConvert,
}

pub type Color = AlphaColor<Oklab>;

pub fn oklch(l: f32, c: f32, h: f32) -> Color { AlphaColor::<Oklch>::new([l, c, h, 1.0]).convert() }

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
      store: &mut self.store,
      scene: vello::Scene::new(),
      scale,
      size: Size::new(
        surface.texture.width() as f64 / scale,
        surface.texture.height() as f64 / scale,
      ),
    };

    self.state.draw(&mut render);

    let scene = render.scene;

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

    queue.submit(std::iter::once(encoder.finish()));
  }
}

impl Render<'_> {
  pub fn size(&self) -> Size { self.size }

  pub fn stroke(&mut self, shape: &impl Shape, color: Color, mut stroke: Stroke) {
    stroke.width *= self.scale;

    self.scene.stroke(
      &stroke,
      Affine::scale(self.scale),
      peniko::Brush::Solid(encode_color(color)),
      None,
      shape,
    );
  }

  pub fn fill(&mut self, shape: &impl Shape, color: Color) {
    self.scene.fill(
      peniko::Fill::NonZero,
      Affine::scale(self.scale),
      peniko::Brush::Solid(encode_color(color)),
      None,
      shape,
    );
  }

  pub fn draw_text(&mut self, text: &str, pos: impl Into<Point>, color: Color) -> Rect {
    let mut builder = self.store.layout.ranged_builder(&mut self.store.font, &text, 1.0, false);
    builder.push_default(parley::StyleProperty::Brush(encode_color(color).into()));
    builder.push_default(parley::StyleProperty::FontSize(12.0 * self.scale as f32));
    let mut layout = builder.build(&text);

    layout.break_all_lines(None);
    layout.align(None, parley::Alignment::Start, parley::AlignmentOptions::default());

    let mut rect = Rect::new(0.0, 0.0, f64::from(layout.width()), f64::from(layout.height()));

    let origin = pos.into();

    for line in layout.lines() {
      for item in line.items() {
        let parley::PositionedLayoutItem::GlyphRun(glyph_run) = item else { continue };

        let run = glyph_run.run();
        rect.y0 = rect.y1.round() - rect.height();
        let mut x = rect.x0 as f32 + glyph_run.offset();
        let baseline = (rect.y0 as f32 + glyph_run.baseline()).round();

        self
          .scene
          .draw_glyphs(run.font())
          .brush(&glyph_run.style().brush)
          .hint(true)
          .transform(Affine::translate(origin.to_vec2() * self.scale))
          .glyph_transform(
            run.synthesis().skew().map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0)),
          )
          .font_size(run.font_size())
          .normalized_coords(run.normalized_coords())
          .draw(
            Fill::NonZero,
            glyph_run.glyphs().map(|glyph| {
              let gx = x + glyph.x;
              let gy = baseline + glyph.y;
              x += glyph.advance;
              vello::Glyph { id: glyph.id.into(), x: gx, y: gy }
            }),
          );
      }
    }

    rect.scale_from_origin(1.0 / self.scale) + origin.to_vec2()
  }
}
