mod support;

use support::{CONFIG, Fixture};

const GATES: &str = "toolchain = \"nightly-2026-09-16\"\n\n[[crate]]\npackage = \"fixture\"\nfeatures = [\"gated\"]\ngates = [\"gated\", \"extra\"]\n";

fn spine_after(name: &str, config: &str, code: &str) -> String {
    let fixture = Fixture::new(name);
    fixture.write("truesight.toml", config);
    fixture.append("src/lib.rs", code);
    let run = fixture.run(&["sync"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    fixture.read("api/fixture.txt")
}

#[test]
fn a_derived_trait_is_listed_as_an_impl_line() {
    let spine = spine_after("derived", CONFIG, "\n#[derive(Clone)]\npub struct Token;\n");
    assert!(
        spine
            .lines()
            .any(|line| line == "impl core::clone::Clone for fixture::Token"),
        "{spine}"
    );
    assert!(!spine.contains("fixture::Token::clone"), "{spine}");
}

#[test]
fn a_trait_impl_lists_the_impl_and_not_its_methods() {
    let spine = spine_after(
        "trait-methods",
        CONFIG,
        "\nimpl core::fmt::Display for Get {\n    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {\n        f.write_str(\"get\")\n    }\n}\n",
    );
    assert!(
        spine
            .lines()
            .any(|line| line == "impl core::fmt::Display for fixture::Get"),
        "{spine}"
    );
    assert!(!spine.contains("fixture::Get::fmt"), "{spine}");
}

#[test]
fn a_deprecated_item_carries_the_attribute() {
    let spine = spine_after(
        "deprecated",
        CONFIG,
        "\n#[deprecated(note = \"use always\")]\npub fn old() -> u8 {\n    0\n}\n",
    );
    assert!(
        spine
            .lines()
            .any(|line| line == "#[deprecated] pub fn fixture::old() -> u8"),
        "{spine}"
    );
}

#[test]
fn an_item_behind_any_of_two_gates_names_both() {
    let spine = spine_after(
        "any-gates",
        GATES,
        "\n#[cfg(any(feature = \"gated\", feature = \"extra\"))]\npub fn either() -> u8 {\n    6\n}\n",
    );
    let line = spine
        .lines()
        .find(|line| line.ends_with("pub fn fixture::either() -> u8"))
        .unwrap_or_else(|| panic!("{spine}"));
    assert!(line.starts_with("#[cfg(any("), "{line}");
    assert!(line.contains("feature = \"gated\""), "{line}");
    assert!(line.contains("feature = \"extra\""), "{line}");
}

#[test]
fn a_signature_that_changes_with_a_gate_lists_both_variants() {
    let spine = spine_after(
        "variants",
        CONFIG,
        "\n#[cfg(feature = \"gated\")]\npub fn variant(value: u8) -> u8 {\n    value\n}\n\n#[cfg(not(feature = \"gated\"))]\npub fn variant(_value: u8) -> u8 {\n    0\n}\n",
    );
    assert!(
        spine
            .lines()
            .any(|line| line
                == "#[cfg(feature = \"gated\")] pub fn fixture::variant(value: u8) -> u8"),
        "{spine}"
    );
    assert!(
        spine.lines().any(|line| line
            == "#[cfg(not(feature = \"gated\"))] pub fn fixture::variant(_value: u8) -> u8"),
        "{spine}"
    );
}

#[test]
fn changelog_bullets_name_a_new_impl_and_a_removed_path() {
    let fixture = Fixture::new("bullets");
    fixture.write("truesight.toml", CONFIG);
    fixture.succeed(&["sync"]);
    fixture.append(
        "src/lib.rs",
        "\nimpl core::fmt::Display for Get {\n    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {\n        f.write_str(\"get\")\n    }\n}\n",
    );
    let lib = fixture.read("src/lib.rs");
    fixture.write("src/lib.rs", &lib.replace("pub use shapes::Circle;\n", ""));
    let run = fixture.run(&["diff", "--format", "changelog"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let bullets = run.stdout;
    assert!(!bullets.contains("- Added `Get`."), "{bullets}");
    assert!(
        bullets
            .lines()
            .any(|line| line.contains("impl") && line.contains("Display") && line.contains("Get")),
        "{bullets}"
    );
    assert!(!bullets.contains("Circle::radius"), "{bullets}");
    assert!(
        bullets
            .lines()
            .any(|line| line.contains("`Circle`") && line.contains("`shapes::Circle`")),
        "{bullets}"
    );
}

#[test]
fn a_provided_trait_method_is_not_a_duplicate_path() {
    let fixture = Fixture::new("provided");
    fixture.write("truesight.toml", CONFIG);
    fixture.append(
        "src/lib.rs",
        "\npub trait Greet {\n    fn hi(&self) -> u8 {\n        1\n    }\n}\n\nimpl Greet for Get {}\n",
    );
    let run = fixture.run(&["lint"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert!(!run.stdout.contains("Greet::hi"), "{}", run.stdout);
}

#[test]
fn a_hidden_item_is_not_listed() {
    let spine = spine_after(
        "hidden",
        CONFIG,
        "\n#[doc(hidden)]\npub fn concealed() -> u8 {\n    0\n}\n",
    );
    assert!(!spine.contains("concealed"), "{spine}");
}
