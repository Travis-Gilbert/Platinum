//! Deterministic vault generator: the same options write the same bytes.
use anyhow::{Context, Result, bail};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
};
use walkdir::WalkDir;

pub struct Options {
    pub notes: usize,
    pub seed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub notes: usize,
    pub large_notes: usize,
    pub canvases: usize,
    pub bases: usize,
    pub attachments: usize,
    pub bytes: u64,
    /// SHA-256 over every file's relative path and bytes, in path order.
    pub digest: String,
}

const ADJECTIVES: &[&str] = &[
    "quiet", "bright", "ancient", "narrow", "golden", "hollow", "rapid", "gentle", "silver",
    "distant", "crooked", "patient", "vivid", "frozen", "hidden", "modest", "rugged", "sleepy",
    "tender", "upright", "woven", "brittle", "radiant", "steady", "wild",
];
const NOUNS: &[&str] = &[
    "harbor", "lantern", "orchard", "compass", "meadow", "archive", "bridge", "engine", "garden",
    "ledger", "summit", "signal", "studio", "tunnel", "window", "harvest", "kernel", "lattice",
    "mirror", "network", "quarry", "ribbon", "shelter", "thread", "vessel",
];
const VERBS: &[&str] = &[
    "follows", "borrows", "measures", "protects", "reveals", "carries", "joins", "tracks", "bends",
    "feeds", "outlasts", "answers",
];
const HEADINGS: &[&str] = &[
    "Overview",
    "Background",
    "Details",
    "Decisions",
    "Open questions",
    "Next steps",
    "References",
    "Notes",
    "Summary",
    "Timeline",
    "Risks",
    "Log",
];
const TOP_FOLDERS: &[&str] = &[
    "projects",
    "areas",
    "journal",
    "reference",
    "people",
    "meetings",
    "archive",
    "library",
    "inbox",
];
const SUB_FOLDERS: &[&str] = &[
    "orion", "atlas", "delta", "harbor", "meadow", "quarry", "summit", "tundra", "vector",
    "willow", "zephyr", "ember", "fjord", "glacier", "helix",
];
const TAGS: &[&str] = &[
    "idea",
    "draft",
    "review",
    "reading",
    "meeting",
    "person",
    "reference",
    "todo",
    "research",
    "design",
    "writing",
    "finance",
    "café",
    "日本語",
    "проект",
];
const STATUSES: &[&str] = &["Planned", "In progress", "Done", "Blocked"];
const CALLOUTS: &[&str] = &[
    "note", "abstract", "info", "todo", "tip", "success", "question", "warning", "failure",
    "danger", "bug", "example", "quote",
];
const TASK_MARKS: &[char] = &[
    ' ', ' ', ' ', ' ', ' ', 'x', 'x', 'x', 'x', '/', '-', '>', '?', '!',
];
const LANGUAGES: &[&str] = &["rust", "python", "javascript", "json", "bash", "sql", "css"];
const BASES: [(&str, &str); 2] = [
    (
        "bases/Everything.base",
        "views:\n  - type: table\n    name: Everything\n    order:\n      - file.name\n      - status\n",
    ),
    (
        "bases/Projects.base",
        "filters:\n  and:\n    - file.inFolder(\"projects\")\nviews:\n  - type: list\n    name: Projects\n  - type: cards\n    name: Cards\n",
    ),
];
/// The smallest valid PNG: one transparent pixel.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];
const IMAGES: usize = 6;
/// Every `LARGE_EVERY` notes, starting at `LARGE_EVERY / 2`, is a large note.
const LARGE_EVERY: usize = 250;

struct Note {
    path: String,
    title: String,
    headings: Vec<String>,
    block: bool,
    daily: bool,
    large: bool,
    /// No other note shares the file name, so `[[Title]]` is enough.
    unique: bool,
}

struct Gen {
    rng: ChaCha8Rng,
}

