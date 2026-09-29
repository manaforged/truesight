mod support;

use support::Fixture;

const CONFIG: &str = "toolchain = \"nightly-2026-09-16\"\n\n[[crate]]\npackage = \"fixture\"\n";

const BEFORE: &str = "pub mod shapes {\n    pub struct Circle;\n\n    pub enum Shape {\n        Circle,\n        Square,\n    }\n\n    pub struct Unit;\n}\n\npub mod style {\n    pub struct Unit;\n}\n\npub mod units {\n    pub struct Unit;\n}\n";

fn moved(name: &str) -> Fixture {
    let fixture = Fixture::new(name);
    fixture.git(&["init", "-q"]);
    fixture.write("truesight.toml", CONFIG);
    fixture.write("src/lib.rs", BEFORE);
    let run = fixture.run(&["sync"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    fixture.release("v0.1.0");
    let after = format!(
        "{}\npub use shapes::{{Circle, Shape}};\n",
        BEFORE.replacen("pub mod shapes {", "mod shapes {", 1)
    );
    fixture.write("src/lib.rs", &after);
    fixture
}

#[test]
fn migrate_rewrites_moved_paths_and_reports_an_ambiguous_one() {
    let fixture = moved("migrate");
    fixture.write(
        "callers/main.rs",
        "use fixture::shapes::{Circle, Shape as Kind};\n\nfn pick() -> Kind {\n    fixture::shapes::Shape::Circle\n}\n\nfn unit() -> fixture::shapes::Unit {\n    fixture::shapes::Unit\n}\n",
    );
    let run = fixture.run(&["migrate", "--from", "v0.1.0", "callers"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let main = fixture.read("callers/main.rs");
    assert!(
        main.starts_with("use fixture::Circle;\nuse fixture::Shape as Kind;\n"),
        "{main}"
    );
    assert!(main.contains("    fixture::Shape::Circle\n"), "{main}");
    assert!(
        main.contains("fn unit() -> fixture::shapes::Unit {\n    fixture::shapes::Unit\n"),
        "{main}"
    );
    assert!(
        run.stdout
            .contains("callers/main.rs:8: `fixture::shapes::Unit` is ambiguous"),
        "{}",
        run.stdout
    );
}

#[test]
fn fix_imports_adds_a_use_after_the_inner_attributes() {
    let fixture = moved("fix-imports");
    fixture.write(
        "callers/lib.rs",
        "#![allow(dead_code)]\n//! Callers of the fixture.\n\nfn make() -> Circle {\n    Circle\n}\n",
    );
    fixture.write(
        "errors.txt",
        "error[E0412]: cannot find type `Circle` in this scope\n --> callers/lib.rs:4:14\n  |\n4 | fn make() -> Circle {\n  |              ^^^^^^ not found in this scope\n\nerror[E0425]: cannot find value `Circle` in this scope\n --> callers/lib.rs:5:5\n",
    );
    let run = fixture.run(&[
        "migrate",
        "--from",
        "v0.1.0",
        "callers",
        "--fix-imports",
        "errors.txt",
    ]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert_eq!(
        fixture.read("callers/lib.rs"),
        "#![allow(dead_code)]\n//! Callers of the fixture.\n\nuse fixture::Circle;\nfn make() -> Circle {\n    Circle\n}\n"
    );
    assert!(run.stdout.contains("1 use line added"), "{}", run.stdout);
}
