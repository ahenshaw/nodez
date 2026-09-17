//! The domain: a node library describing a container service stack.
//!
//! This is the part you would replace for your own config format. Everything
//! downstream — the editor, the traversal, the generator — is driven by what is
//! declared here.

use egui::Color32;
use nodez::{NodeLibrary, NodeTemplate, ParamSpec, SocketSpec, TemplateId, Widget};

/// Socket types, in the order they are registered.
///
/// Kept as a named struct so the domain is readable at a glance and so app code
/// can reason about types without string lookups.
#[allow(dead_code)]
pub struct Types {
    pub text: nodez::DataTypeId,
    pub number: nodez::DataTypeId,
    pub flag: nodez::DataTypeId,
    pub image: nodez::DataTypeId,
    pub env: nodez::DataTypeId,
    pub port: nodez::DataTypeId,
    pub mount: nodez::DataTypeId,
    pub network: nodez::DataTypeId,
    pub health: nodez::DataTypeId,
    pub service: nodez::DataTypeId,
}

/// Template handles, so the app can add nodes without string lookups.
#[allow(dead_code)]
pub struct Templates {
    pub text: TemplateId,
    pub number: TemplateId,
    pub flag: TemplateId,
    pub secret: TemplateId,
    pub join: TemplateId,
    pub image: TemplateId,
    pub env_var: TemplateId,
    pub env_file: TemplateId,
    pub port: TemplateId,
    pub volume: TemplateId,
    pub network: TemplateId,
    pub healthcheck: TemplateId,
    pub service: TemplateId,
    pub stack: TemplateId,
}

/// Everything the editor needs to know about this domain.
pub struct Domain {
    pub library: NodeLibrary,
    #[allow(dead_code)]
    pub types: Types,
    pub templates: Templates,
}

const INPUT: Color32 = Color32::from_rgb(0x3B, 0x52, 0x70);
const CONVERT: Color32 = Color32::from_rgb(0x4A, 0x5F, 0x3B);
const BUILD: Color32 = Color32::from_rgb(0x6E, 0x5A, 0x2E);
const RUNTIME: Color32 = Color32::from_rgb(0x7A, 0x4A, 0x2E);
const OUTPUT: Color32 = Color32::from_rgb(0x5C, 0x3B, 0x5C);

