use std::path::{Path, PathBuf};

use cargo_metadata::semver::Version;

use crate::artifacts::{Doc, Pass};
use crate::config::{CHANGELOG, Crate, Project, SUMMARY, canonical, normal};
use crate::error::Error;
use crate::examples;
use crate::markdown::{Align, code, link, short, table};
use crate::modules;
use crate::pages;

const START: &str = "<!-- truesight:";
const CLOSE: &str = "-->";
const END: &str = "<!-- /truesight -->";

#[derive(Clone, Copy)]
enum Block {
    Tasks,
    Surface,
    Features,
    Pages,
    Changes,
    Examples,
}

impl Block {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "tasks" => Some(Self::Tasks),
            "surface" => Some(Self::Surface),
            "features" => Some(Self::Features),
            "pages" => Some(Self::Pages),
            "changes" => Some(Self::Changes),
            "examples" => Some(Self::Examples),
            _ => None,
        }
    }
}

struct Marker<'a> {
    block: Block,
    package: Option<&'a str>,
    version: Option<&'a str>,
}

pub fn fill(
    project: &Project,
    file: &Path,
    docs: &[Doc<'_>],
    pass: Pass,
) -> Result<Option<String>, Error> {
    let text = std::fs::read_to_string(file).map_err(|source| Error::Io {
        path: file.to_owned(),
        source,
    })?;
    let fail = |message: String| Error::Markdown {
        path: file.to_owned(),
        message,
    };
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let here = file.parent().unwrap_or(Path::new(""));
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut out = String::new();
    let mut fence: Option<(char, usize)> = None;
    let mut found = false;
    let mut index = 0;
    while let Some(raw) = lines.get(index) {
        index += 1;
        out.push_str(raw);
        let line = raw.trim_end_matches(['\n', '\r']);
        if skip_fenced(&mut fence, line) {
            continue;
        }
        let Some(spec) = spec(line) else {
            continue;
        };
        let marker = parse(spec).map_err(fail)?;
        let Some(doc) = pick(project, marker.package, docs)
            .map_err(|message| fail(format!("`{}`: {message}", line.trim())))?
        else {
            continue;
        };
        found = true;
        let end = block_end(&lines, index)
            .ok_or_else(|| fail(format!("`{}` has no `{END}` line after it", line.trim())))?
            .map_err(|()| {
                fail(format!(
                    "`{}` starts a block before the previous one ends",
                    line.trim()
                ))
            })?;
        let body = render(project, &marker, doc, here, pass).map_err(fail)?;
        emit(&mut out, &lines, (index, end), body, newline);
        index = end + 1;
    }
    Ok(found.then_some(out))
}

fn skip_fenced(fence: &mut Option<(char, usize)>, line: &str) -> bool {
    if let Some(open) = *fence {
        if closes(line, open) {
            *fence = None;
        }
        return true;
    }
    *fence = opens(line);
    fence.is_some()
}

fn emit(
    out: &mut String,
    lines: &[&str],
    (start, end): (usize, usize),
    body: Option<String>,
    newline: &str,
) {
    match body {
        Some(body) => {
            out.push_str(newline);
            out.push_str(&body.replace('\n', newline));
            out.push_str(newline);
        }
        None => {
            for kept in lines.get(start..end).unwrap_or_default() {
                out.push_str(kept);
            }
        }
    }
    if let Some(close) = lines.get(end) {
        out.push_str(close);
    }
}

fn block_end(lines: &[&str], from: usize) -> Option<Result<usize, ()>> {
    lines
        .iter()
        .enumerate()
        .skip(from)
        .find_map(|(position, next)| {
            let next = next.trim();
            if next == END {
                Some(Ok(position))
            } else if next.starts_with(START) {
                Some(Err(()))
            } else {
                None
            }
        })
}

fn spec(line: &str) -> Option<&str> {
    line.trim().strip_prefix(START)?.strip_suffix(CLOSE)
}

fn opens(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let marker = trimmed
        .chars()
        .next()
        .filter(|first| *first == '`' || *first == '~')?;
    let count = trimmed
        .chars()
        .take_while(|current| *current == marker)
        .count();
    (count >= 3).then_some((marker, count))
}

fn closes(line: &str, (marker, count): (char, usize)) -> bool {
    let trimmed = line.trim();
    trimmed.chars().count() >= count && trimmed.chars().all(|current| current == marker)
}

fn parse(spec: &str) -> Result<Marker<'_>, String> {
    let mut words = spec.split_whitespace();
    let name = words.next().unwrap_or_default();
    let block = Block::parse(name).ok_or_else(|| {
        format!("unknown block `{name}`; use tasks, surface, features, pages, changes, or examples")
    })?;
    let mut marker = Marker {
        block,
        package: None,
        version: None,
    };
    for word in words {
        if Version::parse(word).is_ok() {
            marker.version = Some(word);
        } else {
            marker.package = Some(word);
        }
    }
    Ok(marker)
}

