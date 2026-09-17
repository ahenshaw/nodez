//! The domain: a container stack, described as Rust types.
//!
//! Each struct here is a kind of node. A field carrying `#[input]` is a socket;
//! a bare field is a parameter drawn in the node body. The field's *type* gives
//! the socket type, its colour and whether it can be typed into, and the outer
//! wrapper gives the arity — `T` is one required link, `Option<T>` is optional,
//! `Multi<T>` is a fan-in.
//!
//! Replace this file to describe your own config format.

use nodez::{Evaluate, Fold, Multi, NodeError, NodeType, SocketType, Value};

// ---------------------------------------------------------------- wire types
//
// `String`, `i64` and `bool` are socket types already, so only the types that
// carry real domain meaning are declared. Colours derive from the type name
// unless one is given; the shapes are semantic, so they stay explicit.

#[derive(Clone, Debug, SocketType)]
#[socket(color = "#C7C729", description = "A container image reference.")]
pub struct ImageRef(String);

/// One link may carry several entries, which is why this is a type of its own
/// rather than the arity wrapper `Multi`.
#[derive(Clone, Debug, SocketType)]
#[socket(color = "#598C5C", shape = "diamond", description = "Environment entries.")]
pub struct EnvList(Vec<String>);

/// The original demo carried env files down the same socket as env entries and
/// told them apart with a match. A type of their own does that in the schema.
#[derive(Clone, Debug, SocketType)]
#[socket(color = "#7FA66B", shape = "diamond", description = "A file of env entries.")]
pub struct EnvFileRef(String);

#[derive(Clone, Debug, SocketType)]
#[socket(color = "#E07A5F", shape = "diamond", description = "A published port.")]
pub struct PortMap(String);

#[derive(Clone, Debug, SocketType)]
#[socket(color = "#6363C7", shape = "diamond", description = "A bind mount.")]
pub struct Mount(Value);

#[derive(Clone, Debug, SocketType)]
#[socket(color = "#4CB3CC", description = "A network the stack declares.")]
pub struct NetworkDef {
    name: String,
    driver: String,
}

#[derive(Clone, Debug, SocketType)]
#[socket(color = "#D6A6CC", description = "A container health probe.")]
pub struct Health(Value);

#[derive(Clone, Debug, SocketType)]
#[socket(color = "#E39B3A", shape = "square", description = "A described service.")]
pub struct ServiceDef {
    name: String,
    body: Value,
}

/// What the whole graph folds to. The stack node has no output socket, so it
/// says `produces` instead.
#[derive(Clone, Debug, SocketType)]
#[socket(color = "#E5E5E5", shape = "square")]
pub struct StackDoc(pub Value);

#[derive(Clone, Copy, Debug, PartialEq, Eq, SocketType)]
#[socket(color = "#CCA6D6", widget = choice)]
pub enum Protocol {
    Tcp,
    Udp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SocketType)]
#[socket(color = "#CCA6D6", widget = choice)]
pub enum Driver {
    Bridge,
    Overlay,
    Host,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SocketType)]
#[socket(color = "#CCA6D6", widget = choice)]
pub enum Restart {
    No,
    Always,
    OnFailure,
    UnlessStopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, SocketType)]
#[socket(color = "#CCA6D6", widget = choice)]
pub enum Version {
    #[socket(rename = "3.9")]
    V3_9,
    #[socket(rename = "3.8")]
    V3_8,
    #[socket(rename = "3.7")]
    V3_7,
}

// -------------------------------------------------------------------- input

#[derive(Debug, NodeType)]
#[node(
    id = "text",
    category = "Input",
    description = "A literal string.",
    keywords = "string, literal",
    output = String
)]
pub struct Text {
    #[input(label = "", hint = "text\u{2026}")]
    value: String,
}

#[derive(Debug, NodeType)]
#[node(
    category = "Input",
    description = "A literal number.",
    keywords = "int, literal",
    output = i64
)]
pub struct Number {
    #[input(label = "")]
    value: i64,
}

