use std::path::PathBuf;

use pathfinder_color::ColorU;
use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::Vector2F;

use super::{
    AfterLayoutContext, AppContext, Element, EventContext, LayoutContext, PaintContext, Point,
    SizeConstraint,
};
use crate::event::DispatchedEvent;

pub struct Video {
    path: PathBuf,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Video {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            size: None,
            origin: None,
        }
    }
}

impl Element for Video {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _: &mut LayoutContext,
        _: &AppContext,
    ) -> Vector2F {
        let pick = |max: f32, min: f32| if max.is_infinite() { min } else { max };
        let size = Vector2F::new(
            pick(constraint.max.x(), constraint.min.x()),
            pick(constraint.max.y(), constraint.min.y()),
        );
        self.size = Some(size);
        size
    }

    fn after_layout(&mut self, _: &mut AfterLayoutContext, _: &AppContext) {}

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        let Some(size) = self.size else {
            return;
        };
        let rect = RectF::new(origin, size);
        ctx.scene
            .draw_rect_with_hit_recording(rect)
            .with_background(ColorU::black());
        ctx.scene.draw_video(rect, self.path.clone());
    }

    fn dispatch_event(
        &mut self,
        _: &DispatchedEvent,
        _: &mut EventContext,
        _: &AppContext,
    ) -> bool {
        false
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }
}
