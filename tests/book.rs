mod support;

use support::Fixture;

#[test]
fn the_overview_page_links_each_task_to_its_type() {
    let fixture = Fixture::new("pages");
    fixture.book();
    fixture.write(
        "api/fixture.toml",
        "[[task]]\nname = \"Make a circle\"\ncall = \"Circle::new\"\n",
    );
    fixture.succeed(&["sync"]);
    let index = fixture.read("book/reference/fixture/index.md");
    assert!(
        index.contains("[`Circle::new`](fixture-shapes.md#circle)"),
        "{index}"
    );
}

#[test]
fn sync_removes_a_page_that_no_module_generates() {
    let fixture = Fixture::new("stale-page");
    fixture.book();
    fixture.write("book/reference/fixture/gone.md", "# gone\n");
    fixture.succeed(&["sync"]);
    assert!(!fixture.root.join("book/reference/fixture/gone.md").exists());
    fixture.succeed(&["check"]);
}

#[test]
fn colliding_item_names_get_mdbook_anchors() {
    let fixture = Fixture::new("anchors");
    fixture.book();
    fixture.succeed(&["sync"]);
    let root = fixture.read("book/reference/fixture/fixture.md");
    assert!(root.contains("[`Get`](#get)"), "{root}");
    assert!(root.contains("[`get`](#get-1)"), "{root}");
}

#[test]
fn each_example_gets_a_page_with_its_run_command_and_source() {
    let fixture = Fixture::new("examples");
    fixture.book();
    fixture.succeed(&["sync"]);
    let page = fixture.read("book/reference/fixture/examples/demo.md");
    assert!(
        page.contains("cargo run -p fixture --example demo"),
        "{page}"
    );
    assert!(
        page.contains("{{#include ../../../../examples/demo.rs}}"),
        "{page}"
    );
}

#[test]
fn llms_txt_links_the_published_pages_and_item_list() {
    let fixture = Fixture::new("llms");
    fixture.book();
    fixture.succeed(&["sync"]);
    let llms = fixture.read("book/llms.txt");
    assert!(llms.contains("(reference/fixture/index.html)"), "{llms}");
    assert!(llms.contains("(reference/fixture/api.txt)"), "{llms}");
}

#[test]
fn init_scaffolds_a_book_fills_the_readme_and_writes_the_reference() {
    let fixture = Fixture::new("init");
    fixture.write(
        "README.md",
        "# fixture\n\nA fixture crate.\n\n## License\n\nMIT\n",
    );
    let run = fixture.run(&["init"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let summary = fixture.read("docs/SUMMARY.md");
    assert!(
        summary.contains("- [fixture API](reference/fixture/index.md)"),
        "{summary}"
    );
    let readme = fixture.read("README.md");
    assert!(
        readme.contains("## API\n\n<!-- truesight:surface -->\n\n"),
        "{readme}"
    );
    assert!(
        readme.find("## API") < readme.find("## License"),
        "{readme}"
    );
    assert!(
        fixture
            .read("docs/introduction.md")
            .contains("{{#include ../README.md}}")
    );
    assert!(fixture.root.join("api/fixture.txt").exists());
}

#[test]
fn impls_for_tuples_and_references_get_named_groups() {
    let fixture = Fixture::new("impl-subjects");
    fixture.book();
    let run = fixture.run(&["sync"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let root = fixture.read("book/reference/fixture/fixture.md");
    assert!(!root.contains("[``]"), "{root}");
    assert!(!root.contains("### ``"), "{root}");
    assert!(root.contains("### `(u8, u8)`"), "{root}");
    assert!(root.contains("### `str`"), "{root}");
}

#[cfg(unix)]
#[test]
fn sync_leaves_a_linked_directory_under_the_reference_alone() {
    let fixture = Fixture::new("linked-dir");
    fixture.book();
    fixture.write("guides/keep.md", "# Keep\n");
    std::fs::create_dir_all(fixture.root.join("book/reference/fixture"))
        .expect("create the reference directory");
    std::os::unix::fs::symlink(
        "../../../guides",
        fixture.root.join("book/reference/fixture/guides"),
    )
    .expect("link the guides");
    fixture.succeed(&["sync"]);
    assert_eq!(fixture.read("guides/keep.md"), "# Keep\n");
}
