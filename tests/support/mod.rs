#![allow(
    dead_code,
    reason = "each test binary uses a subset of the fixture helpers"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const CONFIG: &str = "toolchain = \"nightly-2026-09-16\"\n\n[[crate]]\npackage = \"fixture\"\nfeatures = [\"gated\"]\ngates = [\"gated\"]\n";

pub const BOOK_CONFIG: &str = "toolchain = \"nightly-2026-09-16\"\nbook = \"book\"\n\n[[crate]]\npackage = \"fixture\"\nfeatures = [\"gated\"]\ngates = [\"gated\"]\n";

pub const ADDED: &str = "\npub fn added() -> u8 {\n    4\n}\n";

pub struct Fixture {
    pub root: PathBuf,
}

pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Fixture {
    pub fn new(name: &str) -> Self {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
        if root.exists() {
            fs::remove_dir_all(&root).expect("clear the previous fixture copy");
        }
        copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixture"),
            &root,
        );
        Self { root }
    }

    pub fn write(&self, path: &str, text: &str) {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().expect("a parent directory"))
            .expect("create the directory");
        fs::write(path, text).expect("write the fixture file");
    }

    pub fn book(&self) {
        self.write("truesight.toml", BOOK_CONFIG);
        self.write("book/SUMMARY.md", "# Summary\n");
    }

    pub fn append(&self, path: &str, text: &str) {
        let current = self.read(path);
        self.write(path, &format!("{current}{text}"));
    }

    pub fn git(&self, args: &[&str]) {
        let status = Command::new("git")
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "tag.gpgsign=false",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.com")
            .env("GIT_COMMITTER_NAME", "fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.com")
            .current_dir(&self.root)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?}");
    }

    pub fn release(&self, tag: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", "release"]);
        self.git(&["tag", tag]);
    }

    pub fn read(&self, path: &str) -> String {
        fs::read_to_string(self.root.join(path)).expect("read the fixture file")
    }

    pub fn run(&self, args: &[&str]) -> Run {
        self.run_with(args, &[])
    }

    pub fn succeed(&self, args: &[&str]) -> Run {
        let run = self.run(args);
        assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
        run
    }

    pub fn run_with(&self, args: &[&str], envs: &[(&str, &Path)]) -> Run {
        let output = Command::new(env!("CARGO_BIN_EXE_cargo-truesight"))
            .arg("truesight")
            .args(args)
            .envs(envs.iter().copied())
            .env(
                "GIT_CEILING_DIRECTORIES",
                self.root.parent().expect("a parent directory"),
            )
            .current_dir(&self.root)
            .output()
            .expect("run cargo-truesight");
        Run {
            code: output.status.code().expect("an exit code"),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create the fixture copy");
    for entry in fs::read_dir(from).expect("read the fixture source") {
        let entry = entry.expect("a fixture entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("a file type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy a fixture file");
        }
    }
}
