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
        crate::sequence::take_next(&mut self.next_id)
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
    if let Some(method) = message.get("method").and_then(Value::as_str) {
        apply_event(model, method, message.get("params").unwrap_or(&Value::Null));
        return FollowUp { expand: Vec::new() };
    }
    apply_reply(model, message)
}

fn apply_reply(model: &mut Model, message: &Value) -> FollowUp {
    let mut follow = FollowUp { expand: Vec::new() };
    let Some(id) = message.get("id").and_then(Value::as_i64) else {
        return follow;
    };
    apply_known_reply(model, message, id, &mut follow);
    follow
}

fn apply_known_reply(model: &mut Model, message: &Value, id: i64, follow: &mut FollowUp) {
    let Some(pending) = model.pending.remove(&id) else {
        return;
    };
    if reply_failed(model, message) {
        return;
    }
    apply_pending(
        model,
        pending,
        message.get("result").unwrap_or(&Value::Null),
        follow,
    );
}

fn reply_failed(model: &mut Model, message: &Value) -> bool {
    let Some(error) = message.get("error") else {
        return false;
    };
    let text = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("error");
    model.push(Level::Error, text);
    true
}

fn apply_pending(model: &mut Model, pending: Pending, result: &Value, follow: &mut FollowUp) {
    if let Pending::Ignore = pending {
        return;
    }
    apply_eval_pending(model, pending, result, follow);
}

fn apply_eval_pending(model: &mut Model, pending: Pending, result: &Value, follow: &mut FollowUp) {
    if let Pending::Eval = pending {
        apply_eval(model, result);
        return;
    }
    apply_root_pending(model, pending, result, follow);
}

fn apply_root_pending(model: &mut Model, pending: Pending, result: &Value, follow: &mut FollowUp) {
    if let Pending::Root = pending {
        apply_root(model, result, follow);
        return;
    }
    apply_style_pending(model, pending, result, follow);
}

fn apply_style_pending(model: &mut Model, pending: Pending, result: &Value, follow: &mut FollowUp) {
    if let Pending::Style(node) = pending {
        apply_style(model, node, result);
        return;
    }
    apply_body_pending(model, pending, result);
}

fn apply_body_pending(model: &mut Model, pending: Pending, result: &Value) {
    if let Pending::Body(request) = pending {
        apply_response_body(model, &request, result);
    }
}

fn note_status(inspector: &mut Inspector, generation: u64, status: String) {
    if generation == inspector.model.generation {
        inspector.model.status = status;
    }
}

fn note_message(inspector: &mut Inspector, event: Wire) {
    if let Wire::Message {
        generation,
        message,
    } = event
    {
        apply_current(inspector, generation, &message);
    }
}

fn apply_current(inspector: &mut Inspector, generation: u64, message: &Value) {
    if generation != inspector.model.generation {
        return;
    }
    let follow = apply(&mut inspector.model, message);
    expand_nodes(inspector, follow);
}

fn expand_nodes(inspector: &mut Inspector, follow: FollowUp) {
    for node in follow.expand {
        inspector.command_pending(
            Pending::Ignore,
            "DOM.requestChildNodes",
            json!({ "nodeId": node }),
        );
    }
}

fn apply_eval(model: &mut Model, result: &Value) {
    if let Some(details) = result.get("exceptionDetails") {
        let text = details
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("exception");
        model.push(Level::Error, text);
        return;
    }
    apply_eval_value(model, result);
}

fn apply_eval_value(model: &mut Model, result: &Value) {
    if result.get("result").is_some() {
        model.push(Level::Result, describe_remote(&result["result"]));
    }
}

fn apply_root(model: &mut Model, result: &Value, follow: &mut FollowUp) {
    if let Some(root) = result.get("root").filter(|root| !root.is_null())
        && let Some(id) = adopt_root(model, root)
    {
        follow.expand.push(id);
    }
}

fn apply_style(model: &mut Model, node: i64, result: &Value) {
    if model.selected_node == Some(node) {
        fill_styles(model, result);
    }
}

fn fill_styles(model: &mut Model, result: &Value) {
    if let Some(styles) = result.get("computedStyle").and_then(Value::as_array) {
        model.styles = styles.iter().filter_map(style_pair).collect();
    }
}

fn style_pair(style: &Value) -> Option<(String, String)> {
    let name = style_text(style, "name")?;
    style_named(style, name)
}

