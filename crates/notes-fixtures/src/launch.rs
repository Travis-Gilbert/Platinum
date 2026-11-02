//! A throwaway Obsidian for the reference run: its own `HOME` and profile, a copy of the vault,
//! and a guard that stops it.
//!
//! The CLI page does not say how the CLI finds the app, so this comes from looking at the
//! installed 1.14.5 (not from running it): `obsidian-cli` only connects to `$HOME/.obsidian-cli.sock`
//! and prints "unable to find Obsidian" when nothing answers, it never starts the app (it imports no
//! spawn or exec call), and the app binds `os.homedir()/.obsidian-cli.sock` and reads its vault list
//! and the `cli` switch from `<userData>/obsidian.json`. A private `HOME` plus `--user-data-dir`
//! therefore leaves the CLI nothing to reach but the app started here, and a `sandbox-exec`
//! profile makes the kernel refuse any attempt on the real profile or the real socket.
//!
//! Nothing here reads, lists or writes the real profile: its path appears only as text in the
//! sandbox profile.
use crate::obsidian::{Obsidian, Record};
use anyhow::{Context, Result, bail, ensure};
use fs_extra::dir::CopyOptions;
use rustix::process::{Pid, Signal, kill_process_group};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use signal_hook::{
    consts::{SIGHUP, SIGINT, SIGTERM},
    iterator::{Handle, Signals},
};
use std::{
    fs::{self, File},
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread::sleep,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tempfile::TempDir;

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
const SOCKET: &str = ".obsidian-cli.sock";
/// `sockaddr_un.sun_path` is 104 bytes on macOS, the terminating NUL included.
const SUN_PATH_MAX: usize = 104;
const POLL: Duration = Duration::from_millis(100);
const GRACE: Duration = Duration::from_secs(10);
/// One readiness question; the app queues a request until the vault window is ready, so a slow
/// answer is asked again rather than waited for.
const ASK: Duration = Duration::from_secs(5);

/// Said when the app logged the request but never answered it.
const STALLED: &str = "\nthe app received the request but never answered it, which a blocked main thread does (a keychain prompt did, before --use-mock-keychain); `sample` on the app's pid shows where it waits";

/// Set by the signal thread before it kills the app, so that the main thread, which would see the
/// app die, leaves the exit to that thread.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// The read-only calls of `notes-fixtures probe`, all on <https://obsidian.md/help/cli>.
pub const PROBE: [(&str, &[&str]); 6] = [
    ("version", &[]),
    ("vault", &[]),
    ("vault", &["info=name"]),
    ("vault", &["info=path"]),
    ("files", &["total"]),
    ("help", &[]),
];

pub struct Options {
    /// The `Obsidian.app` folder.
    pub app: PathBuf,
    /// The vault to copy; the original is never opened.
    pub vault: PathBuf,
    /// How long the app gets to bind its socket and open the vault.
    pub startup: Duration,
    /// Extra Electron or Chromium flags for the app.
    pub args: Vec<String>,
}

/// Where the run happened, kept in the manifest.
#[derive(Debug, Clone, Serialize)]
pub struct Isolation {
    pub app: String,
    pub home: String,
    pub user_data_dir: String,
    pub vault_copy: String,
    /// Started under a `sandbox-exec` profile that denies the real profile and socket.
    pub sandboxed: bool,
}

pub struct Instance {
    child: Child,
    stopped: bool,
    signals: Handle,
    isolation: Isolation,
    cli: PathBuf,
    home: PathBuf,
    vault: PathBuf,
    log: PathBuf,
    real_socket: PathBuf,
    real_socket_before: Option<(u64, i64, i64)>,
    // Last, so the app is stopped (by `Drop`) before the directory goes.
    root: TempDir,
}

pub fn app_executable(app: &Path) -> PathBuf {
    app.join("Contents/MacOS/Obsidian")
}

pub fn app_cli(app: &Path) -> PathBuf {
    app.join("Contents/MacOS/obsidian-cli")
}

/// The sandbox profile: everything allowed except the real profile directory and the real CLI
/// socket, which are denied for files and for unix-socket connections, and writes to the real
/// app's window-restoration state.
pub fn sandbox_profile(real_home: &Path) -> String {
    let quote = |p: PathBuf| {
        p.display()
            .to_string()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    };
    let support = real_home.join("Library/Application Support");
    let lower = quote(support.join("obsidian"));
    let upper = quote(support.join("Obsidian"));
    let socket = quote(real_home.join(SOCKET));
    let restored = quote(real_home.join("Library/Saved Application State/md.obsidian.savedState"));
    format!(
        "(version 1)\n(allow default)\n\
         (deny file* (subpath \"{lower}\"))\n\
         (deny file* (subpath \"{upper}\"))\n\
         (deny file-write* (subpath \"{restored}\"))\n\
         (deny file* (literal \"{socket}\"))\n\
         (deny network-outbound (remote unix-socket (path-literal \"{socket}\")))\n\
         (deny network-bind (local unix-socket (path-literal \"{socket}\")))\n"
    )
}

/// What the app reads at start: the vault list (only the copy, opened at launch), `cli` on so the
/// CLI is served, and updates off.
fn seed(profile: &Path, vault: &Path) -> Result<()> {
    let digest = Sha256::digest(vault.as_os_str().as_encoded_bytes());
    let id: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    let mut vaults = serde_json::Map::new();
    vaults.insert(
        id,
        json!({ "path": vault.to_string_lossy(), "ts": ts, "open": true }),
    );
    let config = json!({ "vaults": vaults, "cli": true, "updateDisabled": true });
    fs::write(
        profile.join("obsidian.json"),
        serde_json::to_string_pretty(&config)?,
    )?;
    Ok(())
}

fn real_home() -> Result<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?);
    ensure!(
        home.is_absolute() && home.is_dir(),
        "HOME {} is not a directory",
        home.display()
    );
    Ok(home)
}

