use std::sync::Arc;

use kurbo::{Affine, Point, Rect, Vec2};
use peniko::{
  Blob, Fill, ImageBrush, ImageData,
  color::{AlphaColor, Srgb},
};
use png::{BitDepth, ColorType, Transformations};
use skrifa::{
  GlyphId, MetadataProvider,
  bitmap::{self, BitmapFormat},
  color::ColorGlyph,
  prelude::*,
  raw::TableProvider,
};

use crate::{Color, CursorMode, Render, encode_color, render::RenderStore};

#[derive(Clone, Default)]
pub struct FontMetrics {
  pub line_height:     f64,
  pub character_width: f64,
}

pub struct TextLayout {
  metrics: FontMetrics,

  origin: Point,
  layout: parley::Layout<peniko::Brush>,
  scale:  f64,
}

impl RenderStore {
  pub fn update_metrics(&mut self) {
    const TEXT: &str = " ";
    let mut builder = self.layout.ranged_builder(&mut self.font, TEXT, 1.0, false);
    builder.push_default(parley::StyleProperty::FontSize(16.0));
    builder
      .push_default(parley::StyleProperty::FontStack(parley::FontStack::Source("Iosevka".into())));
    let mut layout = builder.build(TEXT);

    layout.break_all_lines(None);
    layout.align(None, parley::Alignment::Start, parley::AlignmentOptions::default());

    let line = layout.lines().next().unwrap();
    let parley::PositionedLayoutItem::GlyphRun(glyph_run) = line.items().next().unwrap() else {
      unreachable!()
    };

    let metrics = glyph_run.run().metrics();

    self.font_metrics.line_height = f64::from(metrics.line_height);
    self.font_metrics.character_width = f64::from(glyph_run.run().advance());
  }
}

