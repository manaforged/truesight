use std::path::{Path, PathBuf};

use crate::artifacts::{Artifact, Doc};
use crate::config::Project;
use crate::diff::bullets;
use crate::examples;
use crate::history::{Body, Section};
use crate::markdown::{Align, code, link, short, table};
use crate::module_page;
use crate::modules::{self, Counts, Module, within};
use crate::surface::Surface;

pub const OVERVIEW: &str = "index.md";
const CHANGES: &str = "changes.md";
const EXAMPLES: &str = "examples";
const ITEMS: &str = "api.txt";

pub fn dir(book: &Path, package: &str) -> PathBuf {
    book.join("reference").join(package)
}

pub fn changes_path(book: &Path, package: &str) -> PathBuf {
    dir(book, package).join(CHANGES)
}

pub fn file_name(module: &str) -> String {
    let base = module.replace("::", "-");
    if [OVERVIEW, CHANGES].contains(&format!("{base}.md").as_str()) {
        format!("{base}-module.md")
    } else {
        format!("{base}.md")
    }
}

pub fn html(file: &str) -> String {
    file.strip_suffix(".md")
        .map_or_else(|| file.to_owned(), |stem| format!("{stem}.html"))
}

pub fn render(project: &Project, book: &Path, doc: &Doc<'_>) -> Vec<Artifact> {
    let here = dir(book, &doc.surface.package);
    let modules = modules::of(&doc.surface);
    let mut pages = vec![
        Artifact {
            path: here.join(OVERVIEW),
            content: overview(project, &here, doc, &modules),
        },
        Artifact {
            path: here.join(CHANGES),
            content: changes(doc),
        },
        Artifact {
            path: here.join(ITEMS),
            content: doc.surface.spine(),
        },
    ];
    pages.extend(modules.iter().map(|module| Artifact {
        path: here.join(file_name(module.path)),
        content: module_page::render(doc, &modules, module),
    }));
    pages.extend(examples::render(project, &here.join(EXAMPLES), doc));
    pages
}

pub fn list(book: &Path, from_dir: &Path, doc: &Doc<'_>) -> String {
    let here = dir(book, &doc.surface.package);
    let entry = |depth: usize, title: &str, path: &Path| {
        format!(
            "{}- [{title}]({})\n",
            "    ".repeat(depth),
            link(from_dir, path)
        )
    };
    let mut out = entry(
        0,
        &format!("{} API", doc.surface.package),
        &here.join(OVERVIEW),
    );
    for module in modules::of(&doc.surface) {
        out.push_str(&entry(1, module.path, &here.join(file_name(module.path))));
    }
    out.push_str(&entry(1, "API changes", &here.join(CHANGES)));
    if !doc.krate.examples.is_empty() {
        let examples = here.join(EXAMPLES);
        out.push_str(&entry(1, "Examples", &examples.join(OVERVIEW)));
        for example in &doc.krate.examples {
            out.push_str(&entry(
                2,
                &example.name,
                &examples.join(format!("{}.md", example.name)),
            ));
        }
    }
    out
}

