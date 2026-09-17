"""A container-stack config format, described as a nodez node library.

This is the Python counterpart of the Rust `nodez-demo` crate: the same domain,
the same generated document, expressed through the bindings. Replace this module
to describe your own config format.
"""

from __future__ import annotations

import nodez

# Socket types. Colours follow Blender's palette so the graph reads the same
# way: grey and blue for plain values, green for structure, yellow for assets.
TEXT = "Text"
NUMBER = "Number"
FLAG = "Flag"
IMAGE = "Image"
ENV = "Env"
PORT = "Port"
MOUNT = "Mount"
NETWORK = "Network"
HEALTH = "Health"
SERVICE = "Service"

CATEGORY_COLORS = {
    "Input": "#3B5270",
    "Convert": "#4A5F3B",
    "Build": "#6E5A2E",
    "Runtime": "#7A4A2E",
    "Output": "#5C3B5C",
}


def build_library() -> nodez.Library:
    """Register every socket type and node template."""
    lib = nodez.Library()

    lib.add_type(TEXT, "#70B2FF", description="A string. Most fields take one.")
    lib.add_type(NUMBER, "#A1A1A1", description="A numeric literal.")
    lib.add_type(FLAG, "#CCA6D6", description="A boolean switch.")
    lib.add_type(IMAGE, "#C7C729", description="A container image reference.")
    lib.add_type(ENV, "#598C5C", shape="diamond", description="Environment entries.")
    lib.add_type(PORT, "#E07A5F", shape="diamond", description="A published port.")
    lib.add_type(MOUNT, "#6363C7", shape="diamond", description="A bind mount.")
    lib.add_type(NETWORK, "#4CB3CC", description="A network the stack defines.")
    lib.add_type(HEALTH, "#D6A6CC", description="A container health probe.")
    lib.add_type(SERVICE, "#E39B3A", shape="square", description="A described service.")

    # Numbers and flags can be spelled as text; text cannot become either. This
    # is what makes a Number socket droppable onto a Text input in the editor.
    lib.allow_cast(NUMBER, TEXT)
    lib.allow_cast(FLAG, TEXT)

    for category, color in CATEGORY_COLORS.items():
        lib.set_category_color(category, color)

    W = nodez.Widget
    S = nodez.Socket
    P = nodez.Param

    lib.add_template(
        "text", "Text",
        category="Input", width=170.0, keywords=["string", "literal"],
        description="A literal string.",
        inputs=[S("value", TEXT, label="", widget=W.text(hint="text…"), default="")],
        outputs=[S("out", TEXT, label="Text")],
    )
    lib.add_template(
        "number", "Number",
        category="Input", width=150.0, keywords=["int", "float", "literal"],
        description="A literal number.",
        inputs=[S("value", NUMBER, label="", widget=W.float(), default=0.0)],
        outputs=[S("out", NUMBER, label="Number")],
    )
    lib.add_template(
        "flag", "Flag",
        category="Input", width=140.0, keywords=["bool", "toggle"],
        description="A literal true / false.",
        inputs=[S("value", FLAG, label="Enabled", widget=W.checkbox(), default=False)],
        outputs=[S("out", FLAG, label="Flag")],
    )
    lib.add_template(
        "secret", "Secret Reference",
        category="Input", width=190.0, keywords=["env", "interpolate"],
        description="Emits ${NAME}, left for the runtime to fill in.",
        inputs=[S("name", TEXT, widget=W.text(hint="DB_PASSWORD"), default="DB_PASSWORD")],
        outputs=[S("out", TEXT, label="Text")],
    )
    lib.add_template(
        "join", "Join Text",
        category="Convert", width=180.0, keywords=["concat", "merge"],
        description="Concatenates every connected string.",
        inputs=[S("parts", TEXT, multi=True,
                  description="Any number of strings, in connection order.")],
        params=[P("separator", W.text(hint="separator"), default="", show_label=False)],
        outputs=[S("out", TEXT, label="Text")],
    )

    lib.add_template(
        "image", "Image",
        category="Build", width=200.0, keywords=["container", "docker", "registry"],
        description="repository:tag",
        inputs=[
            S("repository", TEXT, widget=W.text(hint="nginx"), default="nginx"),
            S("tag", TEXT, widget=W.text(hint="latest"), default="latest"),
        ],
        outputs=[S("out", IMAGE, label="Image")],
    )
    lib.add_template(
        "env_var", "Environment Variable",
        category="Build", width=210.0, keywords=["env", "variable"],
        description="One KEY=value pair.",
        inputs=[
            S("key", TEXT, widget=W.text(hint="KEY"), default="KEY"),
            S("value", TEXT, widget=W.text(hint="value"), default=""),
        ],
        outputs=[S("out", ENV, label="Env")],
    )
    lib.add_template(
        "env_file", "Environment File",
        category="Build", width=210.0, keywords=["env", "dotenv", "file"],
        description="Loads variables from a file at start-up.",
        inputs=[S("path", TEXT, widget=W.text(hint=".env"), default=".env")],
        outputs=[S("out", ENV, label="Env")],
    )
    lib.add_template(
        "port", "Port Mapping",
        category="Build", width=190.0, keywords=["expose", "publish", "tcp"],
        description="Publishes a container port on the host.",
        inputs=[
            S("host", NUMBER, widget=W.int(min=1, max=65535), default=8080),
            S("container", NUMBER, widget=W.int(min=1, max=65535), default=80),
        ],
        params=[P("protocol", W.combo(["tcp", "udp"]), default="tcp")],
        outputs=[S("out", PORT, label="Port")],
    )
    lib.add_template(
        "volume", "Volume",
        category="Build", width=210.0, keywords=["mount", "bind", "storage"],
        description="A bind mount or named volume.",
        inputs=[
            S("source", TEXT, widget=W.text(hint="./data"), default="./data"),
            S("target", TEXT, widget=W.text(hint="/var/lib/data"), default="/var/lib/data"),
            S("read_only", FLAG, label="Read Only", widget=W.checkbox(), default=False),
        ],
        outputs=[S("out", MOUNT, label="Mount")],
    )
    lib.add_template(
        "network", "Network",
        category="Build", width=200.0, keywords=["bridge", "overlay"],
        description="A network the stack declares and services join.",
        inputs=[S("name", TEXT, widget=W.text(hint="frontend"), default="frontend")],
        params=[P("driver", W.combo(["bridge", "overlay", "host", "none"]), default="bridge")],
        outputs=[S("out", NETWORK, label="Network")],
    )
    lib.add_template(
        "healthcheck", "Health Check",
        category="Build", width=220.0, keywords=["probe", "liveness"],
        description="Probe run inside the container to decide if it is healthy.",
        inputs=[
            S("command", TEXT, widget=W.text(hint="curl -f localhost/"),
              default="curl -f localhost/"),
            S("interval", NUMBER, label="Interval (s)",
              widget=W.int(min=1, max=3600), default=30),
            S("retries", NUMBER, widget=W.int(min=1, max=20), default=3),
        ],
        outputs=[S("out", HEALTH, label="Health")],
    )

    lib.add_template(
        "service", "Service",
        category="Runtime", width=230.0, keywords=["container", "deployment"],
        description="One service in the stack. Everything it needs plugs in here.",
        params=[
            P("name", W.text(hint="service name"), default="web", show_label=False),
            P("restart", W.combo(["no", "always", "on-failure", "unless-stopped"]),
              default="unless-stopped"),
        ],
        inputs=[
            S("image", IMAGE, description="Required."),
            S("command", TEXT, widget=W.text(hint="(default entrypoint)"), default=""),
            S("replicas", NUMBER, widget=W.int(min=1, max=64), default=1),
            S("environment", ENV, multi=True),
            S("ports", PORT, multi=True),
            S("volumes", MOUNT, multi=True),
            S("networks", NETWORK, multi=True),
            S("depends_on", SERVICE, label="Depends On", multi=True),
            S("healthcheck", HEALTH),
        ],
        outputs=[S("out", SERVICE, label="Service")],
    )
    lib.add_template(
        "stack", "Stack Output",
        category="Output", width=220.0, keywords=["compose", "output", "root"],
        description="The root of the document. Everything reachable from here is emitted.",
        params=[
            P("name", W.text(hint="stack name"), default="my-stack", show_label=False),
            P("version", W.combo(["3.9", "3.8", "3.7"]), default="3.9"),
        ],
        inputs=[
            S("services", SERVICE, multi=True),
            S("networks", NETWORK, multi=True),
        ],
    )

    return lib


