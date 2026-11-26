use std::path::PathBuf;
use std::time::Duration;

use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use warp::integration_testing::step::new_step_with_default_assertions;
use warp::integration_testing::terminal::wait_until_bootstrapped_single_pane_for_tab;
use warp::integration_testing::view_getters::workspace_view;
use warp::workspace::WorkspaceAction;
use warpui_core::async_assert;
use warpui_core::integration::TestStep;

use super::Builder;

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

struct DocumentProbe {
    window_number: isize,
    visible: bool,
    has_item: bool,
}

fn find_document() -> Option<DocumentProbe> {
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
                if !class_name(view).ends_with("WarpDocumentView") {
                    continue;
                }
                let hidden: bool = msg_send![view, isHidden];
                let preview: *mut AnyObject = msg_send![view, preview];
                let item: *mut AnyObject = msg_send![preview, previewItem];
                return Some(DocumentProbe {
                    window_number: msg_send![window, windowNumber],
                    visible: !hidden,
                    has_item: !item.is_null(),
                });
            }
        }
        None
    }
}

pub fn test_document_preview() -> Builder {
    let path = PathBuf::from(
        std::env::var("SPIRIT_TEST_DOCUMENT").expect("SPIRIT_TEST_DOCUMENT must name a document"),
    );
    let shot = std::env::var("SPIRIT_TEST_SHOT").ok();
    Builder::new()
        .with_real_display()
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(
            TestStep::new("Open the document")
                .with_action(move |app, window_id, _| {
                    let workspace = workspace_view(app, window_id);
                    let path = path.clone();
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
                })
                .set_timeout(Duration::from_secs(30))
                .add_named_assertion("preview is visible", |_, _| {
                    let probe = find_document();
                    async_assert!(
                        probe
                            .as_ref()
                            .is_some_and(|probe| probe.visible && probe.has_item),
                        "no visible document preview"
                    )
                })
                .set_post_step_pause(Duration::from_secs(4)),
        )
        .with_step(TestStep::new("Capture the window").add_named_assertion(
            "captured",
            move |_, _| {
                if let (Some(shot), Some(probe)) = (shot.as_ref(), find_document()) {
                    let _ = std::process::Command::new("screencapture")
                        .args(["-x", "-o", "-l", &probe.window_number.to_string(), shot])
                        .status();
                }
                async_assert!(true, "")
            },
        ))
        .with_step(new_step_with_default_assertions("Keep the app responsive"))
}

fn open_document(app: &mut warpui_core::App, window_id: warpui_core::WindowId, path: PathBuf) {
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

pub fn test_document_preview_loads_after_download() -> Builder {
    let source = PathBuf::from(
        std::env::var("SPIRIT_TEST_DOCUMENT").expect("SPIRIT_TEST_DOCUMENT must name a document"),
    );
    let target = std::env::temp_dir()
        .join("spirit-late-document")
        .join(source.file_name().expect("file name"));
    let _ = std::fs::remove_file(&target);
    let _ = std::fs::create_dir_all(target.parent().expect("parent"));
    let opened = target.clone();
    let arrived = target.clone();
    let reopened = target.clone();
    Builder::new()
        .with_real_display()
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(
            TestStep::new("Open a document that is still downloading")
                .with_action(move |app, window_id, _| open_document(app, window_id, opened.clone()))
                .set_post_step_pause(Duration::from_secs(2))
                .add_named_assertion("no preview while loading", |_, _| {
                    async_assert!(
                        find_document().is_none(),
                        "preview shown before the file exists"
                    )
                }),
        )
        .with_step(
            TestStep::new("The download finishes")
                .with_action(move |app, window_id, _| {
                    std::fs::copy(&source, &arrived).expect("copy document");
                    open_document(app, window_id, reopened.clone());
                })
                .set_timeout(Duration::from_secs(20))
                .add_named_assertion("preview appears", |_, _| {
                    let probe = find_document();
                    async_assert!(
                        probe
                            .as_ref()
                            .is_some_and(|probe| probe.visible && probe.has_item),
                        "document never appeared"
                    )
                }),
        )
        .with_step(new_step_with_default_assertions("Keep the app responsive"))
}