#[derive(Debug, NodeType)]
#[node(
    category = "Input",
    description = "A literal true / false.",
    keywords = "bool, toggle",
    output = bool
)]
pub struct Flag {
    #[input(label = "Enabled")]
    value: bool,
}

#[derive(Debug, NodeType)]
#[node(
    id = "secret",
    label = "Secret Reference",
    category = "Input",
    description = "Emits ${NAME}, left for the runtime to fill in.",
    keywords = "env, interpolate, variable",
    output = String
)]
pub struct Secret {
    #[input(default = "DB_PASSWORD")]
    name: String,
}

// ------------------------------------------------------------------ convert

#[derive(Debug, NodeType)]
#[node(
    id = "join",
    label = "Join Text",
    category = "Convert",
    description = "Concatenates every connected string.",
    keywords = "concat, merge",
    output = String
)]
pub struct Join {
    #[param(hint = "separator", hide_label)]
    separator: String,
    #[input(description = "Any number of strings, in connection order.")]
    parts: Multi<String>,
}

// -------------------------------------------------------------------- build

#[derive(Debug, NodeType)]
#[node(
    category = "Build",
    description = "repository:tag",
    keywords = "container, docker, registry",
    output = ImageRef
)]
pub struct Image {
    #[input(default = "nginx")]
    repository: String,
    #[input(default = "latest")]
    tag: String,
}

#[derive(Debug, NodeType)]
#[node(
    id = "env_var",
    label = "Environment Variable",
    category = "Build",
    description = "One KEY=value pair.",
    keywords = "env, variable",
    output = EnvList
)]
pub struct EnvVar {
    #[input(default = "KEY")]
    key: String,
    #[input]
    value: String,
}

#[derive(Debug, NodeType)]
#[node(
    id = "env_file",
    label = "Environment File",
    category = "Build",
    description = "Loads variables from a file at start-up.",
    keywords = "env, dotenv, file",
    output = EnvFileRef
)]
pub struct EnvFile {
    #[input(default = ".env")]
    path: String,
}

#[derive(Debug, NodeType)]
#[node(
    id = "port",
    label = "Port Mapping",
    category = "Build",
    description = "Publishes a container port on the host.",
    keywords = "expose, publish, tcp",
    output = PortMap
)]
pub struct Port {
    #[param(default = Protocol::Tcp)]
    protocol: Protocol,
    #[input(default = 8080, min = 1, max = 65535)]
    host: i64,
    #[input(default = 80, min = 1, max = 65535)]
    container: i64,
}

#[derive(Debug, NodeType)]
#[node(
    id = "volume",
    category = "Build",
    description = "A bind mount or named volume.",
    keywords = "mount, bind, storage",
    output = Mount
)]
pub struct Volume {
    #[input(default = "./data")]
    source: String,
    #[input(default = "/var/lib/data")]
    target: String,
    #[input(label = "Read Only")]
    read_only: bool,
}

#[derive(Debug, NodeType)]
#[node(
    id = "network",
    category = "Build",
    description = "A network the stack declares and services join.",
    keywords = "bridge, overlay",
    output = NetworkDef
)]
pub struct Network {
    #[param(default = Driver::Bridge)]
    driver: Driver,
    #[input(default = "frontend")]
    name: String,
}

#[derive(Debug, NodeType)]
#[node(
    id = "healthcheck",
    label = "Health Check",
    category = "Build",
    description = "Probe run inside the container to decide if it is healthy.",
    keywords = "probe, liveness",
    output = Health
)]
pub struct HealthCheck {
    #[input(default = "curl -f localhost/")]
    command: String,
    #[input(label = "Interval (s)", default = 30, min = 1, max = 3600)]
    interval: i64,
    #[input(default = 3, min = 1, max = 20)]
    retries: i64,
}

// ------------------------------------------------------------------ runtime