pub fn target(modules: &[Module<'_>], path: &str) -> Option<(String, String)> {
    modules.iter().find_map(|module| {
        let group = module
            .groups
            .iter()
            .find(|group| within(module.path, group.name, path))?;
        let anchor = module.anchors().remove(group.name)?;
        Some((file_name(module.path), anchor))
    })
}

pub fn summary_line(modules: &[Module<'_>]) -> String {
    let totals = Counts::total(modules);
    let aliases = if totals.aliases == 1 {
        String::from("1 is a re-export alias")
    } else {
        format!("{} are re-export aliases", totals.aliases)
    };
    let reexport_only = modules::reexport_only(modules).count();
    format!(
        "{}{} at {}; {aliases}. {}, {}. {}, {reexport_only} re-export only.",
        amount(totals.items, "public item"),
        kinds(&totals),
        amount(totals.paths(), "path"),
        amount(totals.trait_impls, "trait impl line"),
        amount(totals.inherent_impls, "inherent impl line"),
        amount(modules.len(), "module"),
    )
}

fn kinds(totals: &Counts) -> String {
    let named: Vec<String> = [
        (totals.types, "type"),
        (totals.functions, "function"),
        (totals.methods, "method"),
        (totals.fields, "field"),
        (totals.variants, "variant"),
        (totals.constants, "constant"),
        (totals.statics, "static"),
        (totals.macros, "macro"),
        (totals.others, "other"),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, noun)| amount(count, noun))
    .collect();
    if named.is_empty() {
        String::new()
    } else {
        format!(" ({})", named.join(", "))
    }
}

pub fn amount(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

pub fn module_table(modules: &[Module<'_>], linked: bool) -> String {
    let rows: Vec<Vec<String>> = modules
        .iter()
        .map(|module| {
            let counts = module.counts();
            let mut name = if linked {
                format!("[{}]({})", code(module.path), file_name(module.path))
            } else {
                code(module.path)
            };
            if counts.reexport_only() {
                name.push_str(" (re-export only)");
            }
            let columns = [
                counts.items,
                counts.types,
                counts.functions,
                counts.methods,
                counts.aliases,
            ];
            [name]
                .into_iter()
                .chain(columns.map(|count| count.to_string()))
                .collect()
        })
        .collect();
    table(
        &[
            ("Module", Align::Left),
            ("Items", Align::Right),
            ("Types", Align::Right),
            ("Functions", Align::Right),
            ("Methods", Align::Right),
            ("Aliases", Align::Right),
        ],
        &rows,
    )
}

pub fn feature_table(surface: &Surface) -> Option<String> {
    let roots = surface.feature_roots();
    if roots.is_empty() {
        return None;
    }
    let rows: Vec<Vec<String>> = roots
        .iter()
        .map(|(gate, paths)| {
            let adds: Vec<String> = paths
                .iter()
                .map(|path| code(short(path, &surface.name)))
                .collect();
            let adds = if adds.is_empty() {
                String::from("no public items")
            } else {
                adds.join(", ")
            };
            vec![code(gate), adds]
        })
        .collect();
    Some(table(
        &[("Feature", Align::Left), ("Adds", Align::Left)],
        &rows,
    ))
}

pub fn section_text(section: &Section, name: &str) -> String {
    match &section.body {
        Body::First(lines) => format!("First recorded API: {lines} lines in the item list.\n"),
        Body::Changes(paths) if paths.is_empty() => String::from("No API changes.\n"),
        Body::Changes(paths) => bullets(paths, name)
            .into_iter()
            .map(|bullet| format!("{bullet}\n"))
            .collect(),
    }
}

fn provenance(project: &Project, doc: &Doc<'_>) -> String {
    let surface = &doc.surface;
    format!(
        "Generated by truesight from the compiler's view of {} {} (features: {}; toolchain {}; rustdoc format {}). Do not edit this page: change the code or {}, then run `cargo truesight sync`.",
        code(&surface.package),
        surface.version,
        surface.features,
        code(&surface.toolchain),
        surface.format,
        code(&project.show(&doc.krate.intent)),
    )
}

fn overview(project: &Project, here: &Path, doc: &Doc<'_>, modules: &[Module<'_>]) -> String {
    let surface = &doc.surface;
    let mut out = format!(
        "# {} API\n\n{}\n\n{} [`{ITEMS}`]({ITEMS}) lists the public items and impls, one per line; a `pub use` path repeats the item with its fields, variants, or trait items, while impls and their methods appear once, under the path where the item is defined. [API changes]({CHANGES}) lists the changes in each release.\n",
        code(&surface.package),
        provenance(project, doc),
        summary_line(modules),
    );
    if !doc.tasks.is_empty() {
        let rows: Vec<Vec<String>> = doc
            .tasks
            .iter()
            .map(|task| {
                let call = code(short(&task.call, &surface.name));
                let resolved = surface.resolve(&task.call).unwrap_or(&task.call);
                let call = match target(modules, resolved) {
                    Some((file, anchor)) => format!("[{call}]({file}#{anchor})"),
                    None => call,
                };
                vec![
                    task.name.clone(),
                    call,
                    guide(project, here, task.guide.as_deref()),
                ]
            })
            .collect();
        out.push_str("\n## Tasks\n\n");
        out.push_str(&table(
            &[
                ("Task", Align::Left),
                ("Call", Align::Left),
                ("Guide", Align::Left),
            ],
            &rows,
        ));
    }
    out.push_str("\n## Modules\n\n");
    out.push_str(&module_table(modules, true));
    if let Some(features) = feature_table(surface) {
        out.push_str("\n## Features\n\n");
        out.push_str(&features);
    }
    if !doc.krate.examples.is_empty() {
        out.push_str("\n## Examples\n\n");
        out.push_str(&examples::table(doc, |name| {
            format!("{EXAMPLES}/{name}.md")
        }));
    }
    out
}

fn guide(project: &Project, here: &Path, guide: Option<&Path>) -> String {
    let Some(guide) = guide else {
        return String::new();
    };
    let path = project.root.join(guide);
    let title = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("# ").map(str::to_owned))
        })
        .unwrap_or_else(|| project.show(&path));
    format!("[{title}]({})", link(here, &path))
}

fn changes(doc: &Doc<'_>) -> String {
    let mut out = format!(
        "# {} API changes\n\nGenerated by truesight from the item list committed at each release tag ({}). Do not edit this page.\n",
        code(&doc.surface.package),
        code(&doc.krate.tag),
    );
    for section in &doc.history.sections {
        out.push_str(&format!(
            "\n## {}\n\n{}",
            section.title,
            section_text(section, &doc.surface.name)
        ));
    }
    out
}
