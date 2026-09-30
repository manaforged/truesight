use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::config::{Project, read_optional};
use crate::error::Error;
use crate::journeys;
use crate::markdown::short;
use crate::package::Crate;
use crate::surface::{Kind, Surface};

pub struct Coverage {
    pub items: usize,
    pub rustdoc: usize,
    pub reference: usize,
    pub missing: Vec<String>,
}

pub struct Gaps {
    pub undocumented: Vec<String>,
    pub stale: Vec<String>,
}

struct Row {
    file: PathBuf,
    line: usize,
    headings: Vec<String>,
    named: bool,
    first: Vec<String>,
    words: Vec<String>,
}

pub fn measure(krate: &Crate, surface: &Surface) -> Result<Coverage, Error> {
    let rows = rows(krate)?;
    let words: HashSet<&str> = rows
        .iter()
        .flat_map(|row| row.words.iter().map(String::as_str))
        .collect();
    let modules = modules(surface);
    let mut coverage = Coverage {
        items: 0,
        rustdoc: 0,
        reference: 0,
        missing: Vec::new(),
    };
    for named in surface.named() {
        let Some(path) = named.paths.first().copied() else {
            continue;
        };
        let top = path
            .rsplit_once("::")
            .is_some_and(|(parent, _)| modules.contains(parent));
        if named.kind == Kind::Mod || !top {
            continue;
        }
        coverage.items += 1;
        let name = path.rsplit("::").next().unwrap_or(path);
        if surface
            .entries
            .iter()
            .any(|entry| entry.id == named.id && entry.summary.is_some())
        {
            coverage.rustdoc += 1;
        } else if words.contains(name) {
            coverage.reference += 1;
        } else {
            coverage.missing.push(short(path, &surface.name).to_owned());
        }
    }
    coverage.missing.sort();
    Ok(coverage)
}

pub fn sections(project: &Project, krate: &Crate, surface: &Surface) -> Result<Gaps, Error> {
    let rows = rows(krate)?;
    let mut gaps = Gaps {
        undocumented: Vec::new(),
        stale: Vec::new(),
    };
    for (heading, module) in &krate.doc_sections {
        section(project, surface, &rows, heading, module, &mut gaps);
    }
    Ok(gaps)
}

pub fn unknown_paths(
    project: &Project,
    krate: &Crate,
    surface: &Surface,
) -> Result<Vec<String>, Error> {
    let known = known_paths(project, krate, surface)?;
    let mut found = Vec::new();
    for row in rows(krate)?.iter().filter(|row| row.named) {
        let mut owner: Option<&str> = None;
        for token in &row.first {
            let token = clean(token);
            if !resolves(token, owner, &known) {
                found.push(format!(
                    "{}:{} names `{token}`, which is not public",
                    project.show(&row.file),
                    row.line
                ));
            }
            if owner.is_none() {
                owner = token.rsplit_once("::").map(|(parent, _)| parent);
            }
        }
    }
    Ok(found)
}

fn section(
    project: &Project,
    surface: &Surface,
    rows: &[Row],
    heading: &str,
    module: &str,
    gaps: &mut Gaps,
) {
    let base = if module.is_empty() {
        surface.name.clone()
    } else {
        format!("{}::{module}", surface.name)
    };
    let members: BTreeSet<&str> = surface
        .named()
        .into_iter()
        .flat_map(|named| named.paths)
        .filter_map(|path| path.strip_prefix(base.as_str())?.strip_prefix("::"))
        .filter(|rest| !rest.contains("::"))
        .collect();
    let listed: Vec<&Row> = rows
        .iter()
        .filter(|row| row.headings.iter().any(|title| title == heading))
        .collect();
    if listed.is_empty() {
        gaps.undocumented
            .push(format!("no table under the heading `{heading}`"));
        return;
    }
    let mut names = BTreeSet::new();
    for row in listed {
        let Some(first) = row.first.first() else {
            continue;
        };
        let name = clean(first);
        names.insert(name);
        if !members.contains(name) {
            gaps.stale.push(format!(
                "{}:{} lists `{name}` under `{heading}`, which `{}` does not export",
                project.show(&row.file),
                row.line,
                short(&base, &surface.name)
            ));
        }
    }
    for member in members.iter().filter(|member| !names.contains(*member)) {
        gaps.undocumented
            .push(format!("`{base}::{member}` has no row under `{heading}`"));
    }
}

