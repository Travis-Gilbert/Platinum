//! The ide-cm peer: the web client itself, attached to the embedded host the
//! way it attaches to any host. The check starts ide-cm's dev server
//! (`bun run dev`) against the host's served origin and credential file, and
//! drives its page in headless Chrome through `script/code-check-ide-cm.cjs`,
//! one JSON request per line.
//!
//! `FRAME_CODE_PEER_IDE_CM` names the ide-cm checkout, `FRAME_CODE_PEER_PORT`
//! its dev server port (14063 by default), and the driver's own variables
//! (`FRAME_CODE_PEER_PUPPETEER`, `FRAME_CODE_PEER_CHROME`) pass through.
use std::collections::BTreeSet;
use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;

use anyhow::Context as _;
use futures::channel::oneshot;
use serde_json::{Value, json};

type Request = (Value, oneshot::Sender<anyhow::Result<Value>>);

pub(super) struct Page {
    requests: mpsc::Sender<Request>,
    server: Child,
    driver: Child,
    /// The ide-cm commit the page was served from.
    pub revision: String,
    state: Value,
    /// Every `document.conflict` the page's live stream delivered, by
    /// document and revision.
    conflicts_received: BTreeSet<(String, u64)>,
    /// Every conflict banner the page has shown, by path and revision.
    banners_shown: BTreeSet<(String, u64)>,
    pub terminal: Option<String>,
    /// Requests the page could not carry out.
    pub failures: Vec<String>,
}

