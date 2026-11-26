use std::path::PathBuf;
use std::time::Duration;

use objc2::encode::{Encode, Encoding};
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use warp::integration_testing::step::new_step_with_default_assertions;
use warp::integration_testing::terminal::wait_until_bootstrapped_single_pane_for_tab;
use warp::integration_testing::view_getters::workspace_view;
use warp::workspace::WorkspaceAction;
use warpui_core::integration::TestStep;
use warpui_core::{App, WindowId, async_assert};

use super::Builder;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct NsPoint {
    x: f64,
    y: f64,
}

unsafe impl Encode for NsPoint {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct NsRect {
    origin: NsPoint,
    size: NsSize,
}

unsafe impl Encode for NsRect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[NsPoint::ENCODING, NsSize::ENCODING]);
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct NsSize {
    width: f64,
    height: f64,
}

unsafe impl Encode for NsSize {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}

#[derive(Debug)]
struct VideoProbe {
    view: *mut AnyObject,
    window: *mut AnyObject,
    frame: NsRect,
    hidden: bool,
    ready: bool,
    rate: f32,
}

fn class_name(object: *mut AnyObject) -> String {
    unsafe { (*object).class().name().to_string_lossy().into_owned() }
}

fn subviews(view: *mut AnyObject) -> Vec<*mut AnyObject> {
    unsafe {
        let views: *mut AnyObject = msg_send![view, subviews];
        let count: usize = msg_send![views, count];
        (0..count)
            .map(|index| msg_send![views, objectAtIndex: index])
            .collect()
    }
}

fn find_video() -> Option<VideoProbe> {
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let windows: *mut AnyObject = msg_send![app, windows];
        let count: usize = msg_send![windows, count];
        for index in 0..count {
            let window: *mut AnyObject = msg_send![windows, objectAtIndex: index];
            let content: *mut AnyObject = msg_send![window, contentView];
            if content.is_null() {
                continue;
            }
            for view in subviews(content) {
                if !class_name(view).ends_with("WarpVideoView") {
                    continue;
                }
                let player: *mut AnyObject = msg_send![view, player];
                let item: *mut AnyObject = msg_send![player, currentItem];
                let status: isize = msg_send![item, status];
                return Some(VideoProbe {
                    view,
                    window,
                    frame: msg_send![view, frame],
                    hidden: msg_send![view, isHidden],
                    ready: status == 1,
                    rate: msg_send![player, rate],
                });
            }
        }
        None
    }
}

fn find_descendant(
    view: *mut AnyObject,
    matches: &dyn Fn(*mut AnyObject) -> bool,
) -> Option<*mut AnyObject> {
    if matches(view) {
        return Some(view);
    }
    subviews(view)
        .into_iter()
        .find_map(|child| find_descendant(child, matches))
}

fn dump(view: *mut AnyObject, depth: usize) {
    let frame: NsRect = unsafe { msg_send![view, frame] };
    eprintln!(
        "VIDEODUMP {}{} {:.0},{:.0} {:.0}x{:.0}",
        "  ".repeat(depth),
        class_name(view),
        frame.origin.x,
        frame.origin.y,
        frame.size.width,
        frame.size.height
    );
    for child in subviews(view) {
        dump(child, depth + 1);
    }
}

fn dump_windows() {
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let windows: *mut AnyObject = msg_send![app, windows];
        let count: usize = msg_send![windows, count];
        for index in 0..count {
            let window: *mut AnyObject = msg_send![windows, objectAtIndex: index];
            let visible: bool = msg_send![window, isVisible];
            let occlusion: usize = msg_send![window, occlusionState];
            eprintln!(
                "VIDEODUMP window {} visible={visible} occlusion={occlusion}",
                class_name(window)
            );
            let content: *mut AnyObject = msg_send![window, contentView];
            if !content.is_null() {
                dump(content, 1);
            }
        }
    }
}

fn post_mouse(window: *mut AnyObject, kind: usize, point: NsPoint) {
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        let number: isize = msg_send![window, windowNumber];
        let info: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
        let now: f64 = msg_send![info, systemUptime];
        let event: *mut AnyObject = msg_send![
            class!(NSEvent),
            mouseEventWithType: kind,
            location: point,
            modifierFlags: 0usize,
            timestamp: now,
            windowNumber: number,
            context: std::ptr::null_mut::<AnyObject>(),
            eventNumber: 0isize,
            clickCount: 1isize,
            pressure: 1f32
        ];
        let _: () = msg_send![app, postEvent: event, atStart: false];
    }
}

fn open_video(app: &mut App, window_id: WindowId, path: PathBuf) {
    let workspace = workspace_view(app, window_id);
    app.update(|ctx| {
        ctx.dispatch_typed_action_for_view(
            window_id,
            workspace.id(),
            &WorkspaceAction::OpenFileInNewTab {
                full_path: path,
                line_and_column: None,
            },
        );
    });
}

