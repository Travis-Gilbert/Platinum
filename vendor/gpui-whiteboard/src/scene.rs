//! The persisted scene model: [`Scene`] / [`Element`] / [`ElementKind`] and their
//! geometry payloads, rich-text style spans, [`Camera`], and the local-thumbnail
//! spec — the JSON-(de)serialized layer, split from `lib.rs`.

use super::*;

/// The board document: everything persisted for a whiteboard. Owned and
/// (de)serialized here; the host stores [`Scene::to_json`] opaquely (for Zorite,
/// in the `content` column of a `kind = 'whiteboard'` page).
///
/// Every field is `#[serde(default)]` so older boards keep loading as the model
/// grows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    /// The viewport (pan + zoom). Persisted so a board reopens where you left it.
    #[serde(default)]
    pub camera: Camera,
    /// The board's content, painted in z-order.
    #[serde(default)]
    pub elements: Vec<Element>,
}

impl Scene {
    /// Parse a board from its stored JSON, falling back to an empty board on
    /// empty or malformed input — a corrupt row never blocks opening the tab.
    ///
    /// Upstream behavior, kept for compatibility. It is **lossy**: a document
    /// that fails to parse, or an element that fails to deserialize, vanishes,
    /// and the next save writes the emptier board over the original. Hosts that
    /// must not destroy content use [`Scene::parse`].
    #[deprecated(
        note = "lossy: bad JSON or a bad element is silently dropped and the next save overwrites it; use `Scene::parse`, which retains what it cannot read"
    )]
    pub fn from_json(s: &str) -> Self {
        let mut scene: Self = if s.trim().is_empty() {
            Self::default()
        } else {
            serde_json::from_str(s).unwrap_or_else(|e| {
                log::warn!("whiteboard: ignoring bad scene JSON ({e}); starting empty");
                Self::default()
            })
        };
        scene.sanitize_camera();
        scene
    }

    /// Recoverable parse: read a board from its stored JSON without ever
    /// discarding what it cannot understand.
    ///
    /// - Empty / whitespace-only input is a new, empty board.
    /// - Each element is deserialized on its own. One that fails (unknown kind,
    ///   missing field, bad value) is kept verbatim in
    ///   [`ParsedScene::retained`] with its original position and the error,
    ///   and the rest of the board loads normally.
    /// - A document that is not valid JSON, or is not a scene object, is an
    ///   `Err` carrying the original text ([`SceneParseError::original_text`])
    ///   so the host can keep it instead of opening an empty board over it.
    ///
    /// Write the board back with [`Scene::to_json_with_retained`] so retained
    /// elements survive the round trip in their original positions.
    pub fn parse(json: &str) -> Result<ParsedScene, SceneParseError> {
        if json.trim().is_empty() {
            return Ok(ParsedScene::default());
        }
        let root: serde_json::Value =
            serde_json::from_str(json).map_err(|e| SceneParseError::Syntax {
                message: e.to_string(),
                line: e.line(),
                column: e.column(),
                source: json.to_string(),
            })?;
        let serde_json::Value::Object(mut map) = root else {
            return Err(SceneParseError::Shape {
                message: "a scene document must be a JSON object".to_string(),
                source: json.to_string(),
            });
        };

        let mut camera_error = None;
        let camera = match map.remove("camera") {
            None => Camera::default(),
            Some(v) => serde_json::from_value::<Camera>(v).unwrap_or_else(|e| {
                camera_error = Some(e.to_string());
                Camera::default()
            }),
        };

        let items = match map.remove("elements") {
            None => Vec::new(),
            Some(serde_json::Value::Array(items)) => items,
            Some(_) => {
                return Err(SceneParseError::Shape {
                    message: "`elements` must be an array".to_string(),
                    source: json.to_string(),
                });
            }
        };
        // The verbatim source text of every element, so a retained one is written
        // back byte for byte (a `Value` re-serializes with sorted keys and
        // normalized numbers). Skipped, not fatal, if the scan ever disagrees
        // with the parse; the `Value` is then the fallback.
        let spans = element_spans(json).filter(|spans| spans.len() == items.len());

        let mut scene = Scene {
            camera,
            elements: Vec::with_capacity(items.len()),
        };
        let mut retained = Vec::new();
        for (index, raw) in items.into_iter().enumerate() {
            match serde_json::from_value::<Element>(raw.clone()) {
                Ok(element) => scene.elements.push(element),
                Err(e) => {
                    log::warn!("whiteboard: retaining unreadable element {index} ({e})");
                    retained.push(RetainedElement {
                        index,
                        raw,
                        error: e.to_string(),
                        source: spans
                            .as_ref()
                            .and_then(|spans| spans.get(index))
                            .and_then(|span| json.get(span.clone()))
                            .map(str::to_string),
                    });
                }
            }
        }
        scene.sanitize_camera();
        Ok(ParsedScene {
            scene,
            retained,
            camera_error,
        })
    }

    /// A zero / negative / non-finite zoom (hand-edited JSON) would break the
    /// world<->screen math; reset it.
    fn sanitize_camera(&mut self) {
        if !self.camera.zoom.is_finite() || self.camera.zoom <= 0.0 {
            self.camera.zoom = 1.0;
        }
    }

    /// Serialize for persistence.
    ///
    /// Drops any [`RetainedElement`]s the board was loaded with; a host that
    /// loaded through [`Scene::parse`] persists with
    /// [`Scene::to_json_with_retained`] (or [`WhiteboardView::to_json`]).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Serialize for persistence, writing each retained element back at its
    /// original position and byte for byte.
    ///
    /// Positions: the output holds every readable and every retained element;
    /// a retained element lands at its original `index` whenever enough
    /// elements precede it, else after the last readable one (elements deleted
    /// since the load pull later retained ones toward the front, never past the
    /// end), and retained elements keep their relative order. With nothing
    /// retained the output equals [`Scene::to_json`].
    pub fn to_json_with_retained(&self, retained: &[RetainedElement]) -> String {
        if retained.is_empty() {
            return self.to_json();
        }
        let mut order: Vec<&RetainedElement> = retained.iter().collect();
        order.sort_by_key(|r| r.index);

        let total = self.elements.len() + order.len();
        let mut parts: Vec<String> = Vec::with_capacity(total);
        let mut valid = self.elements.iter();
        let mut pending = order.into_iter().peekable();
        for slot in 0..total {
            let next_is_retained = match (pending.peek(), valid.len()) {
                (Some(_), 0) => true,
                (Some(r), _) => r.index <= slot,
                (None, _) => false,
            };
            if next_is_retained {
                if let Some(r) = pending.next() {
                    parts.push(r.json());
                }
            } else if let Some(element) = valid.next() {
                match serde_json::to_string(element) {
                    Ok(s) => parts.push(s),
                    Err(e) => log::warn!("whiteboard: element {} not serialized ({e})", element.id),
                }
            }
        }
        let camera = serde_json::to_string(&self.camera).unwrap_or_else(|_| "{}".to_string());
        format!(r#"{{"camera":{camera},"elements":[{}]}}"#, parts.join(","))
    }
}

