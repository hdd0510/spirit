use pathfinder_geometry::vector::vec2f;

use super::*;
use crate::rendering;

#[test]
fn test_hit_rect_recording() {
    let mut scene = Scene::new(1., rendering::Config::default());
    assert_eq!(ZIndex::new(0), scene.z_index());

    scene.draw_rect_with_hit_recording(RectF::new(vec2f(0., 0.), vec2f(100., 100.)));
    assert_eq!(ZIndex::new(0), scene.z_index());
    assert!(!scene.is_covered(Point::new(10., 10., ZIndex::new(0))));

    scene.start_layer(ClipBounds::ActiveLayer);
    scene.draw_rect_with_hit_recording(RectF::new(vec2f(50., 50.), vec2f(100., 100.)));
    assert_eq!(ZIndex::new(1), scene.z_index());
    assert!(!scene.is_covered(Point::new(10., 10., ZIndex::new(0))));
    assert!(scene.is_covered(Point::new(60., 60., ZIndex::new(0))));

    scene.start_layer(ClipBounds::ActiveLayer);
    scene.draw_rect_with_hit_recording(RectF::new(vec2f(0., 0.), vec2f(100., 100.)));
    assert_eq!(ZIndex::new(2), scene.z_index());
    assert!(scene.is_covered(Point::new(10., 10., ZIndex::new(0))));
    assert!(scene.is_covered(Point::new(60., 60., ZIndex::new(1))));
}

#[test]
fn test_nested_clip_bounds_with_intersection() {
    let mut scene = Scene::new(1., rendering::Config::default());

    let bounds1 = RectF::new(Vector2F::zero(), Vector2F::new(10., 10.));
    scene.start_layer(ClipBounds::BoundedBy(bounds1));

    let bounds2 = RectF::new(Vector2F::new(5., 5.), Vector2F::new(10., 10.));
    scene.start_layer(ClipBounds::BoundedByActiveLayerAnd(bounds2));

    assert_eq!(
        scene.active_layer().clip_bounds,
        Some(RectF::new(Vector2F::new(5., 5.), Vector2F::new(5., 5.)))
    );
}

#[test]
fn test_nested_clip_bounds_with_no_intersection() {
    let mut scene = Scene::new(1., rendering::Config::default());

    let bounds1 = RectF::new(Vector2F::zero(), Vector2F::new(10., 10.));
    scene.start_layer(ClipBounds::BoundedBy(bounds1));

    let bounds2 = RectF::new(Vector2F::new(100., 100.), Vector2F::new(10., 10.));
    scene.start_layer(ClipBounds::BoundedByActiveLayerAnd(bounds2));

    // We should have explicit bounds for this layer.  (None represents a lack
    // of clipping, not clipping they layer down to nothingness.)
    assert!(scene.active_layer().clip_bounds.is_some());
    // The clip bounds should have an area of zero.
    assert!(scene.active_layer().clip_bounds.unwrap().is_empty());
}

#[test]
fn test_click_through_layer_does_not_cover_lower_layers() {
    let mut scene = Scene::new(1., rendering::Config::default());

    scene.start_layer(ClipBounds::ActiveLayer);
    scene.set_active_layer_click_through();
    scene.draw_rect_with_hit_recording(RectF::new(vec2f(0., 0.), vec2f(100., 100.)));

    assert!(!scene.is_covered(Point::new(10., 10., ZIndex::new(0))));
}

#[test]
fn video_is_covered_only_by_visible_content_above_it() {
    use std::path::PathBuf;

    use pathfinder_color::ColorU;
    use pathfinder_geometry::rect::RectF;

    use crate::geometry::vector::vec2f;

    let video = RectF::new(vec2f(100., 100.), vec2f(200., 100.));
    let is_covered = |rect: RectF, color: ColorU, above: bool| {
        let mut scene = Scene::new(1., Default::default());
        if !above {
            scene
                .draw_rect_with_hit_recording(rect)
                .with_background(color);
        }
        scene.draw_video(video, PathBuf::from("/a.mp4"));
        if above {
            scene.start_overlay_layer(ClipBounds::None);
            scene
                .draw_rect_with_hit_recording(rect)
                .with_background(color);
            scene.stop_layer();
        }
        let covered = scene.videos().next().unwrap().1;
        covered
    };
    let menu = RectF::new(vec2f(150., 80.), vec2f(50., 50.));
    let neighbor = RectF::new(vec2f(300., 100.), vec2f(50., 100.));
    assert!(is_covered(menu, ColorU::black(), true));
    assert!(!is_covered(menu, ColorU::transparent_black(), true));
    assert!(!is_covered(neighbor, ColorU::black(), true));
    assert!(!is_covered(menu, ColorU::black(), false));
}