# --------------------------------------------------------------------- output


def _needs_quotes(s: str) -> bool:
    """Mirror of the Rust emitter's rules.

    A bare colon is fine — `image: nginx:latest` is what a hand-written compose
    file looks like — but a colon followed by a space starts a mapping, and a
    digits-and-colons string like `8080:80` is a sexagesimal number to a YAML
    1.1 parser.
    """
    if s == "" or s.lower() in {"true", "false", "yes", "no", "on", "off", "null", "~"}:
        return True
    try:
        float(s)
        return True
    except ValueError:
        pass
    if ":" in s and all(c.isdigit() or c == ":" for c in s):
        return True
    return (
        s[0] in " -" or s[-1] in " :"
        or ": " in s
        or any(c in s for c in "#{}[],&*!|>'\"%@`\\")
        or "\n" in s
    )


def _scalar(value) -> str:
    if value is None:
        return "~"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        return str(int(value)) if value.is_integer() else str(value)
    text = str(value)
    if _needs_quotes(text):
        return '"' + text.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n") + '"'
    return text


def to_yaml(value, indent: int = 0, inline_start: bool = False) -> str:
    """Emit an ordered dict / list / scalar tree as YAML."""
    pad = "  " * indent
    out = []
    if isinstance(value, dict) and value:
        for i, (key, child) in enumerate(value.items()):
            prefix = "" if (i == 0 and inline_start) else pad
            if isinstance(child, (dict, list)) and child:
                out.append(f"{prefix}{_scalar(key)}:\n")
                out.append(to_yaml(child, indent + 1))
            elif isinstance(child, dict):
                out.append(f"{prefix}{_scalar(key)}: {{}}\n")
            elif isinstance(child, list):
                out.append(f"{prefix}{_scalar(key)}: []\n")
            else:
                out.append(f"{prefix}{_scalar(key)}: {_scalar(child)}\n")
    elif isinstance(value, list) and value:
        for i, item in enumerate(value):
            prefix = "" if (i == 0 and inline_start) else pad
            if isinstance(item, dict) and item:
                out.append(f"{prefix}- ")
                out.append(to_yaml(item, indent + 1, inline_start=True))
            elif isinstance(item, list) and item:
                out.append(f"{prefix}-\n")
                out.append(to_yaml(item, indent + 1))
            else:
                out.append(f"{prefix}- {_scalar(item)}\n")
    else:
        prefix = "" if inline_start else pad
        out.append(f"{prefix}{_scalar(value)}\n")
    return "".join(out)