/// The result of [`Scene::parse`]: the board that loaded, plus everything that
/// could not be read and so was kept aside rather than dropped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParsedScene {
    /// The elements that deserialized, in their original relative order.
    pub scene: Scene,
    /// Elements that failed to deserialize, kept verbatim. Non-empty means the
    /// board opened but is not the whole document; surface it to the user and
    /// persist with [`Scene::to_json_with_retained`].
    pub retained: Vec<RetainedElement>,
    /// Why the `camera` field was replaced by the default, if it was. The camera
    /// is view state rather than content, so this is informational.
    pub camera_error: Option<String>,
}

/// One element that [`Scene::parse`] could not deserialize, kept so a save does
/// not destroy it (a newer writer's element kind, a hand edit, a damaged row).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RetainedElement {
    /// Position in the original `elements` array.
    pub index: usize,
    /// The element as parsed JSON, for inspection.
    pub raw: serde_json::Value,
    /// The deserialization error that retained it.
    pub error: String,
    /// The element's exact source text, when the parse could locate it. This
    /// is what [`Scene::to_json_with_retained`] writes back (so key order,
    /// number spelling, and whitespace survive); `raw` is the fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl RetainedElement {
    /// The element's `id` if it carries a numeric one. A view reserves it so a
    /// newly drawn element never collides with a retained one.
    pub fn id(&self) -> Option<u64> {
        self.raw.get("id").and_then(serde_json::Value::as_u64)
    }

    fn json(&self) -> String {
        self.source
            .clone()
            .unwrap_or_else(|| serde_json::to_string(&self.raw).unwrap_or_else(|_| "null".into()))
    }
}

/// Why [`Scene::parse`] refused a whole document. Both variants carry the
/// original text, so the host can keep the row as it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneParseError {
    /// Not valid JSON.
    Syntax {
        message: String,
        line: usize,
        column: usize,
        source: String,
    },
    /// Valid JSON that is not a scene (not an object, or `elements` is not an
    /// array).
    Shape { message: String, source: String },
}

impl SceneParseError {
    /// The document exactly as it was handed to [`Scene::parse`].
    pub fn original_text(&self) -> &str {
        match self {
            Self::Syntax { source, .. } | Self::Shape { source, .. } => source,
        }
    }
}

impl std::fmt::Display for SceneParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax {
                message,
                line,
                column,
                ..
            } => write!(
                f,
                "whiteboard scene is not valid JSON ({message}, line {line}, column {column})"
            ),
            Self::Shape { message, .. } => write!(f, "whiteboard scene is malformed ({message})"),
        }
    }
}

impl std::error::Error for SceneParseError {}

