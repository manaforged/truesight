mod support;

use support::{BOOK_CONFIG, CONFIG, Fixture};

fn workspace(fixture: &Fixture) {
    let lib = fixture.read("src/lib.rs");
    for name in ["alpha", "beta"] {
        fixture.write(
            &format!("{name}/Cargo.toml"),
            &format!(
                "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[features]\ndefault = [\"extra\"]\nextra = []\ngated = []\n"
            ),
        );
        fixture.write(&format!("{name}/src/lib.rs"), &lib);
        fixture.write(
            &format!("{name}/README.md"),
            &format!("# {name}\n\n## License\n\nMIT\n"),
        );
    }
    fixture.write(
        "Cargo.toml",
        "[workspace]\nmembers = [\"alpha\", \"beta\"]\nresolver = \"3\"\n",
    );
}

fn second_version(fixture: &Fixture) {
    let manifest = fixture.read("beta/Cargo.toml");
    fixture.write(
        "beta/Cargo.toml",
        &manifest.replace("version = \"0.1.0\"", "version = \"0.2.0\""),
    );
}

#[test]
fn init_shares_one_tag_pattern_when_the_crates_share_a_version() {
    let fixture = Fixture::new("shared-version");
    workspace(&fixture);
    let run = fixture.run(&["init"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let config = fixture.read("truesight.toml");
    assert!(!config.contains("tag = "), "{config}");
}

#[test]
fn init_follows_the_tags_the_repository_already_has() {
    let fixture = Fixture::new("shared-tags");
    workspace(&fixture);
    second_version(&fixture);
    fixture.git(&["init", "-q"]);
    fixture.release("v0.1.0");
    let run = fixture.run(&["init"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let config = fixture.read("truesight.toml");
    assert!(!config.contains("tag = "), "{config}");
}

#[test]
fn one_package_leaves_an_unnamed_block_to_a_full_sync() {
    let fixture = Fixture::new("unnamed-block");
    workspace(&fixture);
    fixture.succeed(&["init"]);
    fixture.write(
        "CHANGELOG.md",
        "# Changelog\n\n<!-- truesight:surface -->\n<!-- /truesight -->\n",
    );
    let run = fixture.run(&["sync", "-p", "alpha"]);
    assert_eq!(run.code, 2, "{}{}", run.stdout, run.stderr);
    assert!(run.stderr.contains("name the package"), "{}", run.stderr);
    assert!(!fixture.read("CHANGELOG.md").contains("public items"));
}

#[test]
fn a_block_in_the_changelog_is_filled() {
    let fixture = Fixture::new("changelog-block");
    fixture.write("truesight.toml", CONFIG);
    fixture.write(
        "CHANGELOG.md",
        "# Changelog\n\n<!-- truesight:surface -->\n<!-- /truesight -->\n",
    );
    let run = fixture.run(&["sync"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let changelog = fixture.read("CHANGELOG.md");
    assert!(changelog.contains("public items"), "{changelog}");
}

#[test]
fn init_uses_a_book_outside_docs() {
    let fixture = Fixture::new("book-dir");
    fixture.write("book/book.toml", "[book]\ntitle = \"guide\"\n");
    fixture.write("book/src/SUMMARY.md", "# Summary\n\n- [Guide](guide.md)\n");
    fixture.write("book/src/guide.md", "# Guide\n");
    let run = fixture.run(&["init"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(
        !fixture.root.join("docs").exists(),
        "a second book was scaffolded"
    );
    let config = fixture.read("truesight.toml");
    assert!(config.contains("book = \"book/src\""), "{config}");
    let summary = fixture.read("book/src/SUMMARY.md");
    assert!(summary.contains("- [Guide](guide.md)"), "{summary}");
    assert!(summary.contains("<!-- truesight:pages"), "{summary}");
}

#[test]
fn init_gives_each_crate_of_a_workspace_its_own_tags() {
    let fixture = Fixture::new("crate-tags");
    workspace(&fixture);
    second_version(&fixture);
    let run = fixture.run(&["init"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let config = fixture.read("truesight.toml");
    assert!(config.contains("tag = \"alpha-v{version}\""), "{config}");
    assert!(config.contains("tag = \"beta-v{version}\""), "{config}");
}

#[test]
fn one_package_syncs_without_touching_the_others() {
    let fixture = Fixture::new("one-package");
    workspace(&fixture);
    fixture.succeed(&["init"]);
    let before = fixture.read("docs/llms.txt");
    fixture.append(
        "beta/src/lib.rs",
        "\npub fn beta_only() -> u8 {\n    9\n}\n",
    );
    let run = fixture.run(&["sync", "-p", "alpha"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert_eq!(fixture.read("docs/llms.txt"), before);
    assert!(!fixture.read("api/beta.txt").contains("beta_only"));
}

#[test]
fn a_marker_inside_an_indented_fence_is_left_alone() {
    let fixture = Fixture::new("indented-fence");
    fixture.write("truesight.toml", CONFIG);
    let readme = "# fixture\n\n- Example:\n\n    ```markdown\n    <!-- truesight:surface -->\n    <!-- /truesight -->\n    ```\n";
    fixture.write("README.md", readme);
    let run = fixture.run(&["sync"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert_eq!(fixture.read("README.md"), readme);
}

#[test]
fn llms_txt_takes_its_title_from_the_book() {
    let fixture = Fixture::new("llms-title");
    workspace(&fixture);
    fixture.succeed(&["init"]);
    fixture.write("docs/book.toml", "[book]\ntitle = \"Demo\"\nsrc = \".\"\n");
    fixture.succeed(&["sync"]);
    let llms = fixture.read("docs/llms.txt");
    assert!(llms.starts_with("# Demo\n"), "{llms}");
}

#[test]
fn init_stops_before_it_deletes_a_hand_written_reference_page() {
    let fixture = Fixture::new("hand-written-page");
    fixture.write("book/book.toml", "[book]\ntitle = \"guide\"\n");
    fixture.write("book/src/SUMMARY.md", "# Summary\n");
    fixture.write("book/src/reference/fixture/notes.md", "# Notes\n");
    let run = fixture.run(&["init"]);
    assert_eq!(run.code, 2, "{}{}", run.stdout, run.stderr);
    assert!(run.stderr.contains("notes.md"), "{}", run.stderr);
    assert_eq!(
        fixture.read("book/src/reference/fixture/notes.md"),
        "# Notes\n"
    );
    assert!(
        !fixture.root.join("truesight.toml").exists(),
        "init wrote truesight.toml before it stopped"
    );
}

#[test]
fn a_book_directory_without_a_summary_stops_the_run() {
    let fixture = Fixture::new("no-summary");
    fixture.write("truesight.toml", BOOK_CONFIG);
    let run = fixture.run(&["sync"]);
    assert_eq!(run.code, 2, "{}{}", run.stdout, run.stderr);
    assert!(run.stderr.contains("SUMMARY.md"), "{}", run.stderr);
}

#[test]
fn a_syntax_error_names_the_config_file() {
    let fixture = Fixture::new("bad-toml");
    fixture.write("truesight.toml", "toolchain = [\n");
    let run = fixture.run(&["check"]);
    assert_eq!(run.code, 2, "{}{}", run.stdout, run.stderr);
    assert!(run.stderr.contains("truesight.toml"), "{}", run.stderr);
}

#[test]
fn an_owner_resolves_through_a_glob_re_export() {
    let fixture = Fixture::new("glob-owner");
    fixture.write("truesight.toml", CONFIG);
    fixture.append(
        "src/lib.rs",
        "\nmod engine {\n    pub struct Engine;\n\n    impl Engine {\n        pub fn run(&self) {\n            self.step()\n        }\n\n        fn step(&self) {}\n    }\n}\n\npub use engine::*;\n",
    );
    fixture.write(
        "api/fixture.toml",
        "[[task]]\nname = \"Run the engine\"\ncall = \"Engine::run\"\nowner = \"Engine::step\"\n",
    );
    let run = fixture.succeed(&["lint"]);
    assert!(!run.stdout.contains("unknown-owner"), "{}", run.stdout);
}

#[test]
fn init_writes_a_backslash_in_the_book_path_as_valid_toml() {
    let fixture = Fixture::new("backslash-book");
    fixture.write(
        "book/book.toml",
        "[book]\ntitle = \"guide\"\nsrc = 'sr\\c'\n",
    );
    fixture.write("book/sr\\c/SUMMARY.md", "# Summary\n");
    fixture.succeed(&["init"]);
}

#[test]
fn init_without_a_library_crate_writes_nothing() {
    let fixture = Fixture::new("no-library");
    std::fs::remove_file(fixture.root.join("src/lib.rs")).expect("remove the library");
    fixture.write("src/main.rs", "fn main() {}\n");
    let run = fixture.run(&["init"]);
    assert_eq!(run.code, 2, "{}{}", run.stdout, run.stderr);
    assert!(
        !fixture.root.join("truesight.toml").exists(),
        "init wrote truesight.toml before it stopped"
    );
    assert!(
        !fixture.root.join("docs").exists(),
        "init scaffolded a book before it stopped"
    );
}

#[test]
fn check_for_one_package_names_the_shared_files_it_does_not_compare() {
    let fixture = Fixture::new("check-one-package");
    fixture.write("book/book.toml", "[book]\ntitle = \"guide\"\n");
    fixture.write("book/src/SUMMARY.md", "# Summary\n\n- [Guide](guide.md)\n");
    workspace(&fixture);
    fixture.succeed(&["init"]);
    fixture.append(
        "beta/src/lib.rs",
        "\npub fn beta_only() -> u8 {\n    9\n}\n",
    );
    let run = fixture.run(&["check", "-p", "alpha"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stderr.contains("`-p` skips book/src/llms.txt"),
        "{}",
        run.stderr
    );
}
