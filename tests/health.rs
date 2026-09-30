mod support;

use support::{CONFIG, Fixture, Run};

const DOCS: &str = "# API\n\n## Prelude\n\n| Name | Purpose |\n| --- | --- |\n| `Paint` | Paints. |\n\n## Items\n\n| Name | Purpose |\n| :--- | --- |\n| `shapes::Circle` | A circle. |\n| `get`, `always` | Numbers. |\n";

fn journeys(fixture: &Fixture, budgets: &str) -> Run {
    fixture.write(
        "truesight.toml",
        &format!("{CONFIG}journey-prefix = \"demo\"\n\n[crate.journeys]\n{budgets}"),
    );
    fixture.run(&["lint"])
}

fn documented(fixture: &Fixture, crate_keys: &str, lint: &str) -> Run {
    fixture.write("docs/api.md", DOCS);
    fixture.write(
        "truesight.toml",
        &format!("{CONFIG}docs = [\"docs/api.md\"]\n{crate_keys}\n[lint]\n{lint}"),
    );
    fixture.run(&["lint"])
}

#[test]
fn a_journey_counts_the_library_names_its_example_uses() {
    let fixture = Fixture::new("journey-at-budget");
    let run = journeys(&fixture, "demo = 2\n");
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    fixture.succeed(&["sync"]);
    let run = fixture.succeed(&["health"]);
    assert!(
        run.stdout.contains("1 journeys, 1 at budget: demo 2/2"),
        "{}",
        run.stdout
    );
}

#[test]
fn a_journey_off_its_budget_denies_the_lint() {
    let fixture = Fixture::new("journey-budgets");
    let over = journeys(&fixture, "demo = 1\n");
    assert_eq!(over.code, 1, "{}{}", over.stdout, over.stderr);
    assert!(
        over.stdout.contains(
            "deny[journey-budget] journey `demo` uses 2 names, over its budget of 1: always fixture"
        ),
        "{}",
        over.stdout
    );
    let under = journeys(&fixture, "demo = 3\n");
    assert!(
        under.stdout.contains("lower its budget from 3 to 2"),
        "{}",
        under.stdout
    );
    let missing = journeys(&fixture, "");
    assert!(
        missing
            .stdout
            .contains("journey `demo` uses 2 names and has no budget"),
        "{}",
        missing.stdout
    );
    let orphan = journeys(&fixture, "demo = 2\nghost = 4\n");
    assert!(
        orphan
            .stdout
            .contains("budget `ghost` names no journey example"),
        "{}",
        orphan.stdout
    );
}

#[test]
fn a_journey_does_not_count_its_own_bindings_or_attributes() {
    let fixture = Fixture::new("journey-bindings");
    fixture.write(
        "examples/demo_local.rs",
        "#[allow(unused)]\nfn main() {\n    let always = fixture::get();\n    match Some(always) {\n        Some(area) => drop(area),\n        None => {}\n    }\n    let paint = |radius: u8| radius;\n    drop(paint);\n}\n",
    );
    let run = journeys(&fixture, "demo = 2\ndemo_local = 2\n");
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
}

#[test]
fn the_prelude_budget_counts_the_names_the_prelude_exports() {
    let fixture = Fixture::new("prelude-budget");
    fixture.write(
        "truesight.toml",
        &format!("{CONFIG}prelude = \"prelude\"\nprelude-budget = 1\n"),
    );
    let run = fixture.run(&["lint"]);
    assert!(!run.stdout.contains("prelude-budget"), "{}", run.stdout);
    fixture.write(
        "truesight.toml",
        &format!("{CONFIG}prelude = \"prelude\"\nprelude-budget = 0\n"),
    );
    let run = fixture.run(&["lint"]);
    assert_eq!(run.code, 1, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stdout
            .contains("deny[prelude-budget] the prelude exports 1 names, over its budget of 0"),
        "{}",
        run.stdout
    );
}

#[test]
fn an_item_without_rustdoc_or_a_reference_row_is_undocumented() {
    let fixture = Fixture::new("undocumented");
    let run = documented(&fixture, "", "undocumented = \"deny\"\n");
    assert_eq!(run.code, 1, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stdout
            .contains("deny[undocumented] `Pair` has no rustdoc and no row in the reference docs"),
        "{}",
        run.stdout
    );
    assert!(!run.stdout.contains("`always`"), "{}", run.stdout);
    fixture.run(&["sync"]);
    let run = fixture.run(&["health"]);
    assert!(
        run.stdout
            .contains("4 of 10 module-level items (40%): 0 by rustdoc, 4 by reference docs"),
        "{}",
        run.stdout
    );
}

#[test]
fn a_crate_can_lower_a_lint_that_the_workspace_denies() {
    let fixture = Fixture::new("lint-override");
    let run = documented(
        &fixture,
        "\n[crate.lint]\nundocumented = \"allow\"\n",
        "undocumented = \"deny\"\n",
    );
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(!run.stdout.contains("undocumented"), "{}", run.stdout);
}

#[test]
fn a_documented_section_must_match_the_module_it_names() {
    let fixture = Fixture::new("doc-sections");
    let sections = "doc-sections = { \"Prelude\" = \"prelude\" }\n";
    let run = documented(&fixture, sections, "stale-doc = \"deny\"\n");
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    fixture.write(
        "docs/api.md",
        &DOCS.replace("`Paint` | Paints.", "`Brush` | Brushes."),
    );
    let run = fixture.run(&["lint"]);
    assert_eq!(run.code, 1, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stdout
            .contains("lists `Brush` under `Prelude`, which `prelude` does not export"),
        "{}",
        run.stdout
    );
}

#[test]
fn a_reference_row_that_names_no_public_path_is_reported() {
    let fixture = Fixture::new("unknown-doc-path");
    fixture.write(
        "docs/api.md",
        "| Name | Purpose |\n| --- | --- |\n| `shapes::Square` | Gone. |\n| `Circle::new`, `area` | Fine. |\n",
    );
    fixture.write(
        "truesight.toml",
        &format!("{CONFIG}docs = [\"docs/api.md\"]\n\n[lint]\nunknown-doc-path = \"deny\"\n"),
    );
    let run = fixture.run(&["lint"]);
    assert_eq!(run.code, 1, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stdout
            .contains("docs/api.md:3 names `shapes::Square`, which is not public"),
        "{}",
        run.stdout
    );
    assert!(!run.stdout.contains("`area`"), "{}", run.stdout);
}

#[test]
fn sync_fills_the_readme_health_block() {
    let fixture = Fixture::new("health-block");
    fixture.write("truesight.toml", CONFIG);
    fixture.write(
        "README.md",
        "# fixture\n\n<!-- truesight:health -->\n<!-- /truesight -->\n",
    );
    fixture.succeed(&["sync"]);
    let readme = fixture.read("README.md");
    assert!(readme.contains("| Measure | Value |"), "{readme}");
    assert!(
        readme.contains("| journeys | none configured |"),
        "{readme}"
    );
}
