//! The CRM shell: navigator, table or record, and the ask bar.
//!
//! The table delegate holds a projection of the store. Sorting, width, and
//! column order live there; the companies themselves stay in [`super::model`].

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::base::InteractiveElementExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::resizable::{HandleEdge, resize_handle};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::table::{
    Column, ColumnSort, DataTable, TableDelegate, TableEvent, TableState,
};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::text::TextView;
use gpui_kit::component::tree::{TreeEvent, TreeItem, TreeState, tree};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, h_flex, v_flex,
};
use gpui_kit::{
    App, AppContext as _, Axis, ClickEvent, Context, DragMoveEvent, Empty, Entity, FocusHandle,
    Focusable, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    PathPromptOptions, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, Window, div, prelude::FluentBuilder as _, px, rems,
};

use super::files;
use super::material::{self, Row as MaterialRow};
use super::model::{AskError, Company, Id, Object, Person, Proposal, Stage, Store, View};
use super::shelf::{self, NodeShape, Page, Shelf};
use crate::browser::{Browser, FRAME, TAB_ROW};
use crate::bump;
use crate::places::{Exit, Place, PlacesBump};

/// Sidebar content width, in rems, beside the places bump. Narrow enough
/// for the destination names; drag the trailing edge to change it.
const NAV_WIDTH: f32 = 9.5;
const NAV_MIN: f32 = 8.;
const NAV_MAX: f32 = 18.;
const INSPECTOR: f32 = 280.;

fn tree_from(outline: &shelf::Outline) -> TreeItem {
    TreeItem::new(outline.id.to_string(), outline.name.clone())
        .expanded(outline.expanded)
        .children(outline.children.iter().map(tree_from))
}

fn markdown_body(source: &str) -> String {
    let mut lines = source.lines();
    let Some(first) = lines.next() else {
        return String::new();
    };
    if first.trim_start().starts_with('#') {
        lines.collect::<Vec<_>>().join("\n")
    } else {
        source.to_owned()
    }
}

fn rasterize_pdf(path: &std::path::Path) -> Option<Vec<u8>> {
    let dir = tempfile::Builder::new()
        .prefix("platinum-pdf-")
        .tempdir()
        .ok()?;
    let status = std::process::Command::new("/usr/bin/qlmanage")
        .args(["-t", "-s", "1400", "-o"])
        .arg(dir.path())
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;
    if !status.success() {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    std::fs::read(dir.path().join(format!("{name}.png"))).ok()
}

fn file_notice(name: &str, detail: &str) -> gpui_kit::AnyElement {
    v_flex()
        .id("index-file")
        .debug_selector(|| "index-file".into())
        .size_full()
        .p_4()
        .gap_2()
        .items_start()
        .child(div().text_sm().child(name.to_owned()))
        .child(div().text_xs().child(detail.to_owned()))
        .into_any_element()
}

fn file_add_icon() -> gpui_kit::Img {
    let bytes: &'static [u8] =
        include_bytes!("../../assets/icons/streamline/Common-File-Add--Streamline-Ultimate.png");
    let image = gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, bytes.to_vec());
    gpui_kit::img(std::sync::Arc::new(image))
        .w(rems(1.))
        .h(rems(1.))
}

fn shelf_icon(shape: NodeShape) -> gpui_kit::Img {
    let bytes: &'static [u8] = match shape {
        NodeShape::Folder { open: true, .. } => {
            include_bytes!("../../assets/icons/streamline/File-Open--Streamline-Ultimate.png")
        }
        NodeShape::Folder { .. } => {
            include_bytes!("../../assets/icons/streamline/Folder-Stand--Streamline-Ultimate.png")
        }
        NodeShape::Markdown => {
            include_bytes!(
                "../../assets/icons/streamline/Office-File-Text--Streamline-Ultimate.png"
            )
        }
        NodeShape::Pdf => {
            include_bytes!("../../assets/icons/streamline/Office-File-Pdf--Streamline-Ultimate.png")
        }
        NodeShape::Zip => {
            include_bytes!("../../assets/icons/streamline/Zip-File-Add--Streamline-Ultimate.png")
        }
        NodeShape::File { .. } | NodeShape::Other => {
            include_bytes!("../../assets/icons/streamline/File-Txt-1--Streamline-Ultimate.png")
        }
    };
    let image = gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, bytes.to_vec());
    gpui_kit::img(std::sync::Arc::new(image))
        .w(rems(1.))
        .h(rems(1.))
}

fn tally(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

fn counted(label: &str, count: usize) -> String {
    if count == 0 {
        label.to_owned()
    } else {
        format!("{label} {count}")
    }
}

fn company_in_scope(
    screen: &Screen,
    selected: Option<Target>,
    person_company: impl Fn(Id) -> Option<Id>,
) -> Option<Id> {
    match screen {
        Screen::Company { id, .. } => Some(*id),
        Screen::Person { id } => person_company(*id),
        Screen::Highlights | Screen::Reading(_) | Screen::Inbox => None,
        Screen::List => selected.and_then(|target| target_company(target, person_company)),
    }
}

fn target_company(target: Target, person_company: impl Fn(Id) -> Option<Id>) -> Option<Id> {
    match target {
        Target::Company(id) => Some(id),
        Target::Person(id) => person_company(id),
    }
}

fn ask_label(highlights: bool, company: Option<&str>) -> String {
    if highlights {
        return "Ask about highlights".into();
    }
    match company {
        Some(name) => format!("Ask about {name}"),
        None => "Ask about selected records".to_owned(),
    }
}

fn apply_column_sort(columns: &mut [Column], col_ix: usize, sort: ColumnSort) {
    for (ix, column) in columns.iter_mut().enumerate() {
        if column.sort.is_some() {
            column.sort = Some(if ix == col_ix {
                sort
            } else {
                ColumnSort::Default
            });
        }
    }
}

fn sort_rows(rows: &mut [Row], col_ix: usize, descending: bool) {
    rows.sort_by(|left, right| {
        cell_order(cell_at(left, col_ix), cell_at(right, col_ix), descending)
    });
}

fn cell_at(row: &Row, col_ix: usize) -> &str {
    row.cells
        .get(col_ix)
        .map(|cell| cell.text.as_str())
        .unwrap_or("")
}

fn cell_order(left: &str, right: &str, descending: bool) -> std::cmp::Ordering {
    let order = match (left.parse::<u32>(), right.parse::<u32>()) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        _ => left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase()),
    };
    if descending { order.reverse() } else { order }
}

fn reorder<T>(items: &mut Vec<T>, from: usize, to: usize) -> Option<usize> {
    if from == to || from >= items.len() {
        return None;
    }
    let item = items.remove(from);
    let to = to.min(items.len());
    items.insert(to, item);
    Some(to)
}

fn shift_cell(cells: &mut Vec<Cell>, from: usize, to: usize) {
    if from >= cells.len() {
        return;
    }
    let cell = cells.remove(from);
    let at = to.min(cells.len());
    cells.insert(at, cell);
}
const THEMES: [&str; 4] = ["Buying signal", "Timeline", "Stakeholder", "Risk"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Company(Id),
    Person(Id),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Section {
    Overview,
    Activity,
    Notes,
    Tasks,
    Evidence,
}

impl Section {
    fn index(self) -> usize {
        match self {
            Self::Overview => 0,
            Self::Activity => 1,
            Self::Notes => 2,
            Self::Tasks => 3,
            Self::Evidence => 4,
        }
    }

    fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Overview),
            1 => Some(Self::Activity),
            2 => Some(Self::Notes),
            3 => Some(Self::Tasks),
            4 => Some(Self::Evidence),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Inbox,
    List,
    Company {
        id: Id,
        section: Section,
    },
    Person {
        id: Id,
    },
    Highlights,
    /// A file or folder from the sidebar, shown in the card.
    Reading(u64),
}

/// A sidebar destination. The order is data, saved with the records, so Inbox
/// leads only because that is the order a new sidebar starts in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Destination {
    Inbox,
    Companies,
    People,
    Tasks,
    Highlights,
}

impl Destination {
    fn key(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Companies => "companies",
            Self::People => "people",
            Self::Tasks => "tasks",
            Self::Highlights => "highlights",
        }
    }

    fn parse(key: &str) -> Option<Self> {
        match key {
            "inbox" => Some(Self::Inbox),
            "companies" => Some(Self::Companies),
            "people" => Some(Self::People),
            "tasks" => Some(Self::Tasks),
            "highlights" => Some(Self::Highlights),
            _ => None,
        }
    }
}

fn default_sidebar() -> Vec<Destination> {
    vec![
        Destination::Inbox,
        Destination::Companies,
        Destination::People,
        Destination::Tasks,
        Destination::Highlights,
    ]
}

/// Move the destination named `key` by `delta` places. The returned order is
/// the one to save; `false` means it did not move.
fn move_sidebar(order: &mut Vec<Destination>, key: &str, delta: isize) -> bool {
    let Some(from) = order.iter().position(|item| item.key() == key) else {
        return false;
    };
    let to = (from as isize + delta).clamp(0, order.len() as isize - 1) as usize;
    if from == to {
        return false;
    }
    let item = order.remove(from);
    order.insert(to, item);
    true
}

fn load_sidebar(dir: Option<&std::path::Path>) -> Vec<Destination> {
    let Some(text) = dir.and_then(|dir| std::fs::read_to_string(dir.join("sidebar.json")).ok())
    else {
        return default_sidebar();
    };
    let Ok(keys) = serde_json::from_str::<Vec<String>>(&text) else {
        return default_sidebar();
    };
    let mut order = Vec::new();
    for key in keys {
        if let Some(item) = Destination::parse(&key)
            && !order.contains(&item)
        {
            order.push(item);
        }
    }
    for item in default_sidebar() {
        if !order.contains(&item) {
            order.push(item);
        }
    }
    if order.is_empty() {
        default_sidebar()
    } else {
        order
    }
}

fn load_nav_width(dir: Option<&std::path::Path>) -> f32 {
    let Some(text) = dir.and_then(|dir| std::fs::read_to_string(dir.join("nav-width")).ok()) else {
        return NAV_WIDTH;
    };
    text.trim()
        .parse::<f32>()
        .unwrap_or(NAV_WIDTH)
        .clamp(NAV_MIN, NAV_MAX)
}

/// The sidebar's trailing edge, while it is dragged. It draws nothing.
#[derive(Clone)]
struct ResizeNav;

impl Render for ResizeNav {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// A sidebar row being dragged onto another row.
#[derive(Clone)]
struct ShelfDrag(u64);

impl Render for ShelfDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

fn screen_for(destination: Destination, object: Object) -> (Object, Screen) {
    match destination {
        Destination::Inbox => (object, Screen::Inbox),
        Destination::Companies => (Object::Companies, Screen::List),
        Destination::People => (Object::People, Screen::List),
        Destination::Tasks => (Object::Tasks, Screen::List),
        Destination::Highlights => (object, Screen::Highlights),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InboxPick {
    Company(Id),
    Task(Id),
}

#[derive(Clone)]
enum CellKind {
    Text,
    Muted,
    Stage(Stage),
    /// A relationship drawn as its own control, the way Attio puts a record in a cell.
    Links(Vec<(Target, String)>),
}

#[derive(Clone)]
struct Cell {
    text: String,
    kind: CellKind,
}

#[derive(Clone)]
struct Row {
    id: Id,
    target: Target,
    cells: Vec<Cell>,
}

struct Rows {
    columns: Vec<Column>,
    rows: Vec<Row>,
    unsorted: Vec<Row>,
    pending: Rc<RefCell<Option<Target>>>,
    empty: SharedString,
}

impl Rows {
    fn new(
        pending: Rc<RefCell<Option<Target>>>,
        store: &Store,
        object: Object,
        view: View,
    ) -> Self {
        let (columns, rows, empty) = project(store, object, view);
        Self {
            columns,
            unsorted: rows.clone(),
            rows,
            pending,
            empty,
        }
    }

    fn row(&self, index: usize) -> Option<&Row> {
        self.rows.get(index)
    }

    fn same_columns(&self, columns: &[Column]) -> bool {
        self.columns.len() == columns.len()
            && self
                .columns
                .iter()
                .all(|held| columns.iter().any(|next| held.key == next.key))
    }

    fn set_rows(&mut self, rows: Vec<Row>, empty: SharedString) {
        self.unsorted = rows;
        self.rows = self.unsorted.clone();
        if let Some((index, descending)) =
            self.columns
                .iter()
                .enumerate()
                .find_map(|(index, column)| match column.sort {
                    Some(ColumnSort::Ascending) => Some((index, false)),
                    Some(ColumnSort::Descending) => Some((index, true)),
                    _ => None,
                })
        {
            sort_rows(&mut self.rows, index, descending);
        }
        self.empty = empty;
    }

    fn set_projection(&mut self, columns: Vec<Column>, rows: Vec<Row>, empty: SharedString) {
        self.columns = columns;
        self.set_rows(rows, empty);
    }

    fn apply_widths(&mut self, widths: &[gpui_kit::Pixels]) {
        for (column, width) in self.columns.iter_mut().zip(widths) {
            column.width = *width;
        }
    }

    fn queue(&self, target: Target) {
        *self.pending.borrow_mut() = Some(target);
    }
}

impl TableDelegate for Rows {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }

    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }

    fn column(&self, col_ix: usize, _: &App) -> Column {
        self.columns
            .get(col_ix)
            .cloned()
            .unwrap_or_else(|| Column::new("missing", ""))
    }

    fn perform_sort(
        &mut self,
        col_ix: usize,
        sort: ColumnSort,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        apply_column_sort(&mut self.columns, col_ix, sort);
        self.rows = self.unsorted.clone();
        if sort == ColumnSort::Default {
            return;
        }
        sort_rows(&mut self.rows, col_ix, sort == ColumnSort::Descending);
    }

    fn move_column(
        &mut self,
        from: usize,
        to: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) {
        let Some(to) = reorder(&mut self.columns, from, to) else {
            return;
        };
        self.rows
            .iter_mut()
            .chain(self.unsorted.iter_mut())
            .for_each(|row| shift_cell(&mut row.cells, from, to));
    }

    fn render_td(
        &mut self,
        row_ix: usize,
        col_ix: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let Some(row) = self.rows.get(row_ix) else {
            return div().into_any_element();
        };
        let target = row.target;
        let id = row.id;
        let cell = row.cells.get(col_ix).cloned();
        let Some(cell) = cell else {
            return div().into_any_element();
        };
        if let CellKind::Links(links) = &cell.kind {
            if links.is_empty() {
                return div()
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child("—")
                    .into_any_element();
            }
            return h_flex()
                .gap_1()
                .flex_wrap()
                .children(links.iter().map(|(target, name)| {
                    let target = *target;
                    let key = match target {
                        Target::Company(link) | Target::Person(link) => link.raw(),
                    };
                    Button::new(("crm-cell-link", id.raw().wrapping_mul(97).wrapping_add(key)))
                        .label(name.clone())
                        .ghost()
                        .small()
                        .on_click(cx.listener(
                            move |rows: &mut TableState<Rows>, _: &ClickEvent, _, cx| {
                                rows.delegate_mut().queue(target);
                                cx.notify();
                            },
                        ))
                }))
                .into_any_element();
        }
        let text = match &cell.kind {
            CellKind::Stage(stage) => stage_tag(*stage).into_any_element(),
            CellKind::Muted => div()
                .min_w_0()
                .truncate()
                .text_sm()
                .text_color(cx.theme().foreground)
                .child(cell.text.clone())
                .into_any_element(),
            CellKind::Text => div()
                .min_w_0()
                .truncate()
                .text_sm()
                .when(col_ix == 0, |name| name.font_weight(FontWeight::MEDIUM))
                .child(cell.text.clone())
                .into_any_element(),
            CellKind::Links(_) => div().into_any_element(),
        };
        if col_ix == 0 {
            div()
                .id(("crm-row", id.raw()))
                .debug_selector({
                    let raw = id.raw();
                    move || format!("crm-row-{raw}")
                })
                .min_w_0()
                .child(text)
                .on_click(
                    cx.listener(move |rows: &mut TableState<Rows>, _: &ClickEvent, _, cx| {
                        rows.delegate_mut().queue(target);
                        cx.notify();
                    }),
                )
                .into_any_element()
        } else {
            div().min_w_0().child(text).into_any_element()
        }
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .p_6()
            .text_sm()
            .text_color(cx.theme().foreground)
            .child(self.empty.clone())
    }

    fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
        self.rows
            .get(row_ix)
            .and_then(|row| row.cells.get(col_ix))
            .map(|cell| cell.text.clone())
            .unwrap_or_default()
    }
}

