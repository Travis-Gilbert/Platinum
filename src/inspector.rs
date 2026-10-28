//! The inspection prototype: a log, a document tree, a request list, and the
//! page's WebMCP tools, for the active tab.
//!
//! It is a CDP client of the in-process session in the Servo fork. Nothing
//! here launches Chrome, loads the Chrome DevTools frontend, or opens a
//! socket. `Model::apply` is the state the panel shows; the client only
//! delivers JSON. The methods this client sends are listed in `devtools/PIN`.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc::{self as futures_mpsc};
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
    /// Replies are matched by JSON-RPC id, not by result shape.
    pending: HashMap<i64, Pending>,
    context_id: Option<i64>,
    target: String,
    /// Bumped on every attach and detach. Late events from the previous
    /// page carry the old generation and are dropped.
    generation: u64,
    attached: bool,
    status: String,
    next_id: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Pending {
    Ignore,
    Eval,
    Root,
    Style(i64),
    Body(String),
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
            pending: HashMap::new(),
            context_id: None,
            target: String::new(),
            generation: 0,
            attached: false,
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

/// Fold one CDP message into the panel. Command replies are applied only
/// when their id is one this model is still waiting on.
pub(crate) fn apply(model: &mut Model, message: &Value) -> FollowUp {
    let mut follow = FollowUp { expand: Vec::new() };
    if let Some(method) = message.get("method").and_then(Value::as_str) {
        apply_event(model, method, message.get("params").unwrap_or(&Value::Null));
        return follow;
    }
    let id = message.get("id").and_then(Value::as_i64);
    let Some(id) = id else {
        return follow;
    };
    let Some(pending) = model.pending.remove(&id) else {
        return follow;
    };
    if let Some(error) = message.get("error") {
        let text = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("error");
        model.push(Level::Error, text);
        return follow;
    }
    let result = message.get("result").unwrap_or(&Value::Null);
    match pending {
        Pending::Ignore => {}
        Pending::Eval => {
            if let Some(details) = result.get("exceptionDetails") {
                let text = details
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("exception");
                model.push(Level::Error, text);
            } else if result.get("result").is_some() {
                model.push(Level::Result, describe_remote(&result["result"]));
            }
        }
        Pending::Root => {
            if let Some(root) = result.get("root").filter(|root| !root.is_null())
                && let Some(id) = adopt_root(model, root)
            {
                follow.expand.push(id);
            }
        }
        Pending::Style(node) => {
            if model.selected_node == Some(node)
                && let Some(styles) = result.get("computedStyle").and_then(Value::as_array)
            {
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
        }
        Pending::Body(request) => {
            if let Some(body) = result.get("body").and_then(Value::as_str)
                && let Some(row) = model.requests.iter_mut().find(|row| row.id == request)
            {
                row.body = body.to_owned();
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
            model.pending.clear();
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
            let tools = params
                .get("tools")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            model.tools.retain(|tool| {
                !tools
                    .iter()
                    .any(|removed| removed.get("name").and_then(Value::as_str) == Some(&tool.name))
            });
        }
        "WebMCP.toolResponded" => {
            let name = params
                .get("invocationId")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let status = params.get("status").and_then(Value::as_str).unwrap_or("");
            if status == "success" {
                model.push(
                    Level::Result,
                    format!("{name} → {}", params.get("output").unwrap_or(&Value::Null)),
                );
            } else {
                let error = params
                    .get("errorText")
                    .and_then(Value::as_str)
                    .unwrap_or(status);
                model.push(Level::Error, format!("{name} failed: {error}"));
            }
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
    Attach { generation: u64, target: String },
    Detach { generation: u64 },
    Send { generation: u64, message: Value },
}

enum Wire {
    Status { generation: u64, status: String },
    Message { generation: u64, message: Value },
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
        let client = cx
            .try_global::<crate::engine::Engine>()
            .is_some()
            .then(|| cx.global_mut::<crate::engine::Engine>().take_inspection())
            .flatten();
        thread::Builder::new()
            .name("cdp-client".to_owned())
            .spawn(move || drive(commands_rx, events_tx, client))
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

    /// Follow the page on screen. `None` detaches, including a Start tab or
    /// search results that still hold an old `Page`.
    pub(crate) fn attach(&mut self, target: Option<String>, cx: &mut Context<Self>) {
        let Some(target) = target.filter(|target| !target.is_empty()) else {
            self.detach(cx);
            return;
        };
        if self.model.attached && self.model.target == target {
            return;
        }
        self.model.generation += 1;
        let generation = self.model.generation;
        self.model.reset_for_target(&target);
        self.model.generation = generation;
        self.model.attached = true;
        let _ = self.commands.send(Command::Attach { generation, target });
        self.enable_domains();
        cx.notify();
    }

    fn detach(&mut self, cx: &mut Context<Self>) {
        if !self.model.attached && self.model.status == "No page" {
            return;
        }
        self.model.generation += 1;
        let generation = self.model.generation;
        self.model.attached = false;
        self.model.target.clear();
        self.model.context_id = None;
        self.model.pending.clear();
        self.model.nodes.clear();
        self.model.styles.clear();
        self.model.tools.clear();
        self.model.selected_node = None;
        self.model.status = "No page".to_owned();
        let _ = self.commands.send(Command::Detach { generation });
        cx.notify();
    }

    fn enable_domains(&mut self) {
        for method in [
            "Runtime.enable",
            "Log.enable",
            "Page.enable",
            "DOM.enable",
            "CSS.enable",
            "Overlay.enable",
            "Network.enable",
            "WebMCP.enable",
        ] {
            self.command_pending(Pending::Ignore, method, json!({}));
        }
        self.command_pending(Pending::Ignore, "Page.getFrameTree", json!({}));
        self.command_pending(Pending::Root, "DOM.getDocument", json!({}));
    }

    fn on_wire(&mut self, event: Wire, cx: &mut Context<Self>) {
        match event {
            Wire::Status { generation, status } => {
                if generation == self.model.generation {
                    self.model.status = status;
                }
            }
            Wire::Message {
                generation,
                message,
            } => {
                if generation != self.model.generation {
                    return;
                }
                let follow = apply(&mut self.model, &message);
                for node in follow.expand {
                    self.command_pending(
                        Pending::Ignore,
                        "DOM.requestChildNodes",
                        json!({ "nodeId": node }),
                    );
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
        if !self.model.attached {
            self.model.push(Level::Error, "No page");
            cx.notify();
            return;
        }
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
        self.command_pending(Pending::Eval, "Runtime.evaluate", params);
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
        self.command_pending(
            Pending::Ignore,
            "DOM.setAttributeValue",
            json!({ "nodeId": node, "name": name.trim(), "value": value }),
        );
    }

    fn invoke_tool(&mut self, text: &str) {
        let (name, arguments) = text
            .split_once(' ')
            .map(|(name, rest)| (name, rest.to_owned()))
            .unwrap_or((text, "{}".to_owned()));
        let arguments = serde_json::from_str::<Value>(&arguments).unwrap_or(json!({}));
        self.command_pending(
            Pending::Ignore,
            "WebMCP.invokeTool",
            json!({ "toolName": name, "input": arguments }),
        );
    }

    fn command_pending(&mut self, pending: Pending, method: &str, params: Value) {
        if !self.model.attached {
            return;
        }
        let id = self.model.alloc_id();
        let generation = self.model.generation;
        self.model.pending.insert(id, pending);
        let _ = self.commands.send(Command::Send {
            generation,
            message: json!({
                "id": id,
                "method": method,
                "params": params,
            }),
        });
    }

    fn select_node(&mut self, id: i64, cx: &mut Context<Self>) {
        self.model.selected_node = Some(id);
        self.model.styles.clear();
        self.command_pending(
            Pending::Style(id),
            "CSS.getComputedStyleForNode",
            json!({ "nodeId": id }),
        );
        self.command_pending(
            Pending::Ignore,
            "DOM.requestChildNodes",
            json!({ "nodeId": id }),
        );
        self.command_pending(
            Pending::Ignore,
            "Overlay.highlightNode",
            json!({ "nodeId": id }),
        );
        cx.notify();
    }

    fn load_body(&mut self, id: &str) {
        self.command_pending(
            Pending::Body(id.to_owned()),
            "Network.getResponseBody",
            json!({ "requestId": id }),
        );
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

fn drive(
    commands: Receiver<Command>,
    events: futures_mpsc::UnboundedSender<Wire>,
    client: Option<servo::BuiltinClient>,
) {
    let mut generation = 0u64;
    let mut attached = false;
    loop {
        let incoming = if attached {
            match commands.recv_timeout(Duration::from_millis(30)) {
                Ok(command) => Some(command),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match commands.recv() {
                Ok(command) => Some(command),
                Err(_) => break,
            }
        };
        if let Some(command) = incoming {
            match command {
                Command::Attach {
                    generation: next,
                    target,
                } => {
                    generation = next;
                    attached = true;
                    if let Some(client) = &client {
                        client.attach(&target);
                        let _ = events.unbounded_send(Wire::Status {
                            generation,
                            status: "Connected".to_owned(),
                        });
                    } else {
                        let _ = events.unbounded_send(Wire::Status {
                            generation,
                            status: "Not connected".to_owned(),
                        });
                    }
                }
                Command::Detach { generation: next } => {
                    generation = next;
                    attached = false;
                    if let Some(client) = &client {
                        client.detach();
                    }
                }
                Command::Send {
                    generation: sent,
                    message,
                } if sent == generation && attached => {
                    if let Some(client) = &client {
                        client.send(&message);
                    }
                }
                Command::Send { .. } => {}
            }
        }
        if let Some(client) = &client {
            while let Ok(message) = client.try_recv() {
                let _ = events.unbounded_send(Wire::Message {
                    generation,
                    message,
                });
            }
        }
    }
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
        model.pending.insert(1, Pending::Eval);
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
        model.pending.insert(2, Pending::Eval);
        apply(
            &mut model,
            &json!({ "id": 2, "result": { "result": { "type": "number", "value": 2 } } }),
        );
        assert_eq!(model.lines[0].text, "2");
    }

    #[test]
    fn document_tree_and_children() {
        let mut model = model();
        model.pending.insert(3, Pending::Root);
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
        model.pending.insert(1, Pending::Root);
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
                "params": { "tools": [{ "name": "search_flights", "frameId": "frame-1" }] }
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
    fn response_bodies_follow_their_request_ids() {
        let mut model = model();
        model.requests.push(RequestRow {
            id: "a".into(),
            method: "GET".into(),
            url: "https://example.test/a".into(),
            status: Some(200),
            mime: "text/plain".into(),
            failed: false,
            body: String::new(),
        });
        model.requests.push(RequestRow {
            id: "b".into(),
            method: "GET".into(),
            url: "https://example.test/b".into(),
            status: Some(200),
            mime: "text/plain".into(),
            failed: false,
            body: String::new(),
        });
        model.pending.insert(1, Pending::Body("b".into()));
        model.pending.insert(2, Pending::Body("a".into()));
        apply(
            &mut model,
            &json!({ "id": 2, "result": { "body": "alpha" } }),
        );
        apply(
            &mut model,
            &json!({ "id": 1, "result": { "body": "beta" } }),
        );
        assert_eq!(model.requests[0].body, "alpha");
        assert_eq!(model.requests[1].body, "beta");
    }

    #[test]
    fn a_late_style_does_not_fill_a_different_node() {
        let mut model = model();
        model.selected_node = Some(2);
        model.pending.insert(7, Pending::Style(1));
        apply(
            &mut model,
            &json!({
                "id": 7,
                "result": { "computedStyle": [{ "name": "color", "value": "red" }] }
            }),
        );
        assert!(model.styles.is_empty());
    }
}