impl Render<'_> {
  pub fn layout_text(&mut self, text: &str, pos: impl Into<Point>, color: Color) -> TextLayout {
    let mut builder = self.store.layout.ranged_builder(&mut self.store.font, &text, 1.0, false);
    builder.push_default(parley::StyleProperty::Brush(encode_color(color).into()));
    builder.push_default(parley::StyleProperty::FontSize(16.0 * self.scale as f32));
    builder
      .push_default(parley::StyleProperty::FontStack(parley::FontStack::Source("Iosevka".into())));
    let mut layout = builder.build(&text);

    layout.break_all_lines(None);
    layout.align(None, parley::Alignment::Start, parley::AlignmentOptions::default());

    TextLayout {
      metrics: self.store.font_metrics.clone(),
      origin: pos.into(),
      layout,
      scale: self.scale,
    }
  }

  pub fn draw_text(&mut self, text: &TextLayout) {
    let mut rect =
      Rect::new(0.0, 0.0, f64::from(text.layout.full_width()), f64::from(text.layout.height()));

    let offset = self.offset();

    let transform = Affine::translate((text.origin.to_vec2() + offset) * self.scale);

    for line in text.layout.lines() {
      for item in line.items() {
        let parley::PositionedLayoutItem::GlyphRun(glyph_run) = item else { continue };

        let run = glyph_run.run();
        rect.y0 = rect.y1.round() - rect.height();
        let mut x = rect.x0 as f32 + glyph_run.offset();
        let baseline = (rect.y0 as f32 + glyph_run.baseline()).round();

        let font_data = run.font();
        let font = skrifa::FontRef::from_index(font_data.data.as_ref(), font_data.index).unwrap();
        let bitmaps = font.bitmap_strikes();
        let glyph_transform =
          run.synthesis().skew().map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0));

        let glyphs = glyph_run.glyphs().map(|glyph| {
          let gx = x + glyph.x;
          let gy = baseline + glyph.y;
          x += glyph.advance;
          vello::Glyph { id: glyph.id.into(), x: gx, y: gy }
        });

        if font.colr().is_ok() && font.cpal().is_ok() || !bitmaps.is_empty() {
          // Emojis need color conversion, so we rasterize them by hand.
          self.draw_emoji(&glyph_run, transform, glyph_transform, glyphs);
        } else {
          // Normal characters can be drawn with vello.
          self
            .scene
            .draw_glyphs(run.font())
            .brush(&glyph_run.style().brush)
            .hint(true)
            .transform(transform)
            .glyph_transform(glyph_transform)
            .font_size(run.font_size())
            .normalized_coords(run.normalized_coords())
            .draw(Fill::NonZero, glyphs);
        }
      }
    }
  }

  fn draw_emoji(
    &mut self,
    glyph_run: &parley::GlyphRun<peniko::Brush>,
    transform: Affine,
    glyph_transform: Option<Affine>,
    mut glyphs: impl Iterator<Item = vello::Glyph>,
  ) {
    let run = glyph_run.run();
    let font = run.font();
    let font_size = run.font_size();

    let blob = &font.data.clone();
    let font = skrifa::FontRef::from_index(blob.as_ref(), font.index).unwrap();
    let upem: f32 = font.head().map(|h| h.units_per_em()).unwrap().into();
    let colr_scale =
      Affine::scale_non_uniform((font_size / upem).into(), (-font_size / upem).into());

    let color_collection = font.color_glyphs();
    let bitmaps = font.bitmap_strikes();
    // Only used for COLR glyphs
    /*
    let coords = run.normalized_coords();
    let location = LocationRef::new(&bytemuck::cast_slice(coords));
    */

    loop {
      let Some((emoji, glyph)) = (&mut glyphs).find_map(|glyph| {
        let glyph_id = GlyphId::new(glyph.id);
        if let Some(color) = color_collection.get(glyph_id) {
          Some((EmojiLikeGlyph::Colr(color), glyph))
        } else {
          let bitmap = bitmaps.glyph_for_size(Size::new(font_size), glyph_id)?;
          Some((EmojiLikeGlyph::Bitmap(bitmap), glyph))
        }
      }) else {
        break;
      };

      match emoji {
        EmojiLikeGlyph::Bitmap(bitmap) => {
          let image = match bitmap.data {
            bitmap::BitmapData::Bgra(data) => {
              if bitmap.width * bitmap.height * 4 != u32::try_from(data.len()).unwrap() {
                continue;
              }

              let data: Box<[u8]> = data
                .chunks_exact(4)
                .flat_map(|bytes| {
                  let [b, g, r, a] = bytes.try_into().unwrap();

                  let encoded = encode_color(AlphaColor::<Srgb>::from_rgba8(r, g, b, a).convert());
                  encoded.to_rgba8().to_u8_array()
                })
                .collect();

              ImageData {
                data:       Blob::new(Arc::new(data)),
                format:     peniko::ImageFormat::Rgba8,
                alpha_type: peniko::ImageAlphaType::Alpha,
                width:      bitmap.width,
                height:     bitmap.height,
              }
            }
            bitmap::BitmapData::Png(data) => {
              let mut decoder = png::Decoder::new(data);
              decoder.set_transformations(Transformations::ALPHA | Transformations::STRIP_16);
              let Ok(mut reader) = decoder.read_info() else { continue };

              if reader.output_color_type() != (ColorType::Rgba, BitDepth::Eight) {
                continue;
              }
              let mut buf = vec![0; reader.output_buffer_size()].into_boxed_slice();

              let info = reader.next_frame(&mut buf).unwrap();
              if info.width != bitmap.width || info.height != bitmap.height {
                continue;
              }

              let data: Box<[u8]> = buf
                .chunks_exact(4)
                .flat_map(|bytes| {
                  let [r, g, b, a] = bytes.try_into().unwrap();

                  let encoded = encode_color(AlphaColor::<Srgb>::from_rgba8(r, g, b, a).convert());
                  encoded.to_rgba8().to_u8_array()
                })
                .collect();

              ImageData {
                data:       Blob::new(Arc::new(data)),
                format:     peniko::ImageFormat::Rgba8,
                alpha_type: peniko::ImageAlphaType::Alpha,
                width:      bitmap.width,
                height:     bitmap.height,
              }
            }

            _ => continue,
          };
          let image = ImageBrush::new(image);
          let transform = transform.then_translate(Vec2::new(glyph.x.into(), glyph.y.into()));

          // Logic copied from Skia without examination or careful understanding:
          // https://github.com/google/skia/blob/61ac357e8e3338b90fb84983100d90768230797f/src/ports/SkTypeface_fontations.cpp#L664

          let image_scale_factor = font_size / bitmap.ppem_y;
          let font_units_to_size = font_size / upem;

          // CoreText appears to special case Apple Color Emoji, adding
          // a 100 font unit vertical offset. We do the same but only
          // when both vertical offsets are 0 to avoid incorrect
          // rendering if Apple ever does encode the offset directly in
          // the font.
          let bearing_y = if bitmap.bearing_y == 0.0 && bitmaps.format() == Some(BitmapFormat::Sbix)
          {
            100.0
          } else {
            bitmap.bearing_y
          };

          let transform = transform
            .pre_translate(Vec2 {
              x: (-bitmap.bearing_x * font_units_to_size).into(),
              y: (bearing_y * font_units_to_size).into(),
            })
            // Unclear why this isn't non-uniform
            .pre_scale(image_scale_factor.into())
            .pre_translate(Vec2 {
              x: (-bitmap.inner_bearing_x).into(),
              y: (-bitmap.inner_bearing_y).into(),
            });
          let mut transform = match bitmap.placement_origin {
            bitmap::Origin::TopLeft => transform,
            bitmap::Origin::BottomLeft => {
              transform.pre_translate(Vec2 { x: 0., y: -f64::from(image.image.height) })
            }
          };
          if let Some(glyph_transform) = glyph_transform {
            transform *= glyph_transform;
          }
          self.scene.draw_image(image.as_ref(), transform);
        }
        EmojiLikeGlyph::Colr(_colr) => {
          let _transform = transform
            * Affine::translate(Vec2::new(glyph.x.into(), glyph.y.into()))
            * colr_scale
            * glyph_transform.unwrap_or(Affine::IDENTITY);
          todo!("render colr glyphs");
          /*
          colr
            .paint(
              location,
              &mut DrawColorGlyphs {
                scene: self.scene,
                cpal: &font.cpal().unwrap(),
                outlines: &font.outline_glyphs(),
                transform_stack: vec![Transform::from_kurbo(&transform)],
                clip_box: DEFAULT_CLIP_RECT,
                clip_depth: 0,
                location,
                foreground_brush: self.brush,
              },
            )
            .unwrap();
          */
        }
      }
    }

    /*
    for g in glyph_run.glyphs() {
      let r = Rect::new(
        (x + g.x) as f64,
        (baseline + g.y - run.metrics().ascent) as f64,
        (x + g.x + g.advance) as f64,
        (baseline + g.y + run.metrics().descent) as f64,
      );

      self.scene.fill(
        Fill::NonZero,
        transform,
        &encode_color(super::oklch(1.0, 0.0, 0.0)),
        None,
        &r,
      );

      x += g.advance;
    }
    */
  }
}

enum EmojiLikeGlyph<'a> {
  Bitmap(bitmap::BitmapGlyph<'a>),
  Colr(ColorGlyph<'a>),
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

      _ => Rect::new(
        0.0,
        match mode {
          CursorMode::Underline => self.metrics.line_height * self.scale - CURSOR_WIDTH,
          _ => 0.0,
        },
        match mode {
          CursorMode::Block | CursorMode::Underline => self.metrics.character_width * self.scale,
          CursorMode::Line => CURSOR_WIDTH,
        },
        self.metrics.line_height * self.scale,
      ),
    };

    rect.scale_from_origin(1.0 / self.scale) + self.origin.to_vec2()
  }

  pub fn bounds(&self) -> Rect {
    let rect =
      Rect::new(0.0, 0.0, f64::from(self.layout.full_width()), f64::from(self.layout.height()));
    rect.scale_from_origin(1.0 / self.scale) + self.origin.to_vec2()
  }
}
