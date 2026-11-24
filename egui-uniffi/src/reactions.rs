//!
//! What a node reacts to besides its widget ([UiReactions]): clicks of every kind, the pointer coming and going,
//! dragging, gestures and drag and drop.
//!

use crate::state::{DragPhase, UiPoint, UiState};
use crate::{UiEvent, UiNode, UiReactions, Widget};
use three_d::egui::{self, Id, LayerId, Response, Sense, Ui};

/// What is dragged from a node with [UiReactions::drag_kind], as egui's payload of drag and drop.
pub(crate) struct Dragging {
    node: u64,
    kind: String,
}

/// How long a press lasts until it is long, as egui has it for touch.
const LONG_PRESS: f64 = 0.5;

/// What the scope of a node with `r` senses: the nodes inside come after it and so take what they sense themselves.
pub(crate) fn sense(r: &UiReactions) -> Sense {
    let mut sense = Sense::hover();
    if r.click || r.double_click || r.secondary_click || r.long_press {
        sense |= Sense::click();
    }
    if r.drag || r.drag_kind.is_some() {
        sense |= Sense::drag();
    }
    sense
}

fn ui_key(node: &UiNode) -> Id {
    Id::new(("three-d-egui", node.id, "reactions"))
}

/// The layer a node that is dragged onto another is drawn on, following the pointer; `None` while it is not.
pub(crate) fn lifted(ui: &Ui, node: &UiNode, r: &UiReactions) -> Option<LayerId> {
    let kind = r.drag_kind.as_ref()?;
    let id = ui.data(|d| d.get_temp::<Id>(ui_key(node)))?;
    if !ui.ctx().is_being_dragged(id) {
        return None;
    }
    egui::DragAndDrop::set_payload(
        ui.ctx(),
        Dragging {
            node: node.id,
            kind: kind.clone(),
        },
    );
    Some(LayerId::new(egui::Order::Tooltip, id))
}

/// The events of a node with reactions after it was drawn; `response` is that of its scope (sensing [sense]).
#[allow(clippy::too_many_arguments)]
pub(crate) fn react(
    ui: &Ui,
    state: &mut UiState,
    tree: &crate::tree::Tree,
    node: &UiNode,
    response: &Response,
    r: &UiReactions,
    lifted: Option<LayerId>,
    drop_claimed: &mut bool,
) {
    let ctx = ui.ctx();
    let id = response.id;
    ui.data_mut(|d| d.insert_temp(ui_key(node), id));
    let rect = response.rect;
    let pointer = ui.input(|i| i.pointer.latest_pos());
    let relative = |p: Option<egui::Pos2>| {
        let p = p.unwrap_or(rect.min) - rect.min;
        UiPoint {
            x: p.x as f64,
            y: p.y as f64,
        }
    };
    // Drawn where the pointer is, as far from it as where it was taken
    if let Some(layer) = lifted {
        let (origin, now) = ui.input(|i| (i.pointer.press_origin(), i.pointer.interact_pos()));
        if let (Some(origin), Some(now)) = (origin, now) {
            ctx.transform_layer_shapes(
                layer,
                egui::emath::TSTransform::from_translation(now - origin),
            );
        }
    }
    if let Some(cursor) = r.cursor {
        if response.contains_pointer() && !ctx.is_being_dragged(id) {
            ctx.set_cursor_icon(cursor.into());
        }
    }
    if r.drag_kind.is_some() && response.contains_pointer() && ctx.dragged_id().is_none() {
        ctx.set_cursor_icon(egui::CursorIcon::Grab);
    }
    if r.drag_kind.is_some() && ctx.is_being_dragged(id) {
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    if r.click && response.clicked() {
        state.push(UiEvent::Clicked { id: node.id });
    }
    if r.double_click && response.double_clicked() {
        state.push(UiEvent::DoubleClicked { id: node.id });
    }
    // A long touch is egui's right click, unless it was the node's long press already
    let long_pressed = response.long_touched() && state.long_pressed.contains(&node.id);
    if r.secondary_click && response.secondary_clicked() && !long_pressed {
        state.push(UiEvent::SecondaryClicked {
            id: node.id,
            point: relative(response.interact_pointer_pos().or(pointer)),
        });
    }
    if r.long_press {
        long_press(ui, state, node, response);
    }
    if r.hover {
        let on = response.contains_pointer();
        if on != state.hovered.contains(&node.id) {
            if on {
                state.hovered.insert(node.id);
            } else {
                state.hovered.remove(&node.id);
            }
            state.push(UiEvent::HoverChanged {
                id: node.id,
                hovered: on,
            });
        }
    }
    if r.drag {
        // Two fingers zoom and turn, they do not drag: egui's pointer follows the first one
        let (delta, two) = ui.input(|i| (i.pointer.delta(), i.multi_touch().is_some()));
        let dragged = ctx.is_being_dragged(id) || ctx.drag_started_id() == Some(id);
        let phase = if dragged && !two {
            if state.dragging.insert(node.id) {
                Some(DragPhase::Started)
            } else {
                (delta != egui::Vec2::ZERO).then_some(DragPhase::Moved)
            }
        } else if !dragged && state.dragging.remove(&node.id) {
            Some(DragPhase::Stopped)
        } else {
            None
        };
        if let Some(phase) = phase {
            state.push(UiEvent::Dragged {
                id: node.id,
                phase,
                point: relative(pointer),
                dx: delta.x,
                dy: delta.y,
            });
        }
    }
    if r.gestures && response.contains_pointer() {
        let (zoom, rotation, pan) = ui.input(|i| match i.multi_touch() {
            Some(t) => (t.zoom_delta, t.rotation_delta, t.translation_delta),
            None => (i.zoom_delta(), 0.0, egui::Vec2::ZERO),
        });
        if zoom != 1.0 || rotation != 0.0 || pan != egui::Vec2::ZERO {
            state.push(UiEvent::Gesture {
                id: node.id,
                zoom,
                rotation,
                dx: pan.x,
                dy: pan.y,
            });
        }
    }
    if let Some(kinds) = &r.drop_kinds {
        drop_target(ui, state, tree, node, response, kinds, drop_claimed);
    }
}

/// [UiReactions::long_press]: egui's long touch, or the button held still on it for half a second.
fn long_press(ui: &Ui, state: &mut UiState, node: &UiNode, response: &Response) {
    let down = response.is_pointer_button_down_on();
    // Until the button or the finger comes up: egui's own long touch later takes the press from the node
    if !ui.input(|i| i.pointer.any_down()) {
        state.long_pressed.remove(&node.id);
    }
    let (held, moved) = ui.input(|i| {
        let held = i.pointer.press_start_time().map_or(0.0, |t| i.time - t);
        (held, i.pointer.is_decidedly_dragging())
    });
    let long = response.long_touched() || (down && !moved && held >= LONG_PRESS);
    if down && !moved && !long {
        // Nothing moves while it is held: egui needs a frame when the time is up
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(LONG_PRESS - held + 0.01));
    }
    if long && state.long_pressed.insert(node.id) {
        state.push(UiEvent::LongPressed { id: node.id });
    }
}

