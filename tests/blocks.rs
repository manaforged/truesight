mod support;

use support::{CONFIG, Fixture};

#[test]
fn sync_fills_the_readme_task_block() {
    let fixture = Fixture::new("readme");
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
    let readme = fixture.read("README.md");
    assert!(
        readme.contains("| Make a circle | `Circle::new` |"),
        "{readme}"
    );
}

#[test]
fn an_unknown_block_name_stops_with_the_file_and_name() {
    let fixture = Fixture::new("bad-block");
    fixture.write("truesight.toml", CONFIG);
    fixture.write(
        "README.md",
        "<!-- truesight:tables -->\n<!-- /truesight -->\n",
    );
    let run = fixture.run(&["sync"]);
    assert_eq!(run.code, 2, "{}{}", run.stdout, run.stderr);
    assert!(run.stderr.contains("README.md"), "{}", run.stderr);
    assert!(
        run.stderr.contains("unknown block `tables`"),
        "{}",
        run.stderr
    );
}

#[test]
fn a_marker_inside_a_code_fence_is_left_as_written() {
    let fixture = Fixture::new("fenced");
    fixture.write("truesight.toml", CONFIG);
    let example = "```markdown\n<!-- truesight:tasks -->\n<!-- /truesight -->\n```\n";
    fixture.write(
        "README.md",
        &format!("# fixture\n\n{example}\n<!-- truesight:surface -->\n<!-- /truesight -->\n"),
    );
    fixture.succeed(&["sync"]);
    let readme = fixture.read("README.md");
    assert!(readme.contains(example), "{readme}");
    assert!(readme.contains("public items"), "{readme}");
}

#[test]
fn the_surface_block_counts_each_item_once_and_marks_the_prelude() {
    let fixture = Fixture::new("surface-counts");
    fixture.write("truesight.toml", CONFIG);
    fixture.write(
        "README.md",
        "# fixture\n\n<!-- truesight:surface -->\n<!-- /truesight -->\n",
    );
    let run = fixture.run(&["sync"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let readme = fixture.read("README.md");
    assert!(
        readme.contains(
            "15 public items (5 types, 4 functions, 4 methods, 1 field, 1 static) at 19 paths; 4 are re-export aliases."
        ),
        "{readme}"
    );
    assert!(readme.contains("4 modules, 1 re-export only."), "{readme}");
    assert!(
        readme.contains("| `fixture::prelude` (re-export only) | 0 | 0 | 0 | 0 | 2 |"),
        "{readme}"
    );
    assert!(
        readme.contains("| `fixture::shapes` | 4 | 1 | 0 | 2 | 0 |"),
        "{readme}"
    );
}

#[test]
fn a_readme_with_crlf_endings_keeps_them() {
    let fixture = Fixture::new("crlf");
    fixture.write("truesight.toml", CONFIG);
    fixture.write(
        "README.md",
        "# fixture\r\n\r\n<!-- truesight:surface -->\r\n<!-- /truesight -->\r\n",
    );
    fixture.succeed(&["sync"]);
    let readme = fixture.read("README.md");
    assert_eq!(
        readme.matches('\n').count(),
        readme.matches("\r\n").count(),
        "{readme:?}"
    );
    fixture.succeed(&["check"]);
}

#[test]
fn a_start_marker_inside_an_open_block_stops_with_the_file() {
    let fixture = Fixture::new("nested");
    fixture.write("truesight.toml", CONFIG);
    fixture.write(
        "README.md",
        "<!-- truesight:surface -->\n<!-- truesight:tasks -->\n<!-- /truesight -->\n",
    );
    let run = fixture.run(&["sync"]);
    assert_eq!(run.code, 2, "{}{}", run.stdout, run.stderr);
    assert!(
        run.stderr
            .contains("starts a block before the previous one ends"),
        "{}",
        run.stderr
    );
}
