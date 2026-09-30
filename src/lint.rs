use std::collections::{BTreeSet, HashSet};
use std::fmt;

use crate::Outcome;
use crate::artifacts::{self, Doc};
use crate::config::Project;
use crate::coverage;
use crate::error::Error;
use crate::intent::{self, Task};
use crate::journeys;
use crate::levels::{Level, LintLevels};
use crate::markdown::short;
use crate::modules::{self, Counts, Module};
use crate::package::Crate;
use crate::rustdoc;
use crate::surface::{Kind, Surface};

#[derive(Clone, Copy)]
enum Lint {
    DuplicatePath,
    GlobReexport,
    ReexportOnlyModule,
    NoTask,
    UnknownPath,
    UnknownOwner,
    DuplicateTask,
    MissingGuide,
    OverBudget,
    PreludeBudget,
    JourneyBudget,
    Undocumented,
    StaleDoc,
    UnknownDocPath,
}

impl Lint {
    fn code(self) -> &'static str {
        match self {
            Self::DuplicatePath => "duplicate-path",
            Self::GlobReexport => "glob-reexport",
            Self::ReexportOnlyModule => "reexport-only-module",
            Self::NoTask => "no-task",
            Self::UnknownPath => "unknown-path",
            Self::UnknownOwner => "unknown-owner",
            Self::DuplicateTask => "duplicate-task",
            Self::MissingGuide => "missing-guide",
            Self::OverBudget => "over-budget",
            Self::PreludeBudget => "prelude-budget",
            Self::JourneyBudget => "journey-budget",
            Self::Undocumented => "undocumented",
            Self::StaleDoc => "stale-doc",
            Self::UnknownDocPath => "unknown-doc-path",
        }
    }

    fn level(self, levels: LintLevels) -> Level {
        match self {
            Self::DuplicatePath => levels.duplicate_path,
            Self::GlobReexport => levels.glob_reexport,
            Self::ReexportOnlyModule => levels.reexport_only_module,
            Self::NoTask => levels.no_task,
            Self::Undocumented => levels.undocumented,
            Self::StaleDoc => levels.stale_doc,
            Self::UnknownDocPath => levels.unknown_doc_path,
            Self::UnknownPath
            | Self::UnknownOwner
            | Self::DuplicateTask
            | Self::MissingGuide
            | Self::OverBudget
            | Self::PreludeBudget
            | Self::JourneyBudget => Level::Deny,
        }
    }
}

pub struct Finding {
    pub level: Level,
    lint: Lint,
    message: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.level {
            Level::Allow => "allow",
            Level::Warn => "warn",
            Level::Deny => "deny",
        };
        write!(formatter, "{level}[{}] {}", self.lint.code(), self.message)
    }
}

struct Report {
    levels: LintLevels,
    findings: Vec<Finding>,
}

impl Report {
    fn add(&mut self, lint: Lint, message: String) {
        let level = lint.level(self.levels);
        if level != Level::Allow {
            self.findings.push(Finding {
                level,
                lint,
                message,
            });
        }
    }
}

pub fn run(project: &Project) -> Result<Outcome, Error> {
    let docs = artifacts::document(project)?;
    let denied = print(&docs);
    if docs.iter().all(|doc| doc.findings.is_empty()) {
        println!("no findings");
    }
    Ok(if denied == 0 {
        Outcome::Clean
    } else {
        Outcome::Failed
    })
}