fn stage_tag(stage: Stage) -> Tag {
    let tag = match stage {
        Stage::Lead => Tag::secondary(),
        Stage::Qualified => Tag::info(),
        Stage::Pilot => Tag::warning(),
        Stage::Customer => Tag::success(),
    };
    tag.small().outline().child(stage.label())
}

fn project(store: &Store, object: Object, view: View) -> (Vec<Column>, Vec<Row>, SharedString) {
    match object {
        Object::Companies => {
            let rows = store
                .companies(view)
                .into_iter()
                .map(|company| company_row(store, company))
                .collect();
            let empty = if view == View::NeedsFollowUp {
                "Every company has a next step."
            } else {
                "No companies in this view."
            };
            (company_columns(), rows, empty.into())
        }
        Object::People => {
            let rows = store
                .people()
                .iter()
                .map(|person| person_row(store, person))
                .collect();
            (people_columns(), rows, "No people yet.".into())
        }
        Object::Tasks => {
            let rows = store
                .tasks()
                .iter()
                .map(|task| {
                    let company = store
                        .company(task.company())
                        .map(Company::name)
                        .unwrap_or("—");
                    Row {
                        id: task.id(),
                        target: Target::Company(task.company()),
                        cells: vec![
                            Cell {
                                text: task.title().to_owned(),
                                kind: CellKind::Text,
                            },
                            Cell {
                                text: company.to_owned(),
                                kind: CellKind::Muted,
                            },
                        ],
                    }
                })
                .collect();
            (
                vec![
                    Column::new("task", "Task").width(px(280.)).sortable(),
                    Column::new("company", "Company").width(px(180.)).sortable(),
                ],
                rows,
                "No tasks yet. Accepting a proposal can add one.".into(),
            )
        }
    }
}

fn company_columns() -> Vec<Column> {
    vec![
        Column::new("name", "Company").width(px(180.)).sortable(),
        Column::new("domain", "Domain").width(px(150.)).sortable(),
        Column::new("stage", "Stage").width(px(120.)).sortable(),
        Column::new("people", "People").width(px(200.)).sortable(),
        Column::new("owner", "Owner").width(px(140.)).sortable(),
        Column::new("last", "Last interaction")
            .width(px(140.))
            .sortable(),
        Column::new("next", "Next interaction")
            .width(px(170.))
            .sortable(),
    ]
}

fn people_columns() -> Vec<Column> {
    vec![
        Column::new("name", "Person").width(px(180.)).sortable(),
        Column::new("title", "Title").width(px(160.)).sortable(),
        Column::new("email", "Email").width(px(220.)).sortable(),
        Column::new("company", "Company").width(px(180.)).sortable(),
    ]
}