#[derive(Debug, NodeType)]
#[node(
    id = "service",
    category = "Runtime",
    description = "One service in the stack. Everything it needs plugs in here.",
    keywords = "container, deployment",
    output = ServiceDef
)]
pub struct Service {
    #[param(default = "web", hint = "service name", hide_label)]
    name: String,
    #[param(default = Restart::UnlessStopped)]
    restart: Restart,

    #[input(description = "Required.")]
    image: ImageRef,
    #[input(hint = "(default entrypoint)")]
    command: String,
    #[input(default = 1, min = 1, max = 64)]
    replicas: i64,
    #[input]
    environment: Multi<EnvList>,
    #[input(label = "Env Files")]
    env_file: Multi<EnvFileRef>,
    #[input]
    ports: Multi<PortMap>,
    #[input]
    volumes: Multi<Mount>,
    #[input]
    networks: Multi<NetworkDef>,
    #[input(label = "Depends On")]
    depends_on: Multi<ServiceDef>,
    #[input]
    healthcheck: Option<Health>,
}

// ------------------------------------------------------------------- output

#[derive(Debug, NodeType)]
#[node(
    id = "stack",
    label = "Stack Output",
    category = "Output",
    description = "The root of the document. Everything reachable from here is emitted.",
    keywords = "compose, output, root, file",
    produces = StackDoc
)]
pub struct Stack {
    #[param(default = "my-stack", hint = "stack name", hide_label)]
    name: String,
    #[param(default = Version::V3_9)]
    version: Version,

    #[input]
    services: Multi<ServiceDef>,
    #[input]
    networks: Multi<NetworkDef>,
}

/// Every node kind, so the library registers in one call.
pub type Nodes = (
    Text,
    Number,
    Flag,
    Secret,
    Join,
    Image,
    EnvVar,
    EnvFile,
    Port,
    Volume,
    Network,
    HealthCheck,
    Service,
    Stack,
);

// ------------------------------------------------------- the config fold

/// Folding the graph into a config document.
pub struct Config;
impl Fold for Config {}

impl Evaluate<Config> for Text {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(self.value.clone())
    }
}

impl Evaluate<Config> for Number {
    fn evaluate(&self) -> Result<i64, NodeError> {
        Ok(self.value)
    }
}

impl Evaluate<Config> for Flag {
    fn evaluate(&self) -> Result<bool, NodeError> {
        Ok(self.value)
    }
}

impl Evaluate<Config> for Secret {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(format!("${{{}}}", self.name))
    }
}

impl Evaluate<Config> for Join {
    fn evaluate(&self) -> Result<String, NodeError> {
        Ok(self.parts.iter().cloned().collect::<Vec<_>>().join(&self.separator))
    }
}

impl Evaluate<Config> for Image {
    fn evaluate(&self) -> Result<ImageRef, NodeError> {
        if self.repository.is_empty() {
            return Err(NodeError::custom("Image needs a repository."));
        }
        Ok(ImageRef(if self.tag.is_empty() {
            self.repository.clone()
        } else {
            format!("{}:{}", self.repository, self.tag)
        }))
    }
}

impl Evaluate<Config> for EnvVar {
    fn evaluate(&self) -> Result<EnvList, NodeError> {
        if self.key.is_empty() {
            return Err(NodeError::custom("Environment Variable needs a key."));
        }
        Ok(EnvList(vec![format!("{}={}", self.key, self.value)]))
    }
}

impl Evaluate<Config> for EnvFile {
    fn evaluate(&self) -> Result<EnvFileRef, NodeError> {
        if self.path.is_empty() {
            return Err(NodeError::custom("Environment File needs a path."));
        }
        Ok(EnvFileRef(self.path.clone()))
    }
}

impl Evaluate<Config> for Port {
    fn evaluate(&self) -> Result<PortMap, NodeError> {
        let (host, container) = (self.host, self.container);
        Ok(PortMap(match self.protocol {
            Protocol::Tcp => format!("{host}:{container}"),
            Protocol::Udp => format!("{host}:{container}/udp"),
        }))
    }
}