impl Gen {
    fn below(&mut self, n: usize) -> usize {
        self.rng.random_range(0..n as u32) as usize
    }
    fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }
    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len())]
    }
    /// Low indices come out more often, so the first notes become hubs.
    fn hub(&mut self, n: usize) -> usize {
        let (a, b) = (self.below(n), self.below(n));
        a.min(b)
    }
    fn title_case(&mut self, xs: &[&str]) -> String {
        let w = self.pick(xs);
        let mut c = w.chars();
        c.next()
            .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
    }
    fn date(&mut self) -> String {
        format!(
            "{}-{:02}-{:02}",
            2020 + self.below(7),
            1 + self.below(12),
            1 + self.below(28)
        )
    }
    fn tag(&mut self) -> String {
        match self.below(10) {
            0..=3 => format!("project/{}", self.pick(SUB_FOLDERS)),
            4..=5 => format!("area/{}", self.pick(SUB_FOLDERS)),
            _ => self.pick(TAGS).to_string(),
        }
    }
}

pub fn generate(opts: &Options, out: &Path) -> Result<Summary> {
    if opts.notes == 0 {
        bail!("--notes must be at least 1");
    }
    if out.exists() && fs::read_dir(out)?.next().is_some() {
        bail!(
            "{} is not empty; a vault is only reproducible from scratch",
            out.display()
        );
    }
    let mut g = Gen {
        rng: ChaCha8Rng::seed_from_u64(opts.seed),
    };
    let notes = plan(&mut g, opts.notes);
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for (i, n) in notes.iter().enumerate() {
        files.insert(n.path.clone(), render(&mut g, &notes, i).into_bytes());
    }
    let canvases = (opts.notes / 1000).max(1);
    for k in 0..canvases {
        files.insert(
            format!("boards/Board {}.canvas", k + 1),
            canvas(&mut g, &notes, k).into_bytes(),
        );
    }
    for (path, text) in BASES {
        files.insert(path.to_string(), text.as_bytes().to_vec());
    }
    for k in 0..IMAGES {
        files.insert(format!("attachments/img-{k}.png"), PNG.to_vec());
    }
    let mut bytes = 0;
    for (path, data) in &files {
        let to = out.join(path);
        fs::create_dir_all(to.parent().context("path without a parent")?)?;
        fs::write(&to, data).with_context(|| format!("writing {}", to.display()))?;
        bytes += data.len() as u64;
    }
    Ok(Summary {
        notes: notes.len(),
        large_notes: notes.iter().filter(|n| n.large).count(),
        canvases,
        bases: BASES.len(),
        attachments: IMAGES,
        bytes,
        digest: tree_digest(out)?,
    })
}

/// SHA-256 over every file's relative path, length and bytes, in path order.
pub fn tree_digest(dir: &Path) -> Result<String> {
    let mut h = Sha256::new();
    for e in WalkDir::new(dir).sort_by_file_name() {
        let e = e?;
        if !e.file_type().is_file() {
            continue;
        }
        let rel = e
            .path()
            .strip_prefix(dir)?
            .to_string_lossy()
            .replace('\\', "/");
        let data = fs::read(e.path())?;
        h.update(rel.as_bytes());
        h.update([0]);
        h.update((data.len() as u64).to_le_bytes());
        h.update(&data);
    }
    Ok(h.finalize().iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    }))
}

fn folders(g: &mut Gen) -> Vec<String> {
    let mut all = Vec::new();
    for top in TOP_FOLDERS {
        all.push(top.to_string());
        let mut subs = BTreeSet::new();
        for _ in 0..g.range(2, 5) {
            subs.insert(g.pick(SUB_FOLDERS));
        }
        for sub in subs {
            all.push(format!("{top}/{sub}"));
            if g.chance(35) {
                all.push(format!("{top}/{sub}/{}", g.pick(SUB_FOLDERS)));
            }
        }
    }
    all
}