fn pick<'d, 'a>(
    project: &Project,
    package: Option<&str>,
    docs: &'d [Doc<'a>],
) -> Result<Option<&'d Doc<'a>>, String> {
    match (package, docs) {
        (Some(name), _) => match docs.iter().find(|doc| doc.surface.package == name) {
            Some(doc) => Ok(Some(doc)),
            None if project.skipped.iter().any(|skipped| skipped == name) => Ok(None),
            None => Err(format!(
                "truesight does not document a package named `{name}`"
            )),
        },
        (None, [doc]) if project.skipped.is_empty() => Ok(Some(doc)),
        (None, _) => Err(String::from(
            "name the package, for example `<!-- truesight:surface PACKAGE -->`",
        )),
    }
}

fn render(
    project: &Project,
    marker: &Marker<'_>,
    doc: &Doc<'_>,
    here: &Path,
    pass: Pass,
) -> Result<Option<String>, String> {
    let surface = &doc.surface;
    if matches!(marker.block, Block::Changes) && pass.keeps_history(&doc.history) {
        return Ok(None);
    }
    Ok(Some(match marker.block {
        Block::Tasks => tasks(project, doc),
        Block::Surface => {
            let modules = modules::of(surface);
            format!(
                "{}\n\n{}",
                pages::summary_line(&modules),
                pages::module_table(&modules, false)
            )
        }
        Block::Features => pages::feature_table(surface).unwrap_or_else(|| {
            String::from("No feature gates are configured in `truesight.toml`.\n")
        }),
        Block::Pages => match &project.book {
            Some(book) => pages::list(book, here, doc),
            None => {
                return Err(String::from(
                    "the pages block needs `book` in truesight.toml",
                ));
            }
        },
        Block::Changes => changes(marker, doc)?,
        Block::Examples => example_table(doc, here),
    }))
}

fn tasks(project: &Project, doc: &Doc<'_>) -> String {
    if doc.tasks.is_empty() {
        return format!(
            "{} records no tasks.\n",
            code(&project.show(&doc.krate.intent))
        );
    }
    let rows: Vec<Vec<String>> = doc
        .tasks
        .iter()
        .map(|task| {
            vec![
                task.name.clone(),
                code(short(&task.call, &doc.surface.name)),
            ]
        })
        .collect();
    table(&[("Task", Align::Left), ("Call", Align::Left)], &rows)
}

fn changes(marker: &Marker<'_>, doc: &Doc<'_>) -> Result<String, String> {
    let history = &doc.history.sections;
    let section = match marker.version {
        Some(version) => history.iter().find(|section| section.title == version),
        None => history.first(),
    };
    section
        .map(|section| pages::section_text(section, &doc.surface.name))
        .ok_or_else(|| {
            format!(
                "no release `{}` in the item list history",
                marker.version.unwrap_or_default()
            )
        })
}

fn example_table(doc: &Doc<'_>, here: &Path) -> String {
    let package = &doc.surface.package;
    if doc.krate.examples.is_empty() {
        return format!("{} has no example targets.\n", code(package));
    }
    let rows: Vec<Vec<String>> = doc
        .krate
        .examples
        .iter()
        .map(|example| {
            vec![
                format!("[{}]({})", code(&example.name), link(here, &example.source)),
                code(&examples::run_command(package, example)),
            ]
        })
        .collect();
    table(&[("Example", Align::Left), ("Run", Align::Left)], &rows)
}

pub fn files(
    root: &Path,
    configured: &[PathBuf],
    book: Option<&Path>,
    crates: &[Crate],
) -> Result<Vec<PathBuf>, Error> {
    let mut files = Vec::new();
    let mut seen = Vec::new();
    for path in configured {
        let path = normal(&root.join(path));
        let key = canonical(&path)?;
        push_unique(&mut files, &mut seen, path, key);
    }
    let found = book
        .map(|dir| dir.join(SUMMARY))
        .into_iter()
        .chain([root.join(CHANGELOG)])
        .chain(crates.iter().flat_map(|krate| {
            let changelog = krate.manifest.parent().map(|dir| dir.join(CHANGELOG));
            krate.readme.clone().into_iter().chain(changelog)
        }));
    for path in found.filter(|path| path.is_file()) {
        let key = canonical(&path)?;
        push_unique(&mut files, &mut seen, path, key);
    }
    Ok(files)
}

fn push_unique(files: &mut Vec<PathBuf>, seen: &mut Vec<PathBuf>, path: PathBuf, key: PathBuf) {
    if !seen.contains(&key) {
        seen.push(key);
        files.push(path);
    }
}
