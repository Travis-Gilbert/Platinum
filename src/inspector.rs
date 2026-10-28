//! The inspection prototype: a log, a document tree, a request list, and the
//! page's WebMCP tools, for the active tab.
//!
//! It is a CDP client of the server in the Servo fork (`127.0.0.1:9333`).
//! Nothing here launches Chrome or loads the Chrome DevTools frontend.
//! `Model::apply` is the state the panel shows; the socket only delivers
//! JSON. The methods this client sends are listed in `devtools/PIN`.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::thread;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::{self as futures_mpsc, UnboundedSender};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _, h_flex, scroll::ScrollableElement as _, v_flex,
};
use gpui_kit::{
    AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
    prelude::FluentBuilder as _, px,
};
use serde_json::{Value, json};

const ADDRESS: &str = "127.0.0.1:9333";
const MAX_LINES: usize = 200;
const MAX_REQUESTS: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    Console,
    Elements,
    Network,
    Tools,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Level {
    Log,
    Error,
    Result,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Line {
    level: Level,
    text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NodeRow {
    id: i64,
    parent: i64,
    name: String,
    value: String,
    depth: usize,
    children: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RequestRow {
    id: String,
    method: String,
    url: String,
    status: Option<u16>,
    mime: String,
    failed: bool,
    body: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ToolRow {
    name: String,
    description: String,
    schema: String,
    frame: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Model {
    section: Section,
    lines: Vec<Line>,
    nodes: Vec<NodeRow>,
    requests: Vec<RequestRow>,
    tools: Vec<ToolRow>,
    styles: Vec<(String, String)>,
    selected_node: Option<i64>,
    /// The request whose body the next `Network.getResponseBody` fills.
    pending_body: Option<String>,
    context_id: Option<i64>,
    target: String,
    status: String,
    next_id: i64,
}

impl Model {
    fn new() -> Self {
        Self {
            section: Section::Console,
            lines: Vec::new(),
            nodes: Vec::new(),
            requests: Vec::new(),
            tools: Vec::new(),
            styles: Vec::new(),
            selected_node: None,
            pending_body: None,
            context_id: None,
            target: String::new(),
            status: "Not connected".to_owned(),
            next_id: 1,
        }
    }

    fn reset_for_target(&mut self, target: &str) {
        *self = Self::new();
        self.target = target.to_owned();
        self.status = "Connecting".to_owned();
    }

    fn alloc_id(&mut self) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn push(&mut self, level: Level, text: impl Into<String>) {
        self.lines.push(Line {
            level,
            text: text.into(),
        });
        if self.lines.len() > MAX_LINES {
            let extra = self.lines.len() - MAX_LINES;
            self.lines.drain(0..extra);
        }
    }
}

/// What the panel should ask the page for after applying one message.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct FollowUp {
    pub expand: Vec<i64>,
}

/// Fold one CDP message into the panel. A response is recognized by its
/// result shape, so the socket thread can send `enable` without the panel
/// remembering those ids.
pub(crate) fn apply(model: &mut Model, message: &Value) -> FollowUp {
    let mut follow = FollowUp { expand: Vec::new() };
    if let Some(method) = message.get("method").and_then(Value::as_str) {
        apply_event(model, method, message.get("params").unwrap_or(&Value::Null));
        return follow;
    }
    if let Some(error) = message.get("error") {
        let text = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("error");
        model.push(Level::Error, text);
        return follow;
    }
    let Some(result) = message.get("result") else {
        return follow;
    };
    if let Some(details) = result.get("exceptionDetails") {
        let text = details
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("exception");
        model.push(Level::Error, text);
        return follow;
    }
    if result.get("result").is_some() {
        model.push(Level::Result, describe_remote(&result["result"]));
        return follow;
    }
    if let Some(root) = result.get("root").filter(|root| !root.is_null()) {
        if let Some(id) = adopt_root(model, root) {
            follow.expand.push(id);
        }
        return follow;
    }
    if let Some(styles) = result.get("computedStyle").and_then(Value::as_array) {
        model.styles = styles
            .iter()
            .filter_map(|style| {
                Some((
                    style.get("name")?.as_str()?.to_owned(),
                    style.get("value")?.as_str()?.to_owned(),
                ))
            })
            .collect();
    }
    if let Some(body) = result.get("body").and_then(Value::as_str) {
        if let Some(id) = model.pending_body.take() {
            if let Some(request) = model.requests.iter_mut().find(|row| row.id == id) {
                request.body = body.to_owned();
            }
        }
    }
    follow
}

fn apply_event(model: &mut Model, method: &str, params: &Value) {
    match method {
        "Log.entryAdded" => {
            let entry = &params["entry"];
            let text = entry.get("text").and_then(Value::as_str).unwrap_or("");
            if text == "console.clear" {
                model.lines.clear();
                return;
            }
            let level = match entry.get("level").and_then(Value::as_str).unwrap_or("info") {
                "error" | "warning" => Level::Error,
                _ => Level::Log,
            };
            model.push(level, text);
        }
        "Runtime.executionContextCreated" => {
            model.context_id = params["context"]["id"].as_i64();
        }
        "Runtime.executionContextDestroyed" => {
            model.context_id = None;
            model.nodes.clear();
            model.styles.clear();
            model.tools.clear();
            model.selected_node = None;
        }
        "DOM.setChildNodes" => {
            let parent = params.get("parentId").and_then(Value::as_i64).unwrap_or(0);
            let nodes = params
                .get("nodes")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            set_children(model, parent, &nodes);
        }
        "Network.requestWillBeSent" => {
            let id = params
                .get("requestId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            if model.requests.iter().any(|row| row.id == id) {
                return;
            }
            let request = &params["request"];
            model.requests.push(RequestRow {
                id,
                method: request
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                url: request
                    .get("url")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                status: None,
                mime: String::new(),
                failed: false,
                body: String::new(),
            });
            if model.requests.len() > MAX_REQUESTS {
                let extra = model.requests.len() - MAX_REQUESTS;
                model.requests.drain(0..extra);
            }
        }
        "Network.responseReceived" => {
            let id = params
                .get("requestId")
                .and_then(Value::as_str)
                .unwrap_or("");
            let response = &params["response"];
            if let Some(row) = model.requests.iter_mut().find(|row| row.id == id) {
                row.status = response
                    .get("status")
                    .and_then(Value::as_u64)
                    .map(|code| code as u16);
                row.mime = response
                    .get("mimeType")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
            }
        }
        "Network.loadingFailed" => {
            let id = params
                .get("requestId")
                .and_then(Value::as_str)
                .unwrap_or("");
            if let Some(row) = model.requests.iter_mut().find(|row| row.id == id) {
                row.failed = true;
            } else {
                model.requests.push(RequestRow {
                    id: id.to_owned(),
                    method: String::new(),
                    url: params
                        .get("errorText")
                        .and_then(Value::as_str)
                        .unwrap_or("failed")
                        .to_owned(),
                    status: None,
                    mime: String::new(),
                    failed: true,
                    body: String::new(),
                });
            }
        }
        "WebMCP.toolsAdded" => {
            let tools = params
                .get("tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for tool in tools {
                let name = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                model.tools.retain(|row| row.name != name);
                model.tools.push(ToolRow {
                    name,
                    description: tool
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                    schema: tool
                        .get("inputSchema")
                        .map(|schema| schema.to_string())
                        .unwrap_or_default(),
                    frame: tool
                        .get("frameId")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned(),
                });
            }
        }
        "WebMCP.toolsRemoved" => {
            let names = params
                .get("names")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            model.tools.retain(|tool| {
                !names
                    .iter()
                    .any(|name| name.as_str() == Some(tool.name.as_str()))
            });
        }
        "WebMCP.toolResponded" => {
            let name = params
                .get("invocationId")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let text = if params.get("success").and_then(Value::as_bool) == Some(true) {
                format!(
                    "{name} → {}",
                    params.get("result").and_then(Value::as_str).unwrap_or("")
                )
            } else {
                format!(
                    "{name} failed: {}",
                    params
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("error")
                )
            };
            let level = if text.contains("failed") {
                Level::Error
            } else {
                Level::Result
            };
            model.push(level, text);
        }
        _ => {}
    }
}

fn adopt_root(model: &mut Model, root: &Value) -> Option<i64> {
    let id = root.get("nodeId").and_then(Value::as_i64)?;
    model.nodes.clear();
    model.styles.clear();
    model.selected_node = Some(id);
    model.nodes.push(NodeRow {
        id,
        parent: 0,
        name: root
            .get("nodeName")
            .and_then(Value::as_str)
            .unwrap_or("#document")
            .to_owned(),
        value: String::new(),
        depth: 0,
        children: root
            .get("childNodeCount")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize,
    });
    Some(id)
}

fn set_children(model: &mut Model, parent: i64, nodes: &[Value]) {
    let depth = model
        .nodes
        .iter()
        .find(|node| node.id == parent)
        .map(|node| node.depth + 1)
        .unwrap_or(0);
    drop_subtree(&mut model.nodes, parent);
    let rows: Vec<NodeRow> = nodes
        .iter()
        .filter_map(|node| {
            Some(NodeRow {
                id: node.get("nodeId")?.as_i64()?,
                parent,
                name: node
                    .get("nodeName")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                value: node
                    .get("nodeValue")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                depth,
                children: node
                    .get("childNodeCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize,
            })
        })
        .collect();
    let at = model
        .nodes
        .iter()
        .position(|node| node.id == parent)
        .map(|index| index + 1)
        .unwrap_or(model.nodes.len());
    model.nodes.splice(at..at, rows);
}

fn drop_subtree(nodes: &mut Vec<NodeRow>, parent: i64) {
    let children: Vec<i64> = nodes
        .iter()
        .filter(|node| node.parent == parent)
        .map(|node| node.id)
        .collect();
    nodes.retain(|node| node.parent != parent);
    for child in children {
        drop_subtree(nodes, child);
    }
}

fn describe_remote(value: &Value) -> String {
    if let Some(text) = value.get("value") {
        if let Some(text) = text.as_str() {
            return text.to_owned();
        }
        if !text.is_null() {
            return text.to_string();
        }
    }
    value
        .get("description")
        .and_then(Value::as_str)
        .or_else(|| value.get("type").and_then(Value::as_str))
        .unwrap_or("undefined")
        .to_owned()
}

enum Command {
    Attach(String),
    Send(Value),
}

enum Wire {
    Status(String),
    Message(Value),
}

pub(crate) struct Inspector {
    model: Model,
    expression: gpui_kit::Entity<InputState>,
    commands: mpsc::Sender<Command>,
    _events: Task<()>,
    _input: Subscription,
}

impl Inspector {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let expression = cx.new(|cx| InputState::new(window, cx).placeholder("Evaluate"));
        let input = cx.subscribe_in(&expression, window, Self::on_input);
        let (commands_tx, commands_rx) = mpsc::channel();
        let (events_tx, mut events_rx) = futures_mpsc::unbounded();
        thread::Builder::new()
            .name("cdp-client".to_owned())
            .spawn(move || drive(commands_rx, events_tx))
            .expect("cdp client");
        let events = cx.spawn(async move |inspector, cx| {
            while let Some(event) = events_rx.next().await {
                if inspector
                    .update(cx, |inspector, cx| inspector.on_wire(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            model: Model::new(),
            expression,
            commands: commands_tx,
            _events: events,
            _input: input,
        }
    }

    /// Follow the active tab. The same webview id survives navigation.
    pub(crate) fn attach(&mut self, target: Option<String>, cx: &mut Context<Self>) {
        let Some(target) = target.filter(|target| !target.is_empty()) else {
            self.model.status = "No page".to_owned();
            cx.notify();
            return;
        };
        if self.model.target == target {
            return;
        }
        self.model.reset_for_target(&target);
        let _ = self.commands.send(Command::Attach(target));
        cx.notify();
    }

    fn on_wire(&mut self, event: Wire, cx: &mut Context<Self>) {
        match event {
            Wire::Status(status) => self.model.status = status,
            Wire::Message(message) => {
                let follow = apply(&mut self.model, &message);
                for node in follow.expand {
                    self.command("DOM.requestChildNodes", json!({ "nodeId": node }));
                }
            }
        }
        cx.notify();
    }

    fn on_input(
        &mut self,
        _: &gpui_kit::Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::PressEnter { .. }) {
            return;
        }
        let text = self.expression.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        self.expression.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        match self.model.section {
            Section::Console => self.evaluate(&text),
            Section::Elements => self.edit_attribute(&text),
            Section::Tools => self.invoke_tool(&text),
            Section::Network => {}
        }
        cx.notify();
    }

    fn evaluate(&mut self, expression: &str) {
        let mut params = json!({ "expression": expression });
        if let Some(context) = self.model.context_id {
            params["contextId"] = json!(context);
        }
        self.command("Runtime.evaluate", params);
    }

    fn edit_attribute(&mut self, text: &str) {
        let Some(node) = self.model.selected_node else {
            self.model.push(Level::Error, "Select a node first");
            return;
        };
        let Some((name, value)) = text.split_once('=') else {
            self.model.push(Level::Error, "Use name=value");
            return;
        };
        self.command(
            "DOM.setAttributeValue",
            json!({ "nodeId": node, "name": name.trim(), "value": value }),
        );
    }

    fn invoke_tool(&mut self, text: &str) {
        let (name, arguments) = text
            .split_once(' ')
            .map(|(name, rest)| (name, rest.to_owned()))
            .unwrap_or((text, "{}".to_owned()));
        self.command(
            "WebMCP.invokeTool",
            json!({ "name": name, "arguments": arguments }),
        );
    }

    fn command(&mut self, method: &str, params: Value) {
        let id = self.model.alloc_id();
        let _ = self.commands.send(Command::Send(json!({
            "id": id,
            "method": method,
            "params": params,
        })));
    }

    fn select_node(&mut self, id: i64, cx: &mut Context<Self>) {
        self.model.selected_node = Some(id);
        self.model.styles.clear();
        self.command("CSS.getComputedStyleForNode", json!({ "nodeId": id }));
        self.command("DOM.requestChildNodes", json!({ "nodeId": id }));
        self.command("Overlay.highlightNode", json!({ "nodeId": id }));
        cx.notify();
    }

    fn load_body(&mut self, id: &str) {
        self.model.pending_body = Some(id.to_owned());
        self.command("Network.getResponseBody", json!({ "requestId": id }));
    }
}

impl Render for Inspector {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, background, foreground, muted) = (
            theme.border,
            theme.background,
            theme.foreground,
            theme.muted_foreground,
        );
        let section = self.model.section;
        let status = SharedString::from(self.model.status.clone());
        let hint = match section {
            Section::Console => "Evaluate",
            Section::Elements => "Attribute as name=value",
            Section::Network => "Requests",
            Section::Tools => "Tool name, then JSON arguments",
        };
        v_flex()
            .h(px(240.))
            .w_full()
            .border_t_1()
            .border_color(border)
            .bg(background)
            .text_color(foreground)
            .child(
                h_flex()
                    .px_2()
                    .gap_2()
                    .h_7()
                    .items_center()
                    .children({
                        let console = section_tab(cx, Section::Console, "Console", section);
                        let elements = section_tab(cx, Section::Elements, "Elements", section);
                        let network = section_tab(cx, Section::Network, "Network", section);
                        let tools = section_tab(cx, Section::Tools, "Tools", section);
                        [console, elements, network, tools]
                    })
                    .child(div().flex_1().text_xs().text_color(muted).child(status)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .px_2()
                    .py_1()
                    .child(self.body(cx)),
            )
            .child(
                h_flex()
                    .px_2()
                    .gap_2()
                    .h_8()
                    .items_center()
                    .border_t_1()
                    .border_color(border)
                    .child(div().text_xs().text_color(muted).child(hint))
                    .child(div().flex_1().child(Input::new(&self.expression).small())),
            )
    }
}

fn section_tab(
    cx: &mut Context<Inspector>,
    section: Section,
    label: &'static str,
    current: Section,
) -> impl IntoElement + use<> {
    let selected = section == current;
    let theme = cx.theme();
    div()
        .id(label)
        .debug_selector(|| label.into())
        .text_xs()
        .px_1()
        .cursor_pointer()
        .text_color(if selected {
            theme.foreground
        } else {
            theme.muted_foreground
        })
        .on_click(cx.listener(move |inspector, _, _, cx| {
            inspector.model.section = section;
            cx.notify();
        }))
        .child(label)
}

impl Inspector {
    fn body(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        match self.model.section {
            Section::Console => v_flex()
                .gap_1()
                .children(self.model.lines.iter().map(|line| {
                    let color = match line.level {
                        Level::Error => cx.theme().danger,
                        Level::Result => cx.theme().foreground,
                        Level::Log => cx.theme().muted_foreground,
                    };
                    div()
                        .text_xs()
                        .text_color(color)
                        .child(line.text.clone())
                        .into_any_element()
                }))
                .into_any_element(),
            Section::Elements => {
                let selected = self.model.selected_node;
                v_flex()
                    .gap_1()
                    .children(self.model.nodes.iter().map(|node| {
                        let id = node.id;
                        let mark = if selected == Some(id) { "▸ " } else { "  " };
                        div()
                            .id(("node", id as u64))
                            .text_xs()
                            .cursor_pointer()
                            .pl(px(12. * node.depth as f32))
                            .on_click(cx.listener(move |inspector, _, _, cx| {
                                inspector.select_node(id, cx);
                            }))
                            .child(format!(
                                "{mark}{} {}",
                                node.name,
                                node.value.chars().take(40).collect::<String>()
                            ))
                            .into_any_element()
                    }))
                    .children(self.model.styles.iter().take(24).map(|(name, value)| {
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{name}: {value}"))
                            .into_any_element()
                    }))
                    .into_any_element()
            }
            Section::Network => v_flex()
                .gap_1()
                .children(
                    self.model
                        .requests
                        .iter()
                        .enumerate()
                        .map(|(index, request)| {
                            let id = request.id.clone();
                            let status = if request.failed {
                                "failed".to_owned()
                            } else {
                                request
                                    .status
                                    .map(|code| code.to_string())
                                    .unwrap_or_else(|| "…".to_owned())
                            };
                            div()
                                .id(("request", index as u64))
                                .text_xs()
                                .cursor_pointer()
                                .on_click(cx.listener(move |inspector, _, _, cx| {
                                    inspector.load_body(&id);
                                    cx.notify();
                                }))
                                .child(format!(
                                    "{status} {} {} {}",
                                    request.method, request.mime, request.url
                                ))
                                .when(!request.body.is_empty(), |row| {
                                    row.child(format!(
                                        "  {}",
                                        request.body.chars().take(80).collect::<String>()
                                    ))
                                })
                                .into_any_element()
                        }),
                )
                .into_any_element(),
            Section::Tools => v_flex()
                .gap_1()
                .children(self.model.tools.iter().map(|tool| {
                    div()
                        .text_xs()
                        .child(format!("{} — {}", tool.name, tool.description))
                        .into_any_element()
                }))
                .into_any_element(),
        }
    }
}

fn drive(commands: Receiver<Command>, events: UnboundedSender<Wire>) {
    let mut target = String::new();
    let mut socket: Option<FrameSocket> = None;
    loop {
        let incoming = if socket.is_some() {
            match commands.try_recv() {
                Ok(command) => Some(command),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => break,
            }
        } else {
            match commands.recv_timeout(Duration::from_millis(400)) {
                Ok(command) => Some(command),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        };
        if let Some(command) = incoming {
            match command {
                Command::Attach(id) if id != target => {
                    target = id;
                    socket = None;
                }
                Command::Attach(_) => {}
                Command::Send(message) => {
                    if let Some(sock) = socket.as_mut() {
                        let _ = sock.write_text(&message.to_string());
                    }
                }
            }
        }
        if target.is_empty() {
            continue;
        }
        if socket.is_none() {
            match connect(&target) {
                Ok(stream) => {
                    let _ = events.unbounded_send(Wire::Status("Connected".to_owned()));
                    let mut stream = FrameSocket::new(stream);
                    greet(&mut stream);
                    socket = Some(stream);
                }
                Err(error) => {
                    let _ = events.unbounded_send(Wire::Status(error));
                }
            }
        }
        let mut pending = true;
        if let Some(sock) = socket.as_mut() {
            match sock.drain() {
                Ok(messages) => {
                    pending = messages.is_empty();
                    for text in messages {
                        if let Ok(message) = serde_json::from_str::<Value>(&text) {
                            let _ = events.unbounded_send(Wire::Message(message));
                        }
                    }
                }
                Err(_) => {
                    socket = None;
                    let _ = events.unbounded_send(Wire::Status("Reconnecting".to_owned()));
                    pending = false;
                }
            }
        }
        if pending {
            thread::sleep(Duration::from_millis(30));
        }
    }
}

fn greet(socket: &mut FrameSocket) {
    for (id, method) in [
        (1_000_001, "Runtime.enable"),
        (1_000_002, "Log.enable"),
        (1_000_003, "Page.enable"),
        (1_000_004, "DOM.enable"),
        (1_000_005, "CSS.enable"),
        (1_000_006, "Overlay.enable"),
        (1_000_007, "Network.enable"),
        (1_000_008, "WebMCP.enable"),
    ] {
        let _ = socket.write_text(&json!({ "id": id, "method": method }).to_string());
    }
    let _ =
        socket.write_text(&json!({ "id": 1_000_009, "method": "Page.getFrameTree" }).to_string());
    let _ = socket.write_text(&json!({ "id": 1_000_010, "method": "DOM.getDocument" }).to_string());
}

fn connect(target: &str) -> Result<TcpStream, String> {
    let body = http_get("/json/list")?;
    let list: Value = serde_json::from_str(&body).map_err(|error| error.to_string())?;
    let path = list
        .as_array()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry["id"].as_str() == Some(target))
        })
        .and_then(|entry| entry["webSocketDebuggerUrl"].as_str())
        .and_then(|url| url.find("/devtools/").map(|index| url[index..].to_owned()))
        .ok_or_else(|| "Waiting for the page".to_owned())?;
    let mut stream = TcpStream::connect(ADDRESS).map_err(|error| error.to_string())?;
    let key = "dGhlIHNhbXBsZSBub25jZQ==";
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {ADDRESS}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
    .map_err(|error| error.to_string())?;
    let (header, _) = read_http(&mut stream).map_err(|error| error.to_string())?;
    if !header.starts_with("HTTP/1.1 101") {
        return Err("CDP did not upgrade".to_owned());
    }
    stream
        .set_nonblocking(true)
        .map_err(|error| error.to_string())?;
    Ok(stream)
}

fn http_get(path: &str) -> Result<String, String> {
    let mut stream = TcpStream::connect(ADDRESS).map_err(|error| error.to_string())?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {ADDRESS}\r\nConnection: close\r\n\r\n"
    )
    .map_err(|error| error.to_string())?;
    let (_, body) = read_http(&mut stream).map_err(|error| error.to_string())?;
    Ok(body)
}

fn read_http(stream: &mut TcpStream) -> io::Result<(String, String)> {
    let mut buffer = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if stream.read(&mut byte)? == 0 {
            break;
        }
        buffer.push(byte[0]);
        if buffer.ends_with(b"\r\n\r\n") || buffer.len() > 64 * 1024 {
            break;
        }
    }
    let split = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .unwrap_or(buffer.len());
    let header = String::from_utf8_lossy(&buffer[..split]).into_owned();
    let mut body = buffer[split..].to_vec();
    let length = header
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    while body.len() < length {
        let mut chunk = [0u8; 1024];
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Ok((header, String::from_utf8_lossy(&body).into_owned()))
}

struct FrameSocket {
    stream: TcpStream,
    incoming: Vec<u8>,
    mask: u8,
}

impl FrameSocket {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            incoming: Vec::new(),
            mask: 1,
        }
    }

    fn write_text(&mut self, text: &str) -> io::Result<()> {
        let payload = text.as_bytes();
        let mask = [self.mask, self.mask.wrapping_add(2), 3, 4];
        self.mask = self.mask.wrapping_add(1);
        let mut frame = Vec::with_capacity(payload.len() + 8);
        frame.push(0x81);
        let length = payload.len();
        if length < 126 {
            frame.push(0x80 | length as u8);
        } else if length <= u16::MAX as usize {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(length as u16).to_be_bytes());
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(length as u64).to_be_bytes());
        }
        frame.extend_from_slice(&mask);
        for (index, byte) in payload.iter().enumerate() {
            frame.push(byte ^ mask[index % 4]);
        }
        self.stream.write_all(&frame)?;
        self.stream.flush()
    }

    fn drain(&mut self) -> io::Result<Vec<String>> {
        let mut buf = [0u8; 4096];
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "closed")),
                Ok(n) => self.incoming.extend_from_slice(&buf[..n]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        let mut messages = Vec::new();
        while let Some(text) = take_text(&mut self.incoming, &mut self.stream)? {
            messages.push(text);
        }
        Ok(messages)
    }
}

