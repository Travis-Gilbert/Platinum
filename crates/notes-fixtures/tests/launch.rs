#![cfg(target_os = "macos")]
//! The isolated launch against a fake `Obsidian.app`: a script that records its environment and
//! arguments, tries to read a stand-in for the real profile and touch a stand-in for the real
//! socket, and then holds a unix socket under the `HOME` it was given. No real Obsidian is needed.
mod common;

use common::{SCRIPT, scratch, vault, write_executable};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
enum Mode {
    /// Records itself and holds a socket at `$HOME/.obsidian-cli.sock`, as the app does.
    Serve,
    /// Prints an error and exits.
    Exit,
    /// Starts, never binds a socket.
    Silent,
    /// Serves until the CLI has been asked for `files`, then exits mid-run.
    Dies,
}

struct World {
    dir: PathBuf,
    /// A short TMPDIR: unix socket paths are limited to 104 bytes.
    tmp: PathBuf,
    real_home: PathBuf,
    app: PathBuf,
    app_log: PathBuf,
    cli_log: PathBuf,
    real_socket: PathBuf,
}

impl World {
    fn new(name: &str, short: &str, mode: Mode) -> Self {
        let dir = scratch(name);
        let tmp = std::env::temp_dir().join(short);
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let tmp = tmp.canonicalize().unwrap();
        let real_home = dir.join("home");
        let support = real_home.join("Library/Application Support/obsidian");
        fs::create_dir_all(&support).unwrap();
        fs::write(support.join("secret.txt"), "private").unwrap();
        let real_socket = real_home.join(".obsidian-cli.sock");
        fs::write(&real_socket, "the real socket").unwrap();
        let app = dir.join("Fake.app");
        let world = World {
            app_log: dir.join("app.log"),
            cli_log: dir.join("cli.log"),
            dir,
            tmp,
            real_home,
            app,
            real_socket,
        };
        write_executable(
            &world.app.join("Contents/MacOS/Obsidian"),
            &world.script(mode),
        );
        write_executable(&world.app.join("Contents/MacOS/obsidian-cli"), SCRIPT);
        world
    }

    fn script(&self, mode: Mode) -> String {
        let log = self.app_log.display();
        let secret = self
            .real_home
            .join("Library/Application Support/obsidian/secret.txt");
        match mode {
            Mode::Serve => format!(
                r#"#!/bin/sh
{{
  echo "pid $$"
  echo "home $HOME"
  echo "tmpdir $TMPDIR"
  echo "args $*"
  env | sort | sed 's/^/env /'
  if cat '{secret}' >/dev/null 2>&1; then echo "secret readable"; else echo "secret denied"; fi
  if touch '{socket}' 2>/dev/null; then echo "socket touched"; else echo "socket denied"; fi
  if ls '{support}' >/dev/null 2>&1; then echo "support listed"; else echo "support denied"; fi
}} > '{log}'
cp "${{1#--user-data-dir=}}/obsidian.json" '{log}.json'
exec nc -lU "$HOME/.obsidian-cli.sock"
"#,
                secret = secret.display(),
                socket = self.real_socket.display(),
                support = secret.parent().unwrap().display(),
            ),
            Mode::Exit => "#!/bin/sh\necho 'boom: cannot open a window' >&2\nexit 3\n".into(),
            Mode::Silent => format!("#!/bin/sh\necho \"pid $$\" > '{log}'\nexec sleep 300\n"),
            Mode::Dies => format!(
                r#"#!/bin/sh
echo "pid $$" > '{log}'
nc -lU "$HOME/.obsidian-cli.sock" &
until grep -q 'argv.files' '{cli}' 2>/dev/null; do sleep 0.05; done
echo 'the vault window closed' >&2
exit 0
"#,
                cli = self.cli_log.display(),
            ),
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_notes-fixtures"));
        cmd.args(args)
            .env("HOME", &self.real_home)
            .env("TMPDIR", &self.tmp)
            .env("FAKE_OBSIDIAN_LOG", &self.cli_log)
            // None of these may reach the app.
            .env("NODE_OPTIONS", "--inspect=0")
            .env("ELECTRON_RUN_AS_NODE", "1");
        cmd
    }

    fn reference(&self, extra: &[&str], envs: &[(&str, &str)]) -> Output {
        let out = self.dir.join("out");
        let vault = vault();
        let mut args = vec![
            "reference",
            "--app",
            self.app.to_str().unwrap(),
            "--vault",
            vault.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--settle-ms",
            "0",
        ];
        args.extend_from_slice(extra);
        let mut cmd = self.command(&args);
        for (k, v) in envs {
            cmd.env(k, v);
        }
        cmd.output().unwrap()
    }

    /// `key rest-of-line` pairs of what the fake app recorded; `env` lines are kept apart.
    fn app(&self) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
        let text = fs::read_to_string(&self.app_log).unwrap_or_default();
        let (mut said, mut env) = (BTreeMap::new(), BTreeMap::new());
        for line in text.lines() {
            let (key, rest) = line.split_once(' ').unwrap_or((line, ""));
            if key == "env" {
                let (k, v) = rest.split_once('=').unwrap_or((rest, ""));
                env.insert(k.to_string(), v.to_string());
            } else {
                said.insert(key.to_string(), rest.to_string());
            }
        }
        (said, env)
    }

