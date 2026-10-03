use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::Outcome;
use crate::blocks;
use crate::config::{Project, read_optional};
use crate::diff::Change;
use crate::error::Error;
use crate::history::{self, History};
use crate::intent::{self, Task};
use crate::lint::{self, Finding};
use crate::llms;
use crate::package::Crate;
use crate::pages;
use crate::surface::{self, Surface};

const SHOWN_LINES: usize = 12;

pub struct Doc<'a> {
    pub krate: &'a Crate,
    pub surface: Surface,
    pub tasks: Vec<Task>,
    pub findings: Vec<Finding>,
    pub history: History,
}

pub struct Artifact {
    pub path: PathBuf,
    pub content: String,
}

struct Plan {
    files: Vec<Artifact>,
    owned: Vec<PathBuf>,
    kept: Vec<PathBuf>,
}

#[derive(Clone, Copy)]
pub enum Pass {
    Sync,
    Check,
}

impl Pass {
    pub fn keeps_history(self, history: &History) -> bool {
        !history.tracked || (history.shallow && matches!(self, Self::Sync))
    }
}

pub fn document(project: &Project) -> Result<Vec<Doc<'_>>, Error> {
    project
        .crates
        .iter()
        .map(|krate| {
            let surface = surface::load(project, krate)?;
            let tasks = intent::load(krate)?;
            let findings = lint::findings(project, krate, &surface, &tasks)?;
            let history = history::load(project, krate, &surface.spine())?;
            Ok(Doc {
                krate,
                surface,
                tasks,
                findings,
                history,
            })
        })
        .collect()
}