fn take_text(buffer: &mut Vec<u8>, stream: &mut impl Write) -> io::Result<Option<String>> {
    if buffer.len() < 2 {
        return Ok(None);
    }
    let opcode = buffer[0] & 0x0F;
    let masked = buffer[1] & 0x80 != 0;
    let mut length = (buffer[1] & 0x7F) as usize;
    let mut cursor = 2;
    if length == 126 {
        if buffer.len() < cursor + 2 {
            return Ok(None);
        }
        length = u16::from_be_bytes([buffer[cursor], buffer[cursor + 1]]) as usize;
        cursor += 2;
    } else if length == 127 {
        if buffer.len() < cursor + 8 {
            return Ok(None);
        }
        let mut wide = [0u8; 8];
        wide.copy_from_slice(&buffer[cursor..cursor + 8]);
        length = u64::from_be_bytes(wide) as usize;
        cursor += 8;
    }
    let mask = if masked {
        if buffer.len() < cursor + 4 {
            return Ok(None);
        }
        let mask = [
            buffer[cursor],
            buffer[cursor + 1],
            buffer[cursor + 2],
            buffer[cursor + 3],
        ];
        cursor += 4;
        Some(mask)
    } else {
        None
    };
    if buffer.len() < cursor + length {
        return Ok(None);
    }
    let mut payload = buffer[cursor..cursor + length].to_vec();
    buffer.drain(..cursor + length);
    if let Some(mask) = mask {
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[index % 4];
        }
    }
    if opcode == 0x9 {
        let mut pong = vec![0x8A, 0x80 | payload.len() as u8, 1, 2, 3, 4];
        for (index, byte) in payload.iter().enumerate() {
            pong.push(byte ^ [1, 2, 3, 4][index % 4]);
        }
        stream.write_all(&pong)?;
        return Ok(None);
    }
    if opcode == 0x8 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "close"));
    }
    if opcode != 0x1 && opcode != 0x0 {
        return Ok(None);
    }
    let text = String::from_utf8(payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Ok(Some(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Model {
        Model::new()
    }

    #[test]
    fn console_survives_navigation_and_ignores_the_runtime_duplicate() {
        let mut model = model();
        apply(
            &mut model,
            &json!({
                "method": "Log.entryAdded",
                "params": { "entry": { "level": "info", "text": "hello" } }
            }),
        );
        apply(
            &mut model,
            &json!({
                "method": "Runtime.consoleAPICalled",
                "params": { "type": "log", "args": [{ "value": "hello" }] }
            }),
        );
        apply(
            &mut model,
            &json!({
                "method": "Runtime.executionContextDestroyed",
                "params": { "executionContextId": 1 }
            }),
        );
        assert_eq!(model.lines.len(), 1);
        assert_eq!(model.lines[0].text, "hello");
        assert!(model.context_id.is_none());
    }

    #[test]
    fn evaluate_exception_is_an_error_line() {
        let mut model = model();
        apply(
            &mut model,
            &json!({
                "id": 1,
                "result": {
                    "result": { "type": "object" },
                    "exceptionDetails": { "text": "boom" }
                }
            }),
        );
        assert_eq!(model.lines[0].level, Level::Error);
        assert_eq!(model.lines[0].text, "boom");
    }

    #[test]
    fn evaluate_value_is_a_result_line() {
        let mut model = model();
        apply(
            &mut model,
            &json!({ "id": 2, "result": { "result": { "type": "number", "value": 2 } } }),
        );
        assert_eq!(model.lines[0].text, "2");
    }

    #[test]
    fn document_tree_and_children() {
        let mut model = model();
        let follow = apply(
            &mut model,
            &json!({
                "id": 3,
                "result": { "root": { "nodeId": 1, "nodeName": "#document", "childNodeCount": 1 } }
            }),
        );
        assert_eq!(follow.expand, vec![1]);
        apply(
            &mut model,
            &json!({
                "method": "DOM.setChildNodes",
                "params": {
                    "parentId": 1,
                    "nodes": [{ "nodeId": 2, "nodeName": "HTML", "childNodeCount": 2 }]
                }
            }),
        );
        assert_eq!(model.nodes.len(), 2);
        assert_eq!(model.nodes[1].name, "HTML");
        assert_eq!(model.nodes[1].parent, 1);
    }

    #[test]
    fn navigation_drops_the_tree_and_keeps_the_log() {
        let mut model = model();
        apply(
            &mut model,
            &json!({
                "method": "Log.entryAdded",
                "params": { "entry": { "level": "info", "text": "before" } }
            }),
        );
        apply(
            &mut model,
            &json!({ "id": 1, "result": { "root": { "nodeId": 4, "nodeName": "#document" } } }),
        );
        apply(
            &mut model,
            &json!({ "method": "Runtime.executionContextDestroyed", "params": {} }),
        );
        assert!(model.nodes.is_empty());
        assert_eq!(model.lines[0].text, "before");
    }

    #[test]
    fn network_failure_and_response() {
        let mut model = model();
        apply(
            &mut model,
            &json!({
                "method": "Network.requestWillBeSent",
                "params": {
                    "requestId": "a",
                    "request": { "method": "GET", "url": "https://example.test/" }
                }
            }),
        );
        apply(
            &mut model,
            &json!({
                "method": "Network.responseReceived",
                "params": {
                    "requestId": "a",
                    "response": { "status": 200, "mimeType": "text/html" }
                }
            }),
        );
        apply(
            &mut model,
            &json!({
                "method": "Network.loadingFailed",
                "params": { "requestId": "b", "errorText": "network error" }
            }),
        );
        assert_eq!(model.requests[0].status, Some(200));
        assert_eq!(model.requests[0].mime, "text/html");
        assert!(model.requests[1].failed);
    }

    #[test]
    fn tools_are_added_and_removed() {
        let mut model = model();
        apply(
            &mut model,
            &json!({
                "method": "WebMCP.toolsAdded",
                "params": {
                    "tools": [{
                        "name": "search_flights",
                        "description": "Find flights",
                        "inputSchema": "{}",
                        "frameId": "frame-1"
                    }]
                }
            }),
        );
        apply(
            &mut model,
            &json!({
                "method": "WebMCP.toolsRemoved",
                "params": { "names": ["search_flights"] }
            }),
        );
        assert!(model.tools.is_empty());
    }

    #[test]
    fn clear_empties_the_log() {
        let mut model = model();
        apply(
            &mut model,
            &json!({
                "method": "Log.entryAdded",
                "params": { "entry": { "level": "info", "text": "hello" } }
            }),
        );
        apply(
            &mut model,
            &json!({
                "method": "Log.entryAdded",
                "params": { "entry": { "level": "info", "text": "console.clear" } }
            }),
        );
        assert!(model.lines.is_empty());
    }

    #[test]
    fn unmasked_text_frame_parses() {
        let mut buffer = vec![0x81, 5];
        buffer.extend(b"hello");
        let mut sink = Vec::new();
        let text = take_text(&mut buffer, &mut sink).unwrap();
        assert_eq!(text.as_deref(), Some("hello"));
        assert!(buffer.is_empty());
    }
}
