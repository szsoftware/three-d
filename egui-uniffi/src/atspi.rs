//! The accessibility tree for the screen readers of a Linux desktop (AT-SPI: Orca reads it, Accerciser shows it), where
//! Compose has no accessibility of its own: AccessKit's adapter for Unix publishes egui's tree on the accessibility bus
//! and hands what a reader does back to the overlay.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use three_d::egui::{self, accesskit};

/// What the adapter's thread and the overlay share.
#[derive(Default)]
struct Inbox {
    /// What a screen reader asked for since the last frame.
    requests: Mutex<Vec<accesskit::ActionRequest>>,
    /// [Inbox::requests] has some: the next frame should run.
    pending: AtomicBool,
    /// The last tree, for a reader that comes later.
    last: Mutex<Option<accesskit::TreeUpdate>>,
}

struct Activation(Arc<Inbox>);

impl accesskit::ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<accesskit::TreeUpdate> {
        self.0
            .last
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

struct Actions(Arc<Inbox>);

impl accesskit::ActionHandler for Actions {
    fn do_action(&mut self, request: accesskit::ActionRequest) {
        self.0
            .requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(request);
        self.0.pending.store(true, Ordering::Release);
    }
}

struct Deactivation;

impl accesskit::DeactivationHandler for Deactivation {
    fn deactivate_accessibility(&mut self) {}
}

/// The overlay on the accessibility bus as the application `name`.
pub(crate) struct Bridge {
    adapter: accesskit_unix::Adapter,
    inbox: Arc<Inbox>,
    name: String,
}

impl Bridge {
    pub fn new(name: String) -> Self {
        let inbox = Arc::new(Inbox::default());
        let adapter = accesskit_unix::Adapter::new(
            Activation(inbox.clone()),
            Actions(inbox.clone()),
            Deactivation,
        );
        Self {
            adapter,
            inbox,
            name,
        }
    }

    /// egui's tree of a frame, with the name of the application on its window.
    pub fn update(&mut self, mut update: accesskit::TreeUpdate) {
        if let Some(tree) = &mut update.tree {
            tree.toolkit_name = Some("egui".into());
            tree.toolkit_version = Some("0.34".into());
            let root = tree.root;
            if let Some((_, node)) = update.nodes.iter_mut().find(|(id, _)| *id == root) {
                if node.label().is_none() {
                    node.set_label(self.name.as_str());
                }
            }
        }
        *self.inbox.last.lock().unwrap_or_else(|e| e.into_inner()) = Some(update.clone());
        self.adapter.update_if_active(|| update);
    }

    /// Where the picture is on the screen, in pixels, and whether its window has the keyboard.
    pub fn window(&mut self, x: f64, y: f64, width: f64, height: f64, focused: bool) {
        let rect = accesskit::Rect {
            x0: x,
            y0: y,
            x1: x + width,
            y1: y + height,
        };
        self.adapter.set_root_window_bounds(rect, rect);
        self.adapter.update_window_focus_state(focused);
    }

    /// Whether a screen reader asked for something the next frame should do.
    pub fn pending(&self) -> bool {
        self.inbox.pending.load(Ordering::Acquire)
    }

    /// What a screen reader asked for, as egui's events.
    pub fn take(&self) -> Vec<egui::Event> {
        self.inbox.pending.store(false, Ordering::Release);
        std::mem::take(
            &mut *self
                .inbox
                .requests
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        )
        .into_iter()
        .map(egui::Event::AccessKitActionRequest)
        .collect()
    }
}
