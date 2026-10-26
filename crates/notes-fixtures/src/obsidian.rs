//! The Obsidian CLI as a process: `[vault=<name>] <command> [key=value | flag]...`.
//!
//! Syntax, from <https://obsidian.md/help/cli>: a parameter is `key=value`, a flag is a bare word,
//! `vault=<name>` must come first, and a terminal whose working directory is a vault folder
//! selects that vault when `vault=` is absent. The page says the first command starts the app if
//! it is not running; the macOS binary of 1.14.5 only connects to a socket and tells you to start
//! Obsidian (see [`crate::launch`]), so nothing here relies on that.
use regex::Regex;
use serde::Serialize;
use std::{
    io::{self, Read},
    path::PathBuf,
    process::{Command, ExitStatus, Stdio},
    thread,
    time::Duration,
};
use wait_timeout::ChildExt;

/// Long enough for the slowest documented call (`eval` of the whole metadata cache, a note's DOM).
const TIMEOUT: Duration = Duration::from_secs(120);

pub struct Obsidian {
    program: PathBuf,
    cwd: PathBuf,
    vault: Option<String>,
    envs: Vec<(String, String)>,
    timeout: Duration,
}

/// One invocation and everything it printed. The answers are kept verbatim: most output shapes
/// are not documented, so nothing here guesses at them.
#[derive(Debug, Clone, Serialize)]
pub struct Record {
    pub command: String,
    pub args: Vec<String>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Set when the process could not start or exited with a failure status.
    pub error: Option<String>,
    /// `stdout` as JSON, when the call asked for JSON and the output parses as it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed: Option<serde_json::Value>,
    /// Where `stdout` was written when it is too large to keep inline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout_file: Option<String>,
}

impl Record {
    pub fn ok(&self) -> bool {
        self.error.is_none()
    }

    /// The call as typed, quoting values with spaces: the key records are stored under.
    pub fn key(&self) -> String {
        let quote = |a: &str| match a.split_once('=') {
            Some((k, v)) if v.contains(char::is_whitespace) => format!("{k}=\"{v}\""),
            _ => a.to_string(),
        };
        std::iter::once(self.command.clone())
            .chain(self.args.iter().map(|a| quote(a)))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The value an `eval` call returned. The docs say only "return result", so this accepts a
    /// leading `=> `, a bare JSON value, and a JSON string that itself holds JSON (what
    /// `JSON.stringify` gives back).
    pub fn eval_value(&self) -> Option<serde_json::Value> {
        let text = self.stdout.trim();
        let text = text.strip_prefix("=>").map_or(text, str::trim);
        match serde_json::from_str(text).ok()? {
            serde_json::Value::String(s) => serde_json::from_str(&s)
                .ok()
                .or(Some(serde_json::Value::String(s))),
            v => Some(v),
        }
    }
}

impl Obsidian {
    pub fn new(program: impl Into<PathBuf>, vault_dir: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            cwd: vault_dir.into(),
            vault: None,
            envs: Vec::new(),
            timeout: TIMEOUT,
        }
    }

    /// How long one call may take before the CLI is killed and the call is recorded as failed.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Passes `vault=<name>` ahead of every command.
    pub fn vault_name(mut self, name: Option<String>) -> Self {
        self.vault = name;
        self
    }

    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.envs.push((key.into(), value.into()));
        self
    }

    pub fn argv(&self, command: &str, args: &[&str]) -> Vec<String> {
        self.vault
            .iter()
            .map(|v| format!("vault={v}"))
            .chain(std::iter::once(command.to_string()))
            .chain(args.iter().map(|a| a.to_string()))
            .collect()
    }

    pub fn run(&self, command: &str, args: &[&str]) -> Record {
        let mut record = Record {
            command: command.into(),
            args: args.iter().map(|a| a.to_string()).collect(),
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: None,
            parsed: None,
            stdout_file: None,
        };
        let mut process = Command::new(&self.program);
        process
            .args(self.argv(command, args))
            .current_dir(&self.cwd);
        for (k, v) in &self.envs {
            process.env(k, v);
        }
        match self.capture(&mut process) {
            Ok((status, stdout, stderr)) => {
                record.stdout = String::from_utf8_lossy(&stdout).into_owned();
                record.stderr = String::from_utf8_lossy(&stderr).into_owned();
                match status {
                    None => {
                        record.error = Some(format!("no answer within {:?}; killed", self.timeout));
                    }
                    Some(status) => {
                        record.exit_code = status.code();
                        if !status.success() {
                            record.error = Some(format!("exited with {status}"));
                        } else if let Some(said) = cli_error(&record.stdout) {
                            record.error = Some(format!("the app answered: {said}"));
                        }
                    }
                }
            }
            Err(e) => record.error = Some(format!("could not run {}: {e}", self.program.display())),
        }
        if record.ok() && args.contains(&"format=json") {
            record.parsed = serde_json::from_str(record.stdout.trim()).ok();
        }
        record
    }
}

