#!/usr/bin/env python3
"""Tests for the nodez Python bindings. Run with `python3 test_nodez.py`.

Nothing here opens a window, so it is safe in CI.
"""

import json
import unittest

import nodez
import stack


def tiny_library() -> nodez.Library:
    lib = nodez.Library()
    lib.add_type("Text", "#70B2FF")
    lib.add_type("Number", "#A1A1A1")
    lib.allow_cast("Number", "Text")

    W, S, P = nodez.Widget, nodez.Socket, nodez.Param
    lib.add_template(
        "text", "Text", category="Input",
        inputs=[S("value", "Text", widget=W.text(), default="")],
        outputs=[S("out", "Text")],
    )
    lib.add_template(
        "number", "Number", category="Input",
        inputs=[S("value", "Number", widget=W.float(), default=0.0)],
        outputs=[S("out", "Number")],
    )
    lib.add_template(
        "join", "Join", category="Convert",
        inputs=[S("parts", "Text", multi=True)],
        params=[P("separator", W.text(), default=" ")],
        outputs=[S("out", "Text")],
    )
    lib.add_template(
        "sink", "Output", category="Output",
        inputs=[S("value", "Text")],
    )
    return lib


class TestLibrary(unittest.TestCase):
    def test_registration(self):
        lib = tiny_library()
        self.assertEqual(lib.type_names(), ["Text", "Number"])
        self.assertEqual(lib.template_ids(), ["text", "number", "join", "sink"])
        self.assertEqual(lib.categories(), ["Input", "Convert", "Output"])
        self.assertEqual(lib.inputs_of("join"), ["parts"])
        self.assertEqual(len(lib), 4)

    def test_casts_are_directional(self):
        lib = tiny_library()
        self.assertTrue(lib.compatible("Number", "Text"))
        self.assertFalse(lib.compatible("Text", "Number"))
        self.assertTrue(lib.compatible("Text", "Text"))

    def test_unknown_names_are_reported(self):
        lib = tiny_library()
        with self.assertRaises(ValueError) as caught:
            lib.add_template("bad", "Bad", outputs=[nodez.Socket("out", "Nope")])
        self.assertIn("Nope", str(caught.exception))

    def test_colors(self):
        lib = nodez.Library()
        lib.add_type("A", "#112233")
        lib.add_type("B", (17, 34, 51))
        lib.add_type("C", (0.5, 0.5, 0.5))
        lib.add_type("D", "#11223344")
        self.assertEqual(len(lib.type_names()), 4)
        with self.assertRaises(ValueError):
            lib.add_type("E", "not-a-colour")


