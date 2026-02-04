use be_input::{Action, Direction, Edit, Move};
use kurbo::{Point, Rect, RoundedRect, Stroke};
use unicode_segmentation::UnicodeSegmentation;

use crate::{Notify, Render};

pub struct CommandView {
  notify: Notify,

  command: String,
  cursor:  usize, // in bytes
}

impl CommandView {
  pub fn new(notify: Notify) -> Self { CommandView { command: String::new(), cursor: 0, notify } }

  pub fn layout(&mut self) {}

  pub fn draw(&mut self, render: &mut Render) {
    let bounds = Rect::from_origin_size(Point::ZERO, render.size());

    let radius = 20.0;
    render.fill(&RoundedRect::from_rect(bounds, radius), render.theme().background_raised);
    let stroke = 1.0 / render.scale();
    render.stroke(
      &RoundedRect::from_rect(bounds.inset(-stroke), radius),
      render.theme().background_raised_outline,
      Stroke::new(stroke),
    );

    let bounds = Rect::new(
      20.0,
      render.size().height - 40.0,
      render.size().width - 20.0,
      render.size().height - 20.0,
    );
    render.fill(&bounds, render.theme().background);
    render.stroke(&bounds, render.theme().background_raised_outline, Stroke::new(stroke));

    let layout = render.layout_text(&self.command, render.theme().text);
    let text_pos = Point::new(20.0, render.size().height - 40.0);
    render.draw_text(&layout, text_pos);

    let cursor = layout.cursor(self.cursor as usize, crate::CursorMode::Line);
    render.fill(&(cursor + text_pos.to_vec2()), render.theme().text);
  }

  pub fn perform_action(&mut self, action: Action) {
    match action {
      Action::Move { m: Move::Single(Direction::Left), .. } => self.move_cursor(-1),
      Action::Move { m: Move::Single(Direction::Right), .. } => self.move_cursor(1),

      Action::Edit { e: Edit::Insert('\n'), .. } => {
        self.notify.editor_event(be_editor::EditorEvent::RunCommand(self.command.clone()));
      }
      Action::Edit { e: Edit::Insert(c), .. } => {
        self.command.insert(self.cursor, c);
        self.move_cursor(1);
      }
      Action::Edit { e: Edit::Delete(Move::Single(Direction::Right)), .. } => {
        self.delete_graphemes(1);
      }
      Action::Edit { e: Edit::Backspace, .. } => {
        if self.cursor > 0 {
          self.move_cursor(-1);
          self.delete_graphemes(1);
        }
      }

      _ => {}
    }
  }

  fn move_cursor(&mut self, dist: i32) {
    if dist >= 0 {
      for c in self.command[self.cursor..].graphemes(true).take(dist as usize) {
        self.cursor += c.len();
      }
    } else {
      for c in self.command[..self.cursor].graphemes(true).rev().take(-dist as usize) {
        self.cursor -= c.len();
      }
    }
  }

  fn delete_graphemes(&mut self, len: usize) {
    let count =
      self.command[self.cursor..].graphemes(true).take(len).map(|g| g.len()).sum::<usize>();
    self.command.replace_range(self.cursor..self.cursor + count, "");
  }
}
