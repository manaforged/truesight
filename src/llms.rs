use std::path::{Path, PathBuf};

use crate::artifacts::{Artifact, Doc};
use crate::config::read_book;
use crate::error::Error;
use crate::examples;
use crate::markdown::{link, short};
use crate::modules;
use crate::pages;

pub fn path(book: &Path) -> PathBuf {
    book.join("llms.txt")
}

pub fn render(book: &Path, docs: &[Doc<'_>]) -> Result<Artifact, Error> {
    let title = match docs {
        [doc] => doc.surface.package.clone(),
        _ => match book_title(book)? {
            Some(title) => title,
            None => docs
                .iter()
                .map(|doc| doc.surface.package.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        },
    };
    let mut out = format!(
        "# {title}\n\n> Public API reference that truesight generates from the compiler's rustdoc output, except the task names and calls, which come from the task file.\n"
    );
    for doc in docs {
        let surface = &doc.surface;
        let here = pages::dir(book, &surface.package);
        let page = |file: &str| link(book, &here.join(pages::html(file)));
        let modules = modules::of(surface);
        out.push_str(&format!(
            "\n## {} {}\n\n- [Every public item, one per line]({}): {} lines\n- [API overview]({}): tasks, modules, features, and examples\n- [API changes by release]({})\n",
            surface.package,
            surface.version,
            page("api.txt"),
            surface.entries.len(),
            page("index.md"),
            page("changes.md"),
        ));
        for module in &modules {
            out.push_str(&format!(
                "- [{}]({}): {} items\n",
                module.path,
                page(&pages::file_name(module.path)),
                module.counts().items
            ));
        }
        for example in &doc.krate.examples {
            out.push_str(&format!(
                "- [Example {}]({}): `{}`\n",
                example.name,
                page(&format!("examples/{}.md", example.name)),
                examples::run_command(&surface.package, example)
            ));
        }
        if doc.tasks.is_empty() {
            continue;
        }
        out.push_str(&format!("\n## {} tasks\n\n", surface.package));
        for task in &doc.tasks {
            let call = short(&task.call, &surface.name);
            let resolved = surface.resolve(&task.call).unwrap_or(&task.call);
            match pages::target(&modules, resolved) {
                Some((file, anchor)) => out.push_str(&format!(
                    "- [{}]({}#{anchor}): `{call}`\n",
                    task.name,
                    page(&file)
                )),
                None => out.push_str(&format!("- {}: `{call}`\n", task.name)),
            }
        }
    }
    Ok(Artifact {
        path: path(book),
        content: out,
    })
}

fn book_title(book: &Path) -> Result<Option<String>, Error> {
    for dir in book.ancestors().take(2) {
        if let Some(title) = read_book(dir)?.and_then(|section| section.title) {
            return Ok(Some(title));
        }
    }
    Ok(None)
}