impl Page {
    pub async fn start(
        origin: &str,
        token_file: &Path,
        workspace: &str,
        resource: &str,
        output: &Path,
    ) -> anyhow::Result<Self> {
        let dir = PathBuf::from(
            std::env::var_os("FRAME_CODE_PEER_IDE_CM").context("FRAME_CODE_PEER_IDE_CM")?,
        );
        let port = std::env::var("FRAME_CODE_PEER_PORT").unwrap_or_else(|_| "14063".into());
        let revision = Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
            .unwrap_or_default();
        let log = std::fs::File::create(output.join("ide-cm-dev-server.log"))?;
        let server = Command::new("bun")
            .args([
                "run",
                "dev",
                "--port",
                &port,
                "--strictPort",
                "--host",
                "127.0.0.1",
            ])
            .current_dir(&dir)
            .env("THEOREM_IDE_HOST_URL", origin)
            .env("THEOREM_IDE_HOST_TENANT", "local")
            .env("THEOREM_IDE_HOST_TOKEN_FILE", token_file)
            .env("THEOREM_IDE_HOST_ACTOR", "web:ide-cm")
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            // Its own group, so stopping it stops vite under it.
            .process_group(0)
            .spawn()
            .context("starting ide-cm's dev server")?;
        let script = std::env::var_os("FRAME_CODE_PEER_DRIVER").map_or_else(
            || Path::new(env!("CARGO_MANIFEST_DIR")).join("script/code-check-ide-cm.cjs"),
            PathBuf::from,
        );
        let mut driver = Command::new("node")
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(output.join("ide-cm-driver.log"))?)
            .spawn()
            .context("starting the ide-cm driver")?;
        let (mut stdin, stdout) = (
            driver.stdin.take().context("driver stdin")?,
            driver.stdout.take().context("driver stdout")?,
        );
        let (requests, served) = mpsc::channel::<Request>();
        std::thread::spawn(move || {
            let mut replies = BufReader::new(stdout);
            for (request, reply) in served {
                let mut line = String::new();
                let result = writeln!(stdin, "{request}")
                    .map_err(anyhow::Error::from)
                    .and_then(|()| Ok(replies.read_line(&mut line)?))
                    .and_then(|read| {
                        anyhow::ensure!(read > 0, "the driver exited");
                        let reply: Value = serde_json::from_str(&line)?;
                        match reply.get("ok") {
                            Some(ok) => Ok(ok.clone()),
                            None => Err(anyhow::anyhow!("{}", reply["error"])),
                        }
                    });
                let _ = reply.send(result);
            }
        });
        let mut page = Self {
            requests,
            server,
            driver,
            revision,
            state: Value::Null,
            conflicts_received: BTreeSet::new(),
            banners_shown: BTreeSet::new(),
            terminal: None,
            failures: Vec::new(),
        };
        let url =
            format!("http://127.0.0.1:{port}/?workspace={workspace}&name=checkout&file={resource}");
        let opened = page.request(json!({"op": "open", "url": url})).await?;
        page.observe(opened);
        Ok(page)
    }

    async fn request(&self, request: Value) -> anyhow::Result<Value> {
        let (reply, answer) = oneshot::channel();
        self.requests
            .send((request, reply))
            .map_err(|_| anyhow::anyhow!("the driver thread ended"))?;
        answer.await?
    }

    fn observe(&mut self, state: Value) {
        for conflict in state["received"].as_array().into_iter().flatten() {
            self.conflicts_received.insert((
                conflict["document"].as_str().unwrap_or_default().to_owned(),
                conflict["revision"].as_u64().unwrap_or_default(),
            ));
        }
        for banner in state["banners"].as_array().into_iter().flatten() {
            self.banners_shown.insert((
                banner["path"].as_str().unwrap_or_default().to_owned(),
                banner["revision"].as_u64().unwrap_or_default(),
            ));
        }
        self.state = state;
    }

    pub async fn pump(&mut self) {
        if let Ok(state) = self.request(json!({"op": "state"})).await {
            self.observe(state);
        }
    }

    pub fn text(&self) -> String {
        self.state["text"].as_str().unwrap_or_default().to_owned()
    }

    pub fn revision(&self) -> u64 {
        self.state["revision"].as_u64().unwrap_or_default()
    }

    pub fn settled(&self) -> bool {
        self.state["pending"] == false
    }

    pub fn dirty(&self) -> bool {
        self.state["dirty"] == true
    }

    /// Distinct `document.conflict` events the page received.
    pub fn conflicts_seen(&self) -> usize {
        self.conflicts_received.len()
    }

    /// The conflict banners: every one shown so far, and those showing now.
    pub fn banners(&self) -> Value {
        json!({
            "shown": self.banners_shown.iter()
                .map(|(path, revision)| json!({"path": path, "revision": revision}))
                .collect::<Vec<_>>(),
            "showing": self.state["banners"].clone(),
            "revision": self.revision(),
            "dirty": self.dirty(),
        })
    }

    /// Type the difference to `target` with the page's keyboard: a prefix at
    /// the start of the document or a suffix at its end.
    pub async fn type_text(&mut self, target: &str) -> anyhow::Result<()> {
        let now = self.text();
        let (at, text) = if let Some(suffix) = target.strip_prefix(now.as_str()) {
            ("end", suffix)
        } else if let Some(prefix) = target.strip_suffix(now.as_str()) {
            ("start", prefix)
        } else {
            anyhow::bail!("the page types only at the start or the end");
        };
        let state = self
            .request(json!({"op": "type", "at": at, "text": text}))
            .await?;
        self.observe(state);
        Ok(())
    }

    pub async fn undo(&mut self) -> anyhow::Result<()> {
        let state = self.request(json!({"op": "undo"})).await?;
        self.observe(state);
        Ok(())
    }

    /// An operation from the page's own origin, with its session's CSRF token.
    pub async fn call(&self, operation: &str, arguments: Value) -> Result<Value, String> {
        let reply = self
            .request(json!({"op": "call", "operation": operation, "arguments": arguments}))
            .await
            .map_err(|error| error.to_string())?;
        if reply["status"] == 200 {
            Ok(reply["body"].clone())
        } else {
            Err(reply.to_string())
        }
    }

    /// Open OpenIDEA's Terminal tool window; the shell the host opens for it.
    pub async fn open_terminal(&mut self) -> anyhow::Result<Option<String>> {
        let opened = self.request(json!({"op": "terminal.open"})).await?;
        self.terminal = opened["session"].as_str().map(str::to_owned);
        Ok(self.terminal.clone())
    }

    pub async fn terminal_type(&self, text: &str) -> anyhow::Result<()> {
        self.request(json!({"op": "terminal.type", "text": text}))
            .await
            .map(drop)
    }

    pub async fn terminal_text(&self) -> String {
        self.request(json!({"op": "terminal.text"}))
            .await
            .ok()
            .and_then(|reply| reply["text"].as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    pub async fn shot(&self, path: &Path) {
        let _ = self.request(json!({"op": "shot", "path": path})).await;
    }

    /// Errors the page logged.
    pub async fn errors(&self) -> Vec<String> {
        self.request(json!({"op": "errors"}))
            .await
            .ok()
            .and_then(|reply| serde_json::from_value(reply["errors"].clone()).ok())
            .unwrap_or_default()
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        // Closing the driver's input closes its browser; it is killed only if
        // it has not exited ten seconds later.
        drop(std::mem::replace(&mut self.requests, mpsc::channel().0));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while matches!(self.driver.try_wait(), Ok(None)) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = self.driver.kill();
        let _ = self.driver.wait();
        let _ = Command::new("kill")
            .args(["-TERM", &format!("-{}", self.server.id())])
            .status();
        let _ = self.server.wait();
    }
}