fn label_of(view: *mut AnyObject) -> String {
    unsafe {
        let label: *mut AnyObject = msg_send![view, accessibilityLabel];
        if label.is_null() {
            return String::new();
        }
        let bytes: *const std::ffi::c_char = msg_send![label, UTF8String];
        std::ffi::CStr::from_ptr(bytes)
            .to_string_lossy()
            .into_owned()
    }
}

fn point_in(view: *mut AnyObject, fraction: f64) -> NsPoint {
    unsafe {
        let bounds: NsRect = msg_send![view, bounds];
        let point = NsPoint {
            x: bounds.origin.x + bounds.size.width * fraction,
            y: bounds.origin.y + bounds.size.height / 2.,
        };
        msg_send![view, convertPoint: point, toView: std::ptr::null_mut::<AnyObject>()]
    }
}

fn control(video: &VideoProbe, label: &str) -> *mut AnyObject {
    find_descendant(video.view, &|view| {
        class_name(view).contains("Button") && label_of(view) == label
    })
    .unwrap_or_else(|| panic!("no {label} control"))
}

fn click_at(video: &VideoProbe, point: NsPoint) {
    post_mouse(video.window, 5, point);
    post_mouse(video.window, 1, point);
    post_mouse(video.window, 2, point);
}

fn video_step(
    name: &'static str,
    action: impl Fn(&VideoProbe) + 'static,
    check: impl Fn(&VideoProbe) -> bool + 'static,
) -> TestStep {
    TestStep::new(name)
        .with_action(move |_, _, _| action(&find_video().expect("video")))
        .set_timeout(Duration::from_secs(5))
        .add_named_assertion(name, move |_, _| {
            let video = find_video();
            async_assert!(video.as_ref().is_some_and(&check), "{name}: {video:?}")
        })
        .set_post_step_pause(Duration::from_millis(700))
}

fn seconds(video: &VideoProbe) -> f64 {
    unsafe {
        let slider = find_descendant(video.view, &|view| class_name(view).contains("Slider"))
            .expect("slider");
        msg_send![slider, doubleValue]
    }
}

pub fn test_video_playback() -> Builder {
    let path = PathBuf::from(
        std::env::var("SPIRIT_TEST_VIDEO").expect("SPIRIT_TEST_VIDEO must name a video"),
    );
    Builder::new()
        .with_real_display()
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(
            TestStep::new("Open the video")
                .with_action(move |app, window_id, _| open_video(app, window_id, path.clone()))
                .set_timeout(Duration::from_secs(40))
                .add_named_assertion("player is visible and ready", move |_, _| {
                    let video = find_video();
                    async_assert!(
                        video.as_ref().is_some_and(|video| !video.hidden
                            && video.ready
                            && video.frame.size.width > 100.),
                        "video: {video:?}"
                    )
                })
                .set_post_step_pause(Duration::from_secs(1)),
        )
        .with_step(video_step(
            "Play button starts playback",
            |video| click_at(video, point_in(control(video, "Play"), 0.5)),
            |video| video.rate > 0. && seconds(video) > 0.,
        ))
        .with_step(video_step(
            "Pause button stops playback",
            |video| click_at(video, point_in(control(video, "Pause"), 0.5)),
            |video| video.rate == 0.,
        ))
        .with_step(video_step(
            "Clicking the picture plays",
            |video| click_at(video, point_in(video.view, 0.5)),
            |video| video.rate > 0.,
        ))
        .with_step(video_step(
            "Clicking the picture pauses",
            |video| click_at(video, point_in(video.view, 0.5)),
            |video| video.rate == 0.,
        ))
        .with_step(video_step(
            "Dragging the slider seeks",
            |video| {
                let slider =
                    find_descendant(video.view, &|view| class_name(view).contains("Slider"))
                        .expect("slider");
                post_mouse(video.window, 1, point_in(slider, 0.1));
                for step in 2..=8 {
                    post_mouse(video.window, 6, point_in(slider, step as f64 / 10.));
                }
                post_mouse(video.window, 2, point_in(slider, 0.8));
            },
            |video| (0.7..0.9).contains(&seconds(video)),
        ))
        .with_step(
            TestStep::new("Switch tabs")
                .with_action(|app, window_id, _| {
                    let workspace = workspace_view(app, window_id);
                    app.update(|ctx| {
                        ctx.dispatch_typed_action_for_view(
                            window_id,
                            workspace.id(),
                            &WorkspaceAction::ActivateTab(0),
                        );
                    });
                })
                .add_named_assertion("player is gone", |_, _| {
                    async_assert!(find_video().is_none(), "player still attached")
                }),
        )
        .with_step(new_step_with_default_assertions("Keep the app responsive"))
}
