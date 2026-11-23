# three-d-egui-uniffi

egui over a three-d picture, described from Kotlin (or any language uniffi generates bindings for). Panels, switches,
options and benchmark readouts are drawn by egui into the same picture as the 3D view, so they lie over it on every
platform, also where Compose cannot draw over a native canvas (the JVM with a GL canvas in a `SwingPanel`).

Kotlin describes the interface as a tree and gets back what the user did; how that reads in Kotlin
(`SidePanel("Raytrace") { Checkbox("Denoise", denoise) { denoise = it } }`) is the job of a DSL on top of the generated
bindings, with Compose as the model.

| Call | What it does |
| --- | --- |
| `EguiOverlay()` | an overlay without nodes; it makes its egui when it is painted the first time |
| `setTree(nodes)` | the tree as a flat list of `UiNode(id, parent, widget, enabled, tooltip)`; only when Kotlin's state changed |
| `input(event)` | pointer, key and text events in pixels from the top left; `true` if egui uses it (the camera leaves it alone) |
| `takeEvents()` | `Clicked`, `Checked`, `Selected`, `IntChanged`, `FloatChanged`, `TextChanged`, `Submitted`, `Closed`, with the node's id |
| `needsRepaint()` | whether the next frame looks different (new tree, input, egui's hover and animations) |
| `setScale(pixelsPerPoint)` | Compose's density times a zoom |
| `setStyle(UiStyle(dark, accent))` | egui's dark or light look with an accent color (ARGB) |
| `covers(x, y)`, `bounds(id)` | whether a pixel is on egui, where a node was drawn (for tests that click on it) |
| `paint(context, target, viewport)` | Rust only: draws over the application's picture, on the thread of its three-d context |
| `SceneView.setOverlay(overlay)` | three-d-raytrace-uniffi's view paints it itself and hands it its input first |

## The tree

At the top stand `SidePanel`, `TopBar`, `BottomBar`, `Window` and `Area` (pinned to a corner, an edge or the center).
Inside them: the containers `Row`, `Column`, `Grid`, `Collapsible`, `ScrollArea`, `Group` and the widgets `Text` (body,
heading, strong, small, weak, monospace), `Separator`, `Spacer`, `Button`, `Checkbox`, `Radio`, `ComboBox`, `SliderInt`,
`SliderFloat`, `DragValue`, `TextField`, `ProgressBar`, `Spinner`, `Notice` (info, warning, error). `setTree` refuses a
tree with an id twice, a parent that is not there or holds no nodes, a panel inside a node, or values that do not fit.

Ids are Kotlin's and stay the same from one tree to the next (the DSL derives them from the position and a key), so that
egui keeps focus, scroll positions and open sections.

## Who owns the values

Kotlin, as in Compose: the tree carries the values (`checked`, `value`, `selected`). A value the user changes shows in the
same frame, because the overlay keeps it until Kotlin took the event and sent its next tree. From then on the tree's
value counts, so Kotlin can also refuse a change by sending the old value. A slider dragged across several frames
before Kotlin takes the events reports only its last value.

## Painting

The application paints after its own drawing and before the pixels are read or shown:

```rust
overlay.paint(&context, &RenderTarget::screen(&context, width, height), Viewport::new_at_origo(width, height))?;
```

`paint_color` paints onto a `ColorTarget` (the texture of a view that is read back), `paint_with_events` takes a window's
three-d events as well and marks those egui uses as handled, like three-d's `GUI`.
A frame in which nothing changed (no new tree, no input, no animation of egui, the same size) paints egui's meshes of
the frame before again without running egui (three-d's `GUI::render_previous`), so a resting panel costs only its
drawing. OpenGL/WebGL paint with egui_glow, the wgpu backend with three-d's own painter. Painting belongs to the thread that
painted first; on another three-d context the overlay makes a new egui (and loses what egui remembered).

The crate uses `cargo-uniffi-support` with the same tag as `three-d-raytrace-uniffi`, so that an application with both
links one copy. It has no log bridge of its own: that comes from the crate that calls `log_bridge!()` (the ray tracing one).

## Tests

`cargo test -p three-d-egui-uniffi`: the tree, the values and the events with egui alone; painting, clicks through
`input`, scale, style and repaints on OpenGL through EGL (Linux) and on wgpu. `THREE_D_SKIP_GPU_TESTS=1` leaves out the
graphics card, `THREE_D_EGUI_PICTURE=<dir>` writes the pictures of the options panel test as raw RGBA.
