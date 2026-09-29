mod support;

use support::{ADDED, CONFIG, Fixture};

#[test]
fn api_added_after_a_release_tag_is_listed_as_unreleased() {
    let fixture = Fixture::new("unreleased");
    fixture.book();
    fixture.git(&["init", "-q"]);
    fixture.succeed(&["sync"]);
    fixture.release("v0.1.0");
    fixture.append("src/lib.rs", ADDED);
    fixture.succeed(&["sync"]);
    let changes = fixture.read("book/reference/fixture/changes.md");
    let unreleased = changes.find("## Unreleased\n\n- Added `added`.");
    let first = changes.find("## 0.1.0\n\nFirst recorded API:");
    assert!(
        unreleased.is_some() && first.is_some() && unreleased < first,
        "{changes}"
    );
}

#[test]
fn bumping_the_version_titles_the_pending_changes_with_it() {
    let fixture = Fixture::new("bump");
    fixture.book();
    fixture.git(&["init", "-q"]);
    fixture.succeed(&["sync"]);
    fixture.release("v0.1.0");
    fixture.append("src/lib.rs", ADDED);
    let manifest = fixture
        .read("Cargo.toml")
        .replace("version = \"0.1.0\"", "version = \"0.2.0\"");
    fixture.write("Cargo.toml", &manifest);
    fixture.succeed(&["sync"]);
    let changes = fixture.read("book/reference/fixture/changes.md");
    assert!(
        changes.contains("## 0.2.0\n\n- Added `added`."),
        "{changes}"
    );
    assert!(!changes.contains("Unreleased"), "{changes}");
}

#[test]
fn a_changes_block_shows_the_release_it_names() {
    let fixture = Fixture::new("changes-block");
    fixture.write("truesight.toml", CONFIG);
    fixture.git(&["init", "-q"]);
    fixture.succeed(&["sync"]);
    fixture.release("v0.1.0");
    fixture.append("src/lib.rs", ADDED);
    fixture.write(
        "README.md",
        "<!-- truesight:changes -->\n<!-- /truesight -->\n\n<!-- truesight:changes 0.1.0 -->\n<!-- /truesight -->\n",
    );
    fixture.succeed(&["sync"]);
    let readme = fixture.read("README.md");
    assert!(readme.contains("- Added `added`."), "{readme}");
    assert!(
        readme
            .lines()
            .any(|line| line.starts_with("First recorded API: ")
                && line.ends_with(" lines in the item list.")),
        "{readme}"
    );
}

#[test]
fn outside_git_the_committed_release_history_is_kept() {
    let fixture = Fixture::new("untracked");
    fixture.book();
    let page = "# `fixture` API changes\n\n## 0.1.0\n\nFirst recorded API: 1 public items.\n";
    fixture.write("book/reference/fixture/changes.md", page);
    fixture.succeed(&["sync"]);
    assert_eq!(fixture.read("book/reference/fixture/changes.md"), page);
    let run = fixture.run(&["check"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stderr.contains("not in a git repository"),
        "{}",
        run.stderr
    );
}

#[test]
fn an_older_tag_checks_clean_after_a_newer_release() {
    let fixture = Fixture::new("older-tag");
    fixture.book();
    fixture.git(&["init", "-q"]);
    fixture.succeed(&["sync"]);
    fixture.release("v0.1.0");
    fixture.append("src/lib.rs", ADDED);
    let manifest = fixture
        .read("Cargo.toml")
        .replace("version = \"0.1.0\"", "version = \"0.2.0\"");
    fixture.write("Cargo.toml", &manifest);
    fixture.succeed(&["sync"]);
    fixture.release("v0.2.0");
    fixture.git(&["checkout", "-q", "v0.1.0"]);
    let run = fixture.run(&["check"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
}

#[test]
fn a_tag_from_before_adoption_is_not_a_release() {
    let fixture = Fixture::new("pre-adoption");
    fixture.git(&["init", "-q"]);
    fixture.release("v0.1.0");
    fixture.book();
    fixture.succeed(&["sync"]);
    let changes = fixture.read("book/reference/fixture/changes.md");
    assert!(changes.contains("## 0.1.0\n"), "{changes}");
    assert!(!changes.contains("Unreleased"), "{changes}");
}

#[test]
fn check_in_a_shallow_clone_names_the_missing_history() {
    let fixture = Fixture::new("shallow-origin");
    fixture.book();
    fixture.git(&["init", "-q"]);
    fixture.succeed(&["sync"]);
    fixture.release("v0.1.0");
    fixture.append("src/lib.rs", ADDED);
    fixture.succeed(&["sync"]);
    fixture.release("v0.2.0");
    let clone = Fixture {
        root: fixture.root.with_file_name("shallow-clone"),
    };
    if clone.root.exists() {
        std::fs::remove_dir_all(&clone.root).expect("clear the previous clone");
    }
    let origin = format!("file://{}", fixture.root.display());
    let target = clone.root.to_str().expect("a UTF-8 path");
    fixture.git(&["clone", "-q", "--depth", "1", &origin, target]);
    let run = clone.run(&["check"]);
    assert!(run.stderr.contains("shallow clone"), "{}", run.stderr);
}