impl Domain {
    pub fn new() -> Self {
        let mut library = NodeLibrary::new();

        // -------------------------------------------------------- types
        // Colours follow Blender's socket palette so the graph reads the same
        // way: grey for plain values, green for structure, yellow for assets.
        let text = library.types.register(
            nodez::DataTypeBuilder::new("Text", Color32::from_rgb(0x70, 0xB2, 0xFF))
                .description("A string. Most fields take one."),
        );
        let number = library.types.register(
            nodez::DataTypeBuilder::new("Number", Color32::from_rgb(0xA1, 0xA1, 0xA1))
                .description("A numeric literal."),
        );
        let flag = library.types.register(
            nodez::DataTypeBuilder::new("Flag", Color32::from_rgb(0xCC, 0xA6, 0xD6))
                .description("A boolean switch."),
        );
        let image = library.types.register(
            nodez::DataTypeBuilder::new("Image", Color32::from_rgb(0xC7, 0xC7, 0x29))
                .description("A container image reference."),
        );
        let env = library.types.register(
            nodez::DataTypeBuilder::new("Env", Color32::from_rgb(0x59, 0x8C, 0x5C))
                .shape(nodez::SocketShape::Diamond)
                .description("One or more environment entries."),
        );
        let port = library.types.register(
            nodez::DataTypeBuilder::new("Port", Color32::from_rgb(0xE0, 0x7A, 0x5F))
                .shape(nodez::SocketShape::Diamond)
                .description("A published port mapping."),
        );
        let mount = library.types.register(
            nodez::DataTypeBuilder::new("Mount", Color32::from_rgb(0x63, 0x63, 0xC7))
                .shape(nodez::SocketShape::Diamond)
                .description("A bind mount or named volume."),
        );
        let network = library.types.register(
            nodez::DataTypeBuilder::new("Network", Color32::from_rgb(0x4C, 0xB3, 0xCC))
                .description("A network the stack defines."),
        );
        let health = library.types.register(
            nodez::DataTypeBuilder::new("Health", Color32::from_rgb(0xD6, 0xA6, 0xCC))
                .description("A container health probe."),
        );
        let service = library.types.register(
            nodez::DataTypeBuilder::new("Service", Color32::from_rgb(0xE3, 0x9B, 0x3A))
                .shape(nodez::SocketShape::Square)
                .description("A fully described service."),
        );

        // Numbers and flags can be spelled as text; text cannot become either.
        // This is what makes a Number socket droppable onto a Text input.
        library.types.allow_cast(number, text);
        library.types.allow_cast(flag, text);

        // --------------------------------------------------- categories
        library.set_category_color("Input", INPUT);
        library.set_category_color("Convert", CONVERT);
        library.set_category_color("Build", BUILD);
        library.set_category_color("Runtime", RUNTIME);
        library.set_category_color("Output", OUTPUT);

        // ---------------------------------------------------- templates
        let text_node = library.register(
            NodeTemplate::new("text", "Text")
                .category("Input")
                .width(170.0)
                .description("A literal string.")
                .keywords(["string", "literal"])
                .input(
                    SocketSpec::new("value", text)
                        .label("")
                        .widget(Widget::text_hint("text\u{2026}"), ""),
                )
                .output(SocketSpec::new("out", text).label("Text")),
        );
        let number_node = library.register(
            NodeTemplate::new("number", "Number")
                .category("Input")
                .width(150.0)
                .description("A literal number.")
                .keywords(["int", "float", "literal"])
                .input(SocketSpec::new("value", number).label("").editable(Widget::float()))
                .output(SocketSpec::new("out", number).label("Number")),
        );
        let flag_node = library.register(
            NodeTemplate::new("flag", "Flag")
                .category("Input")
                .width(140.0)
                .description("A literal true / false.")
                .keywords(["bool", "toggle"])
                .input(
                    SocketSpec::new("value", flag)
                        .label("Enabled")
                        .editable(Widget::Checkbox),
                )
                .output(SocketSpec::new("out", flag).label("Flag")),
        );
        let secret = library.register(
            NodeTemplate::new("secret", "Secret Reference")
                .category("Input")
                .width(190.0)
                .description("Emits ${NAME}, left for the runtime to fill in.")
                .keywords(["env", "interpolate", "variable"])
                .input(
                    SocketSpec::new("name", text)
                        .widget(Widget::text_hint("DB_PASSWORD"), "DB_PASSWORD"),
                )
                .output(SocketSpec::new("out", text).label("Text")),
        );
        let join = library.register(
            NodeTemplate::new("join", "Join Text")
                .category("Convert")
                .width(180.0)
                .description("Concatenates every connected string.")
                .keywords(["concat", "merge"])
                .input(
                    SocketSpec::new("parts", text)
                        .multi()
                        .description("Accepts any number of strings, in connection order."),
                )
                .param(
                    ParamSpec::new("separator", Widget::text_hint("separator"))
                        .default_value("")
                        .show_label(false),
                )
                .output(SocketSpec::new("out", text).label("Text")),
        );

        let image_node = library.register(
            NodeTemplate::new("image", "Image")
                .category("Build")
                .width(200.0)
                .description("repository:tag")
                .keywords(["container", "docker", "registry"])
                .input(
                    SocketSpec::new("repository", text)
                        .widget(Widget::text_hint("nginx"), "nginx"),
                )
                .input(SocketSpec::new("tag", text).widget(Widget::text_hint("latest"), "latest"))
                .output(SocketSpec::new("out", image).label("Image")),
        );
        let env_var = library.register(
            NodeTemplate::new("env_var", "Environment Variable")
                .category("Build")
                .width(210.0)
                .description("One KEY=value pair.")
                .keywords(["env", "variable"])
                .input(SocketSpec::new("key", text).widget(Widget::text_hint("KEY"), "KEY"))
                .input(SocketSpec::new("value", text).widget(Widget::text_hint("value"), ""))
                .output(SocketSpec::new("out", env).label("Env")),
        );
        let env_file = library.register(
            NodeTemplate::new("env_file", "Environment File")
                .category("Build")
                .width(210.0)
                .description("Loads variables from a file at start-up.")
                .keywords(["env", "dotenv", "file"])
                .input(SocketSpec::new("path", text).widget(Widget::text_hint(".env"), ".env"))
                .output(SocketSpec::new("out", env).label("Env")),
        );
        let port_node = library.register(
            NodeTemplate::new("port", "Port Mapping")
                .category("Build")
                .width(190.0)
                .description("Publishes a container port on the host.")
                .keywords(["expose", "publish", "tcp"])
                .input(
                    SocketSpec::new("host", number)
                        .widget(Widget::int_range(1, 65535), 8080_i64),
                )
                .input(
                    SocketSpec::new("container", number)
                        .widget(Widget::int_range(1, 65535), 80_i64),
                )
                .param(
                    ParamSpec::new("protocol", Widget::combo(["tcp", "udp"]))
                        .default_value(nodez::Value::Choice("tcp".to_owned())),
                )
                .output(SocketSpec::new("out", port).label("Port")),
        );
        let volume = library.register(
            NodeTemplate::new("volume", "Volume")
                .category("Build")
                .width(210.0)
                .description("A bind mount or named volume.")
                .keywords(["mount", "bind", "storage"])
                .input(
                    SocketSpec::new("source", text)
                        .widget(Widget::text_hint("./data"), "./data"),
                )
                .input(
                    SocketSpec::new("target", text)
                        .widget(Widget::text_hint("/var/lib/data"), "/var/lib/data"),
                )
                .input(
                    SocketSpec::new("read_only", flag)
                        .label("Read Only")
                        .editable(Widget::Checkbox),
                )
                .output(SocketSpec::new("out", mount).label("Mount")),
        );
        let network_node = library.register(
            NodeTemplate::new("network", "Network")
                .category("Build")
                .width(200.0)
                .description("A network the stack declares and services join.")
                .keywords(["bridge", "overlay"])
                .input(
                    SocketSpec::new("name", text)
                        .widget(Widget::text_hint("frontend"), "frontend"),
                )
                .param(
                    ParamSpec::new("driver", Widget::combo(["bridge", "overlay", "host", "none"]))
                        .default_value(nodez::Value::Choice("bridge".to_owned())),
                )
                .output(SocketSpec::new("out", network).label("Network")),
        );
        let healthcheck = library.register(
            NodeTemplate::new("healthcheck", "Health Check")
                .category("Build")
                .width(220.0)
                .description("Probe run inside the container to decide if it is healthy.")
                .keywords(["probe", "liveness"])
                .input(
                    SocketSpec::new("command", text)
                        .widget(Widget::text_hint("curl -f localhost/"), "curl -f localhost/"),
                )
                .input(
                    SocketSpec::new("interval", number)
                        .label("Interval (s)")
                        .widget(Widget::int_range(1, 3600), 30_i64),
                )
                .input(
                    SocketSpec::new("retries", number)
                        .widget(Widget::int_range(1, 20), 3_i64),
                )
                .output(SocketSpec::new("out", health).label("Health")),
        );

        let service_node = library.register(
            NodeTemplate::new("service", "Service")
                .category("Runtime")
                .width(230.0)
                .description("One service in the stack. Everything it needs plugs in here.")
                .keywords(["container", "deployment"])
                .param(
                    ParamSpec::new("name", Widget::text_hint("service name"))
                        .default_value("web")
                        .show_label(false),
                )
                .param(
                    ParamSpec::new(
                        "restart",
                        Widget::combo(["no", "always", "on-failure", "unless-stopped"]),
                    )
                    .default_value(nodez::Value::Choice("unless-stopped".to_owned())),
                )
                .input(SocketSpec::new("image", image).description("Required."))
                .input(
                    SocketSpec::new("command", text)
                        .widget(Widget::text_hint("(default entrypoint)"), ""),
                )
                .input(
                    SocketSpec::new("replicas", number)
                        .widget(Widget::int_range(1, 64), 1_i64),
                )
                .input(SocketSpec::new("environment", env).multi())
                .input(SocketSpec::new("ports", port).multi())
                .input(SocketSpec::new("volumes", mount).multi())
                .input(SocketSpec::new("networks", network).multi())
                .input(
                    SocketSpec::new("depends_on", service)
                        .label("Depends On")
                        .multi(),
                )
                .input(SocketSpec::new("healthcheck", health))
                .output(SocketSpec::new("out", service).label("Service")),
        );
        let stack = library.register(
            NodeTemplate::new("stack", "Stack Output")
                .category("Output")
                .width(220.0)
                .description("The root of the document. Everything reachable from here is emitted.")
                .keywords(["compose", "output", "root", "file"])
                .param(
                    ParamSpec::new("name", Widget::text_hint("stack name"))
                        .default_value("my-stack")
                        .show_label(false),
                )
                .param(
                    ParamSpec::new("version", Widget::combo(["3.9", "3.8", "3.7"]))
                        .default_value(nodez::Value::Choice("3.9".to_owned())),
                )
                .input(SocketSpec::new("services", service).multi())
                .input(SocketSpec::new("networks", network).multi()),
        );

        Self {
            library,
            types: Types {
                text,
                number,
                flag,
                image,
                env,
                port,
                mount,
                network,
                health,
                service,
            },
            templates: Templates {
                text: text_node,
                number: number_node,
                flag: flag_node,
                secret,
                join,
                image: image_node,
                env_var,
                env_file,
                port: port_node,
                volume,
                network: network_node,
                healthcheck,
                service: service_node,
                stack,
            },
        }
    }
}