fn plan(g: &mut Gen, count: usize) -> Vec<Note> {
    let folders = folders(g);
    let mut notes: Vec<Note> = Vec::with_capacity(count);
    let mut used = BTreeSet::new();
    for i in 0..count {
        let large = i % LARGE_EVERY == LARGE_EVERY / 2;
        let daily = !large && g.chance(8);
        let (folder, title) = if daily {
            let date = g.date();
            (format!("journal/{}", &date[..4]), date)
        } else {
            let folder = folders[g.hub(folders.len())].clone();
            let title = if !notes.is_empty() && g.chance(2) {
                notes[g.below(notes.len())].title.clone()
            } else {
                let mut t = format!("{} {}", g.title_case(ADJECTIVES), g.title_case(NOUNS));
                if g.chance(30) {
                    t = format!("{t} {}", g.title_case(NOUNS));
                }
                t
            };
            (folder, title)
        };
        let (mut path, mut n) = (format!("{folder}/{title}.md"), 1);
        while !used.insert(path.clone()) {
            n += 1;
            path = format!("{folder}/{title} {n}.md");
        }
        let title = path[folder.len() + 1..path.len() - 3].to_string();
        let mut headings = BTreeSet::new();
        if large {
            headings.extend((1..=6).map(|k| format!("Section {k}")));
        } else {
            for _ in 0..g.range(2, 5) {
                headings.insert(g.pick(HEADINGS).to_string());
            }
        }
        let block = g.chance(25);
        notes.push(Note {
            path,
            title,
            headings: headings.into_iter().collect(),
            block,
            daily,
            large,
            unique: true,
        });
    }
    let mut seen = BTreeMap::new();
    for n in &notes {
        *seen.entry(n.title.clone()).or_insert(0) += 1;
    }
    for n in &mut notes {
        n.unique = seen[&n.title] == 1;
    }
    notes
}

fn name(n: &Note) -> String {
    if n.unique {
        n.title.clone()
    } else {
        n.path.trim_end_matches(".md").to_string()
    }
}

fn link(g: &mut Gen, notes: &[Note], from: usize) -> String {
    let mut t = g.hub(notes.len());
    if t == from {
        t = (t + 1) % notes.len();
    }
    let n = &notes[t];
    let target = name(n);
    match g.below(100) {
        0..=3 => format!(
            "[[Missing {} {}]]",
            g.title_case(ADJECTIVES),
            g.title_case(NOUNS)
        ),
        4..=53 => format!("[[{target}]]"),
        54..=67 => format!("[[{target}|{}]]", g.pick(NOUNS)),
        68..=79 if !n.headings.is_empty() => {
            let h = &n.headings[g.below(n.headings.len())];
            format!("[[{target}#{h}]]")
        }
        80..=84 if n.block => format!("[[{target}#^b{t}]]"),
        85..=92 => format!("[{}]({})", n.title, n.path.replace(' ', "%20")),
        _ => format!("[[{target}]]"),
    }
}

fn sentence(g: &mut Gen, notes: &[Note], from: usize, footnotes: &mut usize) -> String {
    let mut s = match g.below(3) {
        0 => format!(
            "The {} {} {} the {}.",
            g.pick(ADJECTIVES),
            g.pick(NOUNS),
            g.pick(VERBS),
            g.pick(NOUNS)
        ),
        1 => format!(
            "A {} {} {} every {} in the {}.",
            g.pick(ADJECTIVES),
            g.pick(NOUNS),
            g.pick(VERBS),
            g.pick(NOUNS),
            g.pick(NOUNS)
        ),
        _ => format!(
            "{} and {} {} a {} {}.",
            g.title_case(NOUNS),
            g.pick(NOUNS),
            g.pick(VERBS),
            g.pick(ADJECTIVES),
            g.pick(NOUNS)
        ),
    };
    if g.chance(14) {
        let _ = write!(s, " See {}.", link(g, notes, from));
    }
    if g.chance(8) {
        let _ = write!(s, " #{}", g.tag());
    }
    if g.chance(3) {
        let _ = write!(s, " ==Remember {}.==", g.pick(NOUNS));
    }
    if g.chance(2) {
        let _ = write!(s, " %%a comment about the {}%%", g.pick(NOUNS));
    }
    if g.chance(2) {
        let _ = write!(s, " Use `{}()` here.", g.pick(VERBS));
    }
    if g.chance(1) {
        s.push_str(" The bound is $x^2 + 1$.");
    }
    if g.chance(2) {
        *footnotes += 1;
        let _ = write!(s, "[^{footnotes}]");
    }
    s
}