/// What the CLI printed, drained on two threads so a large answer cannot fill a pipe.
fn drain(mut pipe: impl Read) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = pipe.read_to_end(&mut buf);
    buf
}

impl Obsidian {
    /// Runs the CLI with stdin closed; `None` for the status means it was killed at the timeout.
    fn capture(&self, process: &mut Command) -> io::Result<(Option<ExitStatus>, Vec<u8>, Vec<u8>)> {
        let mut child = process
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let (stdout, stderr) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
        thread::scope(|scope| {
            let out = scope.spawn(move || drain(stdout));
            let err = scope.spawn(move || drain(stderr));
            let status = child.wait_timeout(self.timeout)?;
            if status.is_none() {
                child.kill()?;
                child.wait()?;
            }
            Ok((status, out.join().unwrap(), err.join().unwrap()))
        })
    }
}

/// The app reports failures as text on a successful exit: `handleCli` throws, and the main process
/// prints `Error: <message>`; a vault it does not know, or a CLI that is switched off, get a bare
/// sentence. (Read from the 1.14.5 bundle.)
pub fn cli_error(stdout: &str) -> Option<&str> {
    let first = stdout.trim_start().lines().next()?.trim();
    (first.starts_with("Error:")
        || first == "Vault not found."
        || first.starts_with("Command line interface is not enabled"))
    .then_some(first)
}

/// The first `x.y.z` in what `version` printed. The third-party write-ups show
/// `1.12.7 (installer 1.12.7)`; the docs say only "Show Obsidian version".
pub fn parse_version(output: &str) -> Option<String> {
    let triple = Regex::new(r"(?:^|[^\d.])(\d+\.\d+\.\d+)").ok()?;
    triple.captures(output).map(|c| c[1].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_the_first_triple() {
        assert_eq!(
            parse_version("1.14.5 (installer 1.12.7)\n").as_deref(),
            Some("1.14.5")
        );
        assert_eq!(parse_version("Obsidian v1.12.7").as_deref(), Some("1.12.7"));
        assert_eq!(parse_version("no version here"), None);
    }

    #[test]
    fn vault_comes_first() {
        let o = Obsidian::new("obsidian", ".").vault_name(Some("My Vault".into()));
        assert_eq!(
            o.argv("search", &["query=a b", "total"]),
            ["vault=My Vault", "search", "query=a b", "total"]
        );
        assert_eq!(
            Obsidian::new("obsidian", ".").argv("version", &[]),
            ["version"]
        );
    }

    #[test]
    fn keys_quote_values_with_spaces() {
        let o = Obsidian::new("/nonexistent/obsidian", ".");
        let r = o.run("search", &["query=two words", "total"]);
        assert_eq!(r.key(), "search query=\"two words\" total");
        assert!(
            !r.ok(),
            "a program that does not exist is recorded as a failure"
        );
    }

    #[test]
    fn the_apps_error_text_is_a_failure_even_on_exit_zero() {
        assert_eq!(
            cli_error("Error: Command \"x\" not found.\n"),
            Some("Error: Command \"x\" not found.")
        );
        assert_eq!(cli_error("Vault not found."), Some("Vault not found."));
        assert!(cli_error("Command line interface is not enabled. Please turn it on").is_some());
        assert_eq!(cli_error("notes/Error handling.md\nHome.md"), None);
        assert_eq!(cli_error(""), None);
    }

    #[test]
    fn eval_values_unwrap_stringified_json() {
        let mk = |s: &str| Record {
            command: "eval".into(),
            args: vec![],
            exit_code: Some(0),
            stdout: s.into(),
            stderr: String::new(),
            error: None,
            parsed: None,
            stdout_file: None,
        };
        assert_eq!(
            mk("=> {\"a\":1}\n").eval_value(),
            Some(serde_json::json!({"a": 1}))
        );
        assert_eq!(
            mk("\"{\\\"a\\\":1}\"").eval_value(),
            Some(serde_json::json!({"a": 1}))
        );
        assert_eq!(
            mk("\"preview\"").eval_value(),
            Some(serde_json::json!("preview"))
        );
        assert_eq!(mk("not json").eval_value(), None);
    }
}
