mod support;

use std::path::Path;

use support::{ADDED, CONFIG, Fixture};

#[test]
fn sync_lists_every_public_item_with_its_feature_gate() {
    let fixture = Fixture::new("spine");
    fixture.write("truesight.toml", CONFIG);
    let run = fixture.run(&["sync"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let spine = fixture.read("api/fixture.txt");
    assert!(
        spine
            .lines()
            .any(|line| line == "#[cfg(feature = \"gated\")] pub fn fixture::gated_only() -> u8"),
        "{spine}"
    );
    assert!(
        spine
            .lines()
            .any(|line| line == "pub fn fixture::always() -> u8"),
        "{spine}"
    );
    assert!(!spine.contains("square"), "{spine}");
}

#[test]
fn an_item_listed_twice_carries_its_gate_once() {
    let fixture = Fixture::new("gate-once");
    fixture.write("truesight.toml", CONFIG);
    let run = fixture.run(&["sync"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let spine = fixture.read("api/fixture.txt");
    let from = spine
        .lines()
        .find(|line| {
            line.contains("impl core::convert::From<") && line.ends_with("for fixture::Get")
        })
        .unwrap_or_else(|| panic!("{spine}"));
    assert!(
        from.starts_with("#[cfg(feature = \"gated\")] impl"),
        "{from}"
    );
}

#[test]
fn check_fails_when_the_code_gains_an_item_after_sync() {
    let fixture = Fixture::new("drift");
    fixture.write("truesight.toml", CONFIG);
    fixture.succeed(&["sync"]);
    fixture.succeed(&["check"]);
    fixture.append("src/lib.rs", ADDED);
    let run = fixture.run(&["check"]);
    assert_eq!(run.code, 1, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stdout.contains("stale api/fixture.txt"),
        "{}",
        run.stdout
    );
    assert!(
        run.stdout.contains("+ pub fn fixture::added() -> u8"),
        "{}",
        run.stdout
    );
}

#[test]
fn a_task_that_calls_a_missing_function_denies_the_check() {
    let fixture = Fixture::new("unknown-call");
    fixture.write("truesight.toml", CONFIG);
    fixture.write(
        "api/fixture.toml",
        "[[task]]\nname = \"Measure a circle\"\ncall = \"Circle::perimeter\"\n",
    );
    fixture.run(&["sync"]);
    let run = fixture.run(&["check"]);
    assert_eq!(run.code, 1, "{}{}", run.stdout, run.stderr);
    assert!(run.stdout.contains("deny[unknown-path]"), "{}", run.stdout);
    assert!(run.stdout.contains("`Circle::perimeter`"), "{}", run.stdout);
}

#[test]
fn owners_resolve_at_a_private_path_and_at_a_public_re_export() {
    let fixture = Fixture::new("owners");
    fixture.write("truesight.toml", CONFIG);
    fixture.write(
        "api/fixture.toml",
        "[[task]]\nname = \"Measure a circle\"\ncall = \"Circle::area\"\nowner = \"shapes::helper::square\"\n\n[[task]]\nname = \"Make a circle\"\ncall = \"Circle::new\"\nowner = \"Circle\"\n",
    );
    fixture.succeed(&["sync"]);
    let run = fixture.run(&["check"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
}

#[test]
fn a_private_method_resolves_through_a_re_export_of_its_type() {
    let fixture = Fixture::new("owner-alias");
    fixture.write("truesight.toml", CONFIG);
    fixture.write(
        "api/fixture.toml",
        "[[task]]\nname = \"Measure a circle\"\ncall = \"Circle::area\"\nowner = \"Circle::radius_squared\"\n",
    );
    let run = fixture.run(&["lint"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(!run.stdout.contains("unknown-owner"), "{}", run.stdout);
}

#[test]
fn an_owner_that_does_not_exist_denies_the_check() {
    let fixture = Fixture::new("missing-owner");
    fixture.write("truesight.toml", CONFIG);
    fixture.write(
        "api/fixture.toml",
        "[[task]]\nname = \"Measure a circle\"\ncall = \"Circle::area\"\nowner = \"shapes::helper::cube\"\n",
    );
    fixture.run(&["sync"]);
    let run = fixture.run(&["check"]);
    assert_eq!(run.code, 1, "{}{}", run.stdout, run.stderr);
    assert!(run.stdout.contains("deny[unknown-owner]"), "{}", run.stdout);
    assert!(
        run.stdout.contains("`shapes::helper::cube`"),
        "{}",
        run.stdout
    );
}

#[test]
fn lint_names_both_paths_of_a_re_exported_type_once() {
    let fixture = Fixture::new("duplicate");
    fixture.write("truesight.toml", CONFIG);
    let run = fixture.run(&["lint"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stdout
            .contains("warn[duplicate-path] `Circle` and `shapes::Circle` name one item"),
        "{}",
        run.stdout
    );
    assert!(!run.stdout.contains("Circle::new"), "{}", run.stdout);
}

#[test]
fn lint_ignores_methods_listed_on_a_generic_impl() {
    let fixture = Fixture::new("generic-impl");
    fixture.write("truesight.toml", CONFIG);
    let run = fixture.run(&["lint"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stdout
            .contains("`prelude::Paint` and `style::Paint` name one item"),
        "{}",
        run.stdout
    );
    assert!(!run.stdout.contains("T::paint"), "{}", run.stdout);
}

#[test]
fn workspaces_that_share_a_target_dir_build_apart() {
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("shared-target");
    if target.exists() {
        std::fs::remove_dir_all(&target).expect("clear the shared target dir");
    }
    let builds = || {
        std::fs::read_dir(target.join("truesight"))
            .map(|dirs| dirs.count())
            .unwrap_or(0)
    };
    let mut counts = Vec::new();
    for name in ["apart-one", "apart-two"] {
        let fixture = Fixture::new(name);
        fixture.write("truesight.toml", CONFIG);
        let run = fixture.run_with(&["sync"], &[("CARGO_TARGET_DIR", &target)]);
        assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
        counts.push(builds());
    }
    assert_eq!(counts[1], 2 * counts[0], "{counts:?}");
}

#[test]
fn diff_prints_an_added_function_as_a_changelog_bullet() {
    let fixture = Fixture::new("changelog");
    fixture.write("truesight.toml", CONFIG);
    fixture.succeed(&["sync"]);
    fixture.append("src/lib.rs", ADDED);
    let run = fixture.run(&["diff", "--format", "changelog"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(run.stdout.contains("- Added `added`."), "{}", run.stdout);
}

#[test]
fn a_surface_over_its_budget_denies_the_check() {
    let fixture = Fixture::new("budget");
    fixture.write("truesight.toml", &format!("{CONFIG}budget = 3\n"));
    fixture.run(&["sync"]);
    let run = fixture.run(&["check"]);
    assert_eq!(run.code, 1, "{}{}", run.stdout, run.stderr);
    assert!(run.stdout.contains("deny[over-budget]"), "{}", run.stdout);
}

#[test]
fn the_budget_counts_items_and_not_the_paths_that_re_export_them() {
    let fixture = Fixture::new("budget-items");
    fixture.write("truesight.toml", &format!("{CONFIG}budget = 15\n"));
    fixture.append("src/lib.rs", "\npub use shapes::Circle as Round;\n");
    let run = fixture.run(&["lint"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(!run.stdout.contains("over-budget"), "{}", run.stdout);
    fixture.append("src/lib.rs", ADDED);
    let run = fixture.run(&["lint"]);
    assert_eq!(run.code, 1, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stdout
            .contains("deny[over-budget] 16 public items exceed the budget of 15"),
        "{}",
        run.stdout
    );
}

#[test]
fn lint_warns_about_a_module_that_only_re_exports() {
    let fixture = Fixture::new("reexport-only");
    fixture.write("truesight.toml", CONFIG);
    let run = fixture.run(&["lint"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stdout.contains(
            "warn[reexport-only-module] `prelude` only re-exports items that other modules export"
        ),
        "{}",
        run.stdout
    );
}

#[test]
fn without_a_config_it_summarizes_the_crate_in_the_current_directory() {
    let fixture = Fixture::new("zero-config");
    let run = fixture.run(&[]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stdout
            .starts_with("fixture 0.1.0 · features: default ·"),
        "{}",
        run.stdout
    );
}

#[test]
fn a_task_can_call_a_mutable_static() {
    let fixture = Fixture::new("static-mut");
    fixture.write("truesight.toml", CONFIG);
    fixture.write(
        "api/fixture.toml",
        "[[task]]\nname = \"Count calls\"\ncall = \"COUNTER\"\n",
    );
    fixture.run(&["sync"]);
    let run = fixture.run(&["check"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
}

#[test]
fn a_tag_pattern_without_a_version_slot_is_rejected() {
    let fixture = Fixture::new("tag-pattern");
    fixture.write("truesight.toml", &format!("{CONFIG}tag = \"release\"\n"));
    let run = fixture.run(&["check"]);
    assert_eq!(run.code, 2, "{}{}", run.stdout, run.stderr);
    assert!(run.stderr.contains("has no `{version}`"), "{}", run.stderr);
}
