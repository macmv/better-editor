use kurbo::{Arc, BezPath, Point, Rect, RoundedRect, RoundedRectRadii, Size, Stroke, Vec2};

use crate::{
  Widget, WidgetId,
  widget::{Borders, Corners},
};

pub struct Border {
  borders: Borders,
  radius:  Corners,

  inner: WidgetId,
}

impl Border {
  pub fn new(borders: Borders, inner: WidgetId) -> Self {
    Border::new_with_radius(borders, Corners::all(0.0), inner)
  }

  pub fn new_with_radius(borders: Borders, radius: Corners, inner: WidgetId) -> Self {
    Border { borders, radius, inner }
  }
}

impl Widget for Border {
  fn layout(&mut self, layout: &mut crate::Layout) -> Option<kurbo::Size> {
    let size = layout.layout(self.inner);
    layout.set_bounds(
      self.inner,
      Rect::new(
        self.borders.left,
        self.borders.top,
        self.borders.left + size.width,
        self.borders.top + size.height,
      ),
    );

    Some(Size::new(
      self.borders.left + size.width + self.borders.right,
      self.borders.top + size.height + self.borders.bottom,
    ))
  }

  fn children(&self) -> &[crate::WidgetId] { std::slice::from_ref(&self.inner) }

  fn draw(&mut self, render: &mut crate::Render) {
    if self.radius == Corners::all(0.0) {
      if self.borders.left > 0.0 {
        render
          .fill(&Rect::new(0.0, 0.0, self.borders.left, render.size().height), render.theme().text);
      }
      if self.borders.top > 0.0 {
        render
          .fill(&Rect::new(0.0, 0.0, render.size().width, self.borders.top), render.theme().text);
      }
      if self.borders.right > 0.0 {
        render.fill(
          &Rect::new(
            render.size().width - self.borders.right,
            0.0,
            render.size().width,
            render.size().height,
          ),
          render.theme().text,
        )
      }
      if self.borders.bottom > 0.0 {
        render.fill(
          &Rect::new(
            0.0,
            render.size().height - self.borders.bottom,
            render.size().width,
            render.size().height,
          ),
          render.theme().text,
        )
      }
    } else if self.borders.left == self.borders.right
      && self.borders.top == self.borders.bottom
      && self.borders.left == self.borders.top
    {
      render.stroke(
        &RoundedRect::from_rect(
          Rect::from_origin_size((0.0, 0.0), render.size()).inset(-self.borders.left / 2.0),
          RoundedRectRadii::new(
            self.radius.top_left,
            self.radius.top_right,
            self.radius.bottom_right,
            self.radius.bottom_left,
          ),
        ),
        render.theme().text,
        Stroke::new(self.borders.left),
      );
    } else {
      let mut path = BezPath::new();

      path.move_to(Point::new(self.borders.left, self.borders.top + self.radius.top_left));
      if self.radius.top_left > 0.0 {
        path.extend(
          Arc::new(
            Point::new(
              self.borders.left + self.radius.top_left,
              self.borders.top + self.radius.top_left,
            ),
            Vec2::splat(self.radius.top_left),
            180.0_f64.to_radians(),
            90.0_f64.to_radians(),
            0.0,
          )
          .append_iter(0.1),
        );
      }
      path.line_to(Point::new(
        render.size().width - self.borders.right - self.radius.top_right,
        self.borders.top,
      ));
      if self.radius.top_right > 0.0 {
        path.extend(
          Arc::new(
            Point::new(
              render.size().width - self.borders.right - self.radius.top_right,
              self.borders.top + self.radius.top_right,
            ),
            Vec2::splat(self.radius.top_right),
            270.0_f64.to_radians(),
            90.0_f64.to_radians(),
            0.0,
          )
          .append_iter(0.1),
        );
      }
      path.line_to(Point::new(
        render.size().width - self.borders.right,
        render.size().height - self.borders.bottom - self.radius.bottom_right,
      ));
      if self.radius.bottom_right > 0.0 {
        path.extend(
          Arc::new(
            Point::new(
              render.size().width - self.borders.right - self.radius.bottom_right,
              render.size().height - self.borders.bottom - self.radius.bottom_right,
            ),
            Vec2::splat(self.radius.bottom_right),
            0.0_f64.to_radians(),
            90.0_f64.to_radians(),
            0.0,
          )
          .append_iter(0.1),
        );
      }
      path.line_to(Point::new(
        self.borders.left + self.radius.bottom_left,
        render.size().height - self.borders.bottom,
      ));
      if self.radius.bottom_left > 0.0 {
        path.extend(
          Arc::new(
            Point::new(
              self.borders.left + self.radius.bottom_left,
              render.size().height - self.borders.bottom - self.radius.bottom_left,
            ),
            Vec2::splat(self.radius.bottom_left),
            90.0_f64.to_radians(),
            90.0_f64.to_radians(),
            0.0,
          )
          .append_iter(0.1),
        );
      }
      path.line_to(Point::new(self.borders.left, self.borders.top + self.radius.top_left));

      path.line_to(Point::new(0.0, self.borders.top + self.radius.top_left));
      path.line_to(Point::new(
        0.0,
        render.size().height - self.borders.bottom - self.radius.bottom_left,
      ));
      if self.radius.bottom_left > 0.0 {
        path.extend(
          Arc::new(
            Point::new(
              self.borders.left + self.radius.bottom_left,
              render.size().height - self.borders.bottom - self.radius.bottom_left,
            ),
            Vec2::new(
              self.radius.bottom_left + self.borders.left,
              self.radius.bottom_left + self.borders.bottom,
            ),
            180.0_f64.to_radians(),
            -90.0_f64.to_radians(),
            0.0,
          )
          .append_iter(0.1),
        );
      }
      path.line_to(Point::new(
        render.size().width - self.borders.right - self.radius.bottom_right,
        render.size().height,
      ));
      if self.radius.bottom_right > 0.0 {
        path.extend(
          Arc::new(
            Point::new(
              render.size().width - self.borders.right - self.radius.bottom_right,
              render.size().height - self.borders.bottom - self.radius.bottom_right,
            ),
            Vec2::new(
              self.radius.bottom_right + self.borders.right,
              self.radius.bottom_right + self.borders.bottom,
            ),
            90.0_f64.to_radians(),
            -90.0_f64.to_radians(),
            0.0,
          )
          .append_iter(0.1),
        );
      }
      path.line_to(Point::new(render.size().width, self.borders.top + self.radius.top_right));
      if self.radius.top_right > 0.0 {
        path.extend(
          Arc::new(
            Point::new(
              render.size().width - self.borders.right - self.radius.top_right,
              self.borders.top + self.radius.top_right,
            ),
            Vec2::new(
              self.radius.top_right + self.borders.right,
              self.radius.top_right + self.borders.top,
            ),
            0.0_f64.to_radians(),
            -90.0_f64.to_radians(),
            0.0,
          )
          .append_iter(0.1),
        );
      }
      path.line_to(Point::new(self.borders.left + self.radius.top_left, 0.0));
      if self.radius.top_left > 0.0 {
        path.extend(
          Arc::new(
            Point::new(
              self.borders.left + self.radius.top_left,
              self.borders.top + self.radius.top_left,
            ),
            Vec2::new(
              self.radius.top_left + self.borders.left,
              self.radius.top_left + self.borders.top,
            ),
            270.0_f64.to_radians(),
            -90.0_f64.to_radians(),
            0.0,
          )
          .append_iter(0.1),
        );
      }

      path.close_path();

      render.fill(&path, render.theme().text);
    }
  }
}
