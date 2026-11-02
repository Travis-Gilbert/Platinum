//! A small vault through the public API: parse every file, resolve what each links to, count backlinks.
use frame_notes::*;
use std::collections::BTreeMap;

const FILES: &[(&str, &str)] = &[
    (
        "Home.md",
        "---\ntags: [index]\nrelated: \"[[Projects/Plan]]\"\n---\n# Home\n\nStart at [[Plan]] or [[Plan#C# basics]]. Missing: [[Nowhere]] and [[a/Ghost]].\n![[diagram.png]]\n",
    ),
    (
        "Projects/Plan.md",
        "# Plan\n\n## C# basics\n\nBack to [[Home]] #project/alpha. See [the idea](../Ideas.md#Later) ^top\n",
    ),
    (
        "Projects/Archive/Plan.md",
        "# Old plan\n\n[[Projects/Plan|current]] and [[b/Ghost]]\n",
    ),
    (
        "Ideas.md",
        "# Ideas\n\n## Later\n\nSomething. [[plan#c# BASICS]]\n",
    ),
    ("diagram.png", ""),
    (
        "Board.canvas",
        r##"{"nodes":[{"id":"1","type":"file","file":"Projects/Plan.md","subpath":"#C# basics"},{"id":"2","type":"text","text":"[[Ideas]] and [[Ghost]]"}]}"##,
    ),
];

fn index() -> FileIndex {
    FileIndex::new(FILES.iter().map(|(path, _)| *path))
}

#[test]
fn links_resolve_and_backlinks_count_across_notes_and_canvases() {
    let index = index();
    let mut backlinks: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut unresolved: BTreeMap<String, usize> = BTreeMap::new();
    let mut record = |from: &str, target: Target| match target {
        Target::File(path) => backlinks.entry(path).or_default().push(from.to_string()),
        Target::Unresolved { key, .. } => *unresolved.entry(key).or_default() += 1,
    };

    for (path, text) in FILES {
        match FileKind::of(path) {
            FileKind::Note => {
                for link in &parse_note(text).links {
                    record(path, index.target_of(path, link));
                }
            }
            FileKind::Canvas => {
                for card in canvas_links(text).unwrap() {
                    record(path, index.target_of(path, &card.link));
                }
            }
            _ => {}
        }
    }

    // [[Plan]] is the shortest Plan, Projects/Plan; the archived one is reached only by its path.
    assert_eq!(
        backlinks["Projects/Plan.md"],
        [
            "Home.md",
            "Home.md",
            "Home.md",
            "Projects/Archive/Plan.md",
            "Ideas.md",
            "Board.canvas"
        ]
    );
    assert_eq!(backlinks["Home.md"], ["Projects/Plan.md"]);
    assert_eq!(backlinks["Ideas.md"], ["Projects/Plan.md", "Board.canvas"]);
    assert_eq!(backlinks["diagram.png"], ["Home.md"]);
    assert!(!backlinks.contains_key("Projects/Archive/Plan.md"));
    // a/Ghost and b/Ghost are two targets; the canvas's [[Ghost]] is a third.
    assert_eq!(
        unresolved.into_iter().collect::<Vec<_>>(),
        [
            ("a/ghost".into(), 1),
            ("b/ghost".into(), 1),
            ("ghost".into(), 1),
            ("nowhere".into(), 1)
        ]
    );
}

#[test]
fn a_link_into_a_heading_or_block_is_located_in_the_note_it_names() {
    let index = index();
    let home = parse_note(FILES[0].1);
    let plan = parse_note(FILES[1].1);
    let link = home.links.iter().find(|l| l.fragment.is_some()).unwrap();
    let Target::File(path) = index.target_of("Home.md", link) else {
        panic!("resolved")
    };
    assert_eq!(path, "Projects/Plan.md");
    let span = locate(&plan, link.fragment.as_ref().unwrap()).unwrap();
    assert_eq!(span.slice(FILES[1].1), "## C# basics");
    assert_eq!(
        locate(&plan, &Fragment::Block("top".into())).unwrap().line,
        5
    );
    // Tags nest into their parents.
    let tag = &plan.tags[0];
    assert_eq!(tag.name, "project/alpha");
    assert_eq!(tag.parents().collect::<Vec<_>>(), ["project"]);
}

#[test]
fn what_a_note_defines_is_in_one_place() {
    let home = parse_note(FILES[0].1);
    let fm = home.frontmatter.as_ref().unwrap();
    assert_eq!(fm.properties.len(), 2);
    assert!(
        home.links
            .iter()
            .any(|l| l.in_frontmatter && l.target == "Projects/Plan")
    );
    assert!(
        home.tags
            .iter()
            .any(|t| t.source == TagSource::Frontmatter && t.name == "index")
    );
    assert_eq!(home.headings.len(), 1);
}
