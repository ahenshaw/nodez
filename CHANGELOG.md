# Changelog

Notable changes to `nodez` and `nodez-derive`, which share a version.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
— pre-1.0, so a minor bump is where a breaking change goes.

## [Unreleased]

### Added

- A `futuresdr` example: the same kind of flowgraph as `gnuradio`, emitting a
  Rust `main` against FutureSDR 0.8 instead of a Python top block. The pair is
  the point — the graph, the node kinds and the walk over them are the same
  shape, and one function differs. What that function has to know more of is
  that blocks are typed Rust values and that `connect!` reads its endpoints as
  input port, block, output port, so a second input is `in0.combine_0`.

- Reusable node groups: a graph registered in a `NodeLibrary` as a template,
  the way GNU Radio installs a hier block into its block tree. A group node is
  an ordinary node whose template happens to be a group, so reuse, the add
  menu, categories, colors and sizing all come free. See `nodez::group`, and
  the `Audio Chain` block in the `gnuradio` example.
  - Its interface is read off the inside rather than declared: the Group Input
    and Group Output pads in it, ordered down the canvas, each contributing
    the socket it is named after and taking the type of whatever it is wired
    to.
  - `Graph::flatten` puts every group node's interior back, so evaluation,
    traversal and generators carry on seeing a flat graph and need to know
    nothing about groups. A group that would contain itself is refused when it
    is registered.
  - In the editor: `Ctrl+G` groups the selection, double-clicking a group node
    below its header opens it, `Escape` comes back out, and a breadcrumb says
    where you are. Leaving re-reads the interface off the pads, so every
    instance picks up a socket added inside.
  - `EditorAction` gains `GroupSelection`, `EnterGroup` and `LeaveGroup`. The
    widget has no library to register a group in or look one up, so it asks.
  - Groups are read and written as files: `NodeLibrary::write_group`,
    `read_group` and `load_groups`, and `EditorApp::groups_dir`, which reads a
    directory of them at startup and writes what `Ctrl+G` makes back into it.
    A group file is an ordinary graph document wrapped in what a graph cannot
    say about itself — id, label, category — and the sockets stay out of it,
    because they are read off the pads. The id is not the file's name, so
    renaming a file cannot unmake the documents that used it. Reading a
    directory takes repeated passes, since a group built from another has to
    be read second and nothing in a file says which.
- `Graph::template_names`, the names a graph recorded for the templates it
  was saved against.
- `Graph::convert`, which moves a graph from one node payload to another by
  copying what its templates name. It is what lets an editor on any payload
  open a group, whose interior is always the dynamic one.
- `Graph::absorb`, which copies another graph's nodes and wires into this one
  and says where each of them landed.
- `Graph::name_templates`, which records what every template in a graph is
  called before it is written out, and `Graph::validate` reads the names back
  and resolves them against whatever library is loading the file. A
  `TemplateId` is a position in a library and a position means nothing to a
  library whose templates are registered in another order — a file saved
  today is silently misread, or a node quietly dropped, by a library that has
  gained anything ahead of it. `EditorApp`'s save does this for you.

### Changed

- Whether a wire is routed at all is settled at the clearance it was asked to
  keep, rather than at the tightest one on offer. A reduced clearance is how a
  path is *found* when there is no other; it was also, by accident, a reason
  to prefer a path to the curve — a finer grid would turn up a route a hair
  cheaper and the wire would come out as a run of right angles on a difference
  nobody could see. Which of the routes gets drawn is still the cheapest.

## [0.5.0] — 2026-09-20

Everything in this release came of looking at the same picture and asking why
it read badly. The router learned that a wire costs something where it goes,
not just how far: crossing another wire, and running hard against a node it is
only passing. The layout stopped taking dependency depth for an answer and
started solving for the shortest wires. And an input that has to be wired and
is not now says so, which is a thing the schema always knew and never showed.

On the demo's graph the wires cross sixteen times where they crossed
eighty-six.

### Added

- `SocketSpec::optional`, set by `#[derive(NodeType)]` for an `Option<T>`
  input. `Option<T>` used to reach only the reader, so the schema could not
  tell a required link-only input from one that may be left alone — which is
  the whole of what follows.
- The editor marks an input that has to be wired and is not: the node's
  outline, a halo behind the socket, and the row's label, in
  `EditorStyle::missing_input`. `show_missing_inputs` turns it off. Which
  inputs those are is derived from the schema, not declared: an inline editor
  is a value to fall back on, a fan-in may be empty, `Option<T>` says so
  outright, and a muted node is switched off rather than unfinished.
- `Graph::missing_inputs` and `Graph::is_input_missing`, the same fact without
  the editor, so a generator can refuse a graph for the reason already on
  screen.
- `RouteOptions::hug`: what running hard against a node costs, per pixel of
  wire. Nothing is charged at a full clearance or beyond it — what is priced
  is the room a wire gives up, which is the room the reader loses.
- `RouteOptions::cross`: what going over another wire costs the router, in the
  same pixels `bend` charges for a corner. Wire routes are now chosen by
  counting crossings as well as ground and corners, and the plain curve a wire
  would otherwise keep is priced the same way — so a long diagonal that cuts
  across five other wires gives way to a route that drops to its socket's
  height first and crosses two. Building `RouteOptions` with
  `..Default::default()` picks the new field up.
- Two examples that generate Python: `blender_nodes` writes the `bpy` script
  for a shader tree, and `gnuradio` writes the top-block script for a
  flowgraph. Both walk the graph rather than folding it, which is what a
  generator wants from a graph that describes something rather than computes
  something, and both take `--print` to run without a display.