fn style_named(style: &Value, name: String) -> Option<(String, String)> {
    let value = style_text(style, "value")?;
    Some((name, value))
}

fn style_text(style: &Value, name: &str) -> Option<String> {
    let field = style.get(name)?;
    owned_text(field)
}

fn owned_text(field: &Value) -> Option<String> {
    Some(field.as_str()?.to_owned())
}

fn apply_response_body(model: &mut Model, request: &str, result: &Value) {
    if let Some(body) = result.get("body").and_then(Value::as_str) {
        store_body(model, request, body);
    }
}

fn store_body(model: &mut Model, request: &str, body: &str) {
    if let Some(row) = model.requests.iter_mut().find(|row| row.id == request) {
        row.body = body.to_owned();
    }
}

fn apply_event(model: &mut Model, method: &str, params: &Value) {
    if apply_network_event(model, method, params) {
        return;
    }
    apply_after_network(model, method, params);
}

fn apply_after_network(model: &mut Model, method: &str, params: &Value) {
    if apply_tool_event(model, method, params) {
        return;
    }
    apply_page_event(model, method, params);
}

fn apply_network_event(model: &mut Model, method: &str, params: &Value) -> bool {
    let Some(name) = method.strip_prefix("Network.") else {
        return false;
    };
    route_network(model, name, params);
    true
}

fn route_network(model: &mut Model, name: &str, params: &Value) {
    if name == "requestWillBeSent" {
        note_request(model, params);
        return;
    }
    route_network_response(model, name, params);
}

fn route_network_response(model: &mut Model, name: &str, params: &Value) {
    if name == "responseReceived" {
        note_response(model, params);
        return;
    }
    route_network_failure(model, name, params);
}

fn route_network_failure(model: &mut Model, name: &str, params: &Value) {
    if name == "loadingFailed" {
        note_failure(model, params);
    }
}

fn note_request(model: &mut Model, params: &Value) {
    let id = text_field(params, "requestId");
    if model.requests.iter().any(|row| row.id == id) {
        return;
    }
    push_request(model, params, id);
}

fn push_request(model: &mut Model, params: &Value, id: String) {
    let request = &params["request"];
    model.requests.push(RequestRow {
        id,
        method: text_field(request, "method"),
        url: text_field(request, "url"),
        status: None,
        mime: String::new(),
        failed: false,
        body: String::new(),
    });
    trim_requests(model);
}

fn trim_requests(model: &mut Model) {
    if model.requests.len() > MAX_REQUESTS {
        let extra = model.requests.len() - MAX_REQUESTS;
        model.requests.drain(0..extra);
    }
}

fn note_response(model: &mut Model, params: &Value) {
    let id = params
        .get("requestId")
        .and_then(Value::as_str)
        .unwrap_or("");
    if let Some(row) = model.requests.iter_mut().find(|row| row.id == id) {
        fill_response(row, &params["response"]);
    }
}

fn fill_response(row: &mut RequestRow, response: &Value) {
    row.status = response
        .get("status")
        .and_then(Value::as_u64)
        .map(|code| code as u16);
    row.mime = text_field(response, "mimeType");
}

fn note_failure(model: &mut Model, params: &Value) {
    let id = params
        .get("requestId")
        .and_then(Value::as_str)
        .unwrap_or("");
    if let Some(row) = model.requests.iter_mut().find(|row| row.id == id) {
        row.failed = true;
        return;
    }
    push_failure(model, params, id);
}

