use be_terminal::Terminal;
use kurbo::Rect;

use crate::{Render, TextLayout, oklch};

pub struct Shell {
  terminal: Terminal,

  cached_layouts: Vec<TextLayout>,
  cached_scale:   f64,
}

impl Shell {
  pub fn new() -> Self {
    Shell { terminal: Terminal::new(), cached_layouts: vec![], cached_scale: 0.0 }
  }

  pub fn draw(&mut self, render: &mut Render) {
    if self.cached_scale != render.scale() {
      self.cached_layouts.clear();
      self.cached_scale = render.scale();
    }

    // TODO
    /*
    if self.editor.take_damage_all() {
      self.cached_layouts.clear();
    }

    for line in self.editor.take_damages() {
      self.cached_layouts.remove(&line.as_usize());
    }
    */

    render
      .fill(&Rect::new(0.0, 0.0, render.size().width, render.size().height), oklch(0.3, 0.0, 0.0));

    let line_height = render.store.text.font_metrics().line_height;

    for line in 0..10 {
      let Some(layout) = self.layout_line(render, line) else { break };

      render.draw_text(&layout, (20.0, (line + 1) as f64 * line_height));
    }
  }

  fn layout_line(&mut self, render: &mut Render, i: usize) -> Option<&mut TextLayout> {
    if self.cached_layouts.len() < i {
      return Some(&mut self.cached_layouts[i]);
    }

    let line = self.terminal.line(i)?;

    let layout = render.store.text.layout_builder(&line, render.theme().text, render.scale());

    let layout = layout.build(&line);
    let layout = render.build_layout(layout);

    self.cached_layouts[i] = layout;
    Some(&mut self.cached_layouts[i])
  }
}