fn company_row(store: &Store, company: &Company) -> Row {
    let people = store.people_at(company.id());
    let links: Vec<(Target, String)> = people
        .iter()
        .map(|person| (Target::Person(person.id()), person.name().to_owned()))
        .collect();
    let names = if links.is_empty() {
        "—".to_owned()
    } else {
        links
            .iter()
            .map(|(_, name)| name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    Row {
        id: company.id(),
        target: Target::Company(company.id()),
        cells: vec![
            Cell {
                text: company.name().to_owned(),
                kind: CellKind::Text,
            },
            Cell {
                text: company.domain().to_owned(),
                kind: CellKind::Muted,
            },
            Cell {
                text: company.stage().label().to_owned(),
                kind: CellKind::Stage(company.stage()),
            },
            Cell {
                text: names,
                kind: CellKind::Links(links),
            },
            Cell {
                text: company.owner().to_owned(),
                kind: CellKind::Text,
            },
            Cell {
                text: company.last_interaction().to_owned(),
                kind: CellKind::Muted,
            },
            Cell {
                text: company.next_interaction().to_owned(),
                kind: CellKind::Muted,
            },
        ],
    }
}

fn person_row(store: &Store, person: &Person) -> Row {
    let company = store
        .company(person.company())
        .map(Company::name)
        .unwrap_or("—");
    Row {
        id: person.id(),
        target: Target::Person(person.id()),
        cells: vec![
            Cell {
                text: person.name().to_owned(),
                kind: CellKind::Text,
            },
            Cell {
                text: person.title().to_owned(),
                kind: CellKind::Text,
            },
            Cell {
                text: person.email().to_owned(),
                kind: CellKind::Muted,
            },
            Cell {
                text: company.to_owned(),
                kind: CellKind::Links(vec![(
                    Target::Company(person.company()),
                    company.to_owned(),
                )]),
            },
        ],
    }
}

pub struct Crm {
    focus: FocusHandle,
    store: Store,
    /// `crm.json` and copied files live here when a session directory exists.
    records: Option<PathBuf>,
    shown_source: Option<Id>,
    open_rows: Option<(Id, Vec<MaterialRow>)>,
    object: Object,
    view: View,
    destination: Destination,
    nav: Entity<TreeState>,
    /// Sidebar content width, in rems, not counting the places bump.
    nav_width: f32,
    shelf: Shelf,
    importing: bool,
    reading: Option<(u64, Page)>,
    reading_task: Option<gpui_kit::Task<()>>,
    /// A raster of the PDF currently open, when Quick Look could draw it.
    preview: Option<(PathBuf, std::sync::Arc<gpui_kit::Image>)>,
    /// The Glim reading for the file currently open. Kept so a redraw does not
    /// read the file again.
    glim: Option<(PathBuf, Result<material::Reading, String>)>,
    inbox: Option<InboxPick>,
    screen: Screen,
    selected: Option<Target>,
    recent: Vec<Id>,
    passage: Option<(Id, usize)>,
    suggestion_hidden: bool,
    proposal: Option<Proposal>,
    /// What was typed into the ask field. Shown with the proposal, and kept
    /// out of the insight, which has to stay a quote of the evidence.
    asked: Option<String>,
    ask_note: Option<String>,
    table: Entity<TableState<Rows>>,
    ask: Entity<InputState>,
    note: Entity<InputState>,
    /// Where the places bump's way out goes.
    exit: Exit,
    _table_events: Subscription,
    _opened: Subscription,
    _ask: Subscription,
    _note: Subscription,
    _nav: Subscription,
}

impl Crm {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let records = crate::session::Store::global(cx).map(|store| store.root().join("crm"));
        let store = records
            .as_ref()
            .and_then(|dir| Store::load(dir))
            .unwrap_or_else(Store::sample);
        let shelf = Shelf::load(records.as_deref());
        let destination = shelf
            .rows()
            .iter()
            .find_map(|row| shelf.place_of(row.0).and_then(Destination::parse))
            .unwrap_or(Destination::Inbox);
        let (object, screen) = screen_for(destination, Object::Companies);
        let pending = Rc::new(RefCell::new(None));
        let table = cx.new(|cx| {
            TableState::new(
                Rows::new(pending.clone(), &store, object, View::All),
                window,
                cx,
            )
            .row_header(false)
            .col_selectable(false)
        });
        let nav = cx.new({
            let items = vec![tree_from(&shelf.outline())];
            |cx| TreeState::new(cx).items(items)
        });
        let selected = shelf
            .rows()
            .iter()
            .position(|row| shelf.place_of(row.0) == Some(destination.key()));
        nav.update(cx, |nav, cx| nav.set_selected_index(selected, cx));
        let ask =
            cx.new(|cx| InputState::new(window, cx).placeholder("Summarize from the evidence"));
        let note = cx.new(|cx| InputState::new(window, cx).placeholder("Add a note"));
        let pending_open = pending.clone();
        let table_events = cx.subscribe_in(
            &table,
            window,
            |crm, table, event: &TableEvent, window, cx| {
                crm.on_table_event(table, event, window, cx);
            },
        );
        let opened = cx.observe_in(&table, window, move |crm, _, window, cx| {
            crm.take_pending(pending_open.borrow_mut().take(), window, cx);
        });
        let ask_events = cx.subscribe_in(&ask, window, |crm, _, event: &InputEvent, window, cx| {
            crm.on_ask_enter(event, window, cx);
        });
        let note_events = cx.subscribe_in(
            &note,
            window,
            |crm, note, event: &InputEvent, window, cx| {
                let body = note.read(cx).value().to_string();
                crm.on_note_enter(body, event, window, cx);
            },
        );
        let nav_events = cx.subscribe(&nav, |crm, _, event: &TreeEvent, cx| {
            let (id, expanded) = match event {
                TreeEvent::Expanded(id) => (id, true),
                TreeEvent::Collapsed(id) => (id, false),
            };
            if let Ok(id) = id.parse() {
                crm.shelf.set_expanded(id, expanded);
                crm.persist_shelf();
                cx.notify();
            }
        });
        let nav_width = load_nav_width(records.as_deref());
        Self {
            focus: cx.focus_handle(),
            store,
            records,
            shown_source: None,
            open_rows: None,
            object,
            view: View::All,
            destination,
            nav,
            nav_width,
            shelf,
            importing: false,
            reading: None,
            reading_task: None,
            preview: None,
            glim: None,
            inbox: None,
            screen,
            selected: None,
            recent: Vec::new(),
            passage: None,
            suggestion_hidden: false,
            proposal: None,
            asked: None,
            ask_note: None,
            table,
            ask,
            note,
            _table_events: table_events,
            _opened: opened,
            _ask: ask_events,
            _note: note_events,
            _nav: nav_events,
            exit: Exit::Tabs,
        }
    }

    /// Where the places bump's way out goes: what the place stands over.
    #[cfg(feature = "snapshot")]
    pub(crate) fn show_review_fixture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let company = self.store.companies(View::All)[0].id();
        self.open_target(Target::Company(company), window, cx);
        self.accept_suggestion(cx);
        self.summarize(window, cx);
    }

    pub fn set_exit(&mut self, exit: Exit, cx: &mut Context<Self>) {
        if self.exit != exit {
            self.exit = exit;
            cx.notify();
        }
    }

    fn show_destination(
        &mut self,
        destination: Destination,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.destination = destination;
        let key = destination.key();
        if let Some(ix) = self
            .shelf
            .rows()
            .iter()
            .position(|row| self.shelf.place_of(row.0) == Some(key))
        {
            self.nav
                .update(cx, |nav, cx| nav.set_selected_index(Some(ix), cx));
        }
        match destination {
            Destination::Inbox => {
                self.screen = Screen::Inbox;
                cx.notify();
            }
            Destination::Companies => self.show_object(Object::Companies, window, cx),
            Destination::People => self.show_object(Object::People, window, cx),
            Destination::Tasks => self.show_object(Object::Tasks, window, cx),
            Destination::Highlights => self.show_highlights(window, cx),
        }
    }

    fn set_nav_width(&mut self, rems: f32, cx: &mut Context<Self>) {
        let rems = rems.clamp(NAV_MIN, NAV_MAX);
        if (rems - self.nav_width).abs() < f32::EPSILON {
            return;
        }
        self.nav_width = rems;
        if let Some(dir) = &self.records {
            let _ = std::fs::create_dir_all(dir);
            let _ = std::fs::write(dir.join("nav-width"), rems.to_string());
        }
        cx.notify();
    }

    fn persist_shelf(&self) {
        self.save_records(|dir| self.shelf.save(dir));
    }

    fn save_records(&self, save: impl FnOnce(&Path)) {
        if let Some(dir) = &self.records {
            save(dir);
        }
    }

    fn refresh_tree(&mut self, cx: &mut Context<Self>) {
        let items = vec![tree_from(&self.shelf.outline())];
        self.nav.update(cx, |nav, cx| nav.set_items(items, cx));
        if let Screen::Reading(id) = self.screen
            && self.shelf.is_folder(id)
        {
            self.reading_task = None;
            self.reading = self.shelf.page(id).map(|page| (id, page));
        }
    }

    fn open_shelf_node(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.reading_task = None;
        self.reading = None;
        self.preview = None;
        self.glim = None;
        if let Some(place) = self.shelf.place_of(id).and_then(Destination::parse) {
            self.show_destination(place, window, cx);
            return;
        }
        self.screen = Screen::Reading(id);
        self.select_node(id, cx);
        if self.shelf.is_folder(id) {
            self.reading = self.shelf.page(id).map(|page| (id, page));
            cx.notify();
            return;
        }
        if let Some(read) = self.shelf.reader(id) {
            self.reading_task = Some(cx.spawn(async move |this, cx| {
                let loaded = cx
                    .background_executor()
                    .spawn(async move {
                        let page = read();
                        let preview = match &page {
                            Some(Page::Pdf { path, .. }) => rasterize_pdf(path),
                            _ => None,
                        };
                        let reading = match &page {
                            Some(Page::File { path, .. }) => {
                                Some((path.clone(), material::open(path)))
                            }
                            _ => None,
                        };
                        (page, preview, reading)
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.screen != Screen::Reading(id) || !this.shelf.contains(id) {
                        return;
                    }
                    if let (Some(Page::Pdf { path, .. }), Some(bytes)) = (&loaded.0, loaded.1) {
                        let image = gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, bytes);
                        this.preview = Some((path.clone(), std::sync::Arc::new(image)));
                    }
                    this.reading = loaded.0.map(|page| (id, page));
                    this.glim = loaded.2;
                    cx.notify();
                });
            }));
        }
        cx.notify();
    }

    fn select_node(&mut self, id: u64, cx: &mut Context<Self>) {
        let ix = self.shelf.rows().iter().position(|row| row.0 == id);
        self.nav
            .update(cx, |nav, cx| nav.set_selected_index(ix, cx));
    }

    fn accept_paths(
        &mut self,
        target: u64,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use gpui_kit::component::{WindowExt as _, notification::Notification};
        if self.importing {
            window.push_notification(
                Notification::info(
                    "A file import is still running. Drop these files again when it finishes.",
                ),
                cx,
            );
            return;
        }
        self.importing = true;
        let paths = paths.to_vec();
        let directory = self.shelf.import_directory();
        cx.spawn_in(window, async move |this, cx| {
            let imported = cx
                .background_executor()
                .spawn(async move { super::shelf::Import::read(&paths, directory) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.importing = false;
                match this.shelf.finish_import(target, imported) {
                    Ok(_) => {
                        this.persist_shelf();
                        this.refresh_tree(cx);
                    }
                    Err(error) => {
                        window.push_notification(Notification::error(error.to_string()), cx)
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn create_blank(&mut self, cx: &mut Context<Self>) {
        let parent = self
            .nav
            .read(cx)
            .selected_item()
            .and_then(|item| item.id.parse().ok())
            .filter(|id| self.shelf.is_folder(*id));
        let id = self.shelf.create_file(parent);
        self.persist_shelf();
        self.refresh_tree(cx);
        self.select_node(id, cx);
        self.reading = Some((
            id,
            Page::Markdown {
                title: "Untitled.md".into(),
                source: String::new(),
            },
        ));
        self.reading_task = None;
        self.screen = Screen::Reading(id);
        cx.notify();
    }

    fn delete_node(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.shelf.delete(id) {
            self.persist_shelf();
            self.refresh_tree(cx);
            if let Screen::Reading(shown) = self.screen
                && !self.shelf.contains(shown)
            {
                self.reading_task = None;
                self.reading = None;
                self.screen = Screen::Inbox;
            }
            cx.notify();
        }
    }

    fn inbox_rows(&self) -> Vec<(InboxPick, String, String, String)> {
        let mut rows = Vec::new();
        for task in self.store.tasks() {
            let company = self
                .store
                .company(task.company())
                .map(|company| company.name().to_owned())
                .unwrap_or_default();
            rows.push((
                InboxPick::Task(task.id()),
                task.title().to_owned(),
                company,
                "Task".to_owned(),
            ));
        }
        for company in self.store.companies(View::NeedsFollowUp) {
            let next = company.next_interaction();
            let detail = if next.is_empty() || next == "—" || next == "-" {
                "Needs a follow-up".to_owned()
            } else {
                next.to_owned()
            };
            rows.push((
                InboxPick::Company(company.id()),
                company.name().to_owned(),
                detail,
                company.last_interaction().to_owned(),
            ));
        }
        rows
    }

    fn on_table_event(
        &mut self,
        table: &Entity<TableState<Rows>>,
        event: &TableEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TableEvent::SelectRow(index) => {
                self.selected = table.read(cx).delegate().row(*index).map(|row| row.target);
                cx.notify();
            }
            TableEvent::DoubleClickedRow(index) => {
                if let Some(target) = table.read(cx).delegate().row(*index).map(|row| row.target) {
                    self.open_target(target, window, cx);
                }
            }
            TableEvent::ColumnWidthsChanged(widths) => {
                table.update(cx, |table, _| table.delegate_mut().apply_widths(widths));
            }
            _ => {}
        }
    }

    fn take_pending(
        &mut self,
        pending: Option<Target>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(target) = pending {
            self.open_target(target, window, cx);
        }
    }

    fn on_ask_enter(&mut self, event: &InputEvent, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(event, InputEvent::PressEnter { .. }) {
            self.summarize(window, cx);
        }
    }

    fn on_note_enter(
        &mut self,
        body: String,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::PressEnter { .. }) {
            self.add_note(body, window, cx);
        }
    }

    fn sync_rows(&mut self, cx: &mut Context<Self>) {
        let (columns, mut rows, empty) = project(&self.store, self.object, self.view);
        self.table.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            if delegate.same_columns(&columns) {
                // Projection cells arrive in schema order, while the delegate
                // owns the user's order, widths and sort state.
                for row in &mut rows {
                    row.cells = delegate
                        .columns
                        .iter()
                        .map(|held| {
                            let index = columns
                                .iter()
                                .position(|column| column.key == held.key)
                                .expect("same column membership");
                            row.cells[index].clone()
                        })
                        .collect();
                }
                delegate.set_rows(rows, empty);
            } else {
                delegate.set_projection(columns, rows, empty);
            }
            table.refresh(cx);
        });
    }

    fn show_object(&mut self, object: Object, window: &mut Window, cx: &mut Context<Self>) {
        self.show_list(object, View::All, false, window, cx);
    }

    fn show_view(&mut self, view: View, window: &mut Window, cx: &mut Context<Self>) {
        self.show_list(Object::Companies, view, true, window, cx);
    }

    fn show_list(
        &mut self,
        object: Object,
        view: View,
        keep_selection: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.object = object;
        self.view = view;
        self.screen = Screen::List;
        self.proposal = None;
        self.asked = None;
        self.ask_note = None;
        if !keep_selection {
            self.selected = None;
        }
        self.clear_note(window, cx);
        self.sync_rows(cx);
        window.focus(&self.table.focus_handle(cx), cx);
        cx.notify();
    }

    fn open_target(&mut self, target: Target, window: &mut Window, cx: &mut Context<Self>) {
        self.proposal = None;
        self.asked = None;
        self.ask_note = None;
        self.passage = None;
        self.clear_note(window, cx);
        match target {
            Target::Company(id) if self.store.company(id).is_some() => {
                self.screen = Screen::Company {
                    id,
                    section: Section::Overview,
                };
                self.recent.retain(|held| *held != id);
                self.recent.insert(0, id);
                self.recent.truncate(4);
                self.selected = Some(target);
            }
            Target::Person(id) if self.store.person(id).is_some() => {
                self.screen = Screen::Person { id };
                self.selected = Some(target);
            }
            _ => return,
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn show_highlights(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.screen = Screen::Highlights;
        self.proposal = None;
        self.asked = None;
        self.ask_note = None;
        self.clear_note(window, cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Open the company on the evidence tab with this span selected.
    fn open_span(
        &mut self,
        company: Id,
        source: Id,
        paragraph: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.proposal = None;
        self.asked = None;
        self.ask_note = None;
        self.clear_note(window, cx);
        self.screen = Screen::Company {
            id: company,
            section: Section::Evidence,
        };
        self.load_material(source);
        self.passage = Some((source, paragraph));
        self.recent.retain(|held| *held != company);
        self.recent.insert(0, company);
        self.recent.truncate(4);
        self.selected = Some(Target::Company(company));
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn show_section(&mut self, section: Section, cx: &mut Context<Self>) {
        if let Screen::Company { id, .. } = self.screen {
            self.screen = Screen::Company { id, section };
            if section == Section::Evidence {
                let shown = self.shown_source.filter(|shown| {
                    self.store
                        .sources_at(id)
                        .iter()
                        .any(|source| source.id() == *shown)
                });
                let source =
                    shown.or_else(|| self.store.sources_at(id).last().map(|source| source.id()));
                if let Some(source) = source
                    && !self
                        .open_rows
                        .as_ref()
                        .is_some_and(|(held, _)| *held == source)
                {
                    self.load_material(source);
                }
            }
            cx.notify();
        }
    }

    fn ask_company(&self) -> Option<Id> {
        let store = &self.store;
        company_in_scope(&self.screen, self.selected, |id| {
            store.person(id).map(Person::company)
        })
    }

    fn summarize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(company) = self.ask_company() else {
            self.ask_note = Some(
                if matches!(self.screen, Screen::Highlights) {
                    "Open a highlight first. The summary stays with that company."
                } else {
                    "Select a company first."
                }
                .into(),
            );
            cx.notify();
            return;
        };
        if !matches!(self.screen, Screen::Company { id, .. } if id == company) {
            self.screen = Screen::Company {
                id: company,
                section: Section::Overview,
            };
        }
        let asked = self.ask.read(cx).value().trim().to_owned();
        match self.store.propose(company) {
            Ok(proposal) => {
                self.proposal = Some(proposal);
                self.asked = (!asked.is_empty()).then_some(asked);
                self.ask_note = None;
                self.ask.update(cx, |ask, cx| ask.set_value("", window, cx));
            }
            Err(AskError::NoEvidence) => {
                self.proposal = None;
                self.asked = None;
                self.ask_note =
                    Some("Tag a passage first. A summary has to point at that span.".into());
                self.screen = Screen::Company {
                    id: company,
                    section: Section::Evidence,
                };
            }
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn accept_proposal(&mut self, cx: &mut Context<Self>) {
        let Some(proposal) = self.proposal.take() else {
            return;
        };
        let Some(company) = self.ask_company() else {
            return;
        };
        self.store.accept(company, &proposal);
        self.persist();
        self.screen = Screen::Company {
            id: company,
            section: Section::Activity,
        };
        self.asked = None;
        self.ask_note = None;
        self.sync_rows(cx);
        cx.notify();
    }

    fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.store.undo() {
            return;
        }
        self.persist();
        self.proposal = None;
        self.asked = None;
        self.sync_rows(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn add_note(&mut self, body: String, window: &mut Window, cx: &mut Context<Self>) {
        let Screen::Company { id, .. } = self.screen else {
            return;
        };
        self.store.add_note(id, body);
        self.persist();
        self.clear_note(window, cx);
        cx.notify();
    }

    fn persist(&self) {
        self.save_records(|dir| self.store.save(dir));
    }

    fn attach_file(&mut self, cx: &mut Context<Self>) {
        let Screen::Company { id, .. } = self.screen else {
            return;
        };
        let Some(directory) = self.records.clone() else {
            self.ask_note = Some("File attachments require a saved workspace.".into());
            cx.notify();
            return;
        };
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Attach a file".into()),
        });
        cx.spawn(async move |crm, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let attachment = cx
                .background_executor()
                .spawn(async move { files::prepare(&directory, &path) })
                .await;
            crm.update(cx, |crm, cx| crm.finish_attach(id, attachment, cx))
                .ok();
        })
        .detach();
    }

    fn finish_attach(
        &mut self,
        company: Id,
        attachment: Result<files::Attachment, String>,
        cx: &mut Context<Self>,
    ) {
        let files::Attachment { file, reading } = match attachment {
            Ok(attachment) => attachment,
            Err(error) => {
                self.ask_note = Some(error);
                cx.notify();
                return;
            }
        };
        let Some(source) = self.store.attach(
            company,
            reading.title,
            reading.kind,
            reading.passages,
            Some(file),
        ) else {
            self.ask_note =
                Some("That company is unavailable or the file has nothing to highlight.".into());
            cx.notify();
            return;
        };
        self.shown_source = Some(source);
        self.open_rows = Some((source, reading.rows));
        self.passage = None;
        self.ask_note = None;
        self.screen = Screen::Company {
            id: company,
            section: Section::Evidence,
        };
        self.persist();
        self.sync_rows(cx);
        cx.notify();
    }

    fn show_material(&mut self, source: Id, cx: &mut Context<Self>) {
        self.load_material(source);
        self.passage = None;
        cx.notify();
    }

    fn load_material(&mut self, source: Id) {
        self.shown_source = Some(source);
        self.open_rows = self.stored_file(source).and_then(|path| {
            material::open(&path)
                .ok()
                .map(|reading| (source, reading.rows))
        });
    }

    fn stored_file(&self, source: Id) -> Option<PathBuf> {
        let name = self.store.source(source)?.file()?;
        Some(self.records.as_ref()?.join("files").join(name))
    }

    fn clear_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.note
            .update(cx, |note, cx| note.set_value("", window, cx));
    }

    fn tag_passage(&mut self, theme: &str, cx: &mut Context<Self>) {
        let Some((source, paragraph)) = self.passage else {
            self.ask_note = Some("Select a passage first.".into());
            cx.notify();
            return;
        };
        self.store.tag(source, paragraph, theme);
        self.persist();
        self.ask_note = None;
        cx.notify();
    }

    fn suggestion(&self) -> Option<(Id, usize, &'static str)> {
        if self.suggestion_hidden {
            return None;
        }
        let Screen::Company { id, .. } = self.screen else {
            return None;
        };
        let source = self.store.sources_at(id).into_iter().next()?;
        let paragraph = 1usize;
        let theme = "Buying signal";
        let tagged = self
            .store
            .evidence_for(source.id(), paragraph)
            .is_some_and(|evidence| evidence.themes().iter().any(|held| held == theme));
        (!tagged).then_some((source.id(), paragraph, theme))
    }

    fn accept_suggestion(&mut self, cx: &mut Context<Self>) {
        let Some((source, paragraph, theme)) = self.suggestion() else {
            return;
        };
        self.store.tag(source, paragraph, theme);
        self.persist();
        self.passage = Some((source, paragraph));
        self.ask_note = None;
        cx.notify();
    }

    fn render_nav(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let crm = cx.entity().downgrade();
        let edge = resize_handle::<ResizeNav, ResizeNav>("index-nav-edge", Axis::Horizontal)
            .inside(HandleEdge::Trailing)
            .on_drag(ResizeNav, |drag, _, _, cx| cx.new(|_| drag.clone()))
            .on_double_click(cx.listener(|crm, _, _, cx| crm.set_nav_width(NAV_WIDTH, cx)));
        v_flex()
            .id("crm-nav")
            .debug_selector(|| "crm-nav".into())
            .on_drop(
                cx.listener(|crm, paths: &gpui_kit::ExternalPaths, window, cx| {
                    let root = crm.shelf.root_id();
                    crm.accept_paths(root, paths.paths(), window, cx);
                }),
            )
            .relative()
            .h_full()
            .flex_shrink_0()
            .child(Browser::drag_strip().h(px(TAB_ROW)))
            .child(
                v_flex().flex_1().min_h_0().pl(px(PlacesBump::WIDTH)).child(
                    v_flex()
                        .w(rems(self.nav_width))
                        .flex_1()
                        .min_h_0()
                        .items_start()
                        .child(
                            h_flex()
                                .w_full()
                                .px_2()
                                .pt_2()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .id("index-new-file")
                                        .debug_selector(|| "index-new-file".into())
                                        .p_1()
                                        .child(file_add_icon())
                                        .on_click(cx.listener(|crm, _: &ClickEvent, _, cx| {
                                            crm.create_blank(cx)
                                        })),
                                )
                                .child(
                                    div()
                                        .id("index-delete")
                                        .debug_selector(|| "index-delete".into())
                                        .px_2()
                                        .py_1()
                                        .text_sm()
                                        .child("Delete")
                                        .on_click(cx.listener(|crm, _: &ClickEvent, _, cx| {
                                            let Some(id) = crm
                                                .nav
                                                .read(cx)
                                                .selected_item()
                                                .and_then(|item| item.id.parse().ok())
                                            else {
                                                return;
                                            };
                                            crm.delete_node(id, cx);
                                        })),
                                ),
                        )
                        .child(
                            div().w_full().flex_1().min_h_0().child(
                                tree(&self.nav, {
                                    let rows = self.shelf.rows();
                                    let crm = crm.clone();
                                    move |_, entry, selected, _, _| {
                                        let key = entry.item().id.clone();
                                        let id = key.parse::<u64>().unwrap_or(0);
                                        let row = rows.iter().find(|row| row.0 == id);
                                        let label = entry.item().label.clone();
                                        let depth = entry.depth();
                                        let shape =
                                            row.map(|row| row.3).unwrap_or(NodeShape::Other);
                                        let folder = row.map(|row| row.4).unwrap_or(false);
                                        let open = crm.clone();
                                        let drop_node = crm.clone();
                                        let drop_files = crm.clone();
                                        let row_key = key.clone();
                                        ListItem::new(SharedString::from(format!(
                                            "index-nav-{key}"
                                        )))
                                        .debug_selector(move || format!("index-row-{row_key}"))
                                        .w_full()
                                        .ml(px(14. * depth as f32))
                                        .selected(selected)
                                        .font_weight(if folder {
                                            FontWeight::BOLD
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .child(shelf_icon(shape))
                                        .child(div().text_sm().text_left().child(label))
                                        .on_click(move |_, window, cx| {
                                            let _ = open.update(cx, |crm, cx| {
                                                crm.open_shelf_node(id, window, cx);
                                            });
                                        })
                                        .on_drag(ShelfDrag(id), |drag, _, _, cx| {
                                            cx.new(|_| drag.clone())
                                        })
                                        .on_drop(move |drag: &ShelfDrag, _, cx| {
                                            let _ = drop_node.update(cx, |crm, cx| {
                                                if crm.shelf.drop_node(drag.0, id) {
                                                    crm.persist_shelf();
                                                    crm.refresh_tree(cx);
                                                    cx.notify();
                                                }
                                            });
                                        })
                                        .on_drop(
                                            move |paths: &gpui_kit::ExternalPaths, window, cx| {
                                                let _ = drop_files.update(cx, |crm, cx| {
                                                    crm.accept_paths(id, paths.paths(), window, cx);
                                                });
                                            },
                                        )
                                    }
                                })
                                .context_menu({
                                    let crm = cx.entity().downgrade();
                                    let root = self.shelf.root_id();
                                    move |_, entry, menu, _, _| {
                                        let Ok(id) = entry.item().id.parse::<u64>() else {
                                            return menu;
                                        };
                                        if id == root {
                                            return menu;
                                        }
                                        let up = crm.clone();
                                        let down = crm.clone();
                                        let delete = crm.clone();
                                        menu.item(PopupMenuItem::new("Move up").on_click(
                                            move |_, _, cx| {
                                                let _ = up.update(cx, |crm, cx| {
                                                    if crm.shelf.move_sibling(id, -1) {
                                                        crm.persist_shelf();
                                                        crm.refresh_tree(cx);
                                                    }
                                                });
                                            },
                                        ))
                                        .item(PopupMenuItem::new("Move down").on_click(
                                            move |_, _, cx| {
                                                let _ = down.update(cx, |crm, cx| {
                                                    if crm.shelf.move_sibling(id, 1) {
                                                        crm.persist_shelf();
                                                        crm.refresh_tree(cx);
                                                    }
                                                });
                                            },
                                        ))
                                        .item(
                                            PopupMenuItem::new("Delete").on_click(
                                                move |_, _, cx| {
                                                    let _ = delete.update(cx, |crm, cx| {
                                                        crm.delete_node(id, cx);
                                                    });
                                                },
                                            ),
                                        )
                                    }
                                }),
                            ),
                        ),
                ),
            )
            .child(edge)
    }

    fn render_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let count = self.table.read(cx).delegate().rows_count(cx);
        let count_label = match self.object {
            Object::Companies => tally(count, "company", "companies"),
            Object::People => tally(count, "person", "people"),
            Object::Tasks => tally(count, "task", "tasks"),
        };
        let report = self.store.report();
        let skipped = report.duplicates().len();
        let imported = if skipped == 0 {
            format!(
                "Sample import · {} companies, {} people",
                report.companies_added(),
                report.people_added()
            )
        } else {
            format!(
                "Sample import · {} companies, {} people · {}",
                report.companies_added(),
                report.people_added(),
                tally(skipped, "duplicate skipped", "duplicates skipped")
            )
        };
        let title = if self.object == Object::Companies {
            self.view.label()
        } else {
            self.object.label()
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(
                h_flex()
                    .flex_shrink_0()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .items_center()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.foreground)
                            .child(count_label),
                    )
                    .when(
                        self.object == Object::Companies && self.view != View::All,
                        |bar| {
                            let label = self.view.label();
                            bar.child(
                                Button::new("crm-clear-view")
                                    .label(label)
                                    .small()
                                    .outline()
                                    .selected(true)
                                    .tooltip("Clear this view")
                                    .on_click(cx.listener(|crm, _: &ClickEvent, window, cx| {
                                        crm.show_view(View::All, window, cx);
                                    })),
                            )
                        },
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("crm-open")
                            .label("Open")
                            .ghost()
                            .small()
                            .disabled(self.selected.is_none())
                            .tooltip("Open the selected record")
                            .on_click(cx.listener(|crm, _: &ClickEvent, window, cx| {
                                if let Some(target) = crm.selected {
                                    crm.open_target(target, window, cx);
                                }
                            })),
                    )
                    .when(self.store.can_undo(), |bar| {
                        bar.child(
                            Button::new("crm-undo")
                                .label("Undo")
                                .ghost()
                                .small()
                                .on_click(cx.listener(|crm, _: &ClickEvent, window, cx| {
                                    crm.undo(window, cx);
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .text_color(theme.foreground)
                    .child(imported),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .child(DataTable::new(&self.table).small()),
            )
    }

    fn render_record(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        match self.screen {
            Screen::Company { id, section } => self.render_company(id, section, cx),
            Screen::Person { id } => self.render_person(id, cx),
            Screen::Highlights => self.render_highlights(cx).into_any_element(),
            Screen::List | Screen::Inbox | Screen::Reading(_) => div().into_any_element(),
        }
    }

    fn render_company(
        &mut self,
        id: Id,
        section: Section,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(company) = self.store.company(id) else {
            return div().into_any_element();
        };
        let name = company.name().to_owned();
        let domain = company.domain().to_owned();
        let stage = company.stage();
        let note_count = self.store.notes_at(id).len();
        let task_count = self.store.tasks_at(id).len();
        let evidence_count = self.store.evidence_at(id).len();
        let theme = cx.theme();
        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(
                v_flex()
                    .flex_shrink_0()
                    .px_4()
                    .pt_3()
                    .gap_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Button::new(("crm-back", id.raw()))
                                    .label("Companies")
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|crm, _: &ClickEvent, window, cx| {
                                        crm.show_object(Object::Companies, window, cx);
                                    })),
                            )
                            .child(div().flex_1())
                            .when(self.store.can_undo(), |bar| {
                                bar.child(
                                    Button::new(("crm-undo-record", id.raw()))
                                        .label("Undo")
                                        .ghost()
                                        .small()
                                        .on_click(cx.listener(
                                            |crm, _: &ClickEvent, window, cx| {
                                                crm.undo(window, cx);
                                            },
                                        )),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(name),
                            )
                            .child(stage_tag(stage))
                            .child(div().text_sm().text_color(theme.foreground).child(domain)),
                    )
                    .child(
                        TabBar::new(("crm-sections", id.raw()))
                            .underline()
                            .small()
                            .selected_index(section.index())
                            .on_click(cx.listener(|crm, index: &usize, _, cx| {
                                if let Some(section) = Section::from_index(*index) {
                                    crm.show_section(section, cx);
                                }
                            }))
                            .child(Tab::new().label("Overview"))
                            .child(Tab::new().label("Activity"))
                            .child(Tab::new().label(counted("Notes", note_count)))
                            .child(Tab::new().label(counted("Tasks", task_count)))
                            .child(Tab::new().label(counted("Evidence", evidence_count))),
                    ),
            )
            .child(match section {
                Section::Evidence => self.render_evidence(id, cx).into_any_element(),
                _ => h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_center(id, section, cx))
                    .child(self.render_inspector(id, cx))
                    .into_any_element(),
            })
            .into_any_element()
    }

    fn render_center(
        &mut self,
        id: Id,
        section: Section,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let body = match section {
            Section::Overview => self.render_overview(id, cx).into_any_element(),
            Section::Activity => self.render_activity(id, cx).into_any_element(),
            Section::Notes => self.render_notes(id, cx).into_any_element(),
            Section::Tasks => self.render_tasks(id, cx).into_any_element(),
            Section::Evidence => div().into_any_element(),
        };
        div()
            .id(("crm-center", id.raw()))
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scrollbar()
            .px_4()
            .py_3()
            .child(body)
    }

    fn render_overview(&mut self, id: Id, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let border = theme.border;
        let muted = theme.foreground;
        let radius = theme.radius;
        let insights: Vec<(String, String, usize, Option<Id>)> = self
            .store
            .insights_at(id)
            .iter()
            .map(|insight| {
                (
                    insight.title().to_owned(),
                    insight.body().to_owned(),
                    insight.evidence().len(),
                    insight.evidence().first().copied(),
                )
            })
            .collect();
        let people: Vec<(Id, String, String)> = self
            .store
            .people_at(id)
            .iter()
            .map(|person| {
                (
                    person.id(),
                    person.name().to_owned(),
                    person.title().to_owned(),
                )
            })
            .collect();
        let tasks: Vec<String> = self
            .store
            .tasks_at(id)
            .iter()
            .take(3)
            .map(|task| task.title().to_owned())
            .collect();
        let task_count = self.store.tasks_at(id).len();
        let activity: Vec<String> = self
            .store
            .activity_at(id)
            .iter()
            .take(3)
            .map(|entry| entry.summary().to_owned())
            .collect();
        let evidence = self.store.evidence_at(id).len();
        v_flex()
            .gap_3()
            .when(insights.is_empty(), |page| {
                page.child(div().text_sm().text_color(muted).child(if evidence == 0 {
                    "No insight yet. Tag a passage, then summarize from that evidence."
                } else {
                    "Passages are tagged. Summarize to record an insight that cites them."
                }))
            })
            .children(insights.into_iter().map(|(title, body, spans, first)| {
                v_flex()
                    .gap_1()
                    .p_3()
                    .rounded(radius)
                    .border_1()
                    .border_color(border)
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                    .child(div().text_sm().child(body))
                    .when(spans > 0, |card| {
                        let label = tally(spans, "passage", "passages");
                        card.child(
                            Button::new(("crm-insight", first.map(Id::raw).unwrap_or(0)))
                                .label(label)
                                .ghost()
                                .small()
                                .on_click(cx.listener(move |crm, _: &ClickEvent, window, cx| {
                                    let Some(evidence) =
                                        first.and_then(|id| crm.store.evidence_by_id(id))
                                    else {
                                        return;
                                    };
                                    let company = evidence.company();
                                    let source = evidence.source();
                                    let paragraph = evidence.paragraph();
                                    crm.open_span(company, source, paragraph, window, cx);
                                })),
                        )
                    })
            }))
            .child(self.overview_people(&people, cx))
            .when(task_count > 0, |page| {
                page.child(self.overview_tasks(id, task_count, &tasks, cx))
            })
            .when(!activity.is_empty(), |page| {
                page.child(self.overview_activity(id, &activity, cx))
            })
    }

    fn overview_people(
        &mut self,
        people: &[(Id, String, String)],
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let border = cx.theme().border;
        let radius = cx.theme().radius;
        v_flex()
            .gap_1()
            .p_3()
            .rounded(radius)
            .border_1()
            .border_color(border)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("People"),
            )
            .children(people.iter().cloned().map(|(person, name, title)| {
                Button::new(("crm-person", person.raw()))
                    .label(format!("{name} · {title}"))
                    .ghost()
                    .small()
                    .justify_start()
                    .on_click(cx.listener(move |crm, _: &ClickEvent, window, cx| {
                        crm.open_target(Target::Person(person), window, cx);
                    }))
            }))
    }

    fn overview_tasks(
        &mut self,
        id: Id,
        count: usize,
        titles: &[String],
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let border = cx.theme().border;
        let radius = cx.theme().radius;
        let muted = cx.theme().foreground;
        v_flex()
            .gap_1()
            .p_3()
            .rounded(radius)
            .border_1()
            .border_color(border)
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Tasks"),
                    )
                    .child(
                        Button::new(("crm-open-tasks", id.raw()))
                            .label(tally(count, "task", "tasks"))
                            .ghost()
                            .small()
                            .on_click(cx.listener(|crm, _: &ClickEvent, _, cx| {
                                crm.show_section(Section::Tasks, cx);
                            })),
                    ),
            )
            .children(
                titles
                    .iter()
                    .map(|title| div().text_sm().child(title.clone())),
            )
            .when(count > titles.len(), |card| {
                card.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("{} more", count - titles.len())),
                )
            })
    }

    fn overview_activity(
        &mut self,
        id: Id,
        lines: &[String],
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let border = cx.theme().border;
        let radius = cx.theme().radius;
        v_flex()
            .gap_1()
            .p_3()
            .rounded(radius)
            .border_1()
            .border_color(border)
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Activity"),
                    )
                    .child(
                        Button::new(("crm-open-activity", id.raw()))
                            .label("Activity")
                            .ghost()
                            .small()
                            .on_click(cx.listener(|crm, _: &ClickEvent, _, cx| {
                                crm.show_section(Section::Activity, cx);
                            })),
                    ),
            )
            .children(lines.iter().map(|line| div().text_sm().child(line.clone())))
    }

    fn render_activity(&self, id: Id, cx: &App) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let rows: Vec<(String, String, Option<String>)> = self
            .store
            .activity_at(id)
            .iter()
            .map(|activity| {
                (
                    activity.summary().to_owned(),
                    activity.detail().to_owned(),
                    activity.source().map(str::to_owned),
                )
            })
            .collect();
        v_flex()
            .gap_3()
            .when(!rows.is_empty(), |page| {
                page.child(
                    div()
                        .text_xs()
                        .text_color(theme.foreground)
                        .child("This session"),
                )
            })
            .children(rows.into_iter().map(|(summary, detail, source)| {
                v_flex()
                    .gap_1()
                    .pb_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().text_sm().child(summary))
                    .children(detail.lines().filter(|line| !line.is_empty()).map(|line| {
                        div()
                            .text_xs()
                            .text_color(theme.foreground)
                            .child(line.to_owned())
                    }))
                    .when_some(source, |row, source| {
                        row.child(
                            div()
                                .text_xs()
                                .text_color(theme.foreground)
                                .child(format!("Source · {source}")),
                        )
                    })
            }))
    }

    fn render_notes(&mut self, id: Id, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let mut notes: Vec<String> = self
            .store
            .notes_at(id)
            .iter()
            .map(|note| note.body().to_owned())
            .collect();
        notes.reverse();
        let theme = cx.theme();
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.note).small()),
                    )
                    .child(
                        Button::new(("crm-add-note", id.raw()))
                            .label("Add note")
                            .outline()
                            .small()
                            .on_click(cx.listener(|crm, _: &ClickEvent, window, cx| {
                                let body = crm.note.read(cx).value().to_string();
                                crm.add_note(body, window, cx);
                            })),
                    ),
            )
            .when(notes.is_empty(), |page| {
                page.child(
                    div()
                        .text_sm()
                        .text_color(theme.foreground)
                        .child("No notes yet."),
                )
            })
            .children(notes.into_iter().map(|body| div().text_sm().child(body)))
    }

    fn render_tasks(&mut self, id: Id, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tasks: Vec<(String, Option<Id>)> = self
            .store
            .tasks_at(id)
            .iter()
            .map(|task| (task.title().to_owned(), task.evidence()))
            .collect();
        let muted = cx.theme().foreground;
        v_flex()
            .gap_2()
            .when(tasks.is_empty(), |page| {
                page.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child("No tasks yet. Accept a proposal to record one."),
                )
            })
            .children(tasks.into_iter().map(|(title, evidence)| {
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1().min_w_0().text_sm().child(title))
                    .when_some(evidence, |row, evidence| {
                        row.child(
                            Button::new(("crm-task-passage", evidence.raw()))
                                .label("Open the passage")
                                .ghost()
                                .small()
                                .on_click(cx.listener(move |crm, _: &ClickEvent, window, cx| {
                                    let Some(found) = crm.store.evidence_by_id(evidence) else {
                                        return;
                                    };
                                    let company = found.company();
                                    let source = found.source();
                                    let paragraph = found.paragraph();
                                    crm.open_span(company, source, paragraph, window, cx);
                                })),
                        )
                    })
            }))
    }

    fn render_inspector(&mut self, id: Id, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let Some(company) = self.store.company(id) else {
            return div().into_any_element();
        };
        let stage = company.stage();
        let owner = company.owner().to_owned();
        let employees = company.employees();
        let last = company.last_interaction().to_owned();
        let next = company.next_interaction().to_owned();
        let theme = cx.theme();
        v_flex()
            .id(("crm-inspector", id.raw()))
            .w(px(INSPECTOR))
            .h_full()
            .flex_shrink_0()
            .overflow_y_scrollbar()
            .border_l_1()
            .border_color(theme.border)
            .px_3()
            .py_3()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("Details"),
            )
            .child(self.property("Owner", owner, cx))
            .child(self.property("Employees", employees.to_string(), cx))
            .child(self.property("Last interaction", last, cx))
            .child(self.property("Next interaction", next, cx))
            .child(div().text_xs().text_color(theme.foreground).child("Stage"))
            .child(
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .children(Stage::ALL.into_iter().enumerate().map(|(index, choice)| {
                        Button::new(("crm-stage", id.raw() * 8 + index as u64))
                            .label(choice.label())
                            .small()
                            .ghost()
                            .selected(choice == stage)
                            .on_click(cx.listener(move |crm, _: &ClickEvent, _, cx| {
                                crm.store.set_stage(id, choice);
                                crm.persist();
                                crm.sync_rows(cx);
                                cx.notify();
                            }))
                    })),
            )
            .into_any_element()
    }

    fn property(&self, label: &str, value: String, cx: &App) -> impl IntoElement + use<> {
        h_flex()
            .gap_3()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().foreground)
                    .child(label.to_owned()),
            )
            .child(div().text_sm().child(value))
    }

    fn render_evidence(&mut self, id: Id, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let sources: Vec<(Id, String)> = self
            .store
            .sources_at(id)
            .iter()
            .map(|source| (source.id(), source.title().to_owned()))
            .collect();
        let Some(source_id) = self
            .shown_source
            .filter(|shown| sources.iter().any(|(source, _)| source == shown))
            .or_else(|| sources.last().map(|(source, _)| *source))
        else {
            return self.empty_source(id, cx);
        };
        let Some(source) = self.store.source(source_id) else {
            return self.empty_source(id, cx);
        };
        let title = source.title().to_owned();
        let caption = source.kind().caption();
        let paragraphs = source.paragraphs().to_vec();
        let rows = self
            .open_rows
            .as_ref()
            .filter(|(shown, _)| *shown == source_id)
            .map(|(_, rows)| rows.clone())
            .unwrap_or_else(|| {
                paragraphs
                    .iter()
                    .enumerate()
                    .map(|(index, label)| MaterialRow {
                        depth: 0,
                        label: label.clone(),
                        passage: Some(index),
                    })
                    .collect()
            });
        let evidence: Vec<(usize, Vec<String>)> = self
            .store
            .evidence_at(id)
            .iter()
            .filter(|item| item.source() == source_id)
            .map(|item| (item.paragraph(), item.themes().to_vec()))
            .collect();
        let selected = self.passage;
        let suggestion = self.suggestion().and_then(|(source, paragraph, theme)| {
            (source == source_id).then(|| {
                paragraphs
                    .get(paragraph)
                    .map(|text| (source, paragraph, theme, text.clone()))
            })?
        });
        let muted = cx.theme().foreground;
        let radius = cx.theme().radius;
        let active = cx.theme().list_active;
        let tagged_fill = cx.theme().accent.opacity(0.12);
        v_flex()
            .flex_1()
            .min_h_0()
            .child(self.source_bar(id, source_id, &sources, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .id(("crm-source", source_id.raw()))
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scrollbar()
                            .px_4()
                            .py_3()
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(title),
                                    )
                                    .child(div().text_xs().text_color(muted).child(caption))
                                    .children(rows.into_iter().enumerate().map(|(index, row)| {
                                        let passage = row.passage;
                                        let themes = passage
                                            .and_then(|passage| {
                                                evidence
                                                    .iter()
                                                    .find(|(held, _)| *held == passage)
                                                    .map(|(_, themes)| themes.clone())
                                            })
                                            .unwrap_or_default();
                                        let on = passage.is_some_and(|passage| {
                                            selected == Some((source_id, passage))
                                        });
                                        v_flex()
                                            .id((
                                                "crm-passage",
                                                source_id
                                                    .raw()
                                                    .wrapping_mul(64)
                                                    .wrapping_add(index as u64),
                                            ))
                                            .gap_1()
                                            .pl(px(row.depth as f32 * 12.))
                                            .px_2()
                                            .py_1()
                                            .rounded(radius)
                                            .when(on, |line| line.bg(active))
                                            .when(!on && !themes.is_empty(), |line| {
                                                line.bg(tagged_fill)
                                            })
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .when(passage.is_none(), |line| {
                                                        line.text_color(muted)
                                                    })
                                                    .child(row.label),
                                            )
                                            .when(!themes.is_empty(), |line| {
                                                line.child(h_flex().gap_1().children(
                                                    themes.into_iter().map(|name| {
                                                        Tag::secondary()
                                                            .small()
                                                            .outline()
                                                            .child(name)
                                                    }),
                                                ))
                                            })
                                            .when_some(passage, |line, passage| {
                                                line.on_click(cx.listener(
                                                    move |crm, _: &ClickEvent, _, cx| {
                                                        crm.passage = Some((source_id, passage));
                                                        crm.ask_note = None;
                                                        cx.notify();
                                                    },
                                                ))
                                            })
                                    })),
                            ),
                    )
                    .child(self.render_themes(source_id, suggestion, cx)),
            )
            .into_any_element()
    }

    fn empty_source(&mut self, id: Id, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let muted = cx.theme().foreground;
        v_flex()
            .p_4()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("No source is attached."),
            )
            .child(self.attach_button(id, cx))
            .into_any_element()
    }

    fn source_bar(
        &mut self,
        company: Id,
        shown: Id,
        sources: &[(Id, String)],
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let border = cx.theme().border;
        h_flex()
            .flex_shrink_0()
            .px_3()
            .py_2()
            .gap_1()
            .border_b_1()
            .border_color(border)
            .flex_wrap()
            .children(sources.iter().cloned().map(|(source, title)| {
                Button::new(("crm-source-pick", source.raw()))
                    .label(title)
                    .small()
                    .ghost()
                    .selected(source == shown)
                    .on_click(cx.listener(move |crm, _: &ClickEvent, _, cx| {
                        crm.show_material(source, cx);
                    }))
            }))
            .child(self.attach_button(company, cx))
    }

    fn attach_button(&mut self, company: Id, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        Button::new(("crm-attach", company.raw()))
            .label("Attach file")
            .small()
            .outline()
            .tooltip("JSON, CSV, SQLite, or text")
            .on_click(cx.listener(|crm, _: &ClickEvent, _, cx| {
                crm.attach_file(cx);
            }))
    }

    fn render_themes(
        &mut self,
        source: Id,
        suggestion: Option<(Id, usize, &'static str, String)>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let ready = self.passage.is_some();
        v_flex()
            .id(("crm-themes", source.raw()))
            .w(px(INSPECTOR))
            .h_full()
            .flex_shrink_0()
            .overflow_y_scrollbar()
            .border_l_1()
            .border_color(theme.border)
            .px_3()
            .py_3()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child("Themes"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.foreground)
                    .child(if ready {
                        "Tag the selected passage."
                    } else {
                        "Select a passage, then choose a theme."
                    }),
            )
            .children(THEMES.into_iter().enumerate().map(|(index, name)| {
                Button::new(("crm-theme", source.raw() * 8 + index as u64))
                    .label(name)
                    .small()
                    .w_full()
                    .justify_start()
                    .disabled(!ready)
                    .when(ready, |button| button.outline())
                    .when(!ready, |button| button.ghost())
                    .on_click(cx.listener(move |crm, _: &ClickEvent, _, cx| {
                        crm.tag_passage(name, cx);
                    }))
            }))
            .when_some(suggestion, |rail, (_, paragraph, name, text)| {
                rail.child(
                    v_flex()
                        .gap_2()
                        .pt_2()
                        .border_t_1()
                        .border_color(theme.border)
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child("Suggested highlight"),
                        )
                        .child(div().text_sm().child(text))
                        .child(Tag::info().small().outline().child(name))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new(("crm-suggest-accept", paragraph as u64))
                                        .child(
                                            div()
                                                .debug_selector(|| "crm-suggest-accept".into())
                                                .child("Accept"),
                                        )
                                        .small()
                                        .outline()
                                        .on_click(cx.listener(|crm, _: &ClickEvent, _, cx| {
                                            crm.accept_suggestion(cx);
                                        })),
                                )
                                .child(
                                    Button::new(("crm-suggest-reject", paragraph as u64))
                                        .label("Reject")
                                        .ghost()
                                        .small()
                                        .on_click(cx.listener(|crm, _: &ClickEvent, _, cx| {
                                            crm.suggestion_hidden = true;
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
            })
    }

    fn render_highlights(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let border = cx.theme().border;
        let muted = cx.theme().foreground;
        let radius = cx.theme().radius;
        let spans: Vec<(Id, Id, usize, String, Vec<String>)> = self
            .store
            .evidence()
            .iter()
            .map(|item| {
                (
                    item.company(),
                    item.source(),
                    item.paragraph(),
                    item.text().to_owned(),
                    item.themes().to_vec(),
                )
            })
            .collect();
        let columns: Vec<(String, Vec<(Id, String, Id, String, usize, String)>)> = THEMES
            .iter()
            .map(|label| {
                let cards = spans
                    .iter()
                    .filter(|(_, _, _, _, themes)| themes.iter().any(|theme| theme == label))
                    .filter_map(|(company, source, paragraph, text, _)| {
                        let company_name = self.store.company(*company)?.name().to_owned();
                        let source_title = self.store.source(*source)?.title().to_owned();
                        Some((
                            *company,
                            company_name,
                            *source,
                            source_title,
                            *paragraph,
                            text.clone(),
                        ))
                    })
                    .collect();
                ((*label).to_owned(), cards)
            })
            .collect();
        h_flex()
            .id("crm-highlights-board")
            .flex_1()
            .min_h_0()
            .overflow_x_scrollbar()
            .items_stretch()
            .gap_2()
            .p_3()
            .children(
                columns
                    .into_iter()
                    .enumerate()
                    .map(|(column, (label, cards))| {
                        let count = cards.len();
                        v_flex()
                            .id(("crm-highlight-column", column as u64))
                            .w(px(240.))
                            .h_full()
                            .flex_shrink_0()
                            .rounded(radius)
                            .border_1()
                            .border_color(border)
                            .overflow_y_scrollbar()
                            .child(
                                h_flex()
                                    .px_3()
                                    .py_2()
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(label.clone()),
                                    )
                                    .child(
                                        div().text_xs().text_color(muted).child(count.to_string()),
                                    ),
                            )
                            .when(cards.is_empty(), |column| {
                                column.child(
                                    div()
                                        .px_3()
                                        .text_xs()
                                        .text_color(muted)
                                        .child("No highlights yet."),
                                )
                            })
                            .children(cards.into_iter().map(
                                |(company, company_name, source, source_title, paragraph, text)| {
                                    let key = source
                                        .raw()
                                        .wrapping_mul(32)
                                        .wrapping_add(paragraph as u64);
                                    v_flex()
                                        .id(("crm-highlight", column as u64 * 4096 + key))
                                        .m_2()
                                        .p_2()
                                        .gap_1()
                                        .rounded(radius)
                                        .border_1()
                                        .border_color(border)
                                        .on_click(cx.listener(
                                            move |crm, _: &ClickEvent, window, cx| {
                                                crm.open_span(
                                                    company, source, paragraph, window, cx,
                                                );
                                            },
                                        ))
                                        .child(div().text_sm().child(text))
                                        .child(
                                            div().text_xs().text_color(muted).child(company_name),
                                        )
                                        .child(div().text_xs().text_color(muted).child(format!(
                                            "{source_title}, paragraph {}",
                                            paragraph + 1
                                        )))
                                        .child(
                                            Tag::secondary().small().outline().child(label.clone()),
                                        )
                                },
                            ))
                    }),
            )
    }

    fn render_person(&mut self, id: Id, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(person) = self.store.person(id) else {
            return div().into_any_element();
        };
        let name = person.name().to_owned();
        let title = person.title().to_owned();
        let email = person.email().to_owned();
        let company_id = person.company();
        let company = self
            .store
            .company(company_id)
            .map(|company| company.name().to_owned())
            .unwrap_or_else(|| "Company".into());
        let theme = cx.theme();
        v_flex()
            .flex_1()
            .min_w_0()
            .px_4()
            .py_3()
            .gap_3()
            .child(
                Button::new(("crm-back-people", id.raw()))
                    .label("People")
                    .ghost()
                    .small()
                    .on_click(cx.listener(|crm, _: &ClickEvent, window, cx| {
                        crm.show_object(Object::People, window, cx);
                    })),
            )
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::MEDIUM)
                    .child(name),
            )
            .child(div().text_sm().text_color(theme.foreground).child(title))
            .child(div().text_sm().child(email))
            .child(
                Button::new(("crm-person-company", id.raw()))
                    .label(company)
                    .small()
                    .outline()
                    .on_click(cx.listener(move |crm, _: &ClickEvent, window, cx| {
                        crm.open_target(Target::Company(company_id), window, cx);
                    })),
            )
            .into_any_element()
    }

    fn render_ask(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        v_flex()
            .id("crm-ask-panel")
            .flex_shrink_0()
            .max_h(px(300.))
            .overflow_y_scrollbar()
            .border_t_1()
            .border_color(cx.theme().border)
            .p_3()
            .gap_2()
            .child(div().text_xs().child("Summary from tagged evidence"))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .id("crm-ask-input")
                            .debug_selector(|| "crm-ask-input".into())
                            .flex_1()
                            .child(Input::new(&self.ask).small()),
                    )
                    .child(
                        Button::new("crm-summarize")
                            .child(
                                div()
                                    .debug_selector(|| "crm-summarize".into())
                                    .child("Review summary"),
                            )
                            .small()
                            .outline()
                            .on_click(cx.listener(|crm, _, window, cx| crm.summarize(window, cx))),
                    ),
            )
            .when_some(self.proposal.clone(), |panel, proposal| {
                panel.child(
                    v_flex()
                        .id("crm-proposal")
                        .debug_selector(|| "crm-proposal".into())
                        .gap_2()
                        .text_sm()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    Button::new("crm-accept-proposal")
                                        .child(
                                            div()
                                                .debug_selector(|| "crm-accept-proposal".into())
                                                .child("Accept changes"),
                                        )
                                        .small()
                                        .primary()
                                        .on_click(
                                            cx.listener(|crm, _, _, cx| crm.accept_proposal(cx)),
                                        ),
                                )
                                .child(
                                    Button::new("crm-dismiss-proposal")
                                        .label("Dismiss")
                                        .small()
                                        .ghost()
                                        .on_click(cx.listener(|crm, _, _, cx| {
                                            crm.proposal = None;
                                            crm.asked = None;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .when_some(self.asked.clone(), |panel, asked| panel.child(asked))
                        .child(proposal.summary)
                        .when_some(proposal.stage, |panel, stage| {
                            panel.child(format!("Stage: {}", stage.label()))
                        })
                        .when_some(proposal.next_interaction, |panel, next| {
                            panel.child(format!("Next interaction: {next}"))
                        })
                        .when_some(proposal.task, |panel, task| {
                            panel.child(format!("Task: {task}"))
                        })
                        .child(format!("Insight: {}", proposal.insight_title))
                        .child(proposal.insight_body)
                        .child(format!(
                            "Based on {} tagged passages",
                            proposal.evidence.len()
                        )),
                )
            })
    }

    fn render_reading(&mut self, id: u64, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        match self
            .reading
            .as_ref()
            .filter(|(shown, _)| *shown == id)
            .map(|(_, page)| page.clone())
        {
            Some(Page::Markdown { title, source }) => v_flex()
                .id("index-reading")
                .debug_selector(|| "index-reading".into())
                .size_full()
                .min_h_0()
                .overflow_y_scrollbar()
                .p_4()
                .gap_3()
                .items_start()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(shelf_icon(NodeShape::Markdown))
                        .child(
                            div()
                                .id("index-markdown")
                                .debug_selector(|| "index-markdown".into())
                                .text_xl()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.foreground)
                                .child(title),
                        ),
                )
                .child(
                    TextView::markdown("index-markdown-body", markdown_body(&source))
                        .selectable(true),
                )
                .into_any_element(),
            Some(Page::Pdf { name, path }) => {
                let page =
                    self.preview
                        .as_ref()
                        .filter(|(held, _)| *held == path)
                        .map(|(_, image)| {
                            div()
                                .id("index-pdf-page")
                                .debug_selector(|| "index-pdf-page".into())
                                .child(gpui_kit::img(image.clone()).max_w_full())
                        });
                let drawn = page.is_some();
                v_flex()
                    .id("index-pdf")
                    .debug_selector(|| "index-pdf".into())
                    .size_full()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p_4()
                    .gap_3()
                    .items_start()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(shelf_icon(NodeShape::Pdf))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.foreground)
                                    .child(name),
                            ),
                    )
                    .child(
                        Button::new("index-open-pdf")
                            .label("Open")
                            .small()
                            .on_click({
                                let path = path.clone();
                                move |_, _, _| {
                                    let _ = std::process::Command::new("open").arg(&path).spawn();
                                }
                            }),
                    )
                    .children(page)
                    .when(!drawn, |view| {
                        view.child(
                            div()
                                .text_sm()
                                .child("Open the file to read the rest of it."),
                        )
                    })
                    .into_any_element()
            }
            Some(Page::Folder { name, children }) => v_flex()
                .id("index-folder")
                .debug_selector(|| "index-folder".into())
                .size_full()
                .min_h_0()
                .overflow_y_scrollbar()
                .p_4()
                .gap_2()
                .items_start()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(shelf_icon(NodeShape::Folder {
                            open: true,
                            place: None,
                        }))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.foreground)
                                .child(name),
                        ),
                )
                .children(children.into_iter().map(|name| {
                    div()
                        .pl(px(22.))
                        .text_sm()
                        .text_color(theme.foreground)
                        .child(name)
                }))
                .into_any_element(),
            Some(Page::Error { name, message }) => file_notice(&name, &message),
            Some(Page::File { name, path }) => {
                match self.glim.as_ref().filter(|(held, _)| held == &path) {
                    Some((_, Ok(reading))) => v_flex()
                        .id("index-glim")
                        .debug_selector(|| "index-glim".into())
                        .size_full()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .p_4()
                        .gap_2()
                        .items_start()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(shelf_icon(NodeShape::Other))
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.foreground)
                                        .child(reading.title.clone()),
                                ),
                        )
                        .children(reading.rows.iter().cloned().map(|row| {
                            div()
                                .pl(px(12. * row.depth as f32))
                                .text_sm()
                                .text_color(theme.foreground)
                                .child(row.label)
                        }))
                        .into_any_element(),
                    Some((_, Err(error))) => file_notice(&name, error),
                    None => file_notice(&name, &path.display().to_string()),
                }
            }
            None => file_notice("Opening file", "Reading file contents…"),
        }
    }

    fn render_inbox(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let rows = self.inbox_rows();
        let selected = self
            .inbox
            .filter(|pick| rows.iter().any(|(held, _, _, _)| held == pick))
            .or_else(|| rows.first().map(|(pick, _, _, _)| *pick));
        let list = v_flex()
            .w(px(320.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(theme.border)
            .overflow_y_scrollbar()
            .children(
                rows.iter()
                    .enumerate()
                    .map(|(index, (pick, title, detail, when))| {
                        let pick = *pick;
                        let on = selected == Some(pick);
                        div()
                            .id(("inbox-row", index as u64))
                            .w_full()
                            .px_3()
                            .py_2()
                            .flex()
                            .flex_col()
                            .items_start()
                            .gap_0p5()
                            .when(on, |row| row.bg(theme.list_active))
                            .child(
                                h_flex()
                                    .w_full()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme.foreground)
                                            .child(title.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .text_xs()
                                            .text_color(theme.foreground)
                                            .child(when.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.foreground)
                                    .child(detail.clone()),
                            )
                            .on_click(cx.listener(move |crm, _: &ClickEvent, _, cx| {
                                crm.inbox = Some(pick);
                                cx.notify();
                            }))
                    }),
            );
        let detail = self.render_inbox_detail(selected, cx);
        h_flex()
            .size_full()
            .min_h_0()
            .items_stretch()
            .child(list)
            .child(detail)
    }

    fn render_inbox_detail(
        &mut self,
        selected: Option<InboxPick>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme();
        let Some(pick) = selected else {
            return div()
                .flex_1()
                .p_6()
                .text_sm()
                .text_color(theme.foreground)
                .child("Nothing needs attention.")
                .into_any_element();
        };
        let (title, body, properties, activity) = match pick {
            InboxPick::Task(id) => {
                let task = self.store.tasks().iter().find(|task| task.id() == id);
                let title = task.map(|task| task.title().to_owned()).unwrap_or_default();
                let company_id = task.map(|task| task.company());
                let company = company_id.and_then(|id| self.store.company(id));
                let name = company
                    .map(|company| company.name().to_owned())
                    .unwrap_or_default();
                let properties = vec![
                    ("Kind".to_owned(), "Task".to_owned()),
                    ("Company".to_owned(), name),
                ];
                (
                    title,
                    "Open the company when the task needs the record.".to_owned(),
                    properties,
                    Vec::new(),
                )
            }
            InboxPick::Company(id) => {
                let company = self.store.company(id);
                let title = company
                    .map(|company| company.name().to_owned())
                    .unwrap_or_default();
                let body = company
                    .map(|company| {
                        let next = company.next_interaction();
                        if next.is_empty() || next == "—" || next == "-" {
                            "Needs a follow-up".to_owned()
                        } else {
                            next.to_owned()
                        }
                    })
                    .unwrap_or_default();
                let properties = company
                    .map(|company| {
                        vec![
                            ("Stage".to_owned(), company.stage().label().to_owned()),
                            ("Owner".to_owned(), company.owner().to_owned()),
                            ("Domain".to_owned(), company.domain().to_owned()),
                            ("Last".to_owned(), company.last_interaction().to_owned()),
                        ]
                    })
                    .unwrap_or_default();
                let activity = self
                    .store
                    .activity_at(id)
                    .into_iter()
                    .take(4)
                    .map(|entry| entry.summary().to_owned())
                    .collect();
                (title, body, properties, activity)
            }
        };
        let open = match pick {
            InboxPick::Company(id) => Some(Target::Company(id)),
            InboxPick::Task(id) => self
                .store
                .tasks()
                .iter()
                .find(|task| task.id() == id)
                .map(|task| Target::Company(task.company())),
        };
        h_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .items_stretch()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .p_4()
                    .gap_3()
                    .items_start()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child(title),
                    )
                    .child(div().text_sm().text_color(theme.foreground).child(body))
                    .when_some(open, |column, target| {
                        column.child(
                            Button::new("inbox-open")
                                .label("Open record")
                                .small()
                                .ghost()
                                .on_click(cx.listener(move |crm, _: &ClickEvent, window, cx| {
                                    crm.open_target(target, window, cx);
                                })),
                        )
                    })
                    .children(
                        activity
                            .into_iter()
                            .map(|line| div().text_sm().text_color(theme.foreground).child(line)),
                    ),
            )
            .child(
                v_flex()
                    .w(px(240.))
                    .h_full()
                    .flex_shrink_0()
                    .border_l_1()
                    .border_color(theme.border)
                    .overflow_y_scrollbar()
                    .p_3()
                    .gap_2()
                    .items_start()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.foreground)
                            .child("Properties"),
                    )
                    .children(properties.into_iter().map(|(label, value)| {
                        v_flex()
                            .w_full()
                            .items_start()
                            .gap_0p5()
                            .child(div().text_xs().text_color(theme.foreground).child(label))
                            .child(div().text_sm().text_color(theme.foreground).child(value))
                    })),
            )
            .into_any_element()
    }
}

