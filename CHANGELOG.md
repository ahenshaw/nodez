# Changelog

Notable changes to `nodez` and `nodez-derive`, which share a version.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
— pre-1.0, so a minor bump is where a breaking change goes.

## [Unreleased]

### Added

- `RouteOptions::cross`: what going over another wire costs the router, in the
  same pixels `bend` charges for a corner. Wire routes are now chosen by
  counting crossings as well as ground and corners, and the plain curve a wire
  would otherwise keep is priced the same way — so a long diagonal that cuts
  across five other wires gives way to a route that drops to its socket's
  height first and crosses two. Building `RouteOptions` with
  `..Default::default()` picks the new field up.

### Changed

- `route_links` routes every wire twice: once against the wires placed before
  it, and once more against the finished picture. The first pass cannot judge
  a detour, because the wires it is dodging into have not been placed yet.

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

[Unreleased]: https://github.com/ahenshaw/nodez/compare/v0.4.1...HEAD
[0.4.1]: https://github.com/ahenshaw/nodez/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/ahenshaw/nodez/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/ahenshaw/nodez/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/ahenshaw/nodez/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ahenshaw/nodez/releases/tag/v0.1.0
