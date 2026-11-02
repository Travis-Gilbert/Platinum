//! Recording-only normalization; the live CLI and isolation checks always see original answers.
use crate::{launch::Isolation, obsidian::Record};
use anyhow::Result;
use regex::{Captures, Regex};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The Frame checkout that holds this crate.
fn repo_root() -> Result<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?)
}

pub(crate) struct Normalizer {
    paths: Vec<(Regex, &'static str)>,
    resource: Regex,
    mtime: Regex,
    footnote: Regex,
    tag: Regex,
    style: Regex,
    layout: Regex,
    dimension: Regex,
}

impl Normalizer {
    pub fn new(vault: &Path, isolation: Option<&Isolation>) -> Result<Self> {
        let vault = vault.canonicalize()?;
        let repo = repo_root()?;
        let mut roots = vec![
            (vault.display().to_string(), "__vault__"),
            (repo.display().to_string(), "__repo__"),
        ];
        if let Some(i) = isolation {
            roots.extend([
                (i.vault_copy.clone(), "__vault__"),
                (i.home.clone(), "__home__"),
                (i.user_data_dir.clone(), "__profile__"),
            ]);
            if let Some(root) = Path::new(&i.home).parent() {
                roots.push((root.display().to_string(), "__run__"));
            }
        }
        for (path, token) in roots.clone() {
            if let Some(alias) = path.strip_prefix("/private/var/") {
                roots.push((format!("/var/{alias}"), token));
            } else if path.starts_with("/var/") {
                roots.push((format!("/private{path}"), token));
            }
        }
        roots.sort_by_key(|(path, _)| std::cmp::Reverse(path.len()));
        let paths = roots
            .into_iter()
            .map(|(path, token)| {
                Ok((
                    Regex::new(&format!(r#"{}($|[/\s"'<>?\\])"#, regex::escape(&path)))?,
                    token,
                ))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            paths,
            resource: Regex::new(r#"app://[^/\s"'<>]+/([^\s"'<>]*)"#)?,
            mtime: Regex::new(r"\?\d+(\.\d+)?($|[#&])")?,
            footnote: Regex::new(
                r##"((?:id|data-footnote-id|href)="#?(?:fn|fnref)-\d+(?:-\d+)?-)([a-f0-9]{16})(")"##,
            )?,
            tag: Regex::new(r#"<[^>]+>"#)?,
            style: Regex::new(r#"style="([^"]*)""#)?,
            layout: Regex::new(
                r"((?:padding-bottom|min-height|width|height|top|bottom|left|inset-inline-start|--scale-factor|--thumbnail-width|--thumbnail-height):\s*)-?\d+(?:\.\d+)?(px|%)?|(scaleX\()-?\d+(?:\.\d+)?(\))",
            )?,
            dimension: Regex::new(r#"((?:width|height)=")\d+(?:\.\d+)?(")"#)?,
        })
    }

    fn paths(&self, text: &str) -> String {
        self.paths
            .iter()
            .fold(text.to_string(), |s, (pattern, token)| {
                pattern
                    .replace_all(&s, |c: &Captures| format!("{token}{}", &c[1]))
                    .into_owned()
            })
    }

    pub fn text(&self, text: &str) -> String {
        // Normalize the URL path separately to retain the slash after its authority.
        let text = self.resource.replace_all(text, |c: &Captures| {
            let path = self.paths(&format!("/{}", &c[1]));
            let path = self.mtime.replace_all(&path, "?__mtime__${2}");
            format!("app://__app__/{}", path.trim_start_matches('/'))
        });
        self.paths(&text)
    }

    pub fn value(&self, value: &mut Value) {
        match value {
            Value::String(s) => *s = self.text(s),
            Value::Array(a) => a.iter_mut().for_each(|v| self.value(v)),
            Value::Object(o) => {
                *o = std::mem::take(o)
                    .into_iter()
                    .map(|(k, mut v)| {
                        self.value(&mut v);
                        (self.text(&k), v)
                    })
                    .collect();
            }
            _ => {}
        }
    }

    fn html(&self, text: &str) -> String {
        let mut suffixes = Vec::new();
        let text = self.footnote.replace_all(text, |c: &Captures| {
            let index = match suffixes.iter().position(|s| s == &c[2]) {
                Some(i) => i,
                None => {
                    suffixes.push(c[2].to_string());
                    suffixes.len() - 1
                }
            };
            format!("{}__footnote_{}__{}", &c[1], index + 1, &c[3])
        });
        // Match only generated start tags and known geometry declarations, not authored HTML,
        // image dimensions, SVG geometry, MathJax metrics or text that happens to contain numbers.
        self.tag
            .replace_all(&text, |c: &Captures| {
                let tag = &c[0];
                let generated = [
                    "markdown-preview-sizer",
                    "bases-",
                    "pdf-",
                    "pdfViewer",
                    "textLayer",
                    "canvasWrapper",
                    " class=\"thumbnail",
                    " class=\"page\"",
                ]
                .iter()
                .any(|class| tag.contains(class));
                let pdf_text = tag.starts_with("<span ") && tag.contains("--total-scale-factor");
                let mut tag = if generated || pdf_text {
                    self.style
                        .replace_all(tag, |s: &Captures| {
                            let style = self.layout.replace_all(&s[1], |v: &Captures| {
                                if let Some(prefix) = v.get(1) {
                                    format!(
                                        "{}__layout__{}",
                                        prefix.as_str(),
                                        v.get(2).map_or("", |m| m.as_str())
                                    )
                                } else {
                                    format!("{}__layout__{}", &v[3], &v[4])
                                }
                            });
                            format!("style=\"{style}\"")
                        })
                        .into_owned()
                } else {
                    tag.to_string()
                };
                if tag.starts_with("<canvas ") {
                    tag = self
                        .dimension
                        .replace_all(&tag, "${1}__layout__${2}")
                        .into_owned();
                }
                tag
            })
            .into_owned()
    }

    pub fn record(&self, mut record: Record) -> Record {
        // Parse before replacing strings so escaped JSON paths are normalized as well.
        let mut json = record.parsed.take().or_else(|| {
            if record.command == "eval" {
                record.eval_value()
            } else {
                None
            }
        });
        if let Some(value) = &mut json {
            self.value(value);
            if let Some(rows) = value.as_array_mut() {
                match record.command.as_str() {
                    "search" | "search:context" => {
                        if record
                            .args
                            .iter()
                            .any(|a| a.starts_with("limit=") && a != "limit=0")
                        {
                            rows.fill(Value::String("__limited_result__".into()));
                        } else {
                            rows.sort_by_cached_key(Value::to_string);
                        }
                    }
                    "tags" if record.args.iter().any(|a| a == "sort=count") => {
                        rows.sort_by_cached_key(|v| {
                            (
                                std::cmp::Reverse(
                                    v["count"]
                                        .as_str()
                                        .and_then(|s| s.parse::<u64>().ok())
                                        .unwrap_or(0),
                                ),
                                v["tag"].as_str().unwrap_or_default().to_string(),
                            )
                        });
                    }
                    _ => {}
                }
            }
            if record.command == "eval" {
                record.stdout.clear();
            } else {
                record.stdout = format!("{}\n", serde_json::to_string_pretty(value).unwrap());
            }
            record.parsed = json;
        } else {
            record.stdout = self.text(&record.stdout);
        }
        if record.command == "dev:dom" {
            record.stdout = self.html(&record.stdout);
        }
        record.stderr = self.text(&record.stderr);
        record.error = record.error.map(|s| self.text(&s));
        record.args.iter_mut().for_each(|s| *s = self.text(s));
        record
    }

    pub fn vault_path(&self, vault: &Path) -> Result<String> {
        let vault = vault.canonicalize()?;
        Ok(vault.strip_prefix(repo_root()?).map_or_else(
            |_| "__vault__".into(),
            |p| p.to_string_lossy().replace('\\', "/"),
        ))
    }

    pub fn isolation(&self, isolation: &Isolation) -> Isolation {
        Isolation {
            app: isolation.app.clone(),
            home: "__home__".into(),
            user_data_dir: "__profile__".into(),
            vault_copy: "__vault__".into(),
            sandboxed: isolation.sandboxed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn normalizer() -> Normalizer {
        Normalizer::new(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("vault")
                .as_path(),
            Some(&Isolation {
                app: "/Applications/Obsidian.app".into(),
                home: "/private/var/folders/5f/gw6fzw9n1z15vht733bcn7jr0000gn/T/nfo-1I4818/home".into(),
                user_data_dir: "/private/var/folders/5f/gw6fzw9n1z15vht733bcn7jr0000gn/T/nfo-1I4818/profile".into(),
                vault_copy: "/private/var/folders/5f/gw6fzw9n1z15vht733bcn7jr0000gn/T/nfo-1I4818/vaults/vault".into(),
                sandboxed: true,
            }),
        )
        .unwrap()
    }

    fn record(command: &str, stdout: &str) -> Record {
        Record {
            command: command.into(),
            args: Vec::new(),
            exit_code: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
            error: None,
            parsed: None,
            stdout_file: None,
        }
    }

    #[test]
    fn recorded_paths_and_resource_urls_are_portable() {
        let n = normalizer();
        for prefix in ["/private/var", "/var"] {
            let path = format!(
                "{prefix}/folders/5f/gw6fzw9n1z15vht733bcn7jr0000gn/T/nfo-1I4818/vaults/vault"
            );
            assert_eq!(
                n.text(&format!(
                    "name\tvault\npath\t{path}\nfiles\t92\nfolders\t22\nsize\t92723\n"
                )),
                "name\tvault\npath\t__vault__\nfiles\t92\nfolders\t22\nsize\t92723\n"
            );
            assert_eq!(n.text(&format!("{path}\n")), "__vault__\n");
            let url = format!(
                "app://487b73234b3d512c13a6dc5ae4c9c390a70f{path}/attachments/pixel.png?1793019655954"
            );
            assert_eq!(
                n.text(&format!("<img src=\"{url}\">")),
                "<img src=\"app://__app__/__vault__/attachments/pixel.png?__mtime__\">"
            );
            assert_eq!(
                n.text(&format!("{path}-other")),
                format!("__run__/vaults/vault-other")
            );
        }
        assert_eq!(
            n.text("https://example.org/pixel.png?1793019655954"),
            "https://example.org/pixel.png?1793019655954"
        );
        let s = "app://__app__/__vault__/pixel.png?__mtime__";
        assert_eq!(n.text(s), s);
    }

    #[test]
    fn json_keys_values_and_diagnostics_use_the_same_normalizer() {
        let n = normalizer();
        let path = "/var/folders/5f/gw6fzw9n1z15vht733bcn7jr0000gn/T/nfo-1I4818/vaults/vault";
        let mut r = record(
            "eval",
            &format!(
                "=> {}",
                json!({path: {"path": path, "created": "2026-10-25", "size": 92723}})
            ),
        );
        r.stderr = format!("{path}/attachments/pixel.png");
        r.error = Some(
            "/private/var/folders/5f/gw6fzw9n1z15vht733bcn7jr0000gn/T/nfo-1I4818/home/error".into(),
        );
        let r = n.record(r);
        assert_eq!(
            r.parsed.unwrap(),
            json!({"__vault__": {"path": "__vault__", "created": "2026-10-25", "size": 92723}})
        );
        assert_eq!(r.stderr, "__vault__/attachments/pixel.png");
        assert_eq!(r.error.as_deref(), Some("__home__/error"));
        assert!(r.stdout.is_empty());
    }

    #[test]
    fn dom_normalizes_generated_ids_and_geometry_but_preserves_authored_values() {
        let n = normalizer();
        let html = r##"<sup data-footnote-id="fnref-1-1-b26870d8c870b926" id="fnref-1-1-b26870d8c870b926"><a href="#fn-1-b26870d8c870b926"></a></sup><li id="fn-1-b26870d8c870b926"></li><li id="fn-1-a26870d8c870b926"></li><div class="markdown-preview-sizer" style="padding-bottom: 361px; min-height: 1380px;"></div><div class="bases-tr" style="width: 583px;"></div><div class="pdfViewer" style="--scale-factor: 0.9542483660130718;"></div><canvas width="1168" height="1510"></canvas><span class="textLayerNode" style="transform: scaleX(0.996812);"></span><img width="120" height="50"><div id="custom-id" style="color: red; width: 200px;">2026-10-25</div>"##;
        let out = n.record(record("dev:dom", html)).stdout;
        assert!(out.contains("fnref-1-1-__footnote_1__"));
        assert!(out.contains("href=\"#fn-1-__footnote_1__\""));
        assert!(out.contains("id=\"fn-1-__footnote_2__\""));
        for number in [
            "361",
            "1380",
            "583",
            "0.9542483660130718",
            "1168",
            "1510",
            "0.996812",
        ] {
            assert!(!out.contains(number), "{number}: {out}");
        }
        assert!(out.contains("<img width=\"120\" height=\"50\">"));
        assert!(out.contains("id=\"custom-id\" style=\"color: red; width: 200px;\">2026-10-25"));
        assert_eq!(n.record(record("dev:dom", &out)).stdout, out);
    }

    #[test]
    fn unordered_results_are_canonical_but_semantic_arrays_stay_ordered() {
        let n = normalizer();
        let make = |command, value: Value| {
            let mut r = record(command, &value.to_string());
            r.parsed = Some(value);
            r
        };
        let r = n.record(make("search", json!(["b.md", "a.md"])));
        assert_eq!(r.parsed, Some(json!(["a.md", "b.md"])));
        assert_eq!(
            serde_json::from_str::<Value>(&r.stdout).unwrap(),
            r.parsed.unwrap()
        );
        let mut r = make(
            "search:context",
            json!([{"file":"b.md","matches":[]},{"file":"a.md","matches":[]}]),
        );
        r.args.push("limit=2".into());
        assert_eq!(
            n.record(r).parsed,
            Some(json!(["__limited_result__", "__limited_result__"]))
        );
        let mut r = make(
            "tags",
            json!([{"tag":"#b","count":"2"},{"tag":"#c","count":"10"},{"tag":"#a","count":"2"}]),
        );
        r.args.push("sort=count".into());
        assert_eq!(n.record(r).parsed.unwrap()[1]["tag"], "#a");
        for command in ["outline", "bookmarks", "base:query", "properties", "eval"] {
            let value = json!([{"name":"z", "Time left":"21 days"},{"name":"a"}]);
            assert_eq!(n.record(make(command, value.clone())).parsed, Some(value));
        }
    }
}
