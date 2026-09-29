use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::artifacts::write;
use crate::config::{Crate, canonical, read_optional};
use crate::error::Error;
use crate::homes::Homes;
use crate::markdown::code;
use crate::migrate::{Left, Tally, internal};
use crate::source::{Scope, Source, is_ident};
use crate::surface::Kind;

const ARROW: &str = "--> ";
const LOOKAHEAD: usize = 4;

#[derive(Clone, Copy)]
enum Missing {
    Type,
    Struct,
    Value,
    Path,
}

struct Miss {
    missing: Missing,
    name: String,
    file: PathBuf,
    line: usize,
}

impl Missing {
    fn of(code: &str) -> Option<Self> {
        match code {
            "E0412" => Some(Self::Type),
            "E0422" => Some(Self::Struct),
            "E0425" => Some(Self::Value),
            "E0433" => Some(Self::Path),
            _ => None,
        }
    }

    fn fits(self, kind: Kind) -> bool {
        match self {
            Self::Type => kind.is_type(),
            Self::Struct => matches!(kind, Kind::Struct | Kind::Union | Kind::Type),
            Self::Value => matches!(kind, Kind::Fn | Kind::Const | Kind::Static | Kind::Struct),
            Self::Path => kind == Kind::Mod || kind.is_type(),
        }
    }
}

pub fn fix(
    errors: &Path,
    roots: &[PathBuf],
    homes: &[(&Crate, Homes)],
    tally: &mut Tally,
) -> Result<(), Error> {
    let text = std::fs::read_to_string(errors).map_err(|source| Error::Io {
        path: errors.to_owned(),
        source,
    })?;
    let mut wanted: BTreeMap<PathBuf, Vec<(usize, String)>> = BTreeMap::new();
    for miss in misses(&plain(&text)) {
        let Some(file) = locate(&miss.file, roots) else {
            tally.left.push(Left {
                what: format!(
                    "{} is missing in a file outside the given paths",
                    code(&miss.name)
                ),
                file: miss.file,
                line: miss.line,
            });
            continue;
        };
        match home(&miss, &file, homes) {
            Ok(path) => wanted.entry(file).or_default().push((miss.line, path)),
            Err(what) => tally.left.push(Left {
                file,
                line: miss.line,
                what,
            }),
        }
    }
    for (file, wants) in wanted {
        let added = insert(&file, &wants)?;
        if added > 0 {
            tally.imported += added;
            tally.changed.insert(file);
        }
    }
    Ok(())
}

fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(current) = chars.next() {
        if current == '\u{1b}' {
            for skipped in chars.by_ref() {
                if skipped.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(current);
        }
    }
    out
}

fn misses(text: &str) -> Vec<Miss> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some((code, message)) = line
            .strip_prefix("error[")
            .and_then(|rest| rest.split_once("]: "))
        else {
            continue;
        };
        let Some(missing) = Missing::of(code) else {
            continue;
        };
        let unresolved = message.contains("cannot find") || message.contains("use of undeclared");
        let name = message.split('`').nth(1).filter(|name| is_ident(name));
        let place = lines
            .iter()
            .skip(index + 1)
            .take(LOOKAHEAD)
            .find_map(|next| arrow(next));
        if let (true, false, Some(name), Some((file, line))) =
            (unresolved, message.contains("` in `"), name, place)
        {
            found.push(Miss {
                missing,
                name: name.to_owned(),
                file,
                line,
            });
        }
    }
    found
}

fn arrow(line: &str) -> Option<(PathBuf, usize)> {
    let place = line.trim_start().strip_prefix(ARROW)?;
    let mut parts = place.rsplitn(3, ':');
    parts.next()?;
    let line = parts.next()?.parse().ok()?;
    Some((PathBuf::from(parts.next()?), line))
}

fn locate(file: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = if file.is_absolute() {
        vec![file.to_owned()]
    } else {
        roots
            .iter()
            .flat_map(|root| root.ancestors().map(|dir| dir.join(file)))
            .collect()
    };
    candidates
        .into_iter()
        .filter(|candidate| candidate.is_file())
        .filter_map(|candidate| canonical(&candidate).ok())
        .find(|candidate| roots.iter().any(|root| candidate.starts_with(root)))
}

fn home(miss: &Miss, file: &Path, homes: &[(&Crate, Homes)]) -> Result<String, String> {
    let found: Vec<(&Crate, String)> = homes
        .iter()
        .flat_map(|(krate, homes)| {
            homes
                .import(&miss.name, |kind| miss.missing.fits(kind))
                .into_iter()
                .map(move |path| (*krate, path))
        })
        .collect();
    match found.as_slice() {
        [(krate, path)] => Ok(match path.strip_prefix(krate.name.as_str()) {
            Some(rest) if internal(krate, file) => format!("crate{rest}"),
            _ => path.clone(),
        }),
        [] => Err(format!("{} has no home", code(&miss.name))),
        several => {
            let shown: Vec<String> = several.iter().map(|(_, path)| code(path)).collect();
            Err(format!(
                "{} is ambiguous; candidates: {}",
                code(&miss.name),
                shown.join(", ")
            ))
        }
    }
}

fn insert(file: &Path, wants: &[(usize, String)]) -> Result<usize, Error> {
    let text = read_optional(file)?.unwrap_or_default();
    let source = Source::new(&text);
    let mut scopes: BTreeMap<Scope, BTreeSet<&str>> = BTreeMap::new();
    for (line, path) in wants {
        scopes
            .entry(source.scope(*line))
            .or_default()
            .insert(path.as_str());
    }
    let mut edits: Vec<(usize, String)> = Vec::new();
    let mut added = 0;
    for (scope, paths) in &scopes {
        let (at, indent) = source.insertion(*scope);
        let region = text.get(scope.start..scope.end).unwrap_or_default();
        let mut lines = String::new();
        for path in paths {
            let line = format!("use {path};");
            if !region.lines().any(|existing| existing.trim() == line) {
                lines.push_str(&format!("{indent}{line}\n"));
                added += 1;
            }
        }
        edits.push((at, lines));
    }
    edits.sort_by_key(|(at, _)| Reverse(*at));
    let mut out = text;
    for (at, lines) in edits {
        out.insert_str(at, &lines);
    }
    if added > 0 {
        write(file, &out)?;
    }
    Ok(added)
}