fn push_failure(model: &mut Model, params: &Value, id: &str) {
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

fn apply_tool_event(model: &mut Model, method: &str, params: &Value) -> bool {
    let Some(name) = method.strip_prefix("WebMCP.") else {
        return false;
    };
    route_tool(model, name, params);
    true
}

fn route_tool(model: &mut Model, name: &str, params: &Value) {
    if name == "toolsAdded" {
        add_tools(model, params);
        return;
    }
    route_tool_removal(model, name, params);
}

fn route_tool_removal(model: &mut Model, name: &str, params: &Value) {
    if name == "toolsRemoved" {
        remove_tools(model, params);
        return;
    }
    route_tool_result(model, name, params);
}

fn route_tool_result(model: &mut Model, name: &str, params: &Value) {
    if name == "toolResponded" {
        note_tool_result(model, params);
    }
}

fn add_tools(model: &mut Model, params: &Value) {
    let Some(tools) = params.get("tools").and_then(Value::as_array) else {
        return;
    };
    for tool in tools {
        add_tool(model, tool);
    }
}

fn add_tool(model: &mut Model, tool: &Value) {
    let name = text_field(tool, "name");
    model.tools.retain(|row| row.name != name);
    model.tools.push(ToolRow {
        name,
        description: text_field(tool, "description"),
        schema: tool
            .get("inputSchema")
            .map(|schema| schema.to_string())
            .unwrap_or_default(),
        frame: text_field(tool, "frameId"),
    });
}

fn remove_tools(model: &mut Model, params: &Value) {
    let Some(tools) = params.get("tools").and_then(Value::as_array) else {
        return;
    };
    model
        .tools
        .retain(|tool| !tools.iter().any(|removed| tool_name(removed) == tool.name));
}

fn tool_name(tool: &Value) -> &str {
    tool.get("name").and_then(Value::as_str).unwrap_or("")
}

fn note_tool_result(model: &mut Model, params: &Value) {
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
        return;
    }
    note_tool_failure(model, params, name, status);
}

fn note_tool_failure(model: &mut Model, params: &Value, name: &str, status: &str) {
    let error = params
        .get("errorText")
        .and_then(Value::as_str)
        .unwrap_or(status);
    model.push(Level::Error, format!("{name} failed: {error}"));
}

fn apply_page_event(model: &mut Model, method: &str, params: &Value) {
    if method == "Log.entryAdded" {
        note_log(model, params);
        return;
    }
    apply_context_created(model, method, params);
}

fn apply_context_created(model: &mut Model, method: &str, params: &Value) {
    if method == "Runtime.executionContextCreated" {
        model.context_id = params["context"]["id"].as_i64();
        return;
    }
    apply_context_destroyed(model, method, params);
}

fn apply_context_destroyed(model: &mut Model, method: &str, params: &Value) {
    if method == "Runtime.executionContextDestroyed" {
        drop_page_context(model);
        return;
    }
    apply_child_event(model, method, params);
}

fn apply_child_event(model: &mut Model, method: &str, params: &Value) {
    if method == "DOM.setChildNodes" {
        note_children(model, params);
    }
}

fn note_log(model: &mut Model, params: &Value) {
    let entry = &params["entry"];
    let text = entry.get("text").and_then(Value::as_str).unwrap_or("");
    if text == "console.clear" {
        model.lines.clear();
        return;
    }
    model.push(log_level(entry), text);
}

fn log_level(entry: &Value) -> Level {
    let name = entry.get("level").and_then(Value::as_str).unwrap_or("info");
    if problem_level(name) {
        Level::Error
    } else {
        Level::Log
    }
}

fn problem_level(name: &str) -> bool {
    name == "error" || name == "warning"
}

fn drop_page_context(model: &mut Model) {
    model.context_id = None;
    model.nodes.clear();
    model.styles.clear();
    model.tools.clear();
    model.selected_node = None;
    model.pending.clear();
}

fn note_children(model: &mut Model, params: &Value) {
    let parent = params.get("parentId").and_then(Value::as_i64).unwrap_or(0);
    let nodes = params
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    set_children(model, parent, &nodes);
}

fn text_field(value: &Value, name: &str) -> String {
    value
        .get(name)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned()
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
    let depth = child_depth(model, parent);
    drop_subtree(&mut model.nodes, parent);
    let rows: Vec<NodeRow> = nodes
        .iter()
        .filter_map(|node| node_row(node, parent, depth))
        .collect();
    let at = model
        .nodes
        .iter()
        .position(|node| node.id == parent)
        .map(|index| index + 1)
        .unwrap_or(model.nodes.len());
    model.nodes.splice(at..at, rows);
}

fn child_depth(model: &Model, parent: i64) -> usize {
    model
        .nodes
        .iter()
        .find(|node| node.id == parent)
        .map(|node| node.depth + 1)
        .unwrap_or(0)
}

