use clap::ValueEnum;
use serde::Serialize;

use crate::Outcome;
use crate::config::{Project, read_optional};
use crate::error::Error;
use crate::markdown::relative;
use crate::path::{item_kind, item_path};
use crate::surface::Kind;

#[derive(Serialize)]
struct Listing<'a> {
    package: &'a str,
    file: String,
    items: Vec<Entry<'a>>,
}

#[derive(Serialize)]
struct Entry<'a> {
    kind: String,
    path: String,
    line: &'a str,
}

fn name(kind: Kind) -> String {
    kind.to_possible_value()
        .map_or_else(|| "other".to_owned(), |value| value.get_name().to_owned())
}

pub fn run(project: &Project) -> Result<Outcome, Error> {
    let mut texts = Vec::with_capacity(project.spines.len());
    for (package, spine) in &project.spines {
        if let Some(text) = read_optional(spine)? {
            texts.push((package.as_str(), relative(&project.root, spine), text));
        }
    }
    let listings: Vec<Listing<'_>> = texts
        .iter()
        .map(|(package, file, text)| Listing {
            package,
            file: file.clone(),
            items: text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| Entry {
                    kind: name(item_kind(line).unwrap_or(Kind::Impl)),
                    path: item_path(line),
                    line,
                })
                .collect(),
        })
        .collect();
    let json = serde_json::to_string(&listings).map_err(|source| Error::Json {
        path: project.root.clone(),
        source,
    })?;
    println!("{json}");
    Ok(Outcome::Clean)
}