class ConfigError(Exception):
    """Raised by a node rule; nodez reports it against the node that raised."""


def _rule(node: nodez.EvalNode):
    """Fold one node into a fragment of the document.

    Because sockets are typed, a fragment only ever reaches an input that can
    accept it — so there is no need to tag them. The one pair sharing a type is
    Env, where a variable yields a list and a file yields a dict.
    """
    if node.muted:
        return None

    kind = node.type

    if kind == "text":
        return node.literal("value", "")
    if kind == "number":
        return node.literal("value", 0.0)
    if kind == "flag":
        return bool(node.literal("value", False))
    if kind == "secret":
        return "${%s}" % node.resolve("name", "")
    if kind == "join":
        separator = node.param("separator", "")
        return separator.join(str(p) for p in node.inputs("parts") if p is not None)

    if kind == "image":
        repository = node.resolve("repository", "")
        if not repository:
            raise ConfigError("Image needs a repository.")
        tag = node.resolve("tag", "")
        return f"{repository}:{tag}" if tag else repository

    if kind == "env_var":
        key = node.resolve("key", "")
        if not key:
            raise ConfigError("Environment Variable needs a key.")
        return [f"{key}={node.resolve('value', '')}"]

    if kind == "env_file":
        return {"env_file": node.resolve("path", "")}

    if kind == "port":
        host = int(node.resolve("host", 0))
        container = int(node.resolve("container", 0))
        protocol = node.param("protocol", "tcp")
        return f"{host}:{container}" if protocol == "tcp" else f"{host}:{container}/{protocol}"

    if kind == "volume":
        target = node.resolve("target", "")
        if not target:
            raise ConfigError("Volume needs a target path.")
        mount = {"type": "bind", "source": node.resolve("source", ""), "target": target}
        if node.resolve("read_only", False):
            mount["read_only"] = True
        return mount

    if kind == "network":
        name = node.resolve("name", "")
        if not name:
            raise ConfigError("Network needs a name.")
        return (name, {"driver": node.param("driver", "bridge")})

    if kind == "healthcheck":
        command = node.resolve("command", "")
        if not command:
            raise ConfigError("Health Check needs a command.")
        return {
            "test": ["CMD-SHELL", command],
            "interval": "%ds" % int(node.resolve("interval", 30)),
            "retries": int(node.resolve("retries", 3)),
        }

    if kind == "service":
        return _service(node)
    if kind == "stack":
        return _stack(node)

    raise ConfigError(f"no rule for node type `{kind}`")


