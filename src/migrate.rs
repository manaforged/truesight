use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use clap::Args;

use crate::Outcome;
use crate::artifacts::write;
use crate::config::{Project, canonical, read_optional};
use crate::error::Error;
use crate::history;
use crate::homes::{Doubt, Homes};
use crate::imports;
use crate::markdown::code;
use crate::package::Crate;
use crate::pages::{self, amount};
use crate::rewrite;
use crate::surface;

const SOURCES: [&str; 2] = ["rs", "md"];
const SKIPPED: &str = "target";

#[derive(Args)]
pub struct MigrateArgs {
    #[arg(
        long,
        value_name = "REF",
        help = "Git ref whose item list has the old paths"
    )]
    from: String,
    #[arg(
        required = true,
        value_name = "PATH",
        help = "Files and directories whose .rs and .md files to rewrite"
    )]
    paths: Vec<PathBuf>,
    #[arg(
        long,
        value_name = "FILE",
        help = "Compiler output: add a `use` line for each name it cannot find"
    )]
    fix_imports: Option<PathBuf>,
}

pub struct Left {
    pub file: PathBuf,
    pub line: usize,
    pub what: String,
}

#[derive(Default)]
pub struct Tally {
    pub changed: BTreeSet<PathBuf>,
    pub rewritten: usize,
    pub imported: usize,
    pub left: Vec<Left>,
}

pub fn run(project: &Project, args: &MigrateArgs) -> Result<Outcome, Error> {
    let roots = args
        .paths
        .iter()
        .map(|path| canonical(path))
        .collect::<Result<Vec<_>, _>>()?;
    let files = sources(project, &roots)?;
    let mut tally = Tally::default();
    let mut homes = Vec::new();
    for krate in &project.crates {
        let old =
            history::spine_at(project, krate, &args.from)?.ok_or_else(|| Error::NoSpineAt {
                reference: args.from.clone(),
                path: krate.spine.clone(),
            })?;
        let surface = surface::load(project, krate)?;
        let found = Homes::new(&surface, &old);
        for file in &files {
            rewrite_file(file, krate, &found, &mut tally)?;
        }
        homes.push((krate, found));
    }
    if let Some(errors) = &args.fix_imports {
        imports::fix(errors, &roots, &homes, &mut tally)?;
    }
    tally.print(project, &args.from);
    Ok(Outcome::Clean)
}

pub fn internal(krate: &Crate, file: &Path) -> bool {
    let Some(dir) = krate.library.parent() else {
        return false;
    };
    let dir = canonical(dir).unwrap_or_else(|_| dir.to_owned());
    file.extension().is_some_and(|extension| extension == "rs")
        && file.starts_with(&dir)
        && !file.starts_with(dir.join("bin"))
        && file != dir.join("main.rs")
}

fn rewrite_file(file: &Path, krate: &Crate, homes: &Homes, tally: &mut Tally) -> Result<(), Error> {
    let text = read_optional(file)?.unwrap_or_default();
    let mut roots = vec![homes.name()];
    if internal(krate, file) {
        roots.push("crate");
    }
    let done = rewrite::file(&text, &roots, homes);
    for unsure in done.unsure {
        let what = match unsure.doubt {
            Doubt::Several(paths) => {
                let shown: Vec<String> = paths.iter().map(|path| code(path)).collect();
                format!(
                    "{} is ambiguous; candidates: {}",
                    code(&unsure.path),
                    shown.join(", ")
                )
            }
            Doubt::Nowhere => format!("{} has no new home", code(&unsure.path)),
        };
        tally.left.push(Left {
            file: file.to_owned(),
            line: unsure.line,
            what,
        });
    }
    if done.text != text {
        write(file, &done.text)?;
        tally.changed.insert(file.to_owned());
        tally.rewritten += done.moved;
    }
    Ok(())
}

fn generated(project: &Project) -> Vec<PathBuf> {
    project
        .book
        .iter()
        .flat_map(|book| {
            project
                .crates
                .iter()
                .map(move |krate| pages::dir(book, &krate.package))
        })
        .collect()
}

fn source(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| SOURCES.contains(&extension))
}

fn sources(project: &Project, roots: &[PathBuf]) -> Result<Vec<PathBuf>, Error> {
    let generated = generated(project);
    let mut files = Vec::new();
    let mut pending = roots.to_vec();
    while let Some(path) = pending.pop() {
        if !path.is_dir() {
            if source(&path) {
                files.push(path);
            }
            continue;
        }
        let io = |error| Error::Io {
            path: path.clone(),
            source: error,
        };
        for entry in std::fs::read_dir(&path).map_err(io)? {
            let entry = entry.map_err(io)?;
            let kind = entry.file_type().map_err(io)?;
            let child = entry.path();
            let name = entry.file_name();
            let hidden = name.to_string_lossy().starts_with('.') || name == SKIPPED;
            if kind.is_dir() && !hidden && !generated.contains(&child) {
                pending.push(child);
            } else if kind.is_file() && source(&child) {
                files.push(child);
            }
        }
    }
    files.sort();
    files.dedup();
    Ok(files)
}

impl Tally {
    fn print(&mut self, project: &Project, from: &str) {
        println!(
            "migrate from {from}: {} changed, {} rewritten, {} added, {} left for a human",
            amount(self.changed.len(), "file"),
            amount(self.rewritten, "path"),
            amount(self.imported, "use line"),
            amount(self.left.len(), "path")
        );
        self.left
            .sort_by(|left, right| left.file.cmp(&right.file).then(left.line.cmp(&right.line)));
        for left in &self.left {
            println!("{}:{}: {}", project.show(&left.file), left.line, left.what);
        }
    }
}
