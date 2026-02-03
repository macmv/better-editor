use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Instant};

use be_config::Config;
use kurbo::{Affine, Axis, Point, Rect, Shape, Size, Stroke, Vec2};
use peniko::{
  Gradient,
  color::{AlphaColor, Oklab, Oklch, Srgb},
};

use crate::{Layout, render::text::TextStore, theme::Theme};

mod blitter;
mod cursor;
mod text;
mod window;

pub use cursor::CursorKind;
pub use text::TextLayout;

#[derive(Debug)]
pub enum Event {
  Refresh,
  OpenFile(PathBuf),
  Exit,
  RunCommand(String),
}

pub struct RenderStore {
  proxy: winit::event_loop::EventLoopProxy<Event>,

  pub lsp:    Rc<RefCell<be_lsp::LanguageServerStore>>,
  pub config: Rc<RefCell<Config>>,
  pub text:   TextStore,
  pub theme:  Theme,

  render: vello::Renderer,
}

#[derive(Clone)]
pub struct Notify {
  proxy: winit::event_loop::EventLoopProxy<Event>,
}

pub struct Render<'a> {
  pub store: &'a mut RenderStore,
  scene:     vello::Scene,
  now:       Instant,

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

pub type Color = AlphaColor<Oklab>;

#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
  Solid(Color),
  Gradient(Gradient),
}

impl From<Color> for Brush {
  fn from(value: Color) -> Self { Brush::Solid(value) }
}

impl Default for Brush {
  fn default() -> Self { Self::Solid(Color::TRANSPARENT) }
}

pub fn oklch(l: f32, c: f32, h: f32) -> Color { AlphaColor::<Oklch>::new([l, c, h, 1.0]).convert() }

/// Converts things to sRGB, so that vello uses OkLAB for everything, and then
/// we undo this conversion in the blitter.
pub fn encode_color(color: Color) -> AlphaColor<Srgb> {
  let [l, a, b, alpha] = color.components;

  AlphaColor::new([l, a + 0.5, b + 0.5, alpha])
}

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

