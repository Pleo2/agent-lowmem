use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn accepted_version_forms_print_only_the_package_version_without_side_effects() {
    let fixture = Fixture::new();
    let before = snapshot(&fixture.root);

    for arguments in [&["--version"][..], &["-V"][..]] {
        let output = fixture.run(arguments);

        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, b"agent-lowmem 0.1.0\n");
        assert!(output.stderr.is_empty());
        assert!(!fixture.child_marker.exists());
        assert_eq!(snapshot(&fixture.root), before);
    }
}

#[test]
fn version_command_rejects_aliases_and_combined_arguments() {
    let fixture = Fixture::new();

    for arguments in [
        &["version"][..],
        &["-v"][..],
        &["--version", "--json"][..],
        &["-V", "doctor"][..],
    ] {
        let output = fixture.run(arguments);

        assert_eq!(output.status.code(), Some(2), "accepted {arguments:?}");
    }
}

struct Fixture {
    root: PathBuf,
    sentinels: PathBuf,
    child_marker: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "agent-lowmem-version-cli-{nanos}-{}-{serial}",
            std::process::id()
        ));
        let sentinels = root.join("sentinels");
        let temporary = root.join("tmp");
        fs::create_dir_all(&sentinels).unwrap();
        fs::create_dir(&temporary).unwrap();
        let child_marker = root.join("child-started");
        let sentinel = "#!/bin/sh\nprintf child >> \"$AGENT_LOWMEM_SENTINEL_MARKER\"\nexit 97\n";
        for executable in ["git", "gh", "node", "npm", "pnpm"] {
            let path = sentinels.join(executable);
            fs::write(&path, sentinel).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        Self {
            root: fs::canonicalize(root).unwrap(),
            sentinels,
            child_marker,
        }
    }

    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_agent-lowmem"))
            .args(arguments)
            .current_dir(&self.root)
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.sentinels.display()),
            )
            .env("TMPDIR", self.root.join("tmp"))
            .env("AGENT_LOWMEM_SENTINEL_MARKER", &self.child_marker)
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn collect(root: &Path, directory: &Path, output: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                collect(root, &path, output);
            } else {
                output.push((
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                ));
            }
        }
    }

    let mut output = Vec::new();
    collect(root, root, &mut output);
    output.sort_by(|left, right| left.0.cmp(&right.0));
    output
}