impl Evaluate<Config> for Volume {
    fn evaluate(&self) -> Result<Mount, NodeError> {
        if self.target.is_empty() {
            return Err(NodeError::custom("Volume needs a target path."));
        }
        Ok(Mount(
            Value::map()
                .set("type", "bind")
                .set("source", self.source.clone())
                .set("target", self.target.clone())
                .set_if(self.read_only, "read_only", true)
                .into(),
        ))
    }
}

impl Evaluate<Config> for Network {
    fn evaluate(&self) -> Result<NetworkDef, NodeError> {
        if self.name.is_empty() {
            return Err(NodeError::custom("Network needs a name."));
        }
        Ok(NetworkDef {
            name: self.name.clone(),
            driver: self.driver.to_value().as_str().unwrap_or("bridge").to_owned(),
        })
    }
}

impl Evaluate<Config> for HealthCheck {
    fn evaluate(&self) -> Result<Health, NodeError> {
        if self.command.is_empty() {
            return Err(NodeError::custom("Health Check needs a command."));
        }
        Ok(Health(
            Value::map()
                .set_list("test", ["CMD-SHELL".to_owned(), self.command.clone()])
                .set("interval", format!("{}s", self.interval))
                .set("retries", self.retries)
                .into(),
        ))
    }
}

impl Evaluate<Config> for Service {
    fn evaluate(&self) -> Result<ServiceDef, NodeError> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(NodeError::custom("Service needs a name."));
        }

        let body = Value::map()
            .set("image", self.image.0.clone())
            .set_if(!self.command.is_empty(), "command", self.command.clone())
            .set_if(
                self.restart != Restart::No,
                "restart",
                self.restart.to_value(),
            )
            .set_if(
                self.replicas > 1,
                "deploy",
                Value::map().set("replicas", self.replicas),
            )
            .set_list("ports", self.ports.iter().map(|port| port.0.clone()))
            // `Multi<EnvList>` is a fan-in of lists: flatten across links.
            .set_list(
                "environment",
                self.environment.iter().flat_map(|list| list.0.iter().cloned()),
            )
            .set_list("env_file", self.env_file.iter().map(|file| file.0.clone()))
            .set_list("volumes", self.volumes.iter().map(|mount| mount.0.clone()))
            .set_list(
                "networks",
                self.networks.iter().map(|network| network.name.clone()),
            )
            .set_list(
                "depends_on",
                self.depends_on.iter().map(|service| service.name.clone()),
            )
            .set_some("healthcheck", self.healthcheck.as_ref().map(|h| h.0.clone()));

        Ok(ServiceDef {
            name: name.to_owned(),
            body: body.into(),
        })
    }
}

impl Evaluate<Config> for Stack {
    fn evaluate(&self) -> Result<StackDoc, NodeError> {
        let mut services = nodez::MapBuilder::new();
        for service in self.services.iter() {
            services = services.set(service.name.clone(), service.body.clone());
        }

        let mut networks: Vec<(String, Value)> = self
            .networks
            .iter()
            .map(|network| {
                (
                    network.name.clone(),
                    Value::map().set("driver", network.driver.clone()).into(),
                )
            })
            .collect();

        // A service that names a network implies the stack declares it, even if
        // the network node is not wired to the stack directly.
        for service in self.services.iter() {
            let Some(used) = service.body.get("networks").and_then(Value::as_list) else {
                continue;
            };
            for entry in used {
                let Some(name) = entry.as_str() else { continue };
                if !networks.iter().any(|(declared, _)| declared == name) {
                    networks.push((
                        name.to_owned(),
                        Value::map().set("driver", "bridge").into(),
                    ));
                }
            }
        }

        let name = self.name.trim();
        Ok(StackDoc(
            Value::map()
                .set("version", self.version.to_value())
                .set_if(!name.is_empty(), "name", name)
                .set("services", services)
                .set_map("networks", networks.into_iter().collect())
                .into(),
        ))
    }
}