pub fn run() {
  window::run(|device, surface, proxy| {
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

    let config = Rc::new(RefCell::new(Config::load()));

    let mut lsp_store = be_lsp::LanguageServerStore::default();

    {
      let notifier = Notify { proxy: proxy.clone() };
      lsp_store.set_on_message(move || notifier.wake());
    }

    let store = RenderStore {
      proxy,
      lsp: Rc::new(RefCell::new(lsp_store)),
      text: TextStore::new(&config),
      config,
      render: vello::Renderer::new(&device, vello::RendererOptions::default()).unwrap(),
      theme: Theme::default_theme(),
    };

    let mut app = App {
      state: super::State::new(&store),
      store,

      texture,
      texture_view,
      blitter: blitter::TextureBlitterConvert::new(&device, surface.format),
    };

    if let Some(path) = std::env::args().nth(1) {
      app.state.open(std::path::Path::new(&path));
    }

    app
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
    puffin::profile_scope!("render");

    self.store.text.set_scale(scale);

    let mut layout = Layout::new(
      &mut self.store,
      scale,
      Size::new(surface.texture.width() as f64 / scale, surface.texture.height() as f64 / scale),
    );

    {
      puffin::profile_scope!("layout");
      self.state.layout(&mut layout);
    }

    let now = std::time::Instant::now();
    let mut render = Render {
      store: &mut self.store,
      scene: vello::Scene::new(),
      now,
      scale,
      size: Size::new(
        surface.texture.width() as f64 / scale,
        surface.texture.height() as f64 / scale,
      ),
      stack: vec![],
    };

    {
      puffin::profile_scope!("draw");
      self.state.draw(&mut render);
    }

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

impl Brush {
  pub fn encode(self) -> peniko::Brush {
    match self {
      Brush::Solid(color) => peniko::Brush::Solid(encode_color(color)),
      Brush::Gradient(mut gradient) => {
        for stop in gradient.stops.iter_mut() {
          stop.color = encode_color(stop.color.to_alpha_color()).into();
        }

        peniko::Brush::Gradient(gradient)
      }
    }
  }
}

impl RenderStore {
  pub fn theme(&self) -> &Theme { &self.theme }

  pub fn notifier(&self) -> Notify { Notify { proxy: self.proxy.clone() } }
}

impl<'a> Render<'a> {
  pub fn size(&self) -> Size {
    if let Some(top) = self.stack.last() { top.size() } else { self.size }
  }

  /// TODO: Don't expose this.
  pub(crate) fn scale(&self) -> f64 { self.scale }

  pub fn now(&self) -> Instant { self.now }

  pub fn theme(&self) -> &Theme { &self.store.theme }

  pub fn notifier(&self) -> Notify { Notify { proxy: self.store.proxy.clone() } }

  pub fn split<S>(
    &mut self,
    state: &mut S,
    axis: Axis,
    distance: Distance,
    left: impl FnOnce(&mut S, &mut Render),
    right: impl FnOnce(&mut S, &mut Render),
  ) {
    let mut left_bounds = Rect::from_origin_size(Point::ZERO, self.size());
    let mut right_bounds = Rect::from_origin_size(Point::ZERO, self.size());

    match axis {
      Axis::Vertical => {
        let mut distance = distance.to_pixels_in(self.size().width);
        if distance < 0.0 {
          distance += self.size().width;
        }

        left_bounds.x1 = distance;
        right_bounds.x0 = distance;
      }
      Axis::Horizontal => {
        let mut distance = distance.to_pixels_in(self.size().height);
        if distance < 0.0 {
          distance += self.size().height;
        }

        left_bounds.y1 = distance;
        right_bounds.y0 = distance;
      }
    }

    self.clipped(left_bounds, |render| left(state, render));
    self.clipped(right_bounds, |render| right(state, render));
  }

  pub fn clipped(&mut self, mut rect: Rect, f: impl FnOnce(&mut Render)) {
    rect = rect + self.offset();

    let scaled_rect = rect.scale_from_origin(self.scale).round();
    self.stack.push(scaled_rect.scale_from_origin(1.0 / self.scale));
    self.scene.push_clip_layer(Affine::IDENTITY, &scaled_rect);

    f(self);

    self.stack.pop().expect("no clip layer to pop");
    self.scene.pop_layer();
  }

  fn offset(&self) -> Vec2 {
    if let Some(top) = self.stack.last() { top.origin().to_vec2() } else { Vec2::ZERO }
  }

  fn transform(&self) -> Affine { Affine::scale(self.scale) * Affine::translate(self.offset()) }

  pub fn stroke(&mut self, shape: &impl Shape, color: Color, stroke: Stroke) {
    self.stroke_transform(shape, Affine::IDENTITY, color, stroke);
  }

  pub fn stroke_transform(
    &mut self,
    shape: &impl Shape,
    transform: Affine,
    color: Color,
    stroke: Stroke,
  ) {
    self.scene.stroke(
      &stroke,
      self.transform() * transform,
      peniko::Brush::Solid(encode_color(color)),
      None,
      shape,
    );
  }

  pub fn fill(&mut self, shape: &impl Shape, color: Color) {
    self.fill_transform(shape, Affine::IDENTITY, color);
  }

  pub fn fill_transform(&mut self, shape: &impl Shape, transform: Affine, color: Color) {
    self.scene.fill(
      peniko::Fill::NonZero,
      self.transform() * transform,
      peniko::Brush::Solid(encode_color(color)),
      None,
      shape,
    );
  }

  pub fn drop_shadow(&mut self, rect: Rect, radius: f64, std_dev: f64, color: Color) {
    self.scene.draw_blurred_rounded_rect(
      self.transform(),
      rect,
      encode_color(color),
      radius * self.scale,
      std_dev * self.scale,
    );
  }
}

impl Notify {
  pub fn wake(&self) { self.proxy.send_event(Event::Refresh).unwrap(); }

  pub fn open_file(&self, path: PathBuf) { self.proxy.send_event(Event::OpenFile(path)).unwrap(); }

  pub fn exit(&self) { self.proxy.send_event(Event::Exit).unwrap(); }

  pub fn run_cmd(&self, cmd: String) { self.proxy.send_event(Event::RunCommand(cmd)).unwrap(); }
}

#[derive(Debug, Copy, Clone)]
pub enum CursorMode {
  Line,
  Block,
  Underline,
}