/// Byte ranges of each element of the top-level `elements` array in `json`
/// (already known to be valid JSON), or `None` if the document has no such
/// array. Strings and nesting are tracked, so a `,` or `]` inside an element
/// never ends it early.
fn element_spans(json: &str) -> Option<Vec<std::ops::Range<usize>>> {
    let b = json.as_bytes();
    let ws = |mut i: usize| {
        while matches!(b.get(i), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            i += 1;
        }
        i
    };
    let string_end = |mut i: usize| -> Option<usize> {
        // `i` is at the opening quote.
        i += 1;
        loop {
            match b.get(i)? {
                b'\\' => i += 2,
                b'"' => return Some(i + 1),
                _ => i += 1,
            }
        }
    };
    let value_end = |i: usize| -> Option<usize> {
        match b.get(i)? {
            b'"' => string_end(i),
            b'{' | b'[' => {
                let mut depth = 0usize;
                let mut j = i;
                loop {
                    match b.get(j)? {
                        b'"' => {
                            j = string_end(j)?;
                            continue;
                        }
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => {
                            depth = depth.checked_sub(1)?;
                            if depth == 0 {
                                return Some(j + 1);
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
            }
            _ => {
                let mut j = i;
                while !matches!(
                    b.get(j),
                    None | Some(b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n')
                ) {
                    j += 1;
                }
                Some(j)
            }
        }
    };

    // The root object's members; the last `elements` key wins, as in serde.
    let mut i = ws(0);
    if *b.get(i)? != b'{' {
        return None;
    }
    i += 1;
    let mut array = None;
    loop {
        i = ws(i);
        match b.get(i)? {
            b'}' => break,
            b'"' => {}
            _ => return None,
        }
        let key_end = string_end(i)?;
        let key: String = serde_json::from_str(json.get(i..key_end)?).ok()?;
        i = ws(key_end);
        if *b.get(i)? != b':' {
            return None;
        }
        i = ws(i + 1);
        let end = value_end(i)?;
        if key == "elements" {
            array = Some(i..end);
        }
        i = ws(end);
        match b.get(i)? {
            b',' => i += 1,
            b'}' => break,
            _ => return None,
        }
    }

    let array = array?;
    if *b.get(array.start)? != b'[' {
        return None;
    }
    let mut spans = Vec::new();
    let mut i = array.start + 1;
    loop {
        i = ws(i);
        if *b.get(i)? == b']' {
            break;
        }
        let end = value_end(i)?;
        spans.push(i..end);
        i = ws(end);
        match b.get(i)? {
            b',' => i += 1,
            b']' => break,
            _ => return None,
        }
    }
    Some(spans)
}

/// One board element: a stable id plus its geometry/kind.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub id: u64,
    pub kind: ElementKind,
    /// Stroke / ink color, packed `0xRRGGBBAA`. `None` follows the theme ink, so
    /// uncolored elements still adapt to light/dark. Absent in older boards → 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<u32>,
    /// Fill color for closed shapes (rect/ellipse), packed `0xRRGGBBAA`. `None`
    /// is an unfilled outline. Ignored by other kinds. Absent in older boards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<u32>,
    /// Centered text label for closed shapes (rect / ellipse / …). Rendered at a
    /// font size auto-shrunk so the wrapped text never crosses the shape's
    /// border (see the shape paint path). `None` / empty = no label; ignored by
    /// non-shape kinds. Absent in older boards → `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Color of the shape's [`label`](Self::label), packed `0xRRGGBBAA`. `None`
    /// inks it with the shape's stroke (theme ink if that's unset too). Ignored by
    /// non-shape kinds. Absent in older boards → `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_color: Option<u32>,
    /// Rich-text formatting runs over the element's text (a Text element's
    /// content, or a shape's label): bold / italic / underline / strikethrough /
    /// highlight, each over a byte range. Empty = unstyled. Kept sorted +
    /// non-overlapping and maintained across edits; absent in older boards.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub styles: Vec<StyleSpan>,
    /// Optional whiteboard-native mind-map metadata. Only mind-map nodes carry
    /// this; lines stay regular anchored arrows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mindmap: Option<MindMapNodeMeta>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MindMapSide {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MindMapRootDirection {
    #[default]
    Both,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MindMapConnectorStyle {
    Straight,
    #[default]
    Bezier,
    Orthogonal,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MindMapNodeMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u64>,
    pub side: MindMapSide,
    #[serde(default)]
    pub order: usize,
    #[serde(default)]
    pub root_direction: MindMapRootDirection,
    #[serde(default)]
    pub connector_style: MindMapConnectorStyle,
}

/// Whether a [`RunStyle`] flag is unset — its default, kept out of the JSON.
fn is_false(b: &bool) -> bool {
    !*b
}

/// The formatting of a run of characters; `default()` is plain text.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct RunStyle {
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub strike: bool,
    /// Highlight color behind the glyphs, packed `0xRRGGBBAA`; `None` = none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<u32>,
}

impl RunStyle {
    /// No formatting — needn't be stored (plain runs are implicit gaps).
    fn is_plain(&self) -> bool {
        *self == RunStyle::default()
    }
}

/// A toggleable boolean format. (Highlight is a color, toggled on its own.)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Bold,
    Italic,
    Underline,
    Strike,
}

impl Format {
    pub(crate) fn get(self, s: &RunStyle) -> bool {
        match self {
            Format::Bold => s.bold,
            Format::Italic => s.italic,
            Format::Underline => s.underline,
            Format::Strike => s.strike,
        }
    }
    pub(crate) fn set(self, s: &mut RunStyle, on: bool) {
        match self {
            Format::Bold => s.bold = on,
            Format::Italic => s.italic = on,
            Format::Underline => s.underline = on,
            Format::Strike => s.strike = on,
        }
    }
}

/// A [`RunStyle`] over the byte range `[start, end)` of an element's text. Runs
/// are kept sorted, non-overlapping, and non-plain. See [`Element::styles`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StyleSpan {
    pub start: usize,
    pub end: usize,
    pub style: RunStyle,
}

/// `[s, e)` as contiguous `(start, end, style)` segments — existing runs plus
/// plain gaps. Assumes `spans` is sorted + non-overlapping.
fn style_segments(spans: &[StyleSpan], s: usize, e: usize) -> Vec<(usize, usize, RunStyle)> {
    let mut out = Vec::new();
    let mut pos = s;
    for sp in spans.iter().filter(|sp| sp.start < e && sp.end > s) {
        let a = sp.start.max(s);
        if a > pos {
            out.push((pos, a, RunStyle::default()));
        }
        out.push((a, sp.end.min(e), sp.style));
        pos = sp.end.min(e);
    }
    if pos < e {
        out.push((pos, e, RunStyle::default()));
    }
    out
}

/// Re-sort, merge touching equal runs, and drop plain runs.
fn normalize_styles(mut segs: Vec<(usize, usize, RunStyle)>) -> Vec<StyleSpan> {
    segs.retain(|(a, b, _)| a < b);
    segs.sort_by_key(|(a, _, _)| *a);
    let mut out: Vec<StyleSpan> = Vec::new();
    for (a, b, st) in segs {
        if st.is_plain() {
            continue;
        }
        if let Some(last) = out.last_mut()
            && last.end == a
            && last.style == st
        {
            last.end = b;
            continue;
        }
        out.push(StyleSpan {
            start: a,
            end: b,
            style: st,
        });
    }
    out
}

/// The style covering byte `offset` (the char starting there), else plain.
pub(crate) fn style_at(spans: &[StyleSpan], offset: usize) -> RunStyle {
    spans
        .iter()
        .find(|sp| sp.start <= offset && offset < sp.end)
        .map_or(RunStyle::default(), |sp| sp.style)
}

/// The formatting common to *every* char in `[s, e)` — for menu checkmarks. A
/// collapsed range reports the style just left of the caret (what typing
/// inherits).
pub(crate) fn active_style(spans: &[StyleSpan], s: usize, e: usize) -> RunStyle {
    if s >= e {
        return style_at(spans, s.saturating_sub(1));
    }
    let segs = style_segments(spans, s, e);
    let mut it = segs.iter().map(|&(_, _, st)| st);
    let Some(mut acc) = it.next() else {
        return RunStyle::default();
    };
    for st in it {
        acc.bold &= st.bold;
        acc.italic &= st.italic;
        acc.underline &= st.underline;
        acc.strike &= st.strike;
        acc.highlight = match (acc.highlight, st.highlight) {
            (Some(a), Some(b)) if a == b => Some(a),
            _ => None,
        };
    }
    acc
}

/// Replace the styling of `[s, e)` with `mid` (which covers `[s, e)`), keeping
/// runs outside (splitting any that straddle), then normalize.
fn replace_segment(
    spans: &[StyleSpan],
    s: usize,
    e: usize,
    mid: Vec<(usize, usize, RunStyle)>,
) -> Vec<StyleSpan> {
    let mut segs: Vec<(usize, usize, RunStyle)> = Vec::new();
    for sp in spans {
        if sp.start < s {
            segs.push((sp.start, sp.end.min(s), sp.style));
        }
        if sp.end > e {
            segs.push((sp.start.max(e), sp.end, sp.style));
        }
    }
    segs.extend(mid);
    normalize_styles(segs)
}

/// Toggle `format` over `[s, e)`: removed if every char already has it, else
/// added.
pub(crate) fn toggle_format(
    spans: &[StyleSpan],
    s: usize,
    e: usize,
    format: Format,
) -> Vec<StyleSpan> {
    if s >= e {
        return spans.to_vec();
    }
    let segs = style_segments(spans, s, e);
    let add = !segs.iter().all(|(_, _, st)| format.get(st));
    let mid = segs
        .into_iter()
        .map(|(a, b, mut st)| {
            format.set(&mut st, add);
            (a, b, st)
        })
        .collect();
    replace_segment(spans, s, e, mid)
}

/// Set/clear the highlight color over `[s, e)`: cleared if every char already
/// has `color`, else set to it.
pub(crate) fn toggle_highlight(
    spans: &[StyleSpan],
    s: usize,
    e: usize,
    color: u32,
) -> Vec<StyleSpan> {
    if s >= e {
        return spans.to_vec();
    }
    let segs = style_segments(spans, s, e);
    let on = !segs.iter().all(|(_, _, st)| st.highlight == Some(color));
    let mid = segs
        .into_iter()
        .map(|(a, b, mut st)| {
            st.highlight = on.then_some(color);
            (a, b, st)
        })
        .collect();
    replace_segment(spans, s, e, mid)
}

/// Shift/clip runs when the text `[s, e)` becomes `ins_len` bytes; the inserted
/// bytes take `insert_style`. Keeps runs aligned to the edited text.
pub(crate) fn splice_styles(
    spans: &[StyleSpan],
    s: usize,
    e: usize,
    ins_len: usize,
    insert_style: RunStyle,
) -> Vec<StyleSpan> {
    let delta = ins_len as isize - (e - s) as isize;
    let after = |p: usize| (p as isize + delta).max(0) as usize;
    let mut segs: Vec<(usize, usize, RunStyle)> = Vec::new();
    for sp in spans {
        if sp.start < s {
            segs.push((sp.start, sp.end.min(s), sp.style));
        }
        if sp.end > e {
            segs.push((after(sp.start.max(e)), after(sp.end), sp.style));
        }
    }
    if ins_len > 0 && !insert_style.is_plain() {
        segs.push((s, s + ins_len, insert_style));
    }
    normalize_styles(segs)
}

/// The kinds of thing a board can hold.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ElementKind {
    Draw(Stroke),
    Rect(BoxGeom),
    Ellipse(BoxGeom),
    Diamond(BoxGeom),
    Triangle(BoxGeom),
    RoundRect(BoxGeom),
    Star(BoxGeom),
    Hexagon(BoxGeom),
    Line(SegGeom),
    Arrow(SegGeom),
    Text(TextGeom),
    Embed(EmbedGeom),
    Image(ImageGeom),
}

/// A page-card: a titled box anchored at `(x, y)` that links to a host page
/// (`page_id`). The crate is page-agnostic — the host supplies the id + title
/// and handles opening it; this just stores and draws the card.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmbedGeom {
    pub page_id: i64,
    pub title: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// An image: a box anchored at `(x, y)` referencing a host-managed file (`src`,
/// e.g. `images/<name>`). The crate is storage-agnostic — the host imports the
/// file and supplies the decoded bitmap (see [`ImageFn`]); this stores the
/// reference + geometry and draws it as an overlay.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageGeom {
    pub src: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Rotation about the image's center, radians. Absent in older boards → 0.
    /// The host re-rotates the bitmap to match (see [`ImageFn`]).
    #[serde(default)]
    pub rotation: f32,
}

