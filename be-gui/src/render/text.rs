use kurbo::{Affine, Point, Rect};
use peniko::Fill;
use skrifa::{MetadataProvider, raw::TableProvider};

use crate::{Color, Render, TextLayout, encode_color};

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

    TextLayout { origin: pos.into(), layout, scale: self.scale }
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

        if font.colr().is_ok() && font.cpal().is_ok() || !bitmaps.is_empty() {
          // Emojis need color conversion, so we rasterize them by hand.
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
        } else {
          // Normal characters can be drawn with vello.
          self
            .scene
            .draw_glyphs(run.font())
            .brush(&glyph_run.style().brush)
            .hint(true)
            .transform(transform)
            .glyph_transform(
              run
                .synthesis()
                .skew()
                .map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0)),
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
    }
  }
}