fn modules(surface: &Surface) -> HashSet<&str> {
    surface
        .entries
        .iter()
        .filter(|entry| entry.kind == Kind::Mod)
        .map(|entry| entry.path.as_str())
        .chain([surface.name.as_str()])
        .collect()
}

fn known_paths(
    project: &Project,
    krate: &Crate,
    surface: &Surface,
) -> Result<BTreeSet<String>, Error> {
    let mut known = journeys::paths(&surface.spine(), &krate.name);
    for (library, path) in project
        .spines
        .iter()
        .filter(|(library, _)| *library != krate.name)
    {
        known.extend(journeys::paths(
            &read_optional(path)?.unwrap_or_default(),
            library,
        ));
    }
    Ok(known)
}

fn clean(token: &str) -> &str {
    let token = token.trim().trim_end_matches('!');
    let end = token.find(['<', '(', ' ']).unwrap_or(token.len());
    token.get(..end).unwrap_or(token)
}

fn resolves(token: &str, owner: Option<&str>, known: &BTreeSet<String>) -> bool {
    let suffix = |tail: &str| {
        known
            .iter()
            .any(|path| path == tail || path.ends_with(&format!("::{tail}")))
    };
    if token.contains("::") {
        return suffix(token);
    }
    owner.is_some_and(|owner| suffix(&format!("{owner}::{token}")))
        || known
            .iter()
            .any(|path| path.rsplit("::").next() == Some(token))
}

fn rows(krate: &Crate) -> Result<Vec<Row>, Error> {
    let mut rows = Vec::new();
    for file in &krate.docs {
        let text = read_optional(file)?.unwrap_or_default();
        let mut headings: Vec<(usize, String)> = Vec::new();
        let mut named = false;
        let mut previous = "";
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            let level = trimmed.chars().take_while(|ch| *ch == '#').count();
            if level > 0 && trimmed.chars().nth(level) == Some(' ') {
                headings.retain(|(open, _)| *open < level);
                headings.push((
                    level,
                    trimmed.get(level..).unwrap_or_default().trim().to_owned(),
                ));
                named = false;
            } else if trimmed.starts_with('|') {
                if separator(trimmed) {
                    named = cells(previous).first().is_some_and(|cell| *cell == "Name");
                } else if previous.trim_start().starts_with('|') {
                    let titles = headings.iter().map(|(_, title)| title.clone()).collect();
                    rows.push(row(file, index + 1, titles, named, trimmed));
                }
            } else {
                named = false;
            }
            previous = line;
        }
    }
    Ok(rows)
}

fn separator(line: &str) -> bool {
    let cells = cells(line);
    !cells.is_empty()
        && cells
            .iter()
            .all(|cell| cell.contains('-') && cell.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

fn row(file: &Path, line: usize, headings: Vec<String>, named: bool, text: &str) -> Row {
    let first = cells(text)
        .first()
        .map(|cell| spans(cell).into_iter().map(str::to_owned).collect())
        .unwrap_or_default();
    let words = spans(text)
        .into_iter()
        .flat_map(|span| span.split(|ch: char| !(ch.is_alphanumeric() || ch == '_')))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect();
    Row {
        file: file.to_path_buf(),
        line,
        headings,
        named,
        first,
        words,
    }
}

fn cells(line: &str) -> Vec<&str> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    if inner.is_empty() {
        return Vec::new();
    }
    inner.split('|').map(str::trim).collect()
}

fn spans(text: &str) -> Vec<&str> {
    text.split('`').skip(1).step_by(2).collect()
}
