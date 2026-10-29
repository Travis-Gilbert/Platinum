use frame_notes::{ReconcileResult, Vault};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const BASE: &str = "first\nmiddle\nlast\n";

#[test]
fn child_worker() {
    let Ok(mode) = std::env::var("FRAME_NOTES_CHILD") else {
        return;
    };
    let root = PathBuf::from(std::env::var_os("FRAME_NOTES_ROOT").unwrap());
    let data = PathBuf::from(std::env::var_os("FRAME_NOTES_DATA").unwrap());
    let mut vault = Vault::open(&root, &data).unwrap();
    match mode.as_str() {
        "g6" => {
            let d = vault.open_document("note.md").unwrap();
            vault
                .edit(
                    "note.md",
                    d.revision,
                    "g6-local",
                    "first local\nmiddle\nlast\n",
                )
                .unwrap();
            fs::write(root.join("note.md"), "first\nmiddle\nlast disk\n").unwrap();
            assert!(matches!(
                vault.refresh().unwrap()[0].1,
                ReconcileResult::Applied { .. }
            ));
            vault.save("note.md").unwrap();
            assert_eq!(
                fs::read_to_string(root.join("note.md")).unwrap(),
                "first local\nmiddle\nlast disk\n"
            );
            // A real socket syscall proves this child actually runs under the requested network denial.
            assert!(std::net::TcpListener::bind("127.0.0.1:0").is_err());
        }
        "save" => {
            let d = vault.open_document("note.md").unwrap();
            vault
                .edit(
                    "note.md",
                    d.revision,
                    "accepted-before-kill",
                    &format!("# Accepted\n{}", "durable text\n".repeat(10_000)),
                )
                .unwrap();
            fs::write(data.join("ready"), b"accepted").unwrap();
            loop {
                vault.save("note.md").unwrap();
            }
        }
        "index" => {
            fs::write(
                root.join("note.md"),
                format!("# External\n{}", "[[Missing]] #tag\n".repeat(1_000)),
            )
            .unwrap();
            fs::write(data.join("ready"), b"disk-written").unwrap();
            loop {
                vault.refresh().unwrap();
                vault.rebuild_index().unwrap();
            }
        }
        _ => panic!("unknown child mode"),
    }
}

fn child(mode: &str, root: &std::path::Path, data: &std::path::Path) -> Command {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "child_worker", "--nocapture"])
        .env("FRAME_NOTES_CHILD", mode)
        .env("FRAME_NOTES_ROOT", root)
        .env("FRAME_NOTES_DATA", data)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    cmd
}

fn killed(mode: &str) {
    for delay in [0, 3, 15] {
        let root = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        fs::write(root.path().join("note.md"), BASE).unwrap();
        drop(Vault::open(root.path(), data.path()).unwrap());
        let mut process = child(mode, root.path(), data.path()).spawn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while !data.path().join("ready").exists() {
            if let Some(status) = process.try_wait().unwrap() {
                panic!("child exited before operation: {status}");
            }
            if Instant::now() >= deadline {
                process.kill().unwrap();
                process.wait().unwrap();
                panic!("child did not reach operation");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(delay));
        process.kill().unwrap();
        let status = process.wait().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(status.signal(), Some(libc::SIGKILL));
        }
        let disk = fs::read_to_string(root.path().join("note.md")).unwrap();
        let mut v = Vault::open(root.path(), data.path()).unwrap();
        if mode == "save" {
            let accepted = format!("# Accepted\n{}", "durable text\n".repeat(10_000));
            assert!(disk == BASE || disk == accepted, "truncated save");
            assert_eq!(
                v.open_document("note.md").unwrap().text,
                accepted,
                "accepted authority edit lost"
            );
        } else {
            assert!(disk.starts_with("# External\n"));
        }
        assert_eq!(
            v.index().files()["note.md"].hash,
            rustyred_thg_vfs::ContentHash::of(disk.as_bytes()).0
        );
        let before = v.index().resolved_links();
        v.rebuild_index().unwrap();
        assert_eq!(v.index().resolved_links(), before);
    }
}
#[test]
fn save_survives_sigkill_without_truncation_or_losing_accepted_edit() {
    killed("save");
}
#[test]
fn index_update_survives_sigkill_and_repairs_from_disk() {
    killed("index");
}

#[cfg(target_os = "macos")]
#[test]
fn g6_network_denied_mount_index_edit_save_reconcile() {
    let root = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    fs::write(root.path().join("note.md"), BASE).unwrap();
    let mut command = Command::new("/usr/bin/sandbox-exec");
    command
        .args(["-p", "(version 1) (allow default) (deny network*)"])
        .arg(std::env::current_exe().unwrap())
        .args(["--exact", "child_worker", "--nocapture"])
        .env("FRAME_NOTES_CHILD", "g6")
        .env("FRAME_NOTES_ROOT", root.path())
        .env("FRAME_NOTES_DATA", data.path());
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "G6 child failed (nested sandbox permission may require running outside the Codex sandbox):\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