class TestGraph(unittest.TestCase):
    def setUp(self):
        self.lib = tiny_library()
        self.g = nodez.Graph()

    def test_add_and_remove(self):
        a = self.g.add_node(self.lib, "text", (10.0, 20.0))
        self.assertEqual(self.g.node_count, 1)
        self.assertEqual(self.g.position(a), (10.0, 20.0))
        self.assertEqual(self.g.type_of(self.lib, a), "text")
        self.assertTrue(self.g.remove_node(a))
        self.assertEqual(self.g.node_count, 0)

    def test_connect_typechecks(self):
        text = self.g.add_node(self.lib, "text")
        number = self.g.add_node(self.lib, "number")
        sink = self.g.add_node(self.lib, "sink")

        # Number casts to Text, so this is allowed.
        self.assertIsNone(self.g.why_not_connect(self.lib, (number, "out"), (sink, "value")))
        self.g.connect(self.lib, (number, "out"), (sink, "value"))

        # Text does not cast to Number.
        reason = self.g.why_not_connect(self.lib, (text, "out"), (number, "value"))
        self.assertIn("cannot drive", reason)
        with self.assertRaises(ValueError):
            self.g.connect(self.lib, (text, "out"), (number, "value"))

    def test_single_link_input_is_replaced(self):
        a = self.g.add_node(self.lib, "text")
        b = self.g.add_node(self.lib, "text")
        sink = self.g.add_node(self.lib, "sink")
        self.g.connect(self.lib, (a, "out"), (sink, "value"))
        self.g.connect(self.lib, (b, "out"), (sink, "value"))
        self.assertEqual(self.g.connection_count, 1)
        self.assertEqual(self.g.source_of(sink, "value"), (b, "out"))

    def test_multi_input_accumulates(self):
        join = self.g.add_node(self.lib, "join")
        for _ in range(3):
            a = self.g.add_node(self.lib, "text")
            self.g.connect(self.lib, (a, "out"), (join, "parts"))
        self.assertEqual(len(self.g.links_into(join, "parts")), 3)

    def test_cycles_are_refused(self):
        a = self.g.add_node(self.lib, "join")
        b = self.g.add_node(self.lib, "join")
        self.g.connect(self.lib, (a, "out"), (b, "parts"))
        self.assertIn("cycle", self.g.why_not_connect(self.lib, (b, "out"), (a, "parts")))
        with self.assertRaises(ValueError):
            self.g.connect(self.lib, (b, "out"), (a, "parts"))
        self.assertTrue(self.g.is_acyclic())
        self.assertIsNone(self.g.find_cycle())

    def test_traversal(self):
        a = self.g.add_node(self.lib, "text")
        b = self.g.add_node(self.lib, "join")
        c = self.g.add_node(self.lib, "sink")
        self.g.connect(self.lib, (a, "out"), (b, "parts"))
        self.g.connect(self.lib, (b, "out"), (c, "value"))

        order = self.g.topological_order()
        self.assertLess(order.index(a), order.index(b))
        self.assertLess(order.index(b), order.index(c))
        self.assertEqual(self.g.dependency_order(c), [a, b, c])
        self.assertEqual(self.g.roots(), [a])
        self.assertEqual(self.g.sinks(), [c])
        self.assertEqual(self.g.ancestors(c), [b, a])
        self.assertEqual(self.g.descendants(a), [b, c])
        self.assertTrue(self.g.depends_on(a, c))
        self.assertFalse(self.g.depends_on(c, a))
        self.assertEqual(self.g.depths()[c], 2)
        self.assertEqual(len(self.g.components()), 1)

    def test_values_round_trip(self):
        a = self.g.add_node(self.lib, "text")
        self.g.set_input(self.lib, a, "value", "hello")
        self.assertEqual(self.g.input_value(a, "value"), "hello")

        j = self.g.add_node(self.lib, "join")
        self.g.set_param(self.lib, j, "separator", ", ")
        self.assertEqual(self.g.param(j, "separator"), ", ")

        with self.assertRaises(KeyError):
            self.g.set_input(self.lib, a, "nope", 1)

    def test_combo_values_are_validated(self):
        lib = nodez.Library()
        lib.add_type("Text", "#ffffff")
        lib.add_template("n", "N",
                         params=[nodez.Param("mode", nodez.Widget.combo(["a", "b"]), default="a")])
        g = nodez.Graph()
        n = g.add_node(lib, "n")
        g.set_param(lib, n, "mode", "b")
        self.assertEqual(g.param(n, "mode"), "b")
        with self.assertRaises(ValueError):
            g.set_param(lib, n, "mode", "c")

    def test_json_round_trip(self):
        a = self.g.add_node(self.lib, "text")
        b = self.g.add_node(self.lib, "sink")
        self.g.set_input(self.lib, a, "value", "round trip")
        self.g.connect(self.lib, (a, "out"), (b, "value"))

        restored = nodez.Graph.from_json(self.g.to_json(), self.lib)
        self.assertEqual(restored.node_count, 2)
        self.assertEqual(restored.connection_count, 1)
        self.assertEqual(restored.input_value(a, "value"), "round trip")
        # Saved ids must not be handed out again.
        self.assertNotIn(restored.add_node(self.lib, "text"), (a, b))
        json.loads(self.g.to_json())


