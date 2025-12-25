use be_input::{Action, Edit};
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
    Shell {
      terminal:       Terminal::new(be_terminal::Size { rows: 40, cols: 80 }),
      cached_layouts: vec![],
      cached_scale:   0.0,
    }
  }

  pub fn perform_action(&mut self, action: Action) {
    match action {
      Action::Edit { count: _, e: Edit::Insert(c) } => self.terminal.perform_input(c),

      _ => {}
    }
  }

  pub fn draw(&mut self, render: &mut Render) {
    if self.cached_scale != render.scale() {
      self.cached_layouts.clear();
      self.cached_scale = render.scale();
    }

    self.terminal.update();

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
    let character_width = render.store.text.font_metrics().character_width;
    let height = (render.size().height / line_height).floor() as usize;
    let width = (render.size().width / character_width).floor() as usize;

    self.terminal.set_size(be_terminal::Size { rows: height, cols: width });

    for line in 0..height {
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

    if self.cached_layouts.len() == i {
      self.cached_layouts.push(layout);
    } else {
      self.cached_layouts[i] = layout;
    }

    Some(&mut self.cached_layouts[i])
  }
}