/// A text label: a top-left anchor, its content, and a world-space font size.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TextGeom {
    pub x: f32,
    pub y: f32,
    pub content: String,
    pub size: f32,
    /// Rotation about the text block's center, radians. Absent in older boards → 0.
    #[serde(default)]
    pub rotation: f32,
    /// Cached world-space extent, set each render from the font layout so the
    /// selection box and hit-test fit the real glyphs. Not persisted; a zero
    /// height means unmeasured (a fallback estimate is used until then).
    #[serde(skip)]
    pub measured_w: f32,
    #[serde(skip)]
    pub measured_h: f32,
}

/// Equality is over the persisted fields only: `measured_w` / `measured_h` are a
/// render-time cache, so a repaint must not make an otherwise unchanged text
/// element compare as edited (hosted history relies on that comparison).
impl PartialEq for TextGeom {
    fn eq(&self, other: &Self) -> bool {
        self.x == other.x
            && self.y == other.y
            && self.content == other.content
            && self.size == other.size
            && self.rotation == other.rotation
    }
}

/// A freehand pen stroke: world-space points and a world-space width.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub points: Vec<[f32; 2]>,
    pub width: f32,
}

/// A box (rectangle / ellipse), world-space. `x,y,w,h` describe the *unrotated*
/// box; `rotation` (radians, clockwise) spins it about its center at paint time.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoxGeom {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub width: f32,
    /// Rotation about the box center, radians. Absent in older boards → 0.
    #[serde(default)]
    pub rotation: f32,
}

