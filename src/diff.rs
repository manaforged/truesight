use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use clap::{Args, ValueEnum};

use crate::Outcome;
use crate::config::{Project, read_optional};
use crate::error::Error;
use crate::history;
use crate::markdown::{code, short, short_header};
use crate::path::{Subject, is_mod, item_path, rehome};
use crate::surface;

#[derive(Args, Default)]
pub struct DiffArgs {
    #[arg(
        value_name = "REF",
        help = "Git ref whose item list to compare with [default: the item list on disk]"
    )]
    base: Option<String>,
    #[arg(long, value_enum, default_value_t = Format::Text, help = "Output format")]
    format: Format,
}

#[derive(Clone, Copy, Default, ValueEnum)]
enum Format {
    #[default]
    Text,
    Changelog,
}

pub struct Change<'a> {
    pub added: Vec<&'a str>,
    pub removed: Vec<&'a str>,
    kept: Vec<&'a str>,
}

impl<'a> Change<'a> {
    pub fn between(old: &'a str, new: &'a str) -> Self {
        let before: HashSet<&str> = old.lines().collect();
        let after: HashSet<&str> = new.lines().collect();
        Self {
            added: new.lines().filter(|line| !before.contains(line)).collect(),
            removed: old.lines().filter(|line| !after.contains(line)).collect(),
            kept: old.lines().filter(|line| after.contains(line)).collect(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Verb {
    Added,
    Changed,
    Removed,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum What {
    Subject(Subject),
    Path { path: String, others: Vec<String> },
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Bullet {
    verb: Verb,
    what: What,
}

impl Bullet {
    fn text(&self, name: &str) -> String {
        let verb = match self.verb {
            Verb::Added => "Added",
            Verb::Changed => "Changed",
            Verb::Removed => "Removed",
        };
        match &self.what {
            What::Subject(subject) => format!("- {verb} {}.", show(subject, name)),
            What::Path { path, others } => {
                let relation = match self.verb {
                    Verb::Removed => "stays at",
                    Verb::Added | Verb::Changed => "is also at",
                };
                let others: Vec<String> = others
                    .iter()
                    .map(|other| code(short(other, name)))
                    .collect();
                format!(
                    "- {verb} the path {}; the item {relation} {}.",
                    code(short(path, name)),
                    others.join(", ")
                )
            }
        }
    }
}

fn show(subject: &Subject, name: &str) -> String {
    match subject {
        Subject::Item(path) => code(short(path, name)),
        Subject::Impl(header) => code(&short_header(header, name)),
    }
}

pub struct Paths {
    bullets: BTreeSet<Bullet>,
}

impl Paths {
    pub fn of(change: &Change<'_>) -> Self {
        let known = Known::of(change);
        let added = Side::of(&change.added, &known);
        let removed = Side::of(&change.removed, &known);
        let mut bullets = BTreeSet::new();
        for subject in &added.subjects {
            let verb = if removed.subjects.contains(subject) {
                Verb::Changed
            } else {
                Verb::Added
            };
            bullets.insert(Bullet {
                verb,
                what: What::Subject(subject.clone()),
            });
        }
        for subject in removed
            .subjects
            .iter()
            .filter(|subject| !added.subjects.contains(*subject))
        {
            bullets.insert(Bullet {
                verb: Verb::Removed,
                what: What::Subject(subject.clone()),
            });
        }
        for (verb, reroutes) in [
            (Verb::Added, added.reroutes),
            (Verb::Removed, removed.reroutes),
        ] {
            for (path, others) in reroutes {
                bullets.insert(Bullet {
                    verb,
                    what: What::Path { path, others },
                });
            }
        }
        Self { bullets }
    }

    pub fn is_empty(&self) -> bool {
        self.bullets.is_empty()
    }

    fn count(&self, verb: Verb) -> usize {
        self.bullets
            .iter()
            .filter(|bullet| bullet.verb == verb)
            .count()
    }
}

struct Known<'a> {
    kept: HashSet<&'a str>,
    modules: HashSet<String>,
    homes: HashMap<String, BTreeSet<String>>,
}

impl<'a> Known<'a> {
    fn of(change: &Change<'a>) -> Self {
        let modules = change
            .kept
            .iter()
            .chain(&change.added)
            .chain(&change.removed)
            .filter(|line| is_mod(line))
            .map(|line| item_path(line))
            .collect();
        let mut known = Self {
            kept: change.kept.iter().copied().collect(),
            modules,
            homes: HashMap::new(),
        };
        for line in &change.kept {
            if let Subject::Item(path) = Subject::of(line)
                && let Some((parent, name)) = path.rsplit_once("::")
                && known.is_module(parent)
            {
                known
                    .homes
                    .entry(name.to_owned())
                    .or_default()
                    .insert(parent.to_owned());
            }
        }
        known
    }

    fn is_module(&self, path: &str) -> bool {
        !path.contains("::") || self.modules.contains(path)
    }

    fn elsewhere(&self, root: &str, members: &[&str]) -> Vec<String> {
        let Some((parent, name)) = root.rsplit_once("::") else {
            return Vec::new();
        };
        let Some(homes) = self.homes.get(name).filter(|_| self.is_module(parent)) else {
            return Vec::new();
        };
        homes
            .iter()
            .filter(|home| home.as_str() != parent)
            .filter(|home| {
                members.iter().all(|line| {
                    rehome(line, parent, home)
                        .is_some_and(|moved| self.kept.contains(moved.as_str()))
                })
            })
            .map(|home| format!("{home}::{name}"))
            .collect()
    }
}

struct Side {
    subjects: BTreeSet<Subject>,
    reroutes: BTreeMap<String, Vec<String>>,
}

impl Side {
    fn of(lines: &[&str], known: &Known<'_>) -> Self {
        let parsed: Vec<(&str, Subject)> = lines
            .iter()
            .map(|&line| (line, Subject::of(line)))
            .collect();
        let paths: HashSet<&str> = parsed
            .iter()
            .filter_map(|(_, subject)| subject.item())
            .collect();
        let mut groups: HashMap<&str, Vec<&str>> = HashMap::new();
        for (line, subject) in &parsed {
            if let Some(path) = subject.item() {
                groups.entry(top(path, &paths)).or_default().push(line);
            }
        }
        let mut reroutes = BTreeMap::new();
        let mut covered: HashSet<&str> = HashSet::new();
        for (&root, members) in &groups {
            let others = known.elsewhere(root, members);
            if !others.is_empty() {
                covered.extend(members.iter().copied());
                reroutes.insert(root.to_owned(), others);
            }
        }
        Self {
            subjects: parsed
                .iter()
                .filter(|(line, _)| !covered.contains(line))
                .map(|(_, subject)| subject.clone())
                .collect(),
            reroutes,
        }
    }
}

fn top<'a>(path: &'a str, paths: &HashSet<&str>) -> &'a str {
    let mut top = path;
    while let Some((parent, _)) = top.rsplit_once("::") {
        if !paths.contains(parent) {
            break;
        }
        top = parent;
    }
    top
}

pub fn bullets(paths: &Paths, name: &str) -> Vec<String> {
    paths
        .bullets
        .iter()
        .map(|bullet| bullet.text(name))
        .collect()
}

pub fn run(project: &Project, args: &DiffArgs) -> Result<Outcome, Error> {
    for krate in &project.crates {
        let base = match &args.base {
            Some(reference) => {
                history::spine_at(project, krate, reference)?.ok_or_else(|| Error::NoSpineAt {
                    reference: reference.clone(),
                    path: krate.spine.clone(),
                })?
            }
            None => {
                read_optional(&krate.spine)?.ok_or_else(|| Error::NoSpine(krate.spine.clone()))?
            }
        };
        let surface = surface::load(project, krate)?;
        let current = surface.spine();
        let change = Change::between(&base, &current);
        let paths = Paths::of(&change);
        match args.format {
            Format::Text => {
                let against = args.base.as_deref().map_or_else(
                    || project.show(&krate.spine),
                    |reference| format!("{reference}:{}", project.show(&krate.spine)),
                );
                println!(
                    "{}: {} added, {} changed, {} removed against {against}",
                    krate.package,
                    paths.count(Verb::Added),
                    paths.count(Verb::Changed),
                    paths.count(Verb::Removed)
                );
                for line in &change.removed {
                    println!("- {line}");
                }
                for line in &change.added {
                    println!("+ {line}");
                }
            }
            Format::Changelog => {
                println!("### {} API\n", code(&krate.package));
                if paths.is_empty() {
                    println!("No API changes.");
                }
                for bullet in bullets(&paths, &surface.name) {
                    println!("{bullet}");
                }
                println!();
            }
        }
    }
    Ok(Outcome::Clean)
}