fn paragraph(g: &mut Gen, notes: &[Note], from: usize, footnotes: &mut usize) -> String {
    (0..g.range(2, 5))
        .map(|_| sentence(g, notes, from, footnotes))
        .collect::<Vec<_>>()
        .join(" ")
}

fn section(g: &mut Gen, notes: &[Note], from: usize, footnotes: &mut usize, out: &mut String) {
    match g.below(100) {
        0..=49 => out.push_str(&paragraph(g, notes, from, footnotes)),
        50..=64 => {
            for _ in 0..g.range(2, 6) {
                let _ = writeln!(out, "- {}", sentence(g, notes, from, footnotes));
                if g.chance(25) {
                    let _ = writeln!(out, "  - {}", sentence(g, notes, from, footnotes));
                }
            }
        }
        65..=76 => {
            for _ in 0..g.range(2, 5) {
                let mark = TASK_MARKS[g.below(TASK_MARKS.len())];
                let _ = writeln!(out, "- [{mark}] {}", sentence(g, notes, from, footnotes));
            }
        }
        77..=82 => {
            let fold = ["", "-", "+"][g.below(3)];
            let _ = writeln!(
                out,
                "> [!{}]{fold} {}",
                g.pick(CALLOUTS),
                g.title_case(NOUNS)
            );
            let _ = write!(out, "> {}", sentence(g, notes, from, footnotes));
        }
        83..=86 => {
            let _ = writeln!(out, "```{}", g.pick(LANGUAGES));
            for _ in 0..g.range(3, 6) {
                let _ = writeln!(
                    out,
                    "{} {} = {};",
                    g.pick(NOUNS),
                    g.pick(ADJECTIVES),
                    g.below(1000)
                );
            }
            out.push_str("```");
        }
        87..=90 => {
            out.push_str("| Item | Count | Link |\n| --- | ---: | --- |\n");
            for _ in 0..g.range(2, 4) {
                let _ = writeln!(
                    out,
                    "| {} | {} | {} |",
                    g.pick(NOUNS),
                    g.below(100),
                    link(g, notes, from).replace('|', "\\|")
                );
            }
        }
        91..=94 => match g.below(4) {
            0 => {
                let t = g.hub(notes.len());
                let _ = write!(out, "![[{}]]", name(&notes[t]));
            }
            1 => {
                let t = g.hub(notes.len());
                let h = notes[t].headings.first().cloned().unwrap_or_default();
                let _ = write!(out, "![[{}#{h}]]", name(&notes[t]));
            }
            2 => {
                let _ = write!(out, "![[img-{}.png]]", g.below(IMAGES));
            }
            _ => {
                let _ = write!(
                    out,
                    "![[img-{}.png|{}]]",
                    g.below(IMAGES),
                    100 * g.range(1, 4)
                );
            }
        },
        95..=96 => {
            let _ = write!(
                out,
                "$$\n{} = \\sum_{{i=0}}^{{n}} {}_i\n$$",
                g.pick(NOUNS),
                g.pick(ADJECTIVES)
            );
        }
        97..=98 => {
            let _ = write!(out, "> {}", sentence(g, notes, from, footnotes));
        }
        _ => {
            let _ = write!(out, "%%\n{}\n%%", sentence(g, notes, from, footnotes));
        }
    }
    out.truncate(out.trim_end().len());
    out.push_str("\n\n");
}