fn plan(project: &Project, docs: &[Doc<'_>], pass: Pass) -> Result<Plan, Error> {
    let mut files: Vec<Artifact> = docs
        .iter()
        .map(|doc| Artifact {
            path: doc.krate.spine.clone(),
            content: doc.surface.spine(),
        })
        .collect();
    let mut owned = Vec::new();
    let mut kept = Vec::new();
    if let Some(book) = &project.book {
        for doc in docs {
            owned.push(pages::dir(book, &doc.surface.package));
            let changes = pages::changes_path(book, &doc.surface.package);
            let keep = pass.keeps_history(&doc.history) && changes.exists();
            for page in pages::render(project, book, doc) {
                if keep && page.path == changes {
                    kept.push(page.path);
                } else {
                    files.push(page);
                }
            }
        }
        if project.skipped.is_empty() {
            files.push(llms::render(book, docs)?);
        }
    }
    for file in &project.markdown {
        if let Some(content) = blocks::fill(project, file, docs, pass)? {
            files.push(Artifact {
                path: file.clone(),
                content,
            });
        }
    }
    Ok(Plan { files, owned, kept })
}

pub fn sync(project: &Project) -> Result<Outcome, Error> {
    let docs = document(project)?;
    note_history(project, &docs);
    let plan = plan(project, &docs, Pass::Sync)?;
    let mut updated = 0;
    for file in &plan.files {
        let current = read_optional(&file.path)?;
        if current.as_deref() == Some(file.content.as_str()) {
            continue;
        }
        write(&file.path, &file.content)?;
        let change = Change::between(current.as_deref().unwrap_or_default(), &file.content);
        println!(
            "wrote {} (+{} -{} lines)",
            project.show(&file.path),
            change.added.len(),
            change.removed.len()
        );
        updated += 1;
    }
    for extra in extras(&plan)? {
        std::fs::remove_file(&extra).map_err(|source| Error::Io {
            path: extra.clone(),
            source,
        })?;
        println!("removed {}", project.show(&extra));
        updated += 1;
    }
    println!("{} generated files, {updated} updated", plan.files.len());
    Ok(verdict(lint::print(&docs), 0))
}

pub struct Stale {
    pub path: PathBuf,
    pub generated: bool,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

pub fn staleness(project: &Project, docs: &[Doc<'_>]) -> Result<(usize, Vec<Stale>), Error> {
    let plan = plan(project, docs, Pass::Check)?;
    let mut stale = Vec::new();
    for file in &plan.files {
        let current = read_optional(&file.path)?;
        if current.as_deref() != Some(file.content.as_str()) {
            let change = Change::between(current.as_deref().unwrap_or_default(), &file.content);
            stale.push(Stale {
                path: file.path.clone(),
                generated: true,
                added: change.added.iter().map(ToString::to_string).collect(),
                removed: change.removed.iter().map(ToString::to_string).collect(),
            });
        }
    }
    stale.extend(extras(&plan)?.into_iter().map(|path| Stale {
        path,
        generated: false,
        added: Vec::new(),
        removed: Vec::new(),
    }));
    Ok((plan.files.len(), stale))
}

pub fn check(project: &Project) -> Result<Outcome, Error> {
    let docs = document(project)?;
    note_history(project, &docs);
    note_skipped(project);
    let (generated, stale) = staleness(project, &docs)?;
    for file in &stale {
        if !file.generated {
            println!(
                "stale {} (not generated by truesight)",
                project.show(&file.path)
            );
            continue;
        }
        println!(
            "stale {} (+{} -{} lines)",
            project.show(&file.path),
            file.added.len(),
            file.removed.len()
        );
        for line in file.removed.iter().take(SHOWN_LINES) {
            println!("  - {line}");
        }
        for line in file.added.iter().take(SHOWN_LINES) {
            println!("  + {line}");
        }
    }
    let denied = lint::print(&docs);
    let stale = stale.len();
    if stale > 0 {
        println!("run `cargo truesight sync` to update {stale} stale file(s)");
    } else {
        println!("{generated} generated files are current");
    }
    if denied > 0 {
        println!("{denied} lint finding(s) deny the check");
    }
    Ok(verdict(denied, stale))
}

fn note_history(project: &Project, docs: &[Doc<'_>]) {
    if docs.iter().any(|doc| !doc.history.tracked) {
        eprintln!(
            "note: {} is not in a git repository, so the release history pages and blocks stay as they are",
            project.root.display()
        );
    } else if docs.iter().any(|doc| doc.history.shallow) {
        eprintln!(
            "note: {} is a shallow clone, so truesight cannot read the releases whose tags are outside the fetched history; fetch the full history with `git fetch --unshallow --tags`, or set `fetch-depth: 0` on `actions/checkout`",
            project.root.display()
        );
    }
}

fn note_skipped(project: &Project) {
    if project.skipped.is_empty() {
        return;
    }
    if let Some(book) = &project.book {
        eprintln!(
            "note: `-p` skips {}, which covers every package",
            project.show(&llms::path(book))
        );
    }
    if !project.markdown.is_empty() {
        eprintln!(
            "note: `-p` skips the blocks for {} in {}",
            project
                .skipped
                .iter()
                .map(|name| format!("`{name}`"))
                .collect::<Vec<_>>()
                .join(", "),
            project
                .markdown
                .iter()
                .map(|file| project.show(file))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

fn verdict(denied: usize, stale: usize) -> Outcome {
    if denied == 0 && stale == 0 {
        Outcome::Clean
    } else {
        Outcome::Failed
    }
}

pub fn write(path: &Path, content: &str) -> Result<(), Error> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|source| Error::Io {
            path: dir.to_owned(),
            source,
        })?;
    }
    std::fs::write(path, content).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

fn extras(plan: &Plan) -> Result<Vec<PathBuf>, Error> {
    let planned: HashSet<&Path> = plan
        .files
        .iter()
        .map(|file| file.path.as_path())
        .chain(plan.kept.iter().map(PathBuf::as_path))
        .collect();
    let mut extra = Vec::new();
    for dir in &plan.owned {
        extra.extend(
            pages_under(dir)?
                .into_iter()
                .filter(|path| !planned.contains(path.as_path())),
        );
    }
    extra.sort();
    Ok(extra)
}

pub fn pages_under(root: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut pages = Vec::new();
    let mut pending = vec![root.to_owned()];
    while let Some(dir) = pending.pop() {
        let io = |source| Error::Io {
            path: dir.clone(),
            source,
        };
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => return Err(io(source)),
        };
        for entry in entries {
            let entry = entry.map_err(io)?;
            let kind = entry.file_type().map_err(io)?;
            let path = entry.path();
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file()
                && path
                    .extension()
                    .is_some_and(|extension| extension == "md" || extension == "txt")
            {
                pages.push(path);
            }
        }
    }
    pages.sort();
    Ok(pages)
}