/// A directed segment (line / arrow), world-space.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentAnchor {
    /// The connected shape/text/image element.
    pub element_id: u64,
    /// Which connector on that element: 0/1/2/3 = top/right/bottom/left.
    pub connector: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SegGeom {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub width: f32,
    #[serde(default = "default_segment_style")]
    pub style: SegmentStyle,
    /// Attachment for the first endpoint. Absent in older boards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_anchor: Option<SegmentAnchor>,
    /// Attachment for the second endpoint. Absent in older boards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_anchor: Option<SegmentAnchor>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SegmentStyle {
    Solid,
    Dashed,
}

fn default_segment_style() -> SegmentStyle {
    SegmentStyle::Solid
}

/// The viewport: a world-space pan offset and a zoom factor. The offset is the
/// world point that maps to the canvas's top-left corner, so a screen point
/// `s` (relative to the canvas) is the world point `offset + s / zoom`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default = "one")]
    pub zoom: f32,
}

fn one() -> f32 {
    1.0
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            zoom: 1.0,
        }
    }
}

impl Camera {
    /// The world point under a canvas-relative screen point.
    pub fn screen_to_world(&self, sx: f32, sy: f32) -> (f32, f32) {
        let z = self.zoom.max(MIN_ZOOM);
        (self.x + sx / z, self.y + sy / z)
    }

