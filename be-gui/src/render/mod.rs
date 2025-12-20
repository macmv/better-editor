use kurbo::{Affine, Axis, Point, Rect, Shape, Size, Stroke, Vec2};
use peniko::color::{AlphaColor, Oklab, Oklch, Srgb};

mod blitter;
mod text;
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

  stack: Vec<Rect>,
}

struct App {
  store: RenderStore,
  state: super::State,

  texture:      wgpu::Texture,
  texture_view: wgpu::TextureView,

  blitter: blitter::TextureBlitterConvert,
}

pub struct TextLayout {
  origin: Point,
  layout: parley::Layout<peniko::Brush>,
  scale:  f64,
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
      stack: vec![],
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

pub enum Distance {
  Pixels(f64),
  Percent(f64),
}

impl Distance {
  pub fn to_pixels_in(self, size: f64) -> f64 {
    match self {
      Distance::Pixels(pixels) => pixels,
      Distance::Percent(percent) => size * percent,
    }
  }
}

impl<'a> Render<'a> {
  pub fn size(&self) -> Size {
    if let Some(top) = self.stack.last() { top.size() } else { self.size }
  }

  pub fn split(
    &mut self,
    axis: Axis,
    distance: Distance,
    left: impl FnOnce(&mut Render),
    right: impl FnOnce(&mut Render),
  ) {
    let mut left_bounds = Rect::from_origin_size(Point::ZERO, self.size());
    let mut right_bounds = Rect::from_origin_size(Point::ZERO, self.size());

    match axis {
      Axis::Vertical => {
        let mut distance = distance.to_pixels_in(self.size().width);
        if distance < 0.0 {
          distance += self.size().width;
        }

        // HACK: Without this overlap, there's a gap between splits. This is probably
        // from something being rounded somewhere, as changing the window size
        // makes the gap flicker.
        left_bounds.x1 = distance + 1.0;
        right_bounds.x0 = distance;
      }
      Axis::Horizontal => {
        let mut distance = distance.to_pixels_in(self.size().height);
        if distance < 0.0 {
          distance += self.size().height;
        }

        left_bounds.y1 = distance + 1.0;
        right_bounds.y0 = distance;
      }
    }

    self.clipped(left_bounds, left);
    self.clipped(right_bounds, right);
  }

  pub fn clipped(&mut self, mut rect: Rect, f: impl FnOnce(&mut Render)) {
    rect = rect + self.offset();

    self.stack.push(rect);
    self.scene.push_clip_layer(Affine::IDENTITY, &rect.scale_from_origin(self.scale));

    f(self);

    self.stack.pop().expect("no clip layer to pop");
    self.scene.pop_layer();
  }

  fn offset(&self) -> Vec2 {
    if let Some(top) = self.stack.last() { top.origin().to_vec2() } else { Vec2::ZERO }
  }

  fn transform(&self) -> Affine { Affine::scale(self.scale) * Affine::translate(self.offset()) }

  pub fn stroke(&mut self, shape: &impl Shape, color: Color, mut stroke: Stroke) {
    stroke.width *= self.scale;

    self.scene.stroke(
      &stroke,
      self.transform(),
      peniko::Brush::Solid(encode_color(color)),
      None,
      shape,
    );
  }

  pub fn fill(&mut self, shape: &impl Shape, color: Color) {
    self.scene.fill(
      peniko::Fill::NonZero,
      self.transform(),
      peniko::Brush::Solid(encode_color(color)),
      None,
      shape,
    );
  }
}

#[derive(Copy, Clone)]
pub enum CursorMode {
  Line,
  Block,
  Underline,
}

// NB: This is in pixels, not scaled. This is intentional, as we always want the
// cursor to appear crisp.
const CURSOR_WIDTH: f64 = 2.0;

impl TextLayout {
  pub fn cursor(&self, index: usize, mode: CursorMode) -> Rect {
    let cursor = parley::Cursor::from_byte_index(&self.layout, index, parley::Affinity::Downstream);
    let rect = match cursor.visual_clusters(&self.layout) {
      [_, Some(cluster)] => {
        let line = cluster.line();
        let metrics = line.metrics();

        let width = match mode {
          CursorMode::Line => CURSOR_WIDTH,
          CursorMode::Block | CursorMode::Underline => cluster.advance() as f64,
        };

        let x = cluster.visual_offset().unwrap_or_default() as f64;
        Rect::new(
          x,
          match mode {
            CursorMode::Underline => metrics.max_coord as f64 - CURSOR_WIDTH,
            _ => metrics.min_coord as f64,
          },
          x + width,
          metrics.max_coord as f64,
        )
      }

      [Some(cluster), _] => {
        let line = cluster.line();
        let metrics = line.metrics();

        match mode {
          CursorMode::Line => {}
          CursorMode::Block | CursorMode::Underline => return Rect::ZERO,
        };

        let x = cluster.visual_offset().unwrap_or_default() as f64 + cluster.advance() as f64;
        Rect::new(
          x,
          match mode {
            CursorMode::Underline => metrics.max_coord as f64 - CURSOR_WIDTH,
            _ => metrics.min_coord as f64,
          },
          x + CURSOR_WIDTH,
          metrics.max_coord as f64,
        )
      }

      _ => Rect::ZERO,
    };

    rect.scale_from_origin(1.0 / self.scale) + self.origin.to_vec2()
  }

  pub fn bounds(&self) -> Rect {
    let rect =
      Rect::new(0.0, 0.0, f64::from(self.layout.full_width()), f64::from(self.layout.height()));
    rect.scale_from_origin(1.0 / self.scale) + self.origin.to_vec2()
  }
}