def _service(node: nodez.EvalNode):
    name = (node.param("name", "") or "").strip()
    if not name:
        raise ConfigError("Service needs a name.")

    image = node.input("image")
    if image is None:
        raise ConfigError(f"Service `{name}` has no image connected.")

    body = {"image": image}

    command = node.resolve("command", "")
    if command:
        body["command"] = command

    restart = node.param("restart", "no")
    if restart != "no":
        body["restart"] = restart

    replicas = int(node.resolve("replicas", 1))
    if replicas > 1:
        body["deploy"] = {"replicas": replicas}

    ports = [p for p in node.inputs("ports") if p]
    if ports:
        body["ports"] = ports

    environment, env_files = [], []
    for entry in node.inputs("environment"):
        if isinstance(entry, list):
            environment.extend(entry)
        elif isinstance(entry, dict) and entry.get("env_file"):
            env_files.append(entry["env_file"])
    if environment:
        body["environment"] = environment
    if env_files:
        body["env_file"] = env_files

    volumes = [v for v in node.inputs("volumes") if v]
    if volumes:
        body["volumes"] = volumes

    networks = [n[0] for n in node.inputs("networks") if n]
    if networks:
        body["networks"] = networks

    depends = [d[0] for d in node.inputs("depends_on") if d]
    if depends:
        body["depends_on"] = depends

    probe = node.input("healthcheck")
    if probe:
        body["healthcheck"] = probe

    return (name, body)


def _stack(node: nodez.EvalNode):
    services = dict(s for s in node.inputs("services") if s)
    networks = dict(n for n in node.inputs("networks") if n)

    # A service naming a network implies the stack declares it, even when the
    # network node is not wired to the stack directly.
    for body in services.values():
        for used in body.get("networks", []):
            networks.setdefault(used, {"driver": "bridge"})

    document = {"version": node.param("version", "3.9")}
    name = (node.param("name", "") or "").strip()
    if name:
        document["name"] = name
    document["services"] = services
    if networks:
        document["networks"] = networks
    return document