fn node_row(node: &Value, parent: i64, depth: usize) -> Option<NodeRow> {
    let id = node_id(node)?;
    Some(NodeRow {
        id,
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
}

fn node_id(node: &Value) -> Option<i64> {
    node.get("nodeId")?.as_i64()
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
    if let Some(text) = rendered_field(value) {
        return text;
    }
    remote_label(value)
}

fn rendered_field(value: &Value) -> Option<String> {
    let text = value.get("value")?;
    rendered_value(text)
}

fn rendered_value(text: &Value) -> Option<String> {
    if let Some(text) = text.as_str() {
        return Some(text.to_owned());
    }
    json_text(text)
}

fn json_text(text: &Value) -> Option<String> {
    if text.is_null() {
        None
    } else {
        Some(text.to_string())
    }
}

fn remote_label(value: &Value) -> String {
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

async fn deliver_wires(
    inspector: gpui_kit::WeakEntity<Inspector>,
    cx: &mut gpui_kit::AsyncApp,
    mut events_rx: futures_mpsc::UnboundedReceiver<Wire>,
) {
    while accept_wire(&inspector, cx, &mut events_rx).await {}
}

async fn accept_wire(
    inspector: &gpui_kit::WeakEntity<Inspector>,
    cx: &mut gpui_kit::AsyncApp,
    events_rx: &mut futures_mpsc::UnboundedReceiver<Wire>,
) -> bool {
    let Some(event) = events_rx.next().await else {
        return false;
    };
    inspector
        .update(cx, |inspector, cx| inspector.on_wire(event, cx))
        .is_ok()
}

fn open_target(target: Option<String>) -> Option<String> {
    target.filter(|target| !target.is_empty())
}

fn same_target(model: &Model, target: &str) -> bool {
    model.attached && model.target == target
}

fn already_detached(model: &Model) -> bool {
    !model.attached && model.status == "No page"
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
            deliver_wires(inspector, cx, events_rx).await;
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
        let Some(target) = open_target(target) else {
            self.detach(cx);
            return;
        };
        self.attach_target(target, cx);
    }

    fn attach_target(&mut self, target: String, cx: &mut Context<Self>) {
        if same_target(&self.model, &target) {
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
        if already_detached(&self.model) {
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
        if let Wire::Status { generation, status } = event {
            note_status(self, generation, status);
        } else {
            note_message(self, event);
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
        self.submit_line(window, cx);
    }

    fn submit_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.expression.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        self.expression
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.run_line(&text, cx);
    }

    fn run_line(&mut self, text: &str, cx: &mut Context<Self>) {
        if !self.model.attached {
            self.model.push(Level::Error, "No page");
            cx.notify();
            return;
        }
        self.run_section(text);
        cx.notify();
    }

    fn run_section(&mut self, text: &str) {
        if self.model.section == Section::Console {
            self.evaluate(text);
            return;
        }
        self.run_elements(text);
    }

    fn run_elements(&mut self, text: &str) {
        if self.model.section == Section::Elements {
            self.edit_attribute(text);
            return;
        }
        self.run_tools(text);
    }

    fn run_tools(&mut self, text: &str) {
        if self.model.section == Section::Tools {
            self.invoke_tool(text);
        }
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
        let hint = section_hint(section);
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

fn section_hint(section: Section) -> &'static str {
    if section == Section::Console {
        return "Evaluate";
    }
    hint_after_console(section)
}

fn hint_after_console(section: Section) -> &'static str {
    if section == Section::Elements {
        return "Attribute as name=value";
    }
    hint_after_elements(section)
}

fn hint_after_elements(section: Section) -> &'static str {
    if section == Section::Network {
        return "Requests";
    }
    "Tool name, then JSON arguments"
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
        if self.model.section == Section::Console {
            return console_lines(&self.model, cx);
        }
        body_after_console(self, cx)
    }
}

fn body_after_console(inspector: &Inspector, cx: &mut Context<Inspector>) -> gpui_kit::AnyElement {
    if inspector.model.section == Section::Elements {
        return element_rows(inspector, cx);
    }
    body_after_elements(inspector, cx)
}

fn body_after_elements(inspector: &Inspector, cx: &mut Context<Inspector>) -> gpui_kit::AnyElement {
    if inspector.model.section == Section::Network {
        return request_rows(inspector, cx);
    }
    tool_rows(&inspector.model)
}

fn console_lines(model: &Model, cx: &mut Context<Inspector>) -> gpui_kit::AnyElement {
    let danger = cx.theme().danger;
    let foreground = cx.theme().foreground;
    let muted = cx.theme().muted_foreground;
    v_flex()
        .gap_1()
        .children(model.lines.iter().map(|line| {
            div()
                .text_xs()
                .text_color(line_color(line.level, danger, foreground, muted))
                .child(line.text.clone())
                .into_any_element()
        }))
        .into_any_element()
}

fn line_color(
    level: Level,
    danger: gpui_kit::Hsla,
    foreground: gpui_kit::Hsla,
    muted: gpui_kit::Hsla,
) -> gpui_kit::Hsla {
    if level == Level::Error {
        return danger;
    }
    quiet_color(level, foreground, muted)
}

fn quiet_color(level: Level, foreground: gpui_kit::Hsla, muted: gpui_kit::Hsla) -> gpui_kit::Hsla {
    if level == Level::Result {
        return foreground;
    }
    muted
}

fn element_rows(inspector: &Inspector, cx: &mut Context<Inspector>) -> gpui_kit::AnyElement {
    let selected = inspector.model.selected_node;
    let muted = cx.theme().muted_foreground;
    v_flex()
        .gap_1()
        .children(inspector.model.nodes.iter().map(|node| {
            let id = node.id;
            div()
                .id(("node", id as u64))
                .text_xs()
                .cursor_pointer()
                .pl(px(12. * node.depth as f32))
                .on_click(cx.listener(move |inspector, _, _, cx| {
                    inspector.select_node(id, cx);
                }))
                .child(format!(
                    "{}{} {}",
                    node_mark(selected, id),
                    node.name,
                    node.value.chars().take(40).collect::<String>()
                ))
                .into_any_element()
        }))
        .children(inspector.model.styles.iter().take(24).map(|(name, value)| {
            div()
                .text_xs()
                .text_color(muted)
                .child(format!("{name}: {value}"))
                .into_any_element()
        }))
        .into_any_element()
}

fn node_mark(selected: Option<i64>, id: i64) -> &'static str {
    if selected == Some(id) {
        return "▸ ";
    }
    "  "
}

fn request_rows(inspector: &Inspector, cx: &mut Context<Inspector>) -> gpui_kit::AnyElement {
    v_flex()
        .gap_1()
        .children(
            inspector
                .model
                .requests
                .iter()
                .enumerate()
                .map(|(index, request)| request_row(request, index, cx)),
        )
        .into_any_element()
}

fn request_row(
    request: &RequestRow,
    index: usize,
    cx: &mut Context<Inspector>,
) -> gpui_kit::AnyElement {
    let id = request.id.clone();
    div()
        .id(("request", index as u64))
        .text_xs()
        .cursor_pointer()
        .on_click(cx.listener(move |inspector, _, _, cx| {
            inspector.load_body(&id);
            cx.notify();
        }))
        .child(format!(
            "{} {} {} {}",
            request_status(request),
            request.method,
            request.mime,
            request.url
        ))
        .when(!request.body.is_empty(), |row| {
            row.child(format!(
                "  {}",
                request.body.chars().take(80).collect::<String>()
            ))
        })
        .into_any_element()
}

fn request_status(request: &RequestRow) -> String {
    if request.failed {
        return "failed".to_owned();
    }
    request_code(request)
}

fn request_code(request: &RequestRow) -> String {
    request
        .status
        .map(|code| code.to_string())
        .unwrap_or_else(|| "…".to_owned())
}

fn tool_rows(model: &Model) -> gpui_kit::AnyElement {
    v_flex()
        .gap_1()
        .children(model.tools.iter().map(|tool| {
            div()
                .text_xs()
                .child(format!("{} — {}", tool.name, tool.description))
                .into_any_element()
        }))
        .into_any_element()
}

fn drive(
    commands: Receiver<Command>,
    events: futures_mpsc::UnboundedSender<Wire>,
    client: Option<servo::BuiltinClient>,
) {
    let mut generation = 0u64;
    let mut attached = false;
    while drive_turn(
        &mut generation,
        &mut attached,
        &commands,
        &events,
        client.as_ref(),
    ) {}
}

fn drive_turn(
    generation: &mut u64,
    attached: &mut bool,
    commands: &Receiver<Command>,
    events: &futures_mpsc::UnboundedSender<Wire>,
    client: Option<&servo::BuiltinClient>,
) -> bool {
    let incoming = next_command(commands, *attached);
    if let DrivePoll::Stop = incoming {
        return false;
    }
    take_command(generation, attached, incoming, events, client);
    publish_events(events, client, *generation);
    true
}

enum DrivePoll {
    Stop,
    Wait,
    Command(Command),
}

fn next_command(commands: &Receiver<Command>, attached: bool) -> DrivePoll {
    if attached {
        return poll_attached(commands);
    }
    poll_blocking(commands)
}

fn poll_attached(commands: &Receiver<Command>) -> DrivePoll {
    command_or_wait(commands.recv_timeout(Duration::from_millis(30)))
}

fn command_or_wait(received: Result<Command, RecvTimeoutError>) -> DrivePoll {
    if let Err(error) = received {
        return timeout_or_stop(error);
    }
    ready_command(received)
}

fn ready_command(received: Result<Command, RecvTimeoutError>) -> DrivePoll {
    if let Ok(command) = received {
        return DrivePoll::Command(command);
    }
    DrivePoll::Stop
}

fn timeout_or_stop(error: RecvTimeoutError) -> DrivePoll {
    if let RecvTimeoutError::Timeout = error {
        return DrivePoll::Wait;
    }
    DrivePoll::Stop
}

fn poll_blocking(commands: &Receiver<Command>) -> DrivePoll {
    if let Ok(command) = commands.recv() {
        return DrivePoll::Command(command);
    }
    DrivePoll::Stop
}

fn take_command(
    generation: &mut u64,
    attached: &mut bool,
    incoming: DrivePoll,
    events: &futures_mpsc::UnboundedSender<Wire>,
    client: Option<&servo::BuiltinClient>,
) {
    if let DrivePoll::Command(command) = incoming {
        run_command(generation, attached, command, events, client);
    }
}

fn run_command(
    generation: &mut u64,
    attached: &mut bool,
    command: Command,
    events: &futures_mpsc::UnboundedSender<Wire>,
    client: Option<&servo::BuiltinClient>,
) {
    if let Command::Attach {
        generation: next,
        target,
    } = command
    {
        attach_client(generation, attached, next, &target, events, client);
        return;
    }
    run_detach_or_send(generation, attached, command, client);
}

fn attach_client(
    generation: &mut u64,
    attached: &mut bool,
    next: u64,
    target: &str,
    events: &futures_mpsc::UnboundedSender<Wire>,
    client: Option<&servo::BuiltinClient>,
) {
    *generation = next;
    *attached = true;
    if let Some(client) = client {
        client.attach(target);
        report_status(events, next, "Connected");
        return;
    }
    report_status(events, next, "Not connected");
}

fn report_status(events: &futures_mpsc::UnboundedSender<Wire>, generation: u64, status: &str) {
    let _ = events.unbounded_send(Wire::Status {
        generation,
        status: status.to_owned(),
    });
}

fn run_detach_or_send(
    generation: &mut u64,
    attached: &mut bool,
    command: Command,
    client: Option<&servo::BuiltinClient>,
) {
    if let Command::Detach { generation: next } = command {
        *generation = next;
        *attached = false;
        detach_client(client);
        return;
    }
    send_command(*generation, *attached, command, client);
}

fn detach_client(client: Option<&servo::BuiltinClient>) {
    if let Some(client) = client {
        client.detach();
    }
}

fn send_command(
    generation: u64,
    attached: bool,
    command: Command,
    client: Option<&servo::BuiltinClient>,
) {
    if let Command::Send {
        generation: sent,
        message,
    } = command
    {
        send_current(generation, attached, sent, &message, client);
    }
}

fn send_current(
    generation: u64,
    attached: bool,
    sent: u64,
    message: &Value,
    client: Option<&servo::BuiltinClient>,
) {
    if for_current_page(sent, generation, attached) {
        send_attached(client, message);
    }
}

fn for_current_page(sent: u64, generation: u64, attached: bool) -> bool {
    sent == generation && attached
}

fn send_attached(client: Option<&servo::BuiltinClient>, message: &Value) {
    if let Some(client) = client {
        client.send(message);
    }
}

fn publish_events(
    events: &futures_mpsc::UnboundedSender<Wire>,
    client: Option<&servo::BuiltinClient>,
    generation: u64,
) {
    if let Some(client) = client {
        forward_events(events, client, generation);
    }
}

fn forward_events(
    events: &futures_mpsc::UnboundedSender<Wire>,
    client: &servo::BuiltinClient,
    generation: u64,
) {
    while let Ok(message) = client.try_recv() {
        let _ = events.unbounded_send(Wire::Message {
            generation,
            message,
        });
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