/// Inode and mtime of the real CLI socket, or `None` when there is none.
fn socket_state(path: &Path) -> Option<(u64, i64, i64)> {
    let m = fs::symlink_metadata(path).ok()?;
    Some((m.ino(), m.mtime(), m.mtime_nsec()))
}

/// Whether `vault`'s output (a bare path, or `path<TAB>...` lines) names `want`.
fn names_path(output: &str, want: &Path) -> bool {
    let Ok(want) = want.canonicalize() else {
        return false;
    };
    output
        .lines()
        .filter_map(|l| l.rsplit('\t').next())
        .any(|field| {
            Path::new(field.trim())
                .canonicalize()
                .is_ok_and(|p| p == want)
        })
}

impl Instance {
    /// Copies the vault, seeds the profile and starts the app, then waits until its socket is
    /// under the private home and the CLI answers for the copy. Any failure stops the app and
    /// removes the directory.
    pub fn start(opts: &Options) -> Result<Self> {
        let (exe, cli) = (app_executable(&opts.app), app_cli(&opts.app));
        for program in [&exe, &cli] {
            ensure!(
                program.is_file(),
                "{} is not there; --app takes the Obsidian.app folder",
                program.display()
            );
        }
        ensure!(
            Path::new(SANDBOX_EXEC).is_file(),
            "{SANDBOX_EXEC} is missing, and Obsidian is not started without it: its profile keeps the app off the real data"
        );
        let real_home = real_home()?;
        let vault = opts
            .vault
            .canonicalize()
            .with_context(|| format!("vault {}", opts.vault.display()))?;
        ensure!(vault.is_dir(), "{} is not a folder", vault.display());

        let root = tempfile::Builder::new().prefix("nfo-").tempdir()?;
        let base = root.path().canonicalize()?;
        ensure!(
            !base.starts_with(&real_home) && !real_home.starts_with(&base),
            "the private directory {} overlaps HOME",
            base.display()
        );
        let (home, profile, tmp, vaults) = (
            base.join("home"),
            base.join("profile"),
            base.join("tmp"),
            base.join("vaults"),
        );
        for dir in [&home, &profile, &tmp, &vaults] {
            fs::create_dir(dir)?;
        }
        let socket = home.join(SOCKET);
        ensure!(
            socket.as_os_str().len() < SUN_PATH_MAX,
            "{} is too long for a unix socket; point TMPDIR at a shorter directory, such as /tmp",
            socket.display()
        );
        fs_extra::dir::copy(&vault, &vaults, &CopyOptions::new()).context("copying the vault")?;
        let copy = vaults.join(vault.file_name().context("the vault has no name")?);
        seed(&profile, &copy)?;

        let log = base.join("app.log");
        let output = File::create(&log)?;
        let mut command = Command::new(SANDBOX_EXEC);
        command
            .arg("-p")
            .arg(sandbox_profile(&real_home))
            .arg(&exe)
            .arg(format!("--user-data-dir={}", profile.display()))
            // Chromium cannot start its own sandbox inside ours ("sandbox initialization failed").
            .arg("--no-sandbox")
            // Electron's safeStorage keeps its key in the login keychain. On the CLI's first
            // request the Security framework found no login keychain under the private home and
            // the main thread waited on a password prompt, so the request was never answered. The
            // mock keychain also keeps the app off the user's own keychain.
            .arg("--use-mock-keychain")
            .args(&opts.args)
            .current_dir(&home)
            .env_clear()
            .env("HOME", &home)
            // Foundation finds the home through the user database, not `HOME`; this sends
            // NSHomeDirectory to the private home as well. Defaults still come from the real
            // user's domain, since cfprefsd serves them and ignores this variable.
            .env("CFFIXED_USER_HOME", &home)
            .env("TMPDIR", &tmp)
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .stdin(Stdio::null())
            .stdout(output.try_clone()?)
            .stderr(output)
            .process_group(0);
        for key in ["USER", "LOGNAME"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let real_socket = real_home.join(SOCKET);
        let real_socket_before = socket_state(&real_socket);
        let mut signals = Signals::new([SIGINT, SIGTERM, SIGHUP])?;
        let handle = signals.handle();
        let child = command
            .spawn()
            .with_context(|| format!("starting {}", exe.display()))?;

        // An interrupt must not leave the app running: stop its group and the directory, then go.
        let (pid, dir) = (Pid::from_child(&child), base.clone());
        std::thread::spawn(move || {
            if signals.forever().next().is_some() {
                INTERRUPTED.store(true, Ordering::SeqCst);
                let _ = kill_process_group(pid, Signal::KILL);
                let _ = fs::remove_dir_all(&dir);
                std::process::exit(130);
            }
        });

        let mut instance = Instance {
            child,
            stopped: false,
            signals: handle,
            isolation: Isolation {
                app: opts.app.display().to_string(),
                home: home.display().to_string(),
                user_data_dir: profile.display().to_string(),
                vault_copy: copy.display().to_string(),
                sandboxed: true,
            },
            cli,
            home,
            vault: copy,
            log,
            real_socket,
            real_socket_before,
            root,
        };
        instance.wait_ready(&socket, opts.startup)?;
        Ok(instance)
    }

    fn wait_until(
        &mut self,
        deadline: Instant,
        mut ready: impl FnMut(&mut Self) -> Result<bool>,
    ) -> Result<bool> {
        loop {
            if let Some(status) = self.child.try_wait()? {
                while INTERRUPTED.load(Ordering::SeqCst) {
                    std::thread::park();
                }
                bail!(
                    "Obsidian exited with {status} before it was ready\n{}",
                    self.log_tail()
                );
            }
            if ready(self)? {
                return Ok(true);
            }
            if Instant::now() >= deadline {
                return Ok(false);
            }
            sleep(POLL);
        }
    }

    fn wait_ready(&mut self, socket: &Path, startup: Duration) -> Result<()> {
        let deadline = Instant::now() + startup;
        let bound = self.wait_until(deadline, |_| {
            Ok(fs::symlink_metadata(socket).is_ok_and(|m| m.file_type().is_socket()))
        })?;
        ensure!(
            bound,
            "Obsidian did not bind {} within {startup:?}; refusing to go on\n{}",
            socket.display(),
            self.log_tail()
        );
        let mut last = None;
        let answered = self.wait_until(deadline, |me| {
            let said = me
                .client()
                .timeout(ASK)
                .run("vault", &["info=path"]);
            // A failure or a blank answer is the vault window not being open yet: ask again.
            let ready = said.ok() && !said.stdout.trim().is_empty();
            if ready {
                ensure!(
                    names_path(&said.stdout, &me.vault),
                    "the CLI answered {:?}, not the vault copy {}: it is not talking to the instance started here, so nothing more is run",
                    said.stdout.trim(),
                    me.vault.display()
                );
            }
            last = Some(said);
            Ok(ready)
        })?;
        ensure!(
            answered,
            "the CLI gave no answer for {} within {startup:?}; its last reply to `vault info=path`: {}{}\n{}",
            self.vault.display(),
            last.map_or_else(
                || "none".into(),
                |r| format!(
                    "exit {:?}, stdout {:?}, stderr {:?}, {}",
                    r.exit_code,
                    r.stdout,
                    r.stderr,
                    r.error.unwrap_or_default()
                )
            ),
            if self.log().contains("Received command line") {
                STALLED
            } else {
                ""
            },
            self.log_tail()
        );
        Ok(())
    }

    /// The CLI, aimed at this instance: its `HOME` is the private one and its working directory
    /// is the vault copy.
    pub fn client(&self) -> Obsidian {
        Obsidian::new(&self.cli, &self.vault).env("HOME", &self.home.to_string_lossy())
    }

    pub fn isolation(&self) -> Isolation {
        self.isolation.clone()
    }

    pub fn root(&self) -> &Path {
        self.root.path()
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// What the app wrote to stdout and stderr so far.
    pub fn log(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn log_tail(&self) -> String {
        let text = fs::read_to_string(&self.log).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let tail = lines[lines.len().saturating_sub(60)..].join("\n");
        format!("app log {}:\n{tail}", self.log.display())
    }

    /// SIGTERM to the app's process group, a grace period, then SIGKILL to what is left.
    fn stop(&mut self) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        let pid = Pid::from_child(&self.child);
        let _ = kill_process_group(pid, Signal::TERM);
        let deadline = Instant::now() + GRACE;
        while Instant::now() < deadline && !matches!(self.child.try_wait(), Ok(Some(_))) {
            sleep(Duration::from_millis(50));
        }
        let _ = kill_process_group(pid, Signal::KILL);
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.signals.close();
    }

    /// Stops the app and checks that it was still running and that the real CLI socket was not
    /// touched. The directory goes when the instance is dropped.
    pub fn quit(mut self) -> Result<()> {
        if let Some(status) = self.child.try_wait()?
            && !INTERRUPTED.load(Ordering::SeqCst)
        {
            bail!(
                "Obsidian exited with {status} during the run, so later answers are missing\n{}",
                self.log_tail()
            );
        }
        self.stop();
        let after = socket_state(&self.real_socket);
        ensure!(
            after == self.real_socket_before,
            "{} changed while Obsidian ran (inode and mtime before {:?}, after {after:?})\n{}",
            self.real_socket.display(),
            self.real_socket_before,
            self.log_tail()
        );
        Ok(())
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Runs [`PROBE`] against the instance.
pub fn probe(instance: &Instance) -> Vec<Record> {
    let client = instance.client();
    PROBE
        .iter()
        .map(|(command, args)| client.run(command, args))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_denies_the_real_profile_and_socket_and_nothing_else() {
        let p = sandbox_profile(Path::new("/Users/me"));
        assert!(p.starts_with("(version 1)\n(allow default)\n"));
        for denied in [
            "(subpath \"/Users/me/Library/Application Support/obsidian\")",
            "(subpath \"/Users/me/Library/Application Support/Obsidian\")",
            "(deny file-write* (subpath \"/Users/me/Library/Saved Application State/md.obsidian.savedState\"))",
            "(literal \"/Users/me/.obsidian-cli.sock\")",
            "(path-literal \"/Users/me/.obsidian-cli.sock\")",
        ] {
            assert!(p.contains(denied), "{denied} in\n{p}");
        }
        assert_eq!(p.matches("(deny ").count(), 6);
        let odd = sandbox_profile(Path::new("/Users/a\"b"));
        assert!(odd.contains("/Users/a\\\"b/"), "{odd}");
    }

    #[test]
    fn the_profile_registers_only_the_copy_and_turns_the_cli_on() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path(), Path::new("/x/vault")).unwrap();
        let config: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("obsidian.json")).unwrap())
                .unwrap();
        assert_eq!(config["cli"], true);
        assert_eq!(config["updateDisabled"], true);
        let vaults = config["vaults"].as_object().unwrap();
        assert_eq!(vaults.len(), 1);
        let (id, vault) = vaults.iter().next().unwrap();
        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(vault["path"], "/x/vault");
        assert_eq!(vault["open"], true);
        assert!(vault["ts"].as_u64().unwrap() > 1_600_000_000_000);
    }

