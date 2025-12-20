use kurbo::{Affine, Point, Rect};
use peniko::Fill;

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

    for line in text.layout.lines() {
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
          .transform(Affine::translate((text.origin.to_vec2() + offset) * self.scale))
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
  }
}