pub fn print(docs: &[Doc<'_>]) -> usize {
    let mut denied = 0;
    for doc in docs {
        for finding in &doc.findings {
            println!("{}: {finding}", doc.surface.package);
            denied += usize::from(finding.level == Level::Deny);
        }
    }
    denied
}

pub fn findings(
    project: &Project,
    krate: &Crate,
    surface: &Surface,
    tasks: &[Task],
) -> Result<Vec<Finding>, Error> {
    let mut report = Report {
        levels: krate.lint,
        findings: Vec::new(),
    };
    let modules = modules::of(surface);
    duplicate_paths(surface, &mut report);
    reexport_modules(surface, &modules, &mut report);
    for source in &surface.globs {
        report.add(
            Lint::GlobReexport,
            format!("`pub use {source}::*` re-exports a glob; name each item"),
        );
    }
    task_paths(project, krate, surface, tasks, &mut report)?;
    if report.levels.no_task != Level::Allow {
        uncovered(surface, tasks, &mut report);
    }
    let items = Counts::total(&modules).items;
    if let Some(budget) = krate.budget
        && items > budget
    {
        report.add(
            Lint::OverBudget,
            format!("{items} public items exceed the budget of {budget}"),
        );
    }
    prelude_budget(krate, surface, &mut report);
    for problem in journeys::problems(krate, &journeys::measure(project, krate, &surface.spine())?)
    {
        report.add(Lint::JourneyBudget, problem);
    }
    if report.levels.undocumented != Level::Allow || report.levels.stale_doc != Level::Allow {
        documentation(project, krate, surface, &mut report)?;
    }
    if report.levels.unknown_doc_path != Level::Allow {
        for message in coverage::unknown_paths(project, krate, surface)? {
            report.add(Lint::UnknownDocPath, message);
        }
    }
    Ok(report.findings)
}

fn prelude_budget(krate: &Crate, surface: &Surface, report: &mut Report) {
    let Some(budget) = krate.prelude_budget else {
        return;
    };
    let count = surface.prelude_names().len();
    if count > budget {
        report.add(
            Lint::PreludeBudget,
            format!("the prelude exports {count} names, over its budget of {budget}"),
        );
    } else if count < budget {
        report.add(
            Lint::PreludeBudget,
            format!("the prelude exports {count} names; lower its budget from {budget} to {count}"),
        );
    }
}

fn documentation(
    project: &Project,
    krate: &Crate,
    surface: &Surface,
    report: &mut Report,
) -> Result<(), Error> {
    for path in coverage::measure(krate, surface)?.missing {
        report.add(
            Lint::Undocumented,
            format!("`{path}` has no rustdoc and no row in the reference docs"),
        );
    }
    let gaps = coverage::sections(project, krate, surface)?;
    for message in gaps.undocumented {
        report.add(Lint::Undocumented, message);
    }
    for message in gaps.stale {
        report.add(Lint::StaleDoc, message);
    }
    Ok(())
}

fn reexport_modules(surface: &Surface, modules: &[Module<'_>], report: &mut Report) {
    for module in modules::reexport_only(modules).filter(|module| {
        surface.prelude.as_deref() != Some(module.path) && !surface.in_prelude(module.path)
    }) {
        report.add(
            Lint::ReexportOnlyModule,
            format!(
                "`{}` only re-exports items that other modules export",
                short(module.path, &surface.name)
            ),
        );
    }
}

fn duplicate_paths(surface: &Surface, report: &mut Report) {
    let mut groups: Vec<BTreeSet<&str>> = surface
        .duplicates()
        .into_iter()
        .filter(|named| {
            !named
                .paths
                .iter()
                .skip(1)
                .all(|path| surface.in_prelude(path))
        })
        .map(|named| named.paths.into_iter().collect())
        .collect();
    groups.sort();
    for paths in &groups {
        let shown: Vec<String> = paths
            .iter()
            .map(|path| format!("`{}`", short(path, &surface.name)))
            .collect();
        report.add(
            Lint::DuplicatePath,
            format!(
                "{} name one item; export it at one path",
                shown.join(" and ")
            ),
        );
    }
}

fn task_paths(
    project: &Project,
    krate: &Crate,
    surface: &Surface,
    tasks: &[Task],
    report: &mut Report,
) -> Result<(), Error> {
    let file = project.show(&krate.intent);
    let mut names = HashSet::new();
    for task in tasks {
        if !names.insert(task.name.as_str()) {
            report.add(
                Lint::DuplicateTask,
                format!("{file}: task `{}` appears more than once", task.name),
            );
        }
        if surface.resolve(&task.call).is_none() {
            report.add(
                Lint::UnknownPath,
                format!(
                    "{file}: task `{}` calls `{}`, which is not public",
                    task.name,
                    short(&task.call, &surface.name)
                ),
            );
        }
        if let Some(guide) = &task.guide
            && !project.root.join(guide).is_file()
        {
            report.add(
                Lint::MissingGuide,
                format!(
                    "{file}: task `{}` names guide `{}`, which does not exist",
                    task.name,
                    guide.display()
                ),
            );
        }
    }
    if tasks.iter().all(|task| task.owner.is_none()) {
        return Ok(());
    }
    let private = rustdoc::build(project, krate, &surface.build_features, true)?;
    let known = intent::owner_paths(&private.rustdoc);
    for task in tasks {
        if let Some(owner) = &task.owner
            && !known.contains(owner)
            && surface.resolve(owner).is_none()
        {
            report.add(
                Lint::UnknownOwner,
                format!(
                    "{file}: task `{}` names owner `{}`, which does not exist",
                    task.name,
                    short(owner, &surface.name)
                ),
            );
        }
    }
    Ok(())
}

fn uncovered(surface: &Surface, tasks: &[Task], report: &mut Report) {
    let called: HashSet<&str> = tasks
        .iter()
        .filter_map(|task| surface.resolve(&task.call))
        .collect();
    let missing: BTreeSet<&str> = surface
        .entries
        .iter()
        .filter(|entry| entry.kind == Kind::Fn)
        .map(|entry| entry.path.as_str())
        .filter(|path| !called.contains(path))
        .collect();
    for path in missing {
        report.add(
            Lint::NoTask,
            format!(
                "`{}` is not the call of any task",
                short(path, &surface.name)
            ),
        );
    }
}
