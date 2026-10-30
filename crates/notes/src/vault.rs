//! One durable authority and one filesystem mount per vault; no UI or provider startup.
use crate::authority::SharedAuthority;
use crate::index::{IndexedFile, store};
use crate::{FileKind, IdStore, VaultIndex, VaultSettings, is_hidden, normalize};
use anyhow::{Context, Result, bail, ensure};
use rustyred_thg_core::{DiskObjectStore, RedCoreGraphStore};
use rustyred_thg_vfs::{Vfs, VfsEvent, VfsPath};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::{
    collections::BTreeSet,
    io::Write,
    path::{Path, PathBuf},
};
use theorem_ide_authority::RedCoreAuthority;
use theorem_ide_runtime::authority::DocumentAuthority;
use theorem_ide_runtime::authority::{AuthorityHead, ReconcileResult, content_hash};
use theorem_ide_runtime::{DocumentKey, DocumentKind, IdeRuntime};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Document {
    pub document_id: String,
    pub revision: u64,
    pub text: String,
    #[serde(default)]
    pub reconciliation: Option<ReconcileResult>,
}

pub struct Vault {
    root: PathBuf,
    data: PathBuf,
    vfs: Vfs<RedCoreGraphStore, DiskObjectStore>,
    runtime: IdeRuntime,
    authority: SharedAuthority,
    index: VaultIndex,
    ids: IdStore,
    settings: VaultSettings,
}

