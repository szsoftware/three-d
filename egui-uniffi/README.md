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
| `input(event)` | pointer, touch, key, text, clipboard and input method events in pixels from the top left; `true` if egui uses it (the camera leaves it alone) |
| `takeEvents()` | `Clicked`, `Checked`, `Selected`, `IntChanged`, `FloatChanged`, `TextChanged`, `Submitted`, `Closed`, `ColorChanged`, `DateChanged`, `FocusChanged`, with the node's id (`Closed` also for popups and modals) |
| `takeOutput()` | what egui asks of the platform: text for the clipboard, a wish to paste, links to open, the pointer's look, where text is typed |
| `requestFocus(id)` | gives a node the keyboard (Compose's `FocusRequester`), `null` takes it away |
| `needsRepaint()` | whether the next frame looks different (new tree, input, egui's hover and animations) |
| `setScale(pixelsPerPoint)` | Compose's density times a zoom |
| `setStyle(UiStyle(dark, accent))` | egui's dark or light look with an accent color (ARGB) |
| `setImage(key, bytes)`, `setImageRgba(key, width, height, rgba)`, `removeImage(key)` | images for `Image`, buttons, canvases and plots, by key; PNG, JPEG, GIF, WebP and SVG files or pixels |
| `setCanvasShapes(key, shapes)` | many shapes for a `Canvas` that seldom change (a floor plan), drawn under its own |
| `covers(x, y)`, `bounds(id)` | whether a pixel is on egui, where a node was drawn (for tests that click on it) |
| `paint(context, target, viewport)` | Rust only: draws over the application's picture, on the thread of its three-d context |
| `SceneView.setOverlay(overlay)` | three-d-raytrace-uniffi's view paints it itself and hands it its input first |

## The tree

At the top stand `SidePanel`, `TopBar`, `BottomBar`, `CentralPanel` (the room the others leave), `Window`, `Area`
(pinned to a corner, an edge or the center) and `Modal`. Inside them: the containers `Row`, `Column`, `Columns`, `Sides`,
`Indent`, `Resize`, `Grid`, `Collapsible`, `ScrollArea`, `Group`, `Table` (of `TableRow`s), `Strip`, `MenuBar`, `Menu` and the widgets `Text`, `Separator`, `Spacer`, `Button`, `Checkbox`,
`SelectableLabel`, `ToggleButton`, `RadioButton`, `Switch`, `Radio`, `ComboBox`, `SliderInt`, `SliderFloat`, `DragValue`,
`TextField`, `ColorPicker`, `DatePicker`, `Link`, `ProgressBar`, `Spinner`, `Notice` (info, warning, error) and `Plot`;
`Shortcut` is a key that draws nothing. `setTree` refuses a tree with an id twice, a parent that is not there or holds no
nodes, a panel inside a node, a second `CentralPanel`, `Sides` without exactly two nodes, or values that do not fit (a
step of 0, a logarithmic slider below 0, a field of 0 lines, a day that does not exist, a key without a name).

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

## Inputs

| Widget | Options |
| --- | --- |
| `Button` | `small` (toolbars), `frame = false` (Compose's `TextButton`), `selected`, `shortcut` (a faint key at its right), `fill` |
| `SelectableLabel`, `ToggleButton` | a text that is chosen, a button that stays pressed: tabs and toolbars |
| `RadioButton`, `Checkbox`, `Switch` | a single round button (its group are the other nodes), `indeterminate` (a dash), a switch that slides |
| `SliderInt`, `SliderFloat` | `prefix`, `suffix`, `step`, `vertical`, `clamping` (always, never, only what the user types), `show_value`, `trailing_fill`, `handle` (round, a bar with its aspect) |
| `DragValue` | `prefix`, `hexadecimal`, `angle` (radians, shown and dragged in degrees) |
| `TextField` | `lines` (Enter makes a line, Ctrl+Enter submits), `code` (fixed width, Tab indents), `language` (colored as "rs", "toml", "py", "c", "cpp"; more with the feature `syntect`), `char_limit`, `align`, `read_only` |
| `ColorPicker` | a button with the color that opens egui's picker, `alpha`, `inline` (the picker in the panel) |
| `DatePicker` | a button with the day that opens a calendar (egui_extras), `format` (strftime), `calendar_week`, `highlight_weekends`, `first_year`, `last_year` |
| `Link` | `Clicked`; with a `url` the application opens it as well (`takeOutput().open`) |
| `Shortcut` | `key` with `ctrl` (Command on macOS), `shift`, `alt`: `Clicked` while its node is drawn (in an open window, a shown panel, or always at the top of the tree); without Ctrl or Alt it waits while a field has the keyboard |

A modifier with `focus_events` reports `FocusChanged` when its node gets or loses the keyboard; `requestFocus(id)` gives
it the keyboard from Kotlin.

## Menus, popups and dialogs

`MenuBar` holds `Menu`s, a `Menu` holds the items: buttons (with a `shortcut` text), checkboxes, separators and further
`Menu`s, which open to the side. A click on an item reports it and closes the menu; a `Menu` outside a bar is a button
with a dropdown.

Three kinds of nodes belong to the node they open on instead of standing inside it; their parent may be any node, a
button as well as a text or a panel:

| Node | What it does |
| --- | --- |
| `ContextMenu` | its nodes as the menu of a right click on the parent |
| `Tooltip` | its nodes in a tooltip while the pointer rests on the parent (instead of the `tooltip` text) |
| `Popup` | its nodes next to the parent while `open`, at a `side` (below, above, left, right) and `align`; `close` says which clicks close it (any, beside it, none). When the user closes it, it reports `Closed` and stays closed until the next tree |

A node has at most one `ContextMenu` and one `Tooltip`, any number of `Popup`s. A `Modal` stands at the top, in the
middle over everything, and dims and blocks the rest while it is in the tree; `dismissible` lets Escape or a click beside
it report `Closed`.

## Tables and strips

`Table` (egui_extras' `TableBuilder`) has `columns`, each with a `title`, a `width` (`Auto`, `Initial`, `Exact`,
`Remainder`), `min_width`/`max_width`, `resizable`, `clip`, an `align` for its cells, `sortable` and a text `style`. The
rows come in one of two ways:

| Rows | When | How |
| --- | --- | --- |
| `TableRow` nodes | a few dozen rows with widgets in their cells | one node per column (a `Row` or `Column` for several), fewer leave cells empty; a `TableRow` with `header` as the first one replaces the titles |
| `data` | tens of thousands of rows of text | `setTableData(key, columns, cells)` once the rows change (row by row), `appendTableRows(key, cells)` for a log; the tree only names the key |

Only the rows in sight are drawn, for nodes as for data. With a `selection` (`Single` or `Multiple`) a click selects a
row, Ctrl (Command on a Mac) adds or removes one, Shift selects from the row clicked before; `RowsSelected` reports all
selected rows and `selected` shows them, a double click reports `RowActivated`. A click on a sortable title reports
`SortChanged` (ascending unless it was ascending before); Kotlin sorts its rows, sends them, and `sort` shows the arrow.
`striped`, `resizable` (all columns), `row_height`, `max_height` (the rows scroll beyond it), `stick_to_end` (a log) and
`scroll_to` (once, when the row changes) do what they say. A table takes the room there is: in a `ScrollArea` give it a
`max_height`, so that it scrolls itself and draws only the rows in sight. In a container that shrinks to its content (a
`Column`, a node with a frame) it takes what is seen below it, as does a `Strip`. Columns the user may resize keep their
width of the first frame, in which `Auto` ones guess from the rows in sight: only the last column, if `Remainder`, takes
all that is left in every frame.

`Strip` (egui_extras' `StripBuilder`) divides its room side by side (or one under the other with `vertical`) into cells of
`sizes`: `Exact` points, `Initial` points with bounds, `Relative` parts of the whole strip, `Remainder` for what is left.
One node per cell, a `Strip` in a cell divides that cell.

## Images, canvas and scene

Images are given to the overlay once under a key and named by it in the tree: `setImage(key, bytes)` takes PNG, JPEG,
GIF, WebP and SVG files (egui_extras' loaders, the feature `images`, on by default), `setImageRgba(key, width, height,
rgba)` pixels (four bytes each, a frame of a camera or a rendered thumbnail). Files are decoded on a thread of their own
(in the browser at once) and show a spinner until then; an SVG is drawn sharp at any size, a GIF or WebP moves. `Image`
shows one with a `width` and `height` (one of them keeps its shape, none its own size), a `fit` as Compose's
`ContentScale` (`Contain`, `Cover`, `Fill`), a `tint` (a white icon in any color), `corner_radius`, `rotation` in degrees
and a `description`. A `Button` takes an `image` beside its text (at the height of the text unless `image_size`).

`Canvas` draws in its own coordinates: `view` is the part it shows (a floor plan in metres), kept in shape in the middle
unless `stretch`, with y upwards if `y_up`. Its `CanvasShape`s are `Line`, `Polygon` (also concave, a room as an L),
`Rect`, `Circle`, `Ellipse`, `Curve` (Bézier, quadratic or cubic, `closed` and filled), `Arrow`, `Text` (with an `anchor`)
and `Image`, each with a `fill` and a `stroke` (width in points, color, `dash`). Shapes of `data` (`setCanvasShapes`) lie
under the ones of the tree: the plan once, the moving camera in every tree. `clickable` reports `PointClicked` with the
point in the canvas' coordinates, `report_pointer` reports `PointerAt` while the pointer moves over it (`null` once it
leaves).

`Scene` (egui's `Scene`) shows its nodes on a surface the user drags and zooms (Ctrl and the wheel, a pinch), within
`min_zoom` and `max_zoom`; at first all its nodes are in sight. `view` shows a part of it, given in the points of its
nodes and applied when it changes; with `report_view` the user moving it reports `ViewChanged` with what is shown.
`bounds(id)` of a node in a scene is where it is seen on the screen.

## Reactions, drag and drop, touch

`UiModifier.reactions` (`UiReactions`) makes any node react besides its widget, as Compose's `clickable`,
`combinedClickable`, `hoverable`, `draggable`, `transformable` and drag and drop: a text, an image, a frame with nodes
inside. The nodes inside come first, so a button in a clickable card still takes its own clicks. Text in a node with
`drag` or `drag_kind` is not selectable: pressing on it and moving drags the node, as in egui's drag source.

| Field | Reports |
| --- | --- |
| `click` | `Clicked` |
| `double_click` | `DoubleClicked` after the `Clicked` of its first click |
| `secondary_click` | `SecondaryClicked` with the point in the node, for the right button and a long touch (unless `long_press` took it) |
| `long_press` | `LongPressed` once the button or a finger rested half a second |
| `hover` | `HoverChanged` as the pointer comes and goes (only the last value until Kotlin takes it) |
| `drag` | `Dragged` with `Started`, `Moved` and `Stopped`, the point in the node and the move since the last event |
| `gestures` | `Gesture` with zoom, rotation and move of two fingers, or the zoom of Ctrl and the wheel |
| `cursor` | the pointer's look over it (Compose's `pointerHoverIcon`) |
| `drag_kind` | it can be dragged; it follows the pointer, its place stays empty |
| `drop_kinds` | nodes of these kinds (empty: any) can be dropped onto it: `Dropped` with the dragged node and `index`, before which of its nodes the pointer was (a line shows where) |

The innermost target under the pointer that takes the kind shows it with a frame and gets it. A list sorted by dragging
gives its rows a `drag_kind` and itself the same `drop_kinds`; `index` counts the rows as they are, the dragged one
included.

`UiInput.Touch(id, phase, x, y, force)` brings fingers: the first one also moves egui's pointer (a tap clicks, a long
touch is a right click), two zoom a `Scene` or a `Plot` and report `Gesture`. three-d-raytrace-uniffi's
`InputEvent.Touch` hands them to the overlay first; there the first finger moves the camera as the left button does.

## What the platform does

egui does not reach the clipboard, the browser or the input method itself: `takeOutput()` after a frame says what it asks
for, and Kotlin answers through `input`.

| `UiOutput` | What Kotlin does |
| --- | --- |
| `copy` | puts the text on the clipboard (after Ctrl+C or Ctrl+X, or `UiInput.Copy` and `UiInput.Cut`) |
| `paste` | sends `UiInput.Paste(text)` with the clipboard's text (after Ctrl+V); in a browser the `paste` event brings it |
| `open` | opens each link, in a new tab if asked |
| `cursor` | shows this pointer (35 of them, with their CSS names) |
| `text_input` | a field has the keyboard: show a touch keyboard, put the box of the input method at `cursor` (in pixels) |

An input method composes with `ImeStart`, `ImeCompose(text)` (shown underlined), `ImeCommit(text)` and `ImeEnd`.
`copy`, `paste` and `open` are taken with the output, `cursor` and `text_input` stay as they are.

## Plots

`Plot` draws diagrams with [egui_plot](https://github.com/emilk/egui_plot) (0.35, the one for egui 0.34), for benchmarks:
frame times over time, a bar per scene. Its `series` are drawn in their order:

| `SeriesKind` | What it draws |
| --- | --- |
| `Line(width, fill)` | the points joined by a line, `fill` shades the area down to 0 |
| `Points(radius, shape, filled)` | a marker per point: a circle, square, diamond, cross, plus, the four triangles or an asterisk |
| `Bars(width, stack)` | a bar per value; bars with the same `stack` group stand on each other (bar i on bar i) |
| `Boxes(width)` | a box plot per five values of `y` (lower whisker, Q1, median, Q3, upper whisker) |
| `HLine` | a dashed line across the plot at each `y`, a budget such as 16.7 ms |
| `VLine` | a line across the plot at each `x`, a moment such as a hitch |
| `Polygon(width)` | a closed area through the points, a zone |
| `Text(anchor)` | `labels` at the points |
| `Arrows(tip_length)` | an arrow from each point to the point of `x2` and `y2` |
| `Area` | the band between `y` and `y2`, the spread around a mean |
| `Span(vertical)` | a band across the plot for each pair of values in `y` (in `x` if `vertical`), 30 to 60 frames per second |
| `Heatmap(columns, palette, min, max, show_values)` | a grid of colored cells from `y`, row by row |
| `Image(image, width, height)` | an image given to the overlay, centered at the first point |

`x` empty counts 0, 1, 2 …; with `categories` the x axis names these places ("Cloister", "Garden") instead of numbers.
The pointer on a series shows its values with `x_unit` and `y_unit`; the legend hides a series on a click. With
`interactive` dragging moves the plot, Ctrl and the wheel zoom and a double click shows everything again; plots with the
same `link` show the same part of the x axis and the pointer together. `include_x` and `include_y` are values the axes
always show (0 for frame times). The axes count in steps of 1, 2 and 5. A series' `style` draws its line `Solid`,
`Dashed` or `Dotted`. With `clickable` a click reports `PointClicked` with the values there, with `report_pointer` the
values under the pointer come as `PointerAt`.

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

`cargo test -p three-d-egui-uniffi`: the tree, the values, the events, the clipboard, the input method and the focus with
egui alone; painting, clicks through `input`, scale, style and repaints on OpenGL through EGL (Linux) and on wgpu.
`THREE_D_SKIP_GPU_TESTS=1` leaves out the graphics card, `THREE_D_EGUI_PICTURE=<dir>` writes the pictures of the options
panel, the plots, the settings window, the inputs, an open menu, a modal and the tables as raw RGBA.