fn frontmatter(g: &mut Gen, notes: &[Note], from: usize) -> String {
    let n = &notes[from];
    let mut tags = BTreeSet::new();
    for _ in 0..g.range(1, 4) {
        tags.insert(g.tag());
    }
    if n.daily {
        tags.insert("daily".to_string());
    }
    let mut s = String::from("---\ntags:\n");
    for t in tags {
        let _ = writeln!(s, "  - {t}");
    }
    if g.chance(10) {
        let _ = write!(
            s,
            "aliases:\n  - \"{} {}\"\n",
            g.title_case(ADJECTIVES),
            g.title_case(NOUNS)
        );
    }
    if g.chance(60) {
        let _ = writeln!(s, "status: {}", g.pick(STATUSES));
    }
    if g.chance(30) {
        let _ = writeln!(s, "rating: {}", g.range(1, 5));
    }
    if g.chance(25) {
        let _ = writeln!(s, "due: {}", g.date());
    }
    if n.daily || g.chance(60) {
        let _ = writeln!(
            s,
            "created: {}",
            if n.daily { n.title.clone() } else { g.date() }
        );
    }
    if g.chance(10) {
        let _ = writeln!(s, "published: {}", g.chance(50));
    }
    if g.chance(20) {
        s.push_str("related:\n");
        for _ in 0..g.range(1, 3) {
            let _ = writeln!(s, "  - \"{}\"", link(g, notes, from).replace('"', "'"));
        }
    }
    if g.chance(3) {
        s.push_str("cssclasses:\n  - wide-page\n");
    }
    s.push_str("---\n");
    s
}

fn render(g: &mut Gen, notes: &[Note], from: usize) -> String {
    let n = &notes[from];
    let mut out = String::new();
    if n.daily || g.chance(70) {
        out.push_str(&frontmatter(g, notes, from));
    }
    let _ = writeln!(out, "# {}\n", n.title);
    let mut footnotes = 0;
    let intro = paragraph(g, notes, from, &mut footnotes);
    out.push_str(&intro);
    if n.block {
        let _ = write!(out, " ^b{from}");
    }
    out.push_str("\n\n");
    if n.large {
        for k in 1..=g.range(300, 900) {
            let _ = writeln!(out, "## Section {k}\n");
            for _ in 0..g.range(1, 3) {
                section(g, notes, from, &mut footnotes, &mut out);
            }
        }
    } else {
        let names: &[String] = if n.daily { &[] } else { &n.headings };
        for h in names {
            let _ = writeln!(out, "## {h}\n");
            for _ in 0..g.range(1, 4) {
                section(g, notes, from, &mut footnotes, &mut out);
            }
        }
        if n.daily {
            out.push_str("## Log\n\n");
            section(g, notes, from, &mut footnotes, &mut out);
        }
    }
    for k in 1..=footnotes {
        let _ = writeln!(
            out,
            "[^{k}]: {} {} {}.",
            g.title_case(ADJECTIVES),
            g.pick(NOUNS),
            g.pick(VERBS)
        );
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

fn canvas(g: &mut Gen, notes: &[Note], k: usize) -> String {
    let count = g.range(6, 14);
    let id = |j: usize| format!("{:016x}", ((k as u64) << 32) | j as u64);
    let mut nodes = Vec::new();
    for j in 0..count {
        let (x, y) = ((j % 4) as i64 * 340, (j / 4) as i64 * 260);
        let pos = json!({"id": id(j), "x": x, "y": y, "width": 300, "height": 200});
        let mut node = pos.as_object().cloned().unwrap_or_default();
        match g.below(3) {
            0 => {
                node.insert("type".into(), json!("file"));
                node.insert("file".into(), json!(notes[g.hub(notes.len())].path));
            }
            1 => {
                node.insert("type".into(), json!("text"));
                node.insert("text".into(), json!(sentence(g, notes, 0, &mut 0)));
            }
            _ => {
                node.insert("type".into(), json!("link"));
                node.insert(
                    "url".into(),
                    json!(format!("https://example.com/{}", g.pick(NOUNS))),
                );
            }
        }
        nodes.push(node);
    }
    let edges: Vec<_> = (1..count)
        .filter(|_| g.chance(60))
        .map(|j| json!({"id": format!("e{}", id(j)), "fromNode": id(j - 1), "toNode": id(j), "fromSide": "right", "toSide": "left"}))
        .collect();
    // A workspace that turns on serde_json's `preserve_order` would otherwise change the bytes.
    let mut doc = json!({"nodes": nodes, "edges": edges});
    doc.sort_all_objects();
    let mut text = serde_json::to_string_pretty(&doc).unwrap_or_default();
    text.push('\n');
    text
}
