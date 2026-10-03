mod support;

use support::{CONFIG, Fixture};

fn stale(fixture: &Fixture, path: &str) {
    let run = fixture.run(&["check"]);
    assert_eq!(run.code, 1, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stdout.contains(&format!("stale {path}")),
        "{}",
        run.stdout
    );
}

#[test]
fn check_fails_on_a_hand_edited_book_page() {
    let fixture = Fixture::new("stale-book-page");
    fixture.book();
    fixture.succeed(&["sync"]);
    fixture.append("book/reference/fixture/fixture.md", "\nedited by hand\n");
    stale(&fixture, "book/reference/fixture/fixture.md");
}

#[test]
fn check_fails_on_a_page_no_module_generates() {
    let fixture = Fixture::new("stale-orphan-page");
    fixture.book();
    fixture.succeed(&["sync"]);
    fixture.write("book/reference/fixture/gone.md", "# gone\n");
    stale(&fixture, "book/reference/fixture/gone.md");
}

#[test]
fn check_fails_on_an_edited_llms_txt() {
    let fixture = Fixture::new("stale-llms");
    fixture.book();
    fixture.succeed(&["sync"]);
    fixture.append("book/llms.txt", "\nedited by hand\n");
    stale(&fixture, "book/llms.txt");
}

#[test]
fn check_fails_on_a_hand_edited_readme_block() {
    let fixture = Fixture::new("stale-readme-block");
    fixture.write("truesight.toml", CONFIG);
    fixture.write(
        "README.md",
        "# fixture\n\n<!-- truesight:tasks -->\n<!-- /truesight -->\n",
    );
    fixture.write(
        "api/fixture.toml",
        "[[task]]\nname = \"Make a circle\"\ncall = \"Circle::new\"\n",
    );
    fixture.succeed(&["sync"]);
    let readme = fixture
        .read("README.md")
        .replace("Make a circle", "Make a square");
    fixture.write("README.md", &readme);
    stale(&fixture, "README.md");
}
