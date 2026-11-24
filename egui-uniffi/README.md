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

At the top stand `SidePanel`, `TopBar`, `BottomBar`, `CentralPanel` (the room the others leave), `Window` and `Area`
(pinned to a corner, an edge or the center). Inside them: the containers `Row`, `Column`, `Columns`, `Sides`, `Indent`,
`Resize`, `Grid`, `Collapsible`, `ScrollArea`, `Group` and the widgets `Text`, `Separator`, `Spacer`, `Button`, `Checkbox`,
`Radio`, `ComboBox`, `SliderInt`, `SliderFloat`, `DragValue`, `TextField`, `ProgressBar`, `Spinner`, `Notice` (info,
warning, error) and `Plot`. `setTree` refuses a tree with an id twice, a parent that is not there or holds no nodes, a
panel inside a node, a second `CentralPanel`, `Sides` without exactly two nodes, or values that do not fit.

Fields added after the first version have defaults (`#[uniffi(default)]`), so Kotlin that builds nodes without them keeps
building.

## Layout

Every node may carry a `UiModifier`, as Compose's `Modifier`:

| Field | What it does |
| --- | --- |
| `width`, `height` | exactly this size; a button or a field fills it |
| `min_width` … `max_height` | at least, at most (longer text wraps) |
| `fill_width`, `fill_height` | all the room of the container (`fillMaxWidth`) |
| `background`, `border`, `corner_radius`, `shadow` | a frame of egui around the node |
| `padding`, `margin` | room inside the frame, room around it |
| `align` | where it sits across its container: start, center, end |
| `opacity`, `invisible` | fainter with everything inside; keeps its room but is neither drawn nor reacts |

Sizes count the padding and the border, not the margin (`Modifier.width(120.dp).background(…).padding(8.dp)`). A modified
node's `bounds` include its frame.

`Row` and `Column` take `align` (across), `arrangement` (start, center or end along them, in their order) and `spacing`;
a row may `wrap` into lines, it decides with the sizes of the frame before. The middle needs the length of the frame
before as well (egui lays out in one go): the first frame is laid out twice. Across a row, egui knows the height of a
single widget before it places it, but not of a column, a frame or a widget with a size: these sit in the middle (or at the
bottom) by their height of the frame before, and a row that does not wrap is as high as its highest node from the start, so
that all its nodes line up. The middle and the end of a `Column` need a
known height (a panel), not a `ScrollArea`. `Columns` puts each node in a column of the same width, `Sides` its first
node at the left and its second at the right edge, `Indent` moves its nodes in, `Resize` has a corner to drag.

Panels fold away animated with `expanded = false`; side panels have `min_width` and `max_width`. A `CentralPanel` takes
what the others leave; without its `frame` the picture shows through and only its nodes cover it. Windows may be pinned
(`anchor`, `x` and `y` as the distance), have a `fixed_size`, no `title_bar`, `scroll`, not be `movable`, and
`open = false` fades them out without an event. `ScrollArea` scrolls up and down, sideways or both, and with
`stick_to_end` stays at the end while nodes come in, as a log. `Grid` has `column_spacing`, `row_spacing` and
`min_column_width`.

`Text` is `text` followed by `spans` in their own styles (Compose's `AnnotatedString`): each `TextSpan` may change the
style, color, background and size and be italic, underlined, struck through, code or raised. `wrap` breaks it into lines,
cuts it with "…" or lets it run on; `selectable = false` keeps the user from selecting it.

## Plots

`Plot` draws diagrams with [egui_plot](https://github.com/emilk/egui_plot) (0.35, the one for egui 0.34), for benchmarks:
frame times over time, a bar per scene. Its `series` are drawn in their order:

| `SeriesKind` | What it draws |
| --- | --- |
| `Line(width, fill)` | the points joined by a line, `fill` shades the area down to 0 |
| `Points(radius)` | a dot per point |
| `Bars(width, stack)` | a bar per value; bars with the same `stack` group stand on each other (bar i on bar i) |
| `Boxes(width)` | a box plot per five values of `y` (lower whisker, Q1, median, Q3, upper whisker) |
| `HLine` | a dashed line across the plot at each `y`, a budget such as 16.7 ms |

`x` empty counts 0, 1, 2 …; with `categories` the x axis names these places ("Cloister", "Garden") instead of numbers.
The pointer on a series shows its values with `x_unit` and `y_unit`; the legend hides a series on a click. With
`interactive` dragging moves the plot, Ctrl and the wheel zoom and a double click shows everything again; plots with the
same `link` show the same part of the x axis and the pointer together. `include_x` and `include_y` are values the axes
always show (0 for frame times). The axes count in steps of 1, 2 and 5. Plots report no events.

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
graphics card, `THREE_D_EGUI_PICTURE=<dir>` writes the pictures of the options panel, the plots and the settings window
as raw RGBA.