    /// Pan by a screen-space delta (px): the content follows the gesture.
    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        let z = self.zoom.max(MIN_ZOOM);
        self.x -= dx / z;
        self.y -= dy / z;
    }

    /// Multiply the zoom by `factor`, keeping the world point under the
    /// canvas-relative screen point `(rx, ry)` fixed (zoom-about-cursor).
    pub fn zoom_about(&mut self, rx: f32, ry: f32, factor: f32) {
        let z = self.zoom;
        let z2 = (z * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if (z2 - z).abs() < f32::EPSILON {
            return;
        }
        self.x += rx * (1.0 / z - 1.0 / z2);
        self.y += ry * (1.0 / z - 1.0 / z2);
        self.zoom = z2;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalThumbnailMode {
    Auto,
    Selection,
    Viewport,
    AllContent,
    Element(u64),
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct LocalThumbnailSpec {
    pub anchor_element_id: Option<u64>,
    /// Focus bounds in world coordinates: `[x0, y0, x1, y1]`.
    pub focus_bounds: [f32; 4],
    /// Whole-scene bounds in world coordinates, when available.
    pub scene_bounds: Option<[f32; 4]>,
    /// Recommended camera to render this thumbnail into the requested size.
    pub camera: Camera,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalThumbnailSnapshot {
    pub scene: Scene,
    pub spec: LocalThumbnailSpec,
}

impl LocalThumbnailSnapshot {
    /// Build a chrome-free all-content thumbnail directly from a persisted scene.
    /// Hosts can use this without mounting a full [`WhiteboardView`] entity first.
    pub fn for_scene_all_content(scene: Scene, width_px: f32, height_px: f32) -> Self {
        let width_px = width_px.max(1.0);
        let height_px = height_px.max(1.0);
        let scene_bounds = scene_bbox_for_local_thumbnail(&scene);
        let spec = if let Some(focus) = scene_bounds {
            local_thumbnail_spec_from_bbox(None, focus, scene_bounds, width_px, height_px)
        } else {
            let zoom = scene.camera.zoom.clamp(MIN_ZOOM, MAX_ZOOM);
            LocalThumbnailSpec {
                anchor_element_id: None,
                focus_bounds: [
                    scene.camera.x,
                    scene.camera.y,
                    scene.camera.x + width_px / zoom,
                    scene.camera.y + height_px / zoom,
                ],
                scene_bounds: None,
                camera: Camera {
                    x: scene.camera.x,
                    y: scene.camera.y,
                    zoom,
                },
            }
        };
        Self { scene, spec }
    }
}

pub(crate) fn scene_bbox_for_local_thumbnail(scene: &Scene) -> Option<(f32, f32, f32, f32)> {
    crate::mechanics::scene_bounds(scene)
}

pub(crate) fn local_thumbnail_spec_from_bbox(
    anchor_element_id: Option<u64>,
    focus: (f32, f32, f32, f32),
    scene_bounds: Option<(f32, f32, f32, f32)>,
    width_px: f32,
    height_px: f32,
) -> LocalThumbnailSpec {
    let width_px = width_px.max(1.0);
    let height_px = height_px.max(1.0);
    let focus_w = (focus.2 - focus.0).abs().max(1.0);
    let focus_h = (focus.3 - focus.1).abs().max(1.0);
    let pad_x = (focus_w * 0.18).max(48.0);
    let pad_y = (focus_h * 0.18).max(36.0);
    let padded_w = (focus_w + pad_x * 2.0).max(1.0);
    let padded_h = (focus_h + pad_y * 2.0).max(1.0);
    let zoom = (width_px / padded_w)
        .min(height_px / padded_h)
        .clamp(MIN_ZOOM, MAX_ZOOM);
    let center_x = (focus.0 + focus.2) * 0.5;
    let center_y = (focus.1 + focus.3) * 0.5;
    LocalThumbnailSpec {
        anchor_element_id,
        focus_bounds: [focus.0, focus.1, focus.2, focus.3],
        scene_bounds: scene_bounds.map(|bounds| [bounds.0, bounds.1, bounds.2, bounds.3]),
        camera: Camera {
            x: center_x - width_px / (2.0 * zoom),
            y: center_y - height_px / (2.0 * zoom),
            zoom,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(id: u64, x: f32) -> Element {
        Element {
            id,
            kind: ElementKind::Rect(BoxGeom {
                x,
                y: 0.0,
                w: 10.0,
                h: 10.0,
                width: 1.0,
                rotation: 0.0,
            }),
            stroke: None,
            fill: None,
            label: None,
            label_color: None,
            styles: Vec::new(),
            mindmap: None,
        }
    }

    /// Elements a newer writer might have produced, or a hand edit or a damaged
    /// row: each is valid JSON that is not a readable `Element`, and each is
    /// spelled to defeat re-serialization (key order that a sorted map would
    /// change, `1.0e2` that a number round trip would normalize, padding
    /// whitespace, and string contents that look like structure).
    const UNKNOWN_KIND: &str =
        r#"{ "kind" : {"hologram":{"y":2,"x":1.0e2}}, "id":7, "z" : 1.0e2 }"#;
    const TRICKY: &str = r#"{"id":"not-a-number","kind":{"rect":{}},"note":"a,]} \" [{ tricky","n":[1,[2,{"k":"]"}]]}"#;

    /// A document holding valid elements 1..=3 with the two unreadable ones
    /// between them, in the exact compact form `to_json` writes.
    fn fixture() -> (String, Scene) {
        let scene = Scene {
            camera: Camera {
                x: 4.0,
                y: -2.0,
                zoom: 1.5,
            },
            elements: vec![rect(1, 0.0), rect(2, 20.0), rect(3, 40.0)],
        };
        let e = |i: usize| serde_json::to_string(&scene.elements[i]).unwrap();
        let doc = format!(
            r#"{{"camera":{},"elements":[{},{UNKNOWN_KIND},{},{TRICKY},{}]}}"#,
            serde_json::to_string(&scene.camera).unwrap(),
            e(0),
            e(1),
            e(2),
        );
        (doc, scene)
    }

    #[test]
    fn parse_retains_unreadable_elements_and_round_trips_the_document_byte_for_byte() {
        // Wrong implementations rejected: `Vec<Element>` deserialized as a whole
        // (one bad element fails everything), `filter_map(.ok())` (the bad
        // elements vanish, so `retained` is empty and the output is shorter),
        // and re-serializing the retained `Value` (a sorted-key map rewrites the
        // key order and normalizes `1.0e2`, so the bytes differ from the input).
        let (doc, expected) = fixture();
        let parsed = Scene::parse(&doc).expect("a scene with bad elements still opens");

        // Positive control in the same fixture: the readable elements all loaded.
        assert_eq!(parsed.scene, expected);
        assert_eq!(
            parsed
                .scene
                .elements
                .iter()
                .map(|e| e.id)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );

        // The unreadable ones were kept, with their original positions and cause.
        assert_eq!(parsed.retained.len(), 2);
        assert_eq!(parsed.retained[0].index, 1);
        assert_eq!(parsed.retained[1].index, 3);
        assert_eq!(parsed.retained[0].source.as_deref(), Some(UNKNOWN_KIND));
        assert_eq!(parsed.retained[1].source.as_deref(), Some(TRICKY));
        assert!(parsed.retained.iter().all(|r| !r.error.is_empty()));
        assert_eq!(parsed.retained[0].id(), Some(7));
        assert_eq!(parsed.retained[1].id(), None);

        // Written back: identical to the input, in the same order.
        assert_eq!(parsed.scene.to_json_with_retained(&parsed.retained), doc);
        // The plain `to_json` is the lossy path and says so by being shorter.
        assert!(parsed.scene.to_json().len() < doc.len());
    }

    #[test]
    fn retained_elements_keep_their_order_while_readable_ones_change() {
        // The retained text must survive an edit session, not just an untouched
        // load. A writer that re-inserted retained elements at the end, or let a
        // deleted readable element pull them out of order, fails the order
        // assertions below.
        let (doc, _) = fixture();
        let mut parsed = Scene::parse(&doc).unwrap();

        parsed.scene.elements.remove(0); // delete readable element 1
        parsed.scene.elements.push(rect(9, 90.0)); // draw a new one on top
        let out = parsed.scene.to_json_with_retained(&parsed.retained);

        let again = Scene::parse(&out).unwrap();
        assert_eq!(
            again
                .scene
                .elements
                .iter()
                .map(|e| e.id)
                .collect::<Vec<_>>(),
            [2, 3, 9]
        );
        assert_eq!(again.retained.len(), 2);
        assert_eq!(again.retained[0].source.as_deref(), Some(UNKNOWN_KIND));
        assert_eq!(again.retained[1].source.as_deref(), Some(TRICKY));
        assert!(out.find(UNKNOWN_KIND).unwrap() < out.find(TRICKY).unwrap());

        // Deleting everything readable still keeps what is held aside.
        parsed.scene.elements.clear();
        let only = parsed.scene.to_json_with_retained(&parsed.retained);
        assert!(only.contains(UNKNOWN_KIND) && only.contains(TRICKY));
        assert_eq!(Scene::parse(&only).unwrap().retained.len(), 2);
    }

    #[test]
    fn a_syntax_error_is_an_err_carrying_the_original_text() {
        // Wrong implementation rejected: falling back to an empty board (what
        // `from_json` does), which would make the next save overwrite the row.
        let garbage = r#"{"camera":{"zoom":1},"elements":[{"id":1,"kind":{"rect""#;
        let err = Scene::parse(garbage).expect_err("truncated JSON is not a scene");
        assert!(matches!(err, SceneParseError::Syntax { .. }));
        assert_eq!(err.original_text(), garbage);
        assert!(err.to_string().contains("not valid JSON"));
        let SceneParseError::Syntax { line, column, .. } = &err else {
            unreachable!()
        };
        assert!(*line >= 1 && *column >= 1);

        for bad in ["not json", "{", "[1,2,", "{\"elements\":[1,]}"] {
            let err = Scene::parse(bad).expect_err(bad);
            assert_eq!(err.original_text(), bad);
        }
    }

    #[test]
    fn valid_json_that_is_not_a_scene_is_an_err_with_the_original_text() {
        for (doc, why) in [
            ("[1,2,3]", "root must be an object"),
            ("42", "root must be an object"),
            (r#"{"elements":{"a":1}}"#, "elements must be an array"),
            (r#"{"elements":"nope"}"#, "elements must be an array"),
        ] {
            let err = Scene::parse(doc).expect_err(why);
            assert!(matches!(err, SceneParseError::Shape { .. }), "{why}");
            assert_eq!(err.original_text(), doc);
        }
    }

    #[test]
    fn empty_input_is_a_new_board_and_a_bad_camera_does_not_discard_content() {
        for blank in ["", "  \n\t "] {
            let p = Scene::parse(blank).unwrap();
            assert!(p.scene.elements.is_empty() && p.retained.is_empty());
        }
        // The camera is view state: a bad one resets, is reported, and never
        // takes the elements down with it.
        let doc = format!(
            r#"{{"camera":"sideways","elements":[{}]}}"#,
            serde_json::to_string(&rect(5, 0.0)).unwrap()
        );
        let p = Scene::parse(&doc).unwrap();
        assert!(p.camera_error.is_some());
        assert_eq!(p.scene.camera, Camera::default());
        assert_eq!(p.scene.elements.len(), 1);
        // A zero zoom (hand edit) is repaired the way the lossy loader repairs it.
        let p = Scene::parse(r#"{"camera":{"zoom":0}}"#).unwrap();
        assert_eq!(p.scene.camera.zoom, 1.0);
    }

    #[test]
    fn a_fully_readable_document_has_nothing_retained_and_matches_to_json() {
        let scene = Scene {
            camera: Camera::default(),
            elements: vec![rect(1, 0.0), rect(2, 5.0)],
        };
        let p = Scene::parse(&scene.to_json()).unwrap();
        assert_eq!(p.scene, scene);
        assert!(p.retained.is_empty());
        assert_eq!(p.scene.to_json_with_retained(&p.retained), scene.to_json());
    }

    #[test]
    #[allow(deprecated)]
    fn the_deprecated_lossy_loader_is_the_behavior_parse_exists_to_replace() {
        // Pins the hazard: for the same document, `from_json` returns a board
        // with the unreadable elements gone and no way to get them back, while
        // `parse` still has them.
        let (doc, _) = fixture();
        let lossy = Scene::from_json(&doc);
        assert!(
            lossy.elements.is_empty(),
            "the bad elements took the board down"
        );
        assert_eq!(Scene::parse(&doc).unwrap().retained.len(), 2);
    }

    #[test]
    fn the_span_scanner_agrees_with_the_parser_on_awkward_documents() {
        for doc in [
            r#"{"elements":[]}"#,
            r#"{ "elements" : [ 1 , "a,b" , [ ] , {"x":"]"} , null ] }"#,
            r#"{"a":{"elements":[9]},"elements":[1,2]}"#,
        ] {
            let spans = element_spans(doc).expect(doc);
            let want = serde_json::from_str::<serde_json::Value>(doc).unwrap()["elements"]
                .as_array()
                .unwrap()
                .len();
            assert_eq!(spans.len(), want, "{doc}");
            for span in spans {
                assert!(serde_json::from_str::<serde_json::Value>(&doc[span]).is_ok());
            }
        }
    }
}