### Changed

- `route_links` routes every wire twice: once against the wires placed before
  it, and once more against the finished picture. The first pass cannot judge
  a detour, because the wires it is dodging into have not been placed yet.
- **Breaking in effect, not in signature:** `layered` places nodes differently.
  A node's column is no longer its dependency depth: what comes out is the
  assignment that makes the wires shortest in total, found by network simplex
  (Gansner, Koutsofios, North and Vo, 1993 — the layer assignment `dot` uses),
  and heights are settled towards each node's neighbours rather than by
  centring each column. Code calling it is unaffected; the positions that come
  out are not. On the demo's graph the wires cross 16 times where they used to
  cross 86; over a hundred generated graphs the wires span 2125 columns where
  dependency depth alone spans 2199, and cross 1168 times rather than 1238.
- `route_links` ranks the lines it searches: a line a node's clearance asks
  for now outranks one that is merely a convenient place to set off from, so
  when two fall within a pixel of each other the one there is a reason for
  survives. Dropping the other way round could leave a wire no way past a node
  at all, and it drew straight through it instead.
- `route_links` searches every clearance and takes the cheapest route rather
  than the first that works. Room to spare is worth something but not
  everything, and insisting on it could send a wire the width of the canvas
  round what it could have squeezed past.
- `route_links` no longer folds two grid lines a pixel apart into one. A pixel
  is wider than it sounds: where two nodes' edges are a pixel apart, one of
  the two lanes below them clears both and the other clears neither.

### Internal

- The demo's stack graph is a routing fixture, in two arrangements: the one
  `layered` builds it with, and one arranged by hand in a running editor and
  pinned. The same graph placed two ways is two different problems for a
  router, and the second is the one that turned the crossing penalty up.

## [0.4.1] — 2026-09-20

Nothing in the crates changed. All three are things 0.4.0 went out without,
and none of them could be added to a release already published.

### Added

- `rust-version` on every crate, so an old toolchain says what it wants
  instead of failing on syntax it has never heard of.
- docs.rs now builds with all features, so `derive` and `app` appear in the
  published documentation rather than being left out of it.
- This changelog, and tags on the releases that never got one.

## [0.4.0] — 2026-09-20

Wire routing rewritten. The old router worked from the column layout — nodes
bucketed by dependency depth, wires pinned into the gaps between columns — and
every bug in it came of the same thing: depth is not position, and the two part
company the moment a node moves.

### Changed

- **Breaking:** `RouteOptions::min_lane` is gone and `RouteOptions::bend` takes
  its place. The old field said how narrow a gap was still worth threading; the
  new one says what a corner costs against a pixel of wire. Code that builds a
  `RouteOptions` with `..Default::default()` is unaffected.
- `route_links` finds a route by searching rather than by rule: A* over the grid
  of lines drawn a clearance out from every node's sides, costing length plus a
  penalty per corner, with a step only offered when it misses every node. A wire
  through a node is no longer something the router can express.
- It no longer expects a graph arranged in columns. A hand-moved layout routes
  as well as a freshly laid-out one.
- A wire changes height out in the open where it can, rather than hard against
  the side of a node it has nothing to do with.

### Fixed

- A short wire between two distant heights no longer doubles back on itself. Its
  control points reached further than the span had room for, and past that the
  curve bulges back the way it came.
- A wire meets its socket along its own height, from outside the node. It could
  previously drop onto a socket down the face of the node, crossing whatever
  other sockets it passed.
- A wire climbs no further than what is actually in its way. It could previously
  go over the top of the whole graph, or dive under it, to reach a socket a few
  pixels from where it started.

### Added

- Generated-graph sweeps holding the router to what it promises: no wire under a
  node, none doubling back, none longer than the obvious way round, each meeting
  its sockets level, the same graph routing the same way twice, and the graph
  itself untouched by any of it.

## [0.3.0] — 2026-09-18

### Added

- Wire routing: a wire that would otherwise cut through the nodes between its
  ends is steered around them, with rounded corners and its own line down a
  channel. `Connection::waypoints` carries the bends, `Graph::clear_routing`
  takes them away again, and `route_links` puts them there.
- `Graph::connection_mut`, for editing how a wire is drawn.
- `socket_anchor`, so the router can aim at where a wire really attaches rather
  than at the middle of a node's edge.
- Routing is a toggle in the demo's toolbar, not a one-way trip.

### Fixed

- The repository link pointed at a repository that does not exist, so the link
  on the 0.2.0 crates.io page was a 404.

## [0.2.0] — 2026-09-18

### Added

- `align` and `distribute` for lining up and spreading a selection, and undo for
  moving nodes.
- A multi-input's free socket appears only when a wire is near it.

### Changed

- The window chrome no longer follows the system light/dark theme.

## [0.1.0] — 2026-09-17

First release: a Blender-style node editor widget for egui, with a typed,
traversable graph model, `#[derive(NodeType)]` for describing nodes as Rust
types, and a ready-made editor window behind the `app` feature.

[Unreleased]: https://github.com/ahenshaw/nodez/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/ahenshaw/nodez/compare/v0.4.1...v0.5.0
[0.4.1]: https://github.com/ahenshaw/nodez/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/ahenshaw/nodez/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/ahenshaw/nodez/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/ahenshaw/nodez/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ahenshaw/nodez/releases/tag/v0.1.0