    #[test]
    fn the_vault_output_may_be_a_path_or_a_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap();
        let shown = path.display();
        assert!(names_path(&format!("{shown}\n"), &path));
        assert!(names_path(&format!("name\tv\npath\t{shown}\n"), &path));
        assert!(!names_path("/nowhere\n", &path));
        assert!(!names_path("", &path));
    }

    #[test]
    fn the_kernel_refuses_what_the_profile_denies() {
        let home = tempfile::tempdir().unwrap();
        let support = home.path().join("Library/Application Support/obsidian");
        fs::create_dir_all(&support).unwrap();
        fs::write(support.join("obsidian.json"), "private").unwrap();
        fs::write(home.path().join("elsewhere.txt"), "public").unwrap();
        let cat = |file: &Path| {
            Command::new(SANDBOX_EXEC)
                .arg("-p")
                .arg(sandbox_profile(&home.path().canonicalize().unwrap()))
                .arg("/bin/cat")
                .arg(file.canonicalize().unwrap())
                .output()
                .unwrap()
        };
        let allowed = cat(&home.path().join("elsewhere.txt"));
        assert!(allowed.status.success(), "{allowed:?}");
        assert_eq!(allowed.stdout, b"public");
        let denied = cat(&support.join("obsidian.json"));
        assert!(!denied.status.success());
        assert!(
            String::from_utf8_lossy(&denied.stderr).contains("Operation not permitted"),
            "{denied:?}"
        );
    }
}
