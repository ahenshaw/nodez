#!/usr/bin/env python3
"""nodez Python demo: build a stack graph, traverse it, generate its config,
and edit it in the Blender-style node editor.

    python3 demo.py                 # open the editor with a live config preview
    python3 demo.py --print         # print the generated config and exit
    python3 demo.py --explore       # show what the traversal API reports
    python3 demo.py --load g.json   # start from a saved graph
    python3 demo.py --save g.json   # write the graph back out when the editor closes

Build the extension module first:

    ./nodez-py/build.sh
"""

from __future__ import annotations

import argparse
import sys

try:
    import nodez
except ImportError:
    sys.exit("nodez is not importable — run nodez-py/build.sh first")

import stack


def explore(lib: nodez.Library, graph: nodez.Graph) -> None:
    """Everything here comes from the traversal API."""
    stack_id = next(n for n in graph.node_ids() if graph.type_of(lib, n) == "stack")

    def name(nid: int) -> str:
        return f"{graph.title(nid)}#{nid}"

    print(f"{graph.node_count} nodes, {graph.connection_count} links, "
          f"{len(graph.components())} component(s)")
    print(f"acyclic: {graph.is_acyclic()}")
    print()

    print("roots (nothing feeds them):")
    for nid in graph.roots():
        print("   ", name(nid))
    print("sinks (nothing reads them):")
    for nid in graph.sinks():
        print("   ", name(nid))
    print()

    print(f"the output node {name(stack_id)} depends on "
          f"{len(graph.ancestors(stack_id))} nodes, directly on:")
    for nid in graph.predecessors(stack_id):
        print("   ", name(nid))
    print()

    depths = graph.depths()
    print("evaluation order (column = dependency depth):")
    for i, nid in enumerate(graph.topological_order(), start=1):
        print(f"  {i:>2}. [col {depths[nid]}] {name(nid)}")
    print()

    # Typed sockets are enforced from Python too, not just in the editor.
    a = next(n for n in graph.node_ids() if graph.type_of(lib, n) == "text")
    b = next(n for n in graph.node_ids() if graph.type_of(lib, n) == "number")
    print("type checking:")
    print("  Text -> Number.value:", graph.why_not_connect(lib, (a, "out"), (b, "value")))
    print("  Number -> Text.value:", graph.why_not_connect(lib, (b, "out"), (a, "value"))
          or "allowed (Number casts to Text)")
    print("  cycle back to itself:", graph.why_not_connect(lib, (a, "out"), (a, "value")))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--print", dest="print_only", action="store_true",
                        help="print the generated config and exit")
    parser.add_argument("--explore", action="store_true",
                        help="report what the traversal API sees and exit")
    parser.add_argument("--load", metavar="PATH", help="start from a saved graph")
    parser.add_argument("--save", metavar="PATH", help="write the graph out on exit")
    parser.add_argument("--theme", choices=("dark", "light"), default="dark")
    parser.add_argument("--scroll", choices=("auto", "pan", "zoom"), default="auto",
                        help="what a bare scroll does in the editor")
    args = parser.parse_args()

    lib = stack.build_library()

    if args.load:
        with open(args.load) as f:
            graph = nodez.Graph.from_json(f.read(), lib)
        dropped_nodes, dropped_links = graph.validate(lib)
        if dropped_nodes or dropped_links:
            print(f"repaired on load: dropped {dropped_nodes} node(s) "
                  f"and {dropped_links} link(s)", file=sys.stderr)
    else:
        graph = stack.build_sample(lib)

    if args.explore:
        explore(lib, graph)
        return 0

    if args.print_only:
        text, problems = stack.generate(lib, graph)
        for problem in problems:
            print(f"warning: {problem}", file=sys.stderr)
        sys.stdout.write(text)
        return 0

    # The editor calls this back whenever the graph changes, and shows whatever
    # it returns beside the canvas — so the config regenerates as you wire.
    def on_change(edited: nodez.Graph) -> str:
        text, problems = stack.generate(lib, edited)
        banner = "".join(f"# warning: {p}\n" for p in problems)
        return banner + text

    nodez.edit(
        lib,
        graph,
        title="nodez — stack config (Python)",
        theme=args.theme,
        scroll_mode=args.scroll,
        on_change=on_change,
    )

    text, problems = stack.generate(lib, graph)
    for problem in problems:
        print(f"warning: {problem}", file=sys.stderr)
    print(f"# {graph.node_count} nodes, {graph.connection_count} links")
    sys.stdout.write(text)

    if args.save:
        with open(args.save, "w") as f:
            f.write(graph.to_json())
        print(f"wrote {args.save}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