    fn pid(&self) -> String {
        self.app().0["pid"].clone()
    }

    /// Nothing the run created is left in the TMPDIR.
    fn tmp_is_empty(&self) -> bool {
        fs::read_dir(&self.tmp).unwrap().next().is_none()
    }
}

fn gone(pid: &str) -> bool {
    !Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

fn text(out: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn json(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_app_runs_on_a_private_home_and_profile_and_is_gone_afterwards() {
    let w = World::new("launch-private", "t1", Mode::Serve);
    let socket_before = fs::metadata(&w.real_socket).unwrap();
    let run = w.reference(
        &[
            "--only",
            "vault,tags",
            "--startup-secs",
            "20",
            "--app-arg=--force-device-scale-factor=1",
        ],
        &[],
    );
    assert!(run.status.success(), "{}", text(&run));
    assert!(String::from_utf8_lossy(&run.stdout).starts_with("Obsidian 1.14.5: "));

    let (said, env) = w.app();
    let home = Path::new(&said["home"]);
    let root = home.parent().unwrap();
    assert!(
        home.starts_with(&w.tmp) && !home.starts_with(&w.real_home),
        "{home:?}"
    );
    assert!(
        root.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("nfo-")
    );
    assert_eq!(
        said["args"],
        format!(
            // The mock keychain, so the app never waits on a keychain prompt.
            "--user-data-dir={} --no-sandbox --use-mock-keychain --force-device-scale-factor=1",
            root.join("profile").display()
        )
    );
    assert_eq!(said["tmpdir"], root.join("tmp").display().to_string());
    for leaked in ["NODE_OPTIONS", "ELECTRON_RUN_AS_NODE", "FAKE_OBSIDIAN_LOG"] {
        assert!(
            !env.contains_key(leaked),
            "{leaked} reached the app: {env:?}"
        );
    }
    assert_eq!(env["HOME"], said["home"]);

    // The sandbox profile is what keeps a misdirected app off the real data.
    assert_eq!(said["secret"], "denied", "{said:?}");
    assert_eq!(said["support"], "denied", "{said:?}");
    assert_eq!(said["socket"], "denied", "{said:?}");
    let socket_after = fs::metadata(&w.real_socket).unwrap();
    assert_eq!(
        socket_before.modified().unwrap(),
        socket_after.modified().unwrap()
    );
    assert_eq!(
        fs::read_to_string(&w.real_socket).unwrap(),
        "the real socket"
    );

    // The profile it was given registers only the copy, with the CLI on and updates off.
    let seeded = json(&w.app_log.with_extension("log.json"));
    assert_eq!(
        (seeded["cli"].clone(), seeded["updateDisabled"].clone()),
        (true.into(), true.into())
    );
    let vaults = seeded["vaults"].as_object().unwrap();
    assert_eq!(vaults.len(), 1);
    let copy = vaults.values().next().unwrap()["path"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(copy, root.join("vaults/vault").display().to_string());
    assert_eq!(vaults.values().next().unwrap()["open"], true);

    // Every CLI call ran with the private HOME, in the copy, never in the original.
    let homes = fs::read_to_string(format!("{}.home", w.cli_log.display())).unwrap();
    assert!(homes.lines().count() > 3);
    assert!(homes.lines().all(|h| h == said["home"]), "{homes}");
    let log = fs::read_to_string(&w.cli_log).unwrap();
    let cwds: Vec<&str> = log
        .lines()
        .filter_map(|l| l.strip_prefix("cwd\u{1f}"))
        .collect();
    assert!(cwds.iter().all(|c| *c == copy), "{cwds:?}");
    assert!(log.contains("argv\u{1f}vault\u{1f}info=path"));

    let manifest = json(&w.dir.join("out/manifest.json"));
    assert_eq!(manifest["isolation"]["sandboxed"], true);
    assert_eq!(manifest["isolation"]["vault_copy"], "__vault__");
    assert_eq!(
        manifest["vault"], "crates/notes-fixtures/vault",
        "the manifest names the vault that was copied"
    );

    assert!(gone(&w.pid()), "the app is still running");
    assert!(!root.exists(), "{root:?} was not removed");
    assert!(w.tmp_is_empty());
}

#[test]
fn a_wrong_version_stops_the_app_and_writes_nothing() {
    let w = World::new("launch-version", "t2", Mode::Serve);
    let run = w.reference(
        &["--startup-secs", "20"],
        &[("FAKE_OBSIDIAN_VERSION", "1.13.0")],
    );
    assert!(!run.status.success());
    assert!(text(&run).contains("refusing to run"), "{}", text(&run));
    assert!(!w.dir.join("out").exists());
    assert!(gone(&w.pid()));
    assert!(w.tmp_is_empty());
}

#[test]
fn a_cli_that_answers_for_another_vault_is_refused_at_once() {
    let w = World::new("launch-other-vault", "t3", Mode::Serve);
    let run = w.reference(
        &["--startup-secs", "20"],
        &[("FAKE_OBSIDIAN_VAULT_PATH", "/etc")],
    );
    assert!(!run.status.success());
    assert!(
        text(&run).contains("not talking to the instance started here"),
        "{}",
        text(&run)
    );
    assert!(!w.dir.join("out").exists());
    let calls = fs::read_to_string(&w.cli_log).unwrap();
    assert_eq!(
        calls.matches("argv").count(),
        1,
        "nothing is run after the refusal:\n{calls}"
    );
    assert!(gone(&w.pid()));
    assert!(w.tmp_is_empty());
}

#[test]
fn an_app_that_exits_is_reported_with_its_log_without_waiting_out_the_timeout() {
    let w = World::new("launch-exit", "t4", Mode::Exit);
    let started = Instant::now();
    let run = w.reference(&["--startup-secs", "60"], &[]);
    assert!(!run.status.success());
    let say = text(&run);
    assert!(
        say.contains("exited with") && say.contains("boom: cannot open a window"),
        "{say}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "{:?}",
        started.elapsed()
    );
    assert!(!w.dir.join("out").exists());
    assert!(w.tmp_is_empty());
}

#[test]
fn an_app_that_dies_mid_run_fails_the_run_with_its_log() {
    let w = World::new("launch-dies", "t10", Mode::Dies);
    let run = w.reference(&["--only", "vault,tags", "--startup-secs", "20"], &[]);
    assert!(!run.status.success());
    let say = text(&run);
    assert!(
        say.contains("exited with") && say.contains("during the run"),
        "{say}"
    );
    assert!(say.contains("the vault window closed"), "{say}");
    assert!(w.tmp_is_empty());
}

#[test]
fn an_app_that_never_binds_its_socket_is_stopped_after_the_timeout() {
    let w = World::new("launch-silent", "t5", Mode::Silent);
    let run = w.reference(&["--startup-secs", "1"], &[]);
    assert!(!run.status.success());
    assert!(text(&run).contains("did not bind"), "{}", text(&run));
    assert!(gone(&w.pid()), "the silent app was left running");
    assert!(w.tmp_is_empty());
}

#[test]
fn an_interrupt_stops_the_app_and_removes_the_directory() {
    let w = World::new("launch-signal", "t6", Mode::Silent);
    let out = w.dir.join("out");
    let vault = vault();
    let mut harness = w
        .command(&[
            "reference",
            "--app",
            w.app.to_str().unwrap(),
            "--vault",
            vault.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--startup-secs",
            "120",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !fs::read_to_string(&w.app_log)
        .unwrap_or_default()
        .contains("pid")
    {
        assert!(Instant::now() < deadline, "the fake app never started");
        std::thread::sleep(Duration::from_millis(50));
    }
    let pid = w.pid();
    assert!(!gone(&pid));
    assert!(
        Command::new("kill")
            .args(["-TERM", &harness.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let status = harness.wait().unwrap();
    assert_eq!(status.code(), Some(130), "{status:?}");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !gone(&pid) {
        assert!(Instant::now() < deadline, "the app outlived the harness");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(w.tmp_is_empty());
    assert!(!out.exists());
}

#[test]
fn probe_asks_a_few_read_only_questions_and_quits_the_app() {
    let w = World::new("launch-probe", "t7", Mode::Serve);
    let vault = vault();
    let run = w
        .command(&[
            "probe",
            "--app",
            w.app.to_str().unwrap(),
            "--vault",
            vault.to_str().unwrap(),
            "--startup-secs",
            "20",
        ])
        .output()
        .unwrap();
    assert!(run.status.success(), "{}", text(&run));
    let say = String::from_utf8_lossy(&run.stdout);
    for call in [
        "$ obsidian version",
        "$ obsidian vault info=name",
        "$ obsidian vault info=path",
        "$ obsidian files total",
        "$ obsidian help",
    ] {
        assert!(say.contains(call), "{call} in\n{say}");
    }
    assert!(say.contains("\"sandboxed\": true"));
    assert!(gone(&w.pid()));
    assert!(w.tmp_is_empty());
}

#[test]
fn a_missing_app_is_an_error_before_anything_is_created() {
    let w = World::new("launch-missing", "t8", Mode::Serve);
    let vault = vault();
    let run = w
        .command(&[
            "reference",
            "--app",
            "/nonexistent/Obsidian.app",
            "--vault",
            vault.to_str().unwrap(),
            "--out",
            w.dir.join("out").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!run.status.success());
    assert!(
        text(&run).contains("--app takes the Obsidian.app folder"),
        "{}",
        text(&run)
    );
    assert!(w.tmp_is_empty());
    assert!(!w.dir.join("out").exists());
}

#[test]
fn a_run_needs_either_a_private_app_or_an_explicit_attach() {
    let w = World::new("launch-flags", "t9", Mode::Serve);
    let vault = vault();
    let out = w.dir.join("out");
    let base = [
        "reference",
        "--vault",
        vault.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ];
    let neither = w.command(&base).output().unwrap();
    assert_eq!(neither.status.code(), Some(2));
    let say = String::from_utf8_lossy(&neither.stderr);
    assert!(say.contains("--app") && say.contains("--attach"), "{say}");

    let mut both = base.to_vec();
    both.extend(["--app", w.app.to_str().unwrap(), "--attach"]);
    assert_eq!(w.command(&both).output().unwrap().status.code(), Some(2));

    let mut named = base.to_vec();
    named.extend(["--app", w.app.to_str().unwrap(), "--obsidian", "obsidian"]);
    assert_eq!(w.command(&named).output().unwrap().status.code(), Some(2));
    assert!(w.tmp_is_empty());
}