class TestEvaluate(unittest.TestCase):
    def setUp(self):
        self.lib = tiny_library()
        self.g = nodez.Graph()

    def rule(self, node):
        if node.type == "text":
            return node.literal("value", "")
        if node.type == "number":
            return str(node.literal("value", 0.0))
        if node.type == "join":
            return node.param("separator", " ").join(node.inputs("parts"))
        if node.type == "sink":
            return node.input("value")
        raise AssertionError(node.type)

    def test_fold_in_dependency_order(self):
        hello = self.g.add_node(self.lib, "text")
        world = self.g.add_node(self.lib, "text")
        join = self.g.add_node(self.lib, "join")
        self.g.set_input(self.lib, hello, "value", "hello")
        self.g.set_input(self.lib, world, "value", "world")
        self.g.set_param(self.lib, join, "separator", ", ")
        self.g.connect(self.lib, (hello, "out"), (join, "parts"))
        self.g.connect(self.lib, (world, "out"), (join, "parts"))

        self.assertEqual(self.g.evaluate(self.lib, join, self.rule), "hello, world")

    def test_only_visits_what_the_target_needs(self):
        used = self.g.add_node(self.lib, "text")
        unused = self.g.add_node(self.lib, "text")
        sink = self.g.add_node(self.lib, "sink")
        self.g.connect(self.lib, (used, "out"), (sink, "value"))

        seen = []

        def spy(node):
            seen.append(node.id)
            return self.rule(node)

        self.g.evaluate(self.lib, sink, spy)
        self.assertIn(used, seen)
        self.assertNotIn(unused, seen)

    def test_evaluate_all_returns_every_node(self):
        a = self.g.add_node(self.lib, "text")
        b = self.g.add_node(self.lib, "text")
        results = self.g.evaluate_all(self.lib, self.rule)
        self.assertEqual(set(results), {a, b})

    def test_callback_exceptions_propagate(self):
        a = self.g.add_node(self.lib, "text")

        def boom(node):
            raise RuntimeError("no rule here")

        with self.assertRaises(RuntimeError) as caught:
            self.g.evaluate(self.lib, a, boom)
        self.assertEqual(str(caught.exception), "no rule here")

    def test_eval_node_surface(self):
        a = self.g.add_node(self.lib, "text")
        join = self.g.add_node(self.lib, "join")
        self.g.set_input(self.lib, a, "value", "x")
        self.g.connect(self.lib, (a, "out"), (join, "parts"))

        seen = {}

        def capture(node):
            seen[node.type] = node
            return node.type

        self.g.evaluate(self.lib, join, capture)
        text, joined = seen["text"], seen["join"]
        self.assertEqual(text.title, "Text")
        self.assertFalse(text.muted)
        self.assertEqual(text.literal("value"), "x")
        self.assertFalse(text.is_linked("value"))
        self.assertTrue(joined.is_linked("parts"))
        self.assertEqual(joined.inputs("parts"), ["text"])
        self.assertEqual(joined.input("parts"), "text")
        self.assertIsNone(joined.input("missing"))
        self.assertEqual(joined.params["separator"], " ")


class TestStackDemo(unittest.TestCase):
    """The demo domain, exercised end to end."""

    def test_sample_generates_a_clean_config(self):
        lib = stack.build_library()
        g = stack.build_sample(lib)
        text, problems = stack.generate(lib, g)
        self.assertEqual(problems, [])
        self.assertIn("image: nginx:1.27-alpine", text)
        self.assertIn('- "8080:80"', text)      # sexagesimal risk, must be quoted
        self.assertIn('version: "3.9"', text)   # must stay a string
        self.assertIn("${DB_PASSWORD}", text)   # assembled through Join Text

    def test_missing_image_is_reported_against_its_node(self):
        lib = stack.build_library()
        g = nodez.Graph()
        service = g.add_node(lib, "service")
        g.set_param(lib, service, "name", "lonely")
        out = g.add_node(lib, "stack")
        g.connect(lib, (service, "out"), (out, "services"))

        text, problems = stack.generate(lib, g)
        self.assertTrue(any("no image" in p for p in problems), problems)
        self.assertIn("generation failed", text)

    def test_muted_nodes_drop_out(self):
        lib = stack.build_library()
        g = stack.build_sample(lib)
        health = next(n for n in g.node_ids() if g.type_of(lib, n) == "healthcheck")
        g.set_muted(health, True)
        text, _ = stack.generate(lib, g)
        self.assertNotIn("healthcheck:", text)

    def test_yaml_quoting(self):
        self.assertTrue(stack._needs_quotes("yes"))
        self.assertTrue(stack._needs_quotes("3.9"))
        self.assertTrue(stack._needs_quotes("8080:80"))
        self.assertTrue(stack._needs_quotes("key: value"))
        self.assertFalse(stack._needs_quotes("nginx:latest"))
        self.assertFalse(stack._needs_quotes("/var/lib/data"))


if __name__ == "__main__":
    unittest.main(verbosity=2)