/// [UiReactions::drop_kinds]: the innermost target under the pointer that takes what is dragged shows it, with a line
/// where it would go among its nodes, and gets it when the button comes up.
fn drop_target(
    ui: &Ui,
    state: &mut UiState,
    tree: &crate::tree::Tree,
    node: &UiNode,
    response: &Response,
    kinds: &[String],
    claimed: &mut bool,
) {
    let ctx = ui.ctx();
    let Some(dragging) = egui::DragAndDrop::payload::<Dragging>(ctx) else {
        return;
    };
    let takes = (kinds.is_empty() || kinds.contains(&dragging.kind)) && dragging.node != node.id;
    if !takes || *claimed || !response.contains_pointer() {
        return;
    }
    *claimed = true;
    let Some(pointer) = ctx.pointer_interact_pos() else {
        return;
    };
    let rect = response.rect;
    let visuals = ui.visuals();
    let color = visuals.selection.stroke.color;
    let painter = ctx.layer_painter(LayerId::new(egui::Order::Foreground, response.id));
    painter.rect_stroke(
        rect,
        visuals.widgets.active.corner_radius,
        egui::Stroke::new(2.0_f32, color),
        egui::StrokeKind::Inside,
    );
    // Before the first node whose middle lies beyond the pointer
    let across = matches!(node.widget, Widget::Row { .. });
    let children: Vec<egui::Rect> = tree
        .children(node.id)
        .iter()
        .filter_map(|&c| state.bounds.get(&tree.nodes[c].id))
        .map(|b| egui::Rect::from_min_size(egui::pos2(b.x, b.y), egui::vec2(b.width, b.height)))
        .collect();
    let index = (!children.is_empty()).then(|| {
        children
            .iter()
            .take_while(|c| {
                if across {
                    c.center().x < pointer.x
                } else {
                    c.center().y < pointer.y
                }
            })
            .count()
    });
    if let Some(i) = index {
        let gap = ui.spacing().item_spacing / 2.0;
        let stroke = egui::Stroke::new(2.0_f32, color);
        if across {
            let x = children
                .get(i)
                .map_or(children[children.len() - 1].right() + gap.x, |c| {
                    c.left() - gap.x
                });
            painter.vline(x, rect.y_range(), stroke);
        } else {
            let y = children
                .get(i)
                .map_or(children[children.len() - 1].bottom() + gap.y, |c| {
                    c.top() - gap.y
                });
            painter.hline(rect.x_range(), y, stroke);
        }
    }
    if ui.input(|i| i.pointer.any_released()) {
        if let Some(dropped) = egui::DragAndDrop::take_payload::<Dragging>(ctx) {
            state.push(UiEvent::Dropped {
                id: node.id,
                source: dropped.node,
                index: index.map(|i| i as u32),
            });
        }
    }
}