def generate(lib: nodez.Library, graph: nodez.Graph) -> tuple[str, list[str]]:
    """Fold the graph into a config document. Returns `(text, problems)`."""
    problems: list[str] = []

    stacks = [n for n in graph.node_ids() if graph.type_of(lib, n) == "stack"]
    if not stacks:
        return "# add a Stack Output node\n", ["No Stack Output node."]
    if len(stacks) > 1:
        problems.append(f"{len(stacks)} Stack Output nodes; only the first is emitted.")
    stack_id = stacks[0]

    # Which nodes actually feed the output? `ancestors` is the traversal API.
    contributing = set(graph.ancestors(stack_id)) | {stack_id}
    orphans = graph.node_count - len(contributing)
    if orphans:
        problems.append(f"{orphans} node(s) are not connected to the output and were skipped.")

    try:
        document = graph.evaluate(lib, stack_id, _rule)
    except Exception as e:  # a node rule raised, or the graph has a cycle
        return f"# generation failed\n# {e}\n", problems + [str(e)]

    names = [
        graph.param(n, "name")
        for n in contributing
        if graph.type_of(lib, n) == "service"
    ]
    for name in {n for n in names if names.count(n) > 1}:
        problems.append(f"Duplicate service name `{name}`.")

    return to_yaml(document), problems


# --------------------------------------------------------------- sample graph


def build_sample(lib: nodez.Library) -> nodez.Graph:
    """The same starting graph the Rust demo ships, built through the bindings."""
    g = nodez.Graph()

    def node(template, **values):
        nid = g.add_node(lib, template)
        for key, value in values.items():
            g.set_input(lib, nid, key, value)
        return nid

    frontend = node("network", name="frontend")
    backend = node("network", name="backend")

    web_image = node("image", repository="nginx", tag="1.27-alpine")
    web_port = node("port", host=8080, container=80)
    web_conf = node("volume", source="./nginx.conf",
                    target="/etc/nginx/nginx.conf", read_only=True)
    web = g.add_node(lib, "service")
    g.set_param(lib, web, "name", "web")

    api_image = node("image", repository="ghcr.io/acme/api", tag="2.4.0")
    api_port = node("port", host=9000, container=9000)
    api_replicas = node("number", value=3.0)

    # DATABASE_URL is assembled from literals and a secret reference, which is
    # what the Join Text node is for.
    db_password = node("secret", name="DB_PASSWORD")
    dsn_prefix = node("text", value="postgres://app:")
    dsn_suffix = node("text", value="@db:5432/app")
    dsn = g.add_node(lib, "join")
    g.set_param(lib, dsn, "separator", "")
    api_env = node("env_var", key="DATABASE_URL")

    api = g.add_node(lib, "service")
    g.set_param(lib, api, "name", "api")
    g.set_param(lib, api, "restart", "always")

    db_image = node("image", repository="postgres", tag="16-alpine")
    db_volume = node("volume", source="pgdata", target="/var/lib/postgresql/data")
    db_env = node("env_var", key="POSTGRES_PASSWORD")
    db_health = node("healthcheck", command="pg_isready -U app", interval=10)
    db = g.add_node(lib, "service")
    g.set_param(lib, db, "name", "db")

    stack_out = g.add_node(lib, "stack")
    g.set_param(lib, stack_out, "name", "acme-platform")

    for src, src_socket, dst, dst_socket in [
        (web_image, "out", web, "image"),
        (web_port, "out", web, "ports"),
        (web_conf, "out", web, "volumes"),
        (frontend, "out", web, "networks"),
        (api, "out", web, "depends_on"),
        (api_image, "out", api, "image"),
        (api_port, "out", api, "ports"),
        (api_replicas, "out", api, "replicas"),
        (dsn_prefix, "out", dsn, "parts"),
        (db_password, "out", dsn, "parts"),
        (dsn_suffix, "out", dsn, "parts"),
        (dsn, "out", api_env, "value"),
        (api_env, "out", api, "environment"),
        (frontend, "out", api, "networks"),
        (backend, "out", api, "networks"),
        (db, "out", api, "depends_on"),
        (db_image, "out", db, "image"),
        (db_volume, "out", db, "volumes"),
        (db_password, "out", db_env, "value"),
        (db_env, "out", db, "environment"),
        (db_health, "out", db, "healthcheck"),
        (backend, "out", db, "networks"),
        (web, "out", stack_out, "services"),
        (api, "out", stack_out, "services"),
        (db, "out", stack_out, "services"),
        (frontend, "out", stack_out, "networks"),
        (backend, "out", stack_out, "networks"),
    ]:
        g.connect(lib, (src, src_socket), (dst, dst_socket))

    g.layout(lib)
    return g
