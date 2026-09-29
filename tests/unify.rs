mod support;

use support::Fixture;

const PRELUDE: &str = "toolchain = \"nightly-2026-09-16\"\n\n[lint]\nduplicate-path = \"deny\"\n\n[[crate]]\npackage = \"fixture\"\nprelude = \"fixture::prelude\"\n";

fn line_of(text: &str, needle: &str) -> usize {
    text.lines()
        .position(|line| line.trim() == needle)
        .map_or(0, |index| index + 1)
}

#[test]
fn unify_names_the_line_behind_each_alias_and_keeps_the_prelude() {
    let fixture = Fixture::new("unify");
    fixture.write("truesight.toml", PRELUDE);
    let lib = fixture.read("src/lib.rs");
    let run = fixture.run(&["unify"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let circle = format!(
        "  {}: use adds `Circle`, another path to `shapes::Circle`\n",
        line_of(&lib, "pub use shapes::Circle;")
    );
    let paint = format!(
        "  {}: use adds `prelude::Paint`, another path to `style::Paint` (prelude, kept)\n",
        line_of(&lib, "pub use crate::style::Paint;")
    );
    assert!(run.stdout.contains("\nsrc/lib.rs\n"), "{}", run.stdout);
    assert!(run.stdout.contains(&circle), "{}", run.stdout);
    assert!(run.stdout.contains(&paint), "{}", run.stdout);
    let lint = fixture.run(&["lint"]);
    assert_eq!(lint.code, 1, "{}{}", lint.stdout, lint.stderr);
    assert!(
        lint.stdout
            .contains("deny[duplicate-path] `Circle` and `shapes::Circle` name one item"),
        "{}",
        lint.stdout
    );
    assert!(!lint.stdout.contains("`prelude::Paint`"), "{}", lint.stdout);
    assert!(
        !lint.stdout.contains("reexport-only-module] `prelude`"),
        "{}",
        lint.stdout
    );
}