impl Focusable for Crm {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.screen {
            Screen::List => self.table.focus_handle(cx),
            Screen::Inbox
            | Screen::Reading(_)
            | Screen::Company { .. }
            | Screen::Person { .. }
            | Screen::Highlights => self.focus.clone(),
        }
    }
}

impl Render for Crm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = match self.screen {
            Screen::List => self.render_list(cx).into_any_element(),
            Screen::Inbox => self.render_inbox(cx).into_any_element(),
            Screen::Reading(id) => self.render_reading(id, cx).into_any_element(),
            _ => self.render_record(cx).into_any_element(),
        };
        h_flex()
            .id("crm")
            .debug_selector(|| "crm-shell".into())
            .key_context("Crm")
            .track_focus(&self.focus)
            .on_drag_move(
                cx.listener(|crm, event: &DragMoveEvent<ResizeNav>, window, cx| {
                    let width =
                        event.event.position.x - event.bounds.left() - px(PlacesBump::WIDTH);
                    crm.set_nav_width(f32::from(width) / f32::from(window.rem_size()), cx);
                }),
            )
            .relative()
            .size_full()
            .min_h_0()
            .items_stretch()
            .overflow_hidden()
            .text_color(cx.theme().foreground)
            .child(self.render_nav(cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .pt(px(FRAME))
                    .pr(px(FRAME))
                    .pb(px(FRAME))
                    .child(
                        v_flex()
                            .id("crm-card")
                            .debug_selector(|| "crm-card".into())
                            .relative()
                            .size_full()
                            .min_h_0()
                            .overflow_hidden()
                            .bg(cx.theme().popover)
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded(cx.theme().radius_lg)
                            .when_some(self.ask_note.clone(), |card, note| {
                                card.child(
                                    div()
                                        .id("index-note")
                                        .debug_selector(|| "index-note".into())
                                        .px_3()
                                        .py_2()
                                        .text_sm()
                                        .child(note),
                                )
                            })
                            .child(div().flex_1().min_h_0().overflow_hidden().child(workspace))
                            .when(self.ask_company().is_some(), |card| {
                                card.child(self.render_ask(cx))
                            }),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left_0()
                    .flex()
                    .items_center()
                    .child(
                        PlacesBump::new(Some(Place::Crm))
                            .exit(self.exit)
                            .fill(bump::leading_fill(cx)),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sidebar_starts_at_inbox_and_can_move() {
        let mut order = default_sidebar();
        assert_eq!(order[0], Destination::Inbox);
        assert!(move_sidebar(&mut order, "inbox", 1));
        assert_eq!(order[0], Destination::Companies);
        assert_eq!(order[1], Destination::Inbox);
        assert!(!move_sidebar(&mut order, "missing", 1));
        assert_eq!(load_sidebar(None)[0], Destination::Inbox);
    }

    #[test]
    fn sections_stay_in_nav_order() {
        let sections = [
            Section::Overview,
            Section::Activity,
            Section::Notes,
            Section::Tasks,
            Section::Evidence,
        ];
        for (index, section) in sections.into_iter().enumerate() {
            assert_eq!(section.index(), index);
            assert_eq!(Section::from_index(index), Some(section));
        }
        assert!(Section::from_index(5).is_none());
    }

    #[test]
    fn the_ask_scope_names_the_open_company() {
        let store = Store::sample();
        let acme = store.companies(View::All)[0].id();
        let ada = store.people_at(acme)[0].id();
        let company_of = |id: Id| store.person(id).map(Person::company);
        assert_eq!(
            company_in_scope(
                &Screen::Company {
                    id: acme,
                    section: Section::Overview,
                },
                None,
                company_of,
            ),
            Some(acme)
        );
        assert_eq!(
            company_in_scope(&Screen::Person { id: ada }, None, company_of),
            Some(acme)
        );
        assert_eq!(
            company_in_scope(&Screen::Highlights, None, company_of),
            None
        );
        assert_eq!(
            company_in_scope(&Screen::List, Some(Target::Company(acme)), company_of),
            Some(acme)
        );
        assert_eq!(
            company_in_scope(&Screen::List, Some(Target::Person(ada)), company_of),
            Some(acme)
        );
        assert_eq!(company_in_scope(&Screen::List, None, company_of), None);
        assert_eq!(ask_label(true, Some("Acme")), "Ask about highlights");
        assert_eq!(
            ask_label(false, Some("Acme Robotics")),
            "Ask about Acme Robotics"
        );
        assert_eq!(ask_label(false, None), "Ask about selected records");
    }

    fn row(id: Id, values: &[&str]) -> Row {
        Row {
            id,
            target: Target::Company(id),
            cells: values
                .iter()
                .map(|text| Cell {
                    text: (*text).to_owned(),
                    kind: CellKind::Text,
                })
                .collect(),
        }
    }

    #[test]
    fn rows_sort_numbers_before_names_and_columns_move() {
        assert_eq!(cell_order("2", "10", false), std::cmp::Ordering::Less);
        assert_eq!(cell_order("b", "a", false), std::cmp::Ordering::Greater);
        assert_eq!(cell_order("Ada", "ada", true), std::cmp::Ordering::Equal);
        assert_eq!(cell_order("a", "b", true), std::cmp::Ordering::Greater);

        let mut columns = company_columns();
        let plain = Column::new("note", "Note");
        columns.push(plain);
        apply_column_sort(&mut columns, 1, ColumnSort::Ascending);
        assert_eq!(columns[1].sort, Some(ColumnSort::Ascending));
        assert_eq!(columns[0].sort, Some(ColumnSort::Default));
        assert!(columns.last().unwrap().sort.is_none());

        let acme = Store::sample().companies(View::All)[0].id();
        let mut rows = vec![row(acme, &["10"]), row(acme, &["2"])];
        sort_rows(&mut rows, 0, false);
        assert_eq!(rows[0].cells[0].text, "2");
        sort_rows(&mut rows, 0, true);
        assert_eq!(rows[0].cells[0].text, "10");

        let mut words = vec!["a".to_owned(), "c".to_owned(), "b".to_owned()];
        assert_eq!(reorder(&mut words, 1, 1), None);
        assert_eq!(reorder(&mut words, 9, 0), None);
        assert_eq!(reorder(&mut words, 2, 1), Some(1));
        assert_eq!(words, ["a", "b", "c"]);

        let mut cells = row(acme, &["a", "b"]).cells;
        shift_cell(&mut cells, 9, 0);
        assert_eq!(cells[0].text, "a");
        shift_cell(&mut cells, 0, 1);
        assert_eq!(cells[0].text, "b");
        assert_eq!(cells[1].text, "a");
    }

    #[gpui_kit::test]
    fn returning_to_a_list_preserves_column_order_width_and_sort(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use gpui_kit::component::Root;
        use gpui_kit::{VisualTestContext, size};
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
        });
        let mut held = None;
        let window = cx.open_window(size(px(1400.), px(900.)), |window, cx| {
            let view = cx.new(|cx| Crm::new(window, cx));
            held = Some(view.clone());
            Root::new(view, window, cx)
        });
        let crm = held.unwrap();
        let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
        cx.update(|window, cx| {
            crm.update(cx, |crm, cx| {
                crm.show_object(Object::Companies, window, cx);
                crm.table.update(cx, |table, cx| {
                    let rows = table.delegate_mut();
                    rows.move_column(0, 2, window, cx);
                    rows.columns[2].width = px(321.);
                    rows.perform_sort(2, ColumnSort::Descending, window, cx);
                });
                let before = crm
                    .table
                    .read(cx)
                    .delegate()
                    .rows
                    .iter()
                    .map(|row| {
                        row.cells
                            .iter()
                            .map(|cell| cell.text.clone())
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                let company = crm.store.companies(View::All)[0].id();
                crm.open_target(Target::Company(company), window, cx);
                crm.show_object(Object::Companies, window, cx);
                let rows = crm.table.read(cx).delegate();
                assert_eq!(rows.columns[2].key.as_ref(), "name");
                assert_eq!(rows.columns[2].width, px(321.));
                assert_eq!(rows.columns[2].sort, Some(ColumnSort::Descending));
                let after = rows
                    .rows
                    .iter()
                    .map(|row| {
                        row.cells
                            .iter()
                            .map(|cell| cell.text.clone())
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    after, before,
                    "cells must follow the retained column identities"
                );
            })
        });
    }

    #[gpui_kit::test]
    fn file_reading_cancels_stale_completion_and_refreshes_on_reopen(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use gpui_kit::component::Root;
        use gpui_kit::{VisualTestContext, size};
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.md");
        let second = dir.path().join("second.md");
        std::fs::write(&first, "# First\n").unwrap();
        std::fs::write(&second, "# Second\n").unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
            crate::session::init(dir.path(), cx);
        });
        let mut held = None;
        let window = cx.open_window(size(px(1400.), px(900.)), |window, cx| {
            let view = cx.new(|cx| Crm::new(window, cx));
            held = Some(view.clone());
            Root::new(view, window, cx)
        });
        let crm = held.unwrap();
        let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
        let ids = cx.update(|window, cx| {
            crm.update(cx, |crm, cx| {
                let ids = crm
                    .shelf
                    .drop_paths(crm.shelf.root_id(), &[first, second.clone()]);
                crm.refresh_tree(cx);
                crm.open_shelf_node(ids[0], window, cx);
                crm.open_shelf_node(ids[1], window, cx);
                assert!(crm.reading.is_none(), "reading must leave the UI thread");
                ids
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let crm = crm.read(cx);
            assert!(matches!(&crm.reading, Some((id, Page::Markdown { title, .. })) if *id == ids[1] && title == "Second"));
        });
        std::fs::write(&second, "# Changed\n").unwrap();
        cx.update(|window, cx| crm.update(cx, |crm, cx| crm.open_shelf_node(ids[1], window, cx)));
        cx.run_until_parked();
        cx.update(|window, cx| crm.update(cx, |crm, cx| {
            assert!(matches!(&crm.reading, Some((_, Page::Markdown { title, .. })) if title == "Changed"));
            crm.open_shelf_node(ids[0], window, cx);
            crm.delete_node(ids[0], cx);
        }));
        cx.run_until_parked();
        cx.update(|_, cx| {
            let crm = crm.read(cx);
            assert!(crm.screen == Screen::Inbox);
            assert!(
                crm.reading.is_none(),
                "deleted files must not return from a pending read"
            );
        });
    }

    /// The loop a person takes: open a company, tag the suggested passage,
    /// accept the proposal, and attach a file that comes back from disk.
    #[gpui_kit::test]
    fn opening_a_company_tags_evidence_and_keeps_the_change(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::component::Root;
        use gpui_kit::component::input::InputEvent;
        use gpui_kit::component::table::TableEvent;
        use gpui_kit::{Modifiers, VisualTestContext, px, size};

        let dir = std::env::temp_dir().join(format!("frame-crm-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
            crate::session::init(&dir, cx);
        });
        let mut held = None;
        let window = cx.open_window(size(px(1400.), px(900.)), |window, cx| {
            let view = cx.new(|cx| Crm::new(window, cx));
            held = Some(view.clone());
            Root::new(view, window, cx)
        });
        let crm = held.expect("crm");
        let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();

        let raw = cx.update(|_, cx| crm.read(cx).store.companies(View::All)[0].id().raw());
        let name: &'static str = Box::leak(format!("crm-row-{raw}").into_boxed_str());
        assert!(
            cx.debug_bounds("crm-shell").is_some(),
            "crm shell was not drawn"
        );
        if let Some(at) = cx.debug_bounds(name) {
            cx.simulate_click(at.center(), Modifiers::none());
            cx.run_until_parked();
        } else {
            cx.update(|window, cx| {
                crm.update(cx, |crm, cx| {
                    let id = crm.store.companies(View::All)[0].id();
                    crm.open_target(Target::Company(id), window, cx);
                });
            });
            cx.run_until_parked();
        }

        cx.update(|_, cx| {
            crm.update(cx, |crm, _| {
                assert!(matches!(crm.screen, Screen::Company { .. }));
            })
        });

        for section in [
            Section::Overview,
            Section::Activity,
            Section::Notes,
            Section::Tasks,
            Section::Evidence,
        ] {
            cx.update(|_, cx| crm.update(cx, |crm, cx| crm.show_section(section, cx)));
            cx.run_until_parked();
        }

        let at = cx
            .debug_bounds("crm-suggest-accept")
            .expect("suggestion control");
        cx.simulate_click(at.center(), Modifiers::none());
        cx.run_until_parked();
        let at = cx
            .debug_bounds("crm-ask-input")
            .expect("ask field is mounted");
        cx.simulate_click(at.center(), Modifiers::none());
        cx.simulate_input("Summarize the buying signal");
        let at = cx.debug_bounds("crm-summarize").expect("summary control");
        cx.simulate_click(at.center(), Modifiers::none());
        cx.run_until_parked();
        assert!(
            cx.debug_bounds("crm-proposal").is_some(),
            "proposal is visible"
        );
        cx.update(|_, cx| {
            assert_eq!(
                crm.read(cx).asked.as_deref(),
                Some("Summarize the buying signal")
            );
        });
        let at = cx
            .debug_bounds("crm-accept-proposal")
            .expect("accept control");
        cx.simulate_click(at.center(), Modifiers::none());
        cx.run_until_parked();

        cx.update(|window, cx| {
            crm.update(cx, |crm, cx| {
                assert!(
                    crm.proposal.is_none(),
                    "the visible accept control must handle the click"
                );
                let company = crm.ask_company().unwrap();
                assert_eq!(
                    crm.store.company(company).unwrap().stage(),
                    Stage::Qualified
                );
                crm.on_note_enter(
                    "Call Ada".into(),
                    &InputEvent::PressEnter {
                        secondary: false,
                        shift: false,
                    },
                    window,
                    cx,
                );
                crm.on_ask_enter(&InputEvent::Change, window, cx);
                crm.show_section(Section::Notes, cx);
                crm.show_section(Section::Activity, cx);
            })
        });
        cx.run_until_parked();

        cx.update(|window, cx| {
            crm.update(cx, |crm, cx| {
                crm.show_highlights(window, cx);
                crm.on_ask_enter(
                    &InputEvent::PressEnter {
                        secondary: false,
                        shift: false,
                    },
                    window,
                    cx,
                );
            })
        });
        cx.run_until_parked();

        let companies = cx.update(|_, cx| {
            crm.read(cx)
                .store
                .companies(View::All)
                .iter()
                .map(|company| (company.id(), company.stage()))
                .collect::<Vec<_>>()
        });
        for (id, _) in &companies {
            cx.update(|window, cx| {
                crm.update(cx, |crm, cx| {
                    crm.open_target(Target::Company(*id), window, cx)
                })
            });
            cx.run_until_parked();
        }
        let person = cx.update(|_, cx| crm.read(cx).store.people()[0].id());
        cx.update(|window, cx| {
            crm.update(cx, |crm, cx| {
                crm.open_target(Target::Person(person), window, cx)
            })
        });
        cx.run_until_parked();

        cx.update(|window, cx| {
            crm.update(cx, |crm, cx| {
                crm.show_object(Object::People, window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            crm.update(cx, |crm, cx| crm.show_object(Object::Tasks, window, cx))
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            crm.update(cx, |crm, cx| {
                crm.show_object(Object::Companies, window, cx);
                crm.show_view(View::NeedsFollowUp, window, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|window, cx| crm.update(cx, |crm, cx| crm.show_view(View::All, window, cx)));
        cx.run_until_parked();

        let company = companies[0].0;
        let file = dir.join("buyer.json");
        std::fs::write(&file, "{\"buyer\":\"Ada Chen\"}").unwrap();
        cx.update(|window, cx| {
            crm.update(cx, |crm, cx| {
                let prepared = files::prepare(crm.records.as_ref().unwrap(), &file);
                std::fs::write(&file, "{\"buyer\":\"Changed after snapshot\"}").unwrap();
                crm.finish_attach(company, prepared, cx);
                let source = crm.shown_source.expect("attached");
                crm.load_material(source);
                assert!(crm.stored_file(source).is_some());
                let reopened = material::open(&crm.stored_file(source).unwrap()).unwrap();
                assert_eq!(
                    crm.store.source(source).unwrap().paragraphs(),
                    reopened.passages
                );
                assert!(
                    reopened
                        .passages
                        .iter()
                        .any(|text| text.contains("Ada Chen"))
                );
                assert!(
                    !reopened
                        .passages
                        .iter()
                        .any(|text| text.contains("Changed after snapshot"))
                );
                crm.finish_attach(company, Err("nope".into()), cx);
                assert_eq!(crm.ask_note.as_deref(), Some("nope"));
                crm.finish_attach(
                    company,
                    Ok(files::Attachment {
                        file: "unused".into(),
                        reading: crate::crm::material::Reading {
                            kind: crate::crm::model::MaterialKind::Text,
                            title: "empty".into(),
                            passages: Vec::new(),
                            rows: Vec::new(),
                        },
                    }),
                    cx,
                );
                assert!(crm.ask_note.as_deref().unwrap().contains("nothing"));
                crm.store.set_stage(company, Stage::Pilot);
                crm.show_object(Object::Companies, window, cx);
                let table = crm.table.clone();
                crm.on_table_event(&table, &TableEvent::SelectRow(0), window, cx);
                crm.on_table_event(&table, &TableEvent::DoubleClickedRow(0), window, cx);
                crm.on_table_event(
                    &table,
                    &TableEvent::ColumnWidthsChanged(vec![px(120.)]),
                    window,
                    cx,
                );
                crm.on_table_event(&table, &TableEvent::SelectColumn(0), window, cx);
                crm.take_pending(Some(Target::Company(company)), window, cx);
            })
        });
        // The table only paints the columns in view, so ask it for every cell
        // a person would reach by scrolling, including a relationship with no record.
        for object in [Object::Companies, Object::People, Object::Tasks] {
            cx.update(|window, cx| {
                crm.update(cx, |crm, cx| {
                    crm.show_object(object, window, cx);
                    crm.table.update(cx, |table, cx| {
                        let rows = table.delegate().rows_count(cx);
                        let cols = table.delegate().columns_count(cx);
                        for row in 0..rows {
                            for col in 0..cols {
                                let _ = table.delegate_mut().render_td(row, col, window, cx);
                            }
                        }
                        let _ = table.delegate_mut().render_td(rows + 1, 0, window, cx);
                        let _ = table.delegate_mut().render_td(0, cols + 1, window, cx);
                    });
                });
            });
        }
        cx.update(|window, cx| {
            crm.update(cx, |crm, cx| {
                crm.table.update(cx, |table, cx| {
                    let id = table.delegate().row(0).expect("a row").id;
                    table.delegate_mut().rows.push(Row {
                        id,
                        target: Target::Company(id),
                        cells: vec![Cell {
                            text: String::new(),
                            kind: CellKind::Links(Vec::new()),
                        }],
                    });
                    let row = table.delegate().rows_count(cx) - 1;
                    let _ = table.delegate_mut().render_td(row, 0, window, cx);
                });
            });
        });
        cx.run_until_parked();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Drop real files, a folder, and a zip onto the sidebar and open them.
    #[gpui_kit::test]
    fn dropped_files_show_in_the_sidebar_and_on_the_page_and_persist(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use gpui_kit::component::Root;
        use gpui_kit::{
            ExternalPaths, FileDropEvent, InputEvent as _, Modifiers, VisualTestContext, px, size,
        };

        let dir = std::env::temp_dir().join(format!("frame-index-drop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let session = dir.join("session");
        std::fs::create_dir_all(&session).unwrap();
        let files = dir.join("files");
        std::fs::create_dir_all(files.join("pack").join("inner")).unwrap();
        let md = files.join("hello.md");
        std::fs::write(&md, "# Hello\n\nA paragraph about the record.\n").unwrap();
        std::fs::write(
            files.join("pack").join("inner").join("note.md"),
            "# Nested\n",
        )
        .unwrap();
        let pdf = files.join("brief.pdf");
        std::fs::write(&pdf, tiny_pdf()).unwrap();
        let table = files.join("people.csv");
        std::fs::write(&table, "name,role\nAda,Buyer\n").unwrap();
        let extra = files.join("extra.txt");
        std::fs::write(&extra, "A note to nest.\n").unwrap();
        let archive = zip_of(
            dir.join("zip-src"),
            "inside.md",
            b"# Inside\n\nFrom the archive.\n",
        );

        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::init(cx);
            crate::session::init(&session, cx);
        });
        let mut held = None;
        let window = cx.open_window(size(px(1400.), px(900.)), |window, cx| {
            let view = cx.new(|cx| Crm::new(window, cx));
            held = Some(view.clone());
            Root::new(view, window, cx)
        });
        let crm = held.expect("crm");
        let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();

        let root = cx.update(|_, cx| crm.read(cx).shelf.root_id());
        let index = leaked(format!("index-row-{root}"));
        let at = cx.debug_bounds(index).expect("the Index folder").center();
        cx.update(|window, cx| {
            window.dispatch_event(
                FileDropEvent::Entered {
                    position: at,
                    paths: ExternalPaths(
                        [
                            md.clone(),
                            files.join("pack"),
                            pdf.clone(),
                            archive.clone(),
                            table.clone(),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                FileDropEvent::Submit { position: at }.to_platform_input(),
                cx,
            );
            window.dispatch_event(FileDropEvent::Exited.to_platform_input(), cx);
            window.dispatch_event(FileDropEvent::Ended.to_platform_input(), cx);
        });
        cx.run_until_parked();

        let records = session.join("crm");
        let (hello, pack, inner, brief, people, companies) = cx.update(|_, cx| {
            let shelf = &crm.read(cx).shelf;
            let rows = shelf.rows();
            assert!(
                rows.iter().any(|row| row.1 == "hello.md"),
                "markdown is in the sidebar"
            );
            assert!(
                rows.iter().any(|row| row.1 == "people.csv"),
                "the table is in the sidebar"
            );
            assert!(
                rows.iter().any(|row| row.1 == "pack" && row.4),
                "the folder is in the sidebar"
            );
            assert!(
                rows.iter().any(|row| row.1 == "inside.md"),
                "the zip opened into its file"
            );
            let hello = rows.iter().find(|row| row.1 == "hello.md").unwrap().0;
            let pack = rows.iter().find(|row| row.1 == "pack").unwrap().0;
            let inner = rows.iter().find(|row| row.1 == "inner").unwrap().0;
            let brief = rows.iter().find(|row| row.1 == "brief.pdf").unwrap().0;
            let people = rows.iter().find(|row| row.1 == "people.csv").unwrap().0;
            let companies = rows.iter().find(|row| row.1 == "Companies").unwrap().0;
            assert!(
                rows.iter().find(|row| row.0 == inner).unwrap().2
                    > rows.iter().find(|row| row.0 == pack).unwrap().2
            );
            (hello, pack, inner, brief, people, companies)
        });
        let pack_bounds = cx
            .debug_bounds(leaked(format!("index-row-{pack}")))
            .expect("pack row");
        let inner_bounds = cx
            .debug_bounds(leaked(format!("index-row-{inner}")))
            .expect("nested folder row");
        assert!(
            inner_bounds.left() > pack_bounds.left(),
            "a nested folder is inset: {inner_bounds:?} inside {pack_bounds:?}"
        );

        let hello_at = cx
            .debug_bounds(leaked(format!("index-row-{hello}")))
            .expect("hello row")
            .center();
        cx.simulate_click(hello_at, Modifiers::none());
        cx.run_until_parked();
        let card = cx.debug_bounds("crm-card").expect("card");
        let markdown = cx
            .debug_bounds("index-markdown")
            .expect("formatted markdown");
        assert!(
            markdown.left() >= card.left() && markdown.top() >= card.top(),
            "the note opens on the main page"
        );
        cx.update(|_, cx| {
            crm.update(cx, |crm, _| match crm.shelf.page(hello).unwrap() {
                Page::Markdown { title, source } => {
                    assert_eq!(title, "Hello");
                    assert!(source.starts_with("# Hello"));
                    assert!(!title.contains('#'));
                }
                other => panic!("markdown opened as {other:?}"),
            })
        });

        let brief_at = cx
            .debug_bounds(leaked(format!("index-row-{brief}")))
            .expect("pdf row")
            .center();
        cx.simulate_click(brief_at, Modifiers::none());
        cx.run_until_parked();
        let pdf_view = cx.debug_bounds("index-pdf").expect("the original pdf");
        assert!(pdf_view.left() >= card.left() && pdf_view.top() >= card.top());
        assert!(
            cx.debug_bounds("index-pdf-page").is_some(),
            "the pdf page is drawn, not a text extraction"
        );
        cx.update(|_, cx| {
            crm.update(cx, |crm, _| match crm.shelf.page(brief).unwrap() {
                Page::Pdf { name, path } => {
                    assert_eq!(name, "brief.pdf");
                    assert_eq!(path, pdf);
                    let bytes = std::fs::read(&path).unwrap();
                    assert!(bytes.starts_with(b"%PDF"), "the pdf stays a pdf");
                }
                other => panic!("pdf opened as {other:?}"),
            })
        });

        let people_at = cx
            .debug_bounds(leaked(format!("index-row-{people}")))
            .expect("table row")
            .center();
        cx.simulate_click(people_at, Modifiers::none());
        cx.run_until_parked();
        let glim = cx.debug_bounds("index-glim").expect("glim reading");
        assert!(glim.left() >= card.left() && glim.top() >= card.top());
        let reading = material::open(&table).unwrap();
        assert_eq!(
            reading.passages,
            vec!["name · role".to_owned(), "Ada · Buyer".to_owned()]
        );

        let companies_at = cx
            .debug_bounds(leaked(format!("index-row-{companies}")))
            .expect("companies file")
            .center();
        cx.update(|window, cx| {
            window.dispatch_event(
                FileDropEvent::Entered {
                    position: companies_at,
                    paths: ExternalPaths([extra.clone()].into_iter().collect()),
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                FileDropEvent::Submit {
                    position: companies_at,
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(FileDropEvent::Exited.to_platform_input(), cx);
            window.dispatch_event(FileDropEvent::Ended.to_platform_input(), cx);
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            let shelf = &crm.read(cx).shelf;
            assert!(
                shelf.is_folder(companies),
                "dropping a file onto a file makes a folder"
            );
            let rows = shelf.rows();
            let nested = rows.iter().find(|row| row.1 == "extra.txt").unwrap();
            let parent = rows.iter().find(|row| row.0 == companies).unwrap();
            assert!(nested.2 > parent.2);
        });

        let new_file = cx
            .debug_bounds("index-new-file")
            .expect("new file")
            .center();
        cx.simulate_click(new_file, Modifiers::none());
        cx.run_until_parked();
        let created = Shelf::load(Some(&records));
        assert!(
            created.rows().iter().any(|row| row.1 == "Untitled.md"),
            "the blank file was saved"
        );
        assert!(created.rows().iter().any(|row| row.1 == "hello.md"));

        let delete = cx.debug_bounds("index-delete").expect("delete").center();
        cx.simulate_click(delete, Modifiers::none());
        cx.run_until_parked();
        let deleted = Shelf::load(Some(&records));
        assert!(
            !deleted.rows().iter().any(|row| row.1 == "Untitled.md"),
            "the delete was saved"
        );
        assert!(deleted.rows().iter().any(|row| row.1 == "brief.pdf"));

        let hello_at = cx
            .debug_bounds(leaked(format!("index-row-{hello}")))
            .expect("hello still listed")
            .center();
        cx.simulate_click(hello_at, Modifiers::none());
        cx.run_until_parked();
        let delete = cx.debug_bounds("index-delete").expect("delete").center();
        cx.simulate_click(delete, Modifiers::none());
        cx.run_until_parked();
        let deleted = Shelf::load(Some(&records));
        assert!(!deleted.rows().iter().any(|row| row.1 == "hello.md"));
        assert!(deleted.rows().iter().any(|row| row.1 == "brief.pdf"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn leaked(name: String) -> &'static str {
        Box::leak(name.into_boxed_str())
    }

    fn tiny_pdf() -> Vec<u8> {
        let stream = b"BT /F1 24 Tf 40 80 Td (Hello from PDF) Tj ET";
        let objects = [
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 144] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_vec(),
            format!("<< /Length {} >>\nstream\n{}\nendstream", stream.len(), std::str::from_utf8(stream).unwrap()).into_bytes(),
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n", index + 1).as_bytes());
            out.extend(object);
            out.extend(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
        out.extend(b"0000000000 65535 f \n");
        for offset in offsets {
            out.extend(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    fn zip_of(dir: std::path::PathBuf, name: &str, data: &[u8]) -> std::path::PathBuf {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), data).unwrap();
        let archive = dir.join("pack.zip");
        let zipped = std::process::Command::new("/usr/bin/zip")
            .current_dir(&dir)
            .args(["-X", archive.file_name().unwrap().to_str().unwrap(), name])
            .status()
            .ok()
            .is_some_and(|status| status.success());
        if zipped {
            return archive;
        }
        std::fs::write(&archive, stored_zip(name, data)).unwrap();
        archive
    }

    fn stored_zip(name: &str, data: &[u8]) -> Vec<u8> {
        let name = name.as_bytes();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"PK\x03\x04");
        bytes.extend_from_slice(&20u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(data);
        bytes
    }
}