impl Vault {
    pub fn open(root: impl AsRef<Path>, data: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().canonicalize()?;
        std::fs::create_dir_all(data.as_ref())?;
        let data = data.as_ref().canonicalize()?;
        ensure!(
            !data.starts_with(&root),
            "vault data directory must be outside the mounted folder"
        );
        let settings = match std::fs::read(data.join("settings.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => VaultSettings::default(),
            Err(e) => return Err(e.into()),
        };
        let mut runtime = IdeRuntime::new();
        let authority = SharedAuthority(Arc::new(Mutex::new(RedCoreAuthority::open(
            data.join("authority"),
        )?)));
        runtime.set_authority(Box::new(authority.clone()));
        let mut vfs = Vfs::open_in(
            "frame-notes",
            store(&data.join("vfs"))?,
            DiskObjectStore::open(data.join("blobs"))?,
        )?;
        let before_mount = vfs.generation();
        let mount = vfs.mount(&root)?;
        // The VFS names macOS's /private/var and /private/tmp by their /var and /tmp aliases, and
        // its snapshot and events use that spelling, so the vault adopts it as well.
        let root = vfs
            .root_path(&mount)
            .context("mounted vault has no root path")?
            .to_path_buf();
        runtime.bind_checkout(&root);
        // A new mount already performed a complete scan; only a restored mount needs refresh.
        if vfs.generation() == before_mount {
            vfs.refresh()?;
        }
        let index = VaultIndex::open(&data.join("index"))?;
        let events = vfs
            .events_since(index.generation())
            .into_iter()
            .map(|entry| entry.event)
            .collect::<Vec<_>>();
        let ids = IdStore::load(&root)?;
        let mut vault = Self {
            root,
            data,
            vfs,
            runtime,
            authority,
            index,
            ids,
            settings,
        };
        // Replay structural changes before comparing the current snapshot, retaining ids across offline moves.
        for event in &events {
            if let VfsEvent::Moved { from, to } = event
                && let (Some(from), Some(to)) = (vault.relative(from), vault.relative(to))
            {
                vault.observe_move(&from, &to)?;
            }
        }
        let paths = vault.visible_paths();
        let deleted: Vec<_> = vault
            .index
            .files()
            .keys()
            .filter(|p| !paths.contains(*p))
            .cloned()
            .collect();
        let mut changed = Vec::new();
        for path in &paths {
            let bytes = vault.read_disk(path)?;
            if document_kind(path) {
                // Existing durable edits are recovered before disk reconciliation; they never disappear on reopen.
                let synced = vault
                    .authority
                    .resolve("local", path)?
                    .and_then(|id| vault.authority.head(&id).transpose())
                    .transpose()?
                    .is_some_and(|h| {
                        h.disk_hash.as_deref()
                            == std::str::from_utf8(&bytes)
                                .ok()
                                .map(content_hash)
                                .as_deref()
                    });
                if !synced
                    && (vault.authority.resolve("local", path)?.is_some()
                        || std::str::from_utf8(&bytes)
                            .ok()
                            .is_some_and(|t| validate(path, t).is_ok()))
                {
                    vault.open_document(path)?;
                    vault.close(path)?;
                }
            }
            let hash = rustyred_thg_vfs::ContentHash::of(&bytes).0;
            if vault
                .index
                .files()
                .get(path)
                .is_none_or(|f| f.hash != hash || f.excluded != vault.settings.is_excluded(path))
            {
                changed.push(vault.project(path, &bytes)?);
            }
        }
        for path in &deleted {
            vault.observe_delete(path)?;
        }
        let stale_ids: Vec<_> = vault
            .ids
            .paths()
            .filter(|p| !paths.contains(*p))
            .map(str::to_owned)
            .collect();
        for path in stale_ids {
            vault.ids.delete(&path);
        }
        vault.ids.save_vfs(&mut vault.vfs)?;
        vault.index.update(changed, &deleted)?;
        vault.refresh_folders()?;
        vault.index.checkpoint(vault.vfs.generation())?;
        Ok(vault)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn data_dir(&self) -> &Path {
        &self.data
    }
    pub fn index(&self) -> &VaultIndex {
        &self.index
    }
    pub fn settings(&self) -> &VaultSettings {
        &self.settings
    }
    pub fn ids(&self) -> &IdStore {
        &self.ids
    }
    pub fn vfs(&self) -> &Vfs<RedCoreGraphStore, DiskObjectStore> {
        &self.vfs
    }
    pub fn authority(&self) -> &dyn theorem_ide_runtime::authority::DocumentAuthority {
        self.runtime.authority()
    }

    pub fn set_settings(&mut self, settings: VaultSettings) -> Result<()> {
        let mut temp = tempfile::NamedTempFile::new_in(&self.data)?;
        temp.write_all(&serde_json::to_vec_pretty(&settings)?)?;
        temp.as_file().sync_all()?;
        temp.persist(self.data.join("settings.json"))?;
        std::fs::File::open(&self.data)?.sync_all()?;
        self.settings = settings;
        let changed = self
            .index
            .files()
            .values()
            .cloned()
            .map(|mut f| {
                f.excluded = self.settings.is_excluded(&f.path);
                if let Some(fm) = f.note.as_ref().and_then(|n| n.frontmatter.as_ref()) {
                    f.properties = fm
                        .properties
                        .iter()
                        .map(|(k, v)| (k.clone(), self.settings.property_type(k, v)))
                        .collect();
                }
                f
            })
            .collect();
        self.index.update(changed, &[])
    }

    /// Open at the durable head. Any disk divergence is reconciled first; conflicts are returned separately.
    pub fn open_document(&mut self, path: &str) -> Result<Document> {
        self.check_document(path)?;
        let disk = self.read_disk(path)?;
        let text = std::str::from_utf8(&disk)?;
        if self.authority.resolve("local", path)?.is_none() {
            validate(path, text)?;
        }
        self.runtime.call("documents.open", args(path))?;
        let head = self.head(path)?;
        if head.disk_hash.as_deref() != Some(content_hash(text).as_str()) {
            let result = self.reconcile(path)?;
            if let ReconcileResult::Conflict { .. } = result {
                // Preserve the opened head for a diff; caller can inspect the typed result via reconcile.
                let mut opened = document(&self.head(path)?);
                opened.reconciliation = Some(result);
                return Ok(opened);
            }
        }
        Ok(document(&self.head(path)?))
    }

    /// Create a new Markdown note without replacing an existing disk file.
    /// Subsequent edits use the same authority as every opened document.
    pub fn create_note(&mut self, path: &str, text: &str) -> Result<Document> {
        self.check_path(path)?;
        ensure!(path.ends_with(".md"), "a note must have a Markdown path");
        validate(path, text)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.root.join(path))?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        self.refresh()?;
        self.open_document(path)
    }

    pub fn edit(
        &mut self,
        path: &str,
        revision: u64,
        operation_id: &str,
        text: &str,
    ) -> Result<Document> {
        self.check_document(path)?;
        validate(path, text)?;
        let mut arguments = args(path);
        arguments["base_revision"] = json!(revision);
        arguments["operation_id"] = json!(operation_id);
        arguments["text"] = json!(text);
        let result = self.runtime.call("documents.edit", arguments)?;
        let document: Document = serde_json::from_value(result)?;
        self.index_text(path, &document.text)?;
        Ok(document)
    }

    /// Save only if disk still matches the last recorded frontier. A conflict never writes the file.
    pub fn save(&mut self, path: &str) -> Result<ReconcileResult> {
        self.check_document(path)?;
        let disk = String::from_utf8(self.read_disk(path)?)?;
        if self.head(path)?.disk_hash.as_deref() != Some(content_hash(&disk).as_str()) {
            let result = self.reconcile(path)?;
            if matches!(result, ReconcileResult::Conflict { .. }) {
                return Ok(result);
            }
        }
        validate(path, &self.head(path)?.text)?;
        self.runtime.call("documents.save", args(path))?;
        let head = self.head(path)?;
        self.index_text(path, &head.text)?;
        Ok(ReconcileResult::Applied {
            kept: "authority".into(),
            head: serde_json::to_value(head)?,
        })
    }

    pub fn close(&mut self, path: &str) -> Result<()> {
        self.runtime.call("documents.close", args(path))?;
        Ok(())
    }
    pub fn undo(&mut self, path: &str) -> Result<Document> {
        self.history_edit(path, "documents.undo")
    }
    pub fn redo(&mut self, path: &str) -> Result<Document> {
        self.history_edit(path, "documents.redo")
    }
    fn history_edit(&mut self, path: &str, operation: &str) -> Result<Document> {
        self.check_document(path)?;
        let preview = if operation == "documents.undo" {
            self.runtime.sessions().preview_undo(&key(path))?
        } else {
            self.runtime.sessions().preview_redo(&key(path))?
        };
        if let Some(text) = preview {
            validate(path, &text)?;
        }
        self.runtime.call(operation, args(path))?;
        let head = self.head(path)?;
        validate(path, &head.text)?;
        self.index_text(path, &head.text)?;
        Ok(document(&head))
    }

    pub fn reconcile(&mut self, path: &str) -> Result<ReconcileResult> {
        self.check_document(path)?;
        let was_open = self.runtime.sessions().snapshot(&key(path)).is_ok();
        let mut arguments = args(path);
        arguments["keep"] = json!("disk");
        let result: ReconcileResult =
            serde_json::from_value(self.runtime.call("documents.reconcile", arguments)?)?;
        if matches!(result, ReconcileResult::Applied { .. }) {
            self.index_text(path, &self.head(path)?.text)?;
            if !was_open {
                self.close(path)?;
            }
        }
        Ok(result)
    }

    /// Rescan is the completeness fallback; tests drive this because the sandbox delivers no FSEvents.
    pub fn refresh(&mut self) -> Result<Vec<(String, ReconcileResult)>> {
        self.vfs.refresh()?;
        let events = self
            .vfs
            .events_since(self.index.generation())
            .into_iter()
            .map(|entry| entry.event)
            .collect::<Vec<_>>();
        let results = self.apply_events(&events)?;
        self.refresh_folders()?;
        self.index.checkpoint(self.vfs.generation())?;
        Ok(results)
    }

    /// Consume VFS journal events without rescanning files unrelated to those events.
    pub fn apply_events(&mut self, events: &[VfsEvent]) -> Result<Vec<(String, ReconcileResult)>> {
        let mut paths = BTreeSet::new();
        for event in events {
            if let VfsEvent::Moved { from, to } = event
                && let (Some(from), Some(to)) = (self.relative(from), self.relative(to))
            {
                self.observe_move(&from, &to)?;
            }
            paths.extend(
                event
                    .affected_paths()
                    .into_iter()
                    .filter_map(|p| self.relative(p)),
            );
        }
        let mut results = Vec::new();
        let mut changed = Vec::new();
        let mut deleted = Vec::new();
        for path in paths {
            if !self.root.join(&path).exists() {
                self.observe_delete(&path)?;
                deleted.push(path);
                continue;
            }
            if document_kind(&path) {
                if self.runtime.authority().resolve("local", &path)?.is_some() {
                    let disk = String::from_utf8(self.read_disk(&path)?)?;
                    if self.head(&path)?.disk_hash.as_deref() != Some(content_hash(&disk).as_str())
                    {
                        let result = self.reconcile(&path)?;
                        results.push((path.clone(), result));
                    }
                } else if std::str::from_utf8(&self.read_disk(&path)?)
                    .ok()
                    .is_some_and(|text| validate(&path, text).is_ok())
                {
                    self.open_document(&path)?;
                    self.close(&path)?;
                }
            }
            // Open panes see the accepted session, including unsaved edits. Rebuild/reopen projects disk.
            let bytes = if self.runtime.sessions().snapshot(&key(&path)).is_ok() {
                self.head(&path)?.text.into_bytes()
            } else {
                self.read_disk(&path)?
            };
            changed.push(self.project(&path, &bytes)?);
        }
        self.ids.save_vfs(&mut self.vfs)?;
        self.index.update(changed, &deleted)?;
        Ok(results)
    }

    pub fn write_attachment(&mut self, path: &str, bytes: &[u8]) -> Result<()> {
        self.check_path(path)?;
        ensure!(
            !document_kind(path),
            "notes, canvases and bases must be written through the authority"
        );
        let event = self.vfs.write(&VfsPath::new(self.root.join(path)), bytes)?;
        self.apply_events(&[event])?;
        Ok(())
    }

    pub fn rebuild_index(&mut self) -> Result<()> {
        self.refresh()?;
        let mut changed = Vec::new();
        for path in self.visible_paths() {
            changed.push(self.project(&path, &self.read_disk(&path)?)?);
        }
        let deleted = self
            .index
            .files()
            .keys()
            .filter(|p| !self.root.join(p).exists())
            .cloned()
            .collect::<Vec<_>>();
        self.ids.save_vfs(&mut self.vfs)?;
        self.index.update(changed, &deleted)
    }

    fn observe_delete(&mut self, path: &str) -> Result<()> {
        if let Some(id) = self.authority.resolve("local", path)? {
            let head = self.authority.head(&id)?.context("missing deleted head")?;
            if self.runtime.sessions().snapshot(&key(path)).is_ok() {
                self.close(path)?;
            }
            self.authority
                .retire(&id, head.revision, "external-delete")?;
        }
        self.ids.delete(path);
        Ok(())
    }
    fn observe_move(&mut self, from: &str, to: &str) -> Result<()> {
        if let Some(id) = self.authority.resolve("local", from)? {
            let was_open = self.runtime.sessions().snapshot(&key(from)).is_ok();
            if was_open {
                self.close(from)?;
            }
            self.authority.rename(&id, from, to, "external-move")?;
            if was_open {
                self.open_document(to)?;
            }
        }
        if self.ids.get(from).is_some() || self.ids.get(to).is_none() {
            self.ids.rename(from, to);
        }
        Ok(())
    }

    pub(crate) fn head(&self, path: &str) -> Result<AuthorityHead> {
        let id = self
            .runtime
            .authority()
            .resolve("local", path)?
            .context("document has no authority binding")?;
        self.runtime
            .authority()
            .head(&id)?
            .context("document has no authority head")
    }
    fn index_text(&mut self, path: &str, text: &str) -> Result<()> {
        let file = self.project(path, text.as_bytes())?;
        self.ids.save_vfs(&mut self.vfs)?;
        self.index.update(vec![file], &[])
    }
    fn project(&mut self, path: &str, bytes: &[u8]) -> Result<IndexedFile> {
        let text = document_kind(path)
            .then(|| std::str::from_utf8(bytes))
            .transpose()?;
        let mut file = VaultIndex::project(
            path,
            text,
            rustyred_thg_vfs::ContentHash::of(bytes).0,
            None,
            &self.settings,
        );
        let preferred = file
            .note
            .as_ref()
            .and_then(|n| n.frontmatter.as_ref())
            .and_then(|f| f.properties.get("theorem-id"))
            .and_then(Value::as_str);
        file.id = document_kind(path).then(|| self.ids.ensure(path, preferred).to_owned());
        Ok(file)
    }
    fn refresh_folders(&mut self) -> Result<()> {
        let mut folders = BTreeSet::new();
        for entry in walkdir::WalkDir::new(&self.root)
            .min_depth(1)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !e.file_name().to_string_lossy().starts_with('.'))
        {
            let entry = entry?;
            if entry.file_type().is_dir() {
                folders.insert(
                    entry
                        .path()
                        .strip_prefix(&self.root)?
                        .to_str()
                        .context("non-UTF8 vault folder")?
                        .to_owned(),
                );
            }
        }
        self.index.update_folders(folders)
    }
    fn relative(&self, path: &VfsPath) -> Option<String> {
        let p = path.as_path().strip_prefix(&self.root).ok()?.to_str()?;
        (!is_hidden(p)).then(|| p.to_owned())
    }
    fn visible_paths(&self) -> BTreeSet<String> {
        self.vfs
            .snapshot()
            .paths()
            .filter_map(|p| self.relative(p))
            .collect()
    }
    fn read_disk(&self, path: &str) -> Result<Vec<u8>> {
        self.check_path(path)?;
        Ok(std::fs::read(self.root.join(path))?)
    }
    fn check_document(&self, path: &str) -> Result<()> {
        self.check_path(path)?;
        ensure!(document_kind(path), "not a note, canvas or base: {path}");
        Ok(())
    }
    fn check_path(&self, path: &str) -> Result<()> {
        ensure!(
            normalize(path).as_deref() == Some(path) && !is_hidden(path),
            "invalid vault path: {path}"
        );
        self.vfs
            .validate_mutation_path(&VfsPath::new(self.root.join(path)))?;
        Ok(())
    }
}
fn args(path: &str) -> Value {
    json!({"workspace_id":"local","resource_id":path})
}
fn key(path: &str) -> DocumentKey {
    DocumentKey {
        workspace_id: "local".into(),
        resource_id: path.into(),
        kind: DocumentKind::SourceFile,
    }
}
fn document(head: &AuthorityHead) -> Document {
    Document {
        document_id: head.document_id.clone(),
        revision: head.revision,
        text: head.text.clone(),
        reconciliation: None,
    }
}
pub(crate) fn document_kind(path: &str) -> bool {
    matches!(
        FileKind::of(path),
        FileKind::Note | FileKind::Canvas | FileKind::Base
    )
}
pub(crate) fn validate(path: &str, text: &str) -> Result<()> {
    match FileKind::of(path) {
        FileKind::Canvas => {
            serde_json::from_str::<Value>(text).context("canvas must parse as JSON")?;
        }
        FileKind::Base => {
            serde_saphyr::from_str::<Value>(text).context("base must parse as YAML")?;
        }
        FileKind::Note => (),
        _ => bail!("not an authority document"),
    }
    Ok(())
}
