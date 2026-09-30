use crate::Outcome;
use crate::artifacts::{self, Doc};
use crate::config::Project;
use crate::coverage::{self, Coverage};
use crate::error::Error;
use crate::journeys::{self, Journey};
use crate::levels::Level;
use crate::markdown::{Align, table};
use crate::modules::{self, Counts};

struct Health {
    counts: Counts,
    modules: usize,
    budget: Option<usize>,
    prelude: Option<usize>,
    prelude_budget: Option<usize>,
    journeys: Vec<Journey>,
    coverage: Coverage,
    unknown: usize,
    deny: usize,
    warn: usize,
}

fn measure(project: &Project, doc: &Doc<'_>) -> Result<Health, Error> {
    let modules = modules::of(&doc.surface);
    let level = |wanted: Level| {
        doc.findings
            .iter()
            .filter(|finding| finding.level == wanted)
            .count()
    };
    Ok(Health {
        counts: Counts::total(&modules),
        modules: modules.len(),
        budget: doc.krate.budget,
        prelude: doc
            .surface
            .prelude
            .as_ref()
            .map(|_| doc.surface.prelude_names().len()),
        prelude_budget: doc.krate.prelude_budget,
        journeys: journeys::measure(project, doc.krate, &doc.surface.spine())?,
        coverage: coverage::measure(doc.krate, &doc.surface)?,
        unknown: coverage::unknown_paths(project, doc.krate, &doc.surface)?.len(),
        deny: level(Level::Deny),
        warn: level(Level::Warn),
    })
}

fn budget(value: Option<usize>) -> String {
    value.map_or_else(String::new, |budget| format!("; budget {budget}"))
}

impl Health {
    fn lines(&self) -> Vec<(&'static str, String)> {
        let counts = &self.counts;
        vec![
            (
                "surface",
                format!(
                    "{} items in {} modules: {} types, {} functions, {} methods, {} fields, {} variants, {} constants; {} aliases{}",
                    counts.items,
                    self.modules,
                    counts.types,
                    counts.functions,
                    counts.methods,
                    counts.fields,
                    counts.variants,
                    counts.constants,
                    counts.aliases,
                    budget(self.budget)
                ),
            ),
            (
                "app tier",
                self.prelude.map_or_else(
                    || String::from("no prelude configured"),
                    |names| format!("{names} prelude names{}", budget(self.prelude_budget)),
                ),
            ),
            ("journeys", self.journey_line()),
            ("documented", self.coverage_line()),
            (
                "doc paths",
                format!(
                    "{} reference rows name paths that are not public",
                    self.unknown
                ),
            ),
            (
                "findings",
                format!("{} deny, {} warn", self.deny, self.warn),
            ),
        ]
    }

    fn journey_line(&self) -> String {
        if self.journeys.is_empty() {
            return String::from("none configured");
        }
        let within = self
            .journeys
            .iter()
            .filter(|journey| journey.budget == Some(journey.names.len()))
            .count();
        let each: Vec<String> = self
            .journeys
            .iter()
            .map(|journey| match journey.budget {
                Some(budget) => format!("{} {}/{budget}", journey.name, journey.names.len()),
                None => format!("{} {}", journey.name, journey.names.len()),
            })
            .collect();
        format!(
            "{} journeys, {within} at budget: {}",
            self.journeys.len(),
            each.join(", ")
        )
    }

    fn coverage_line(&self) -> String {
        let coverage = &self.coverage;
        let documented = coverage.rustdoc + coverage.reference;
        let percent = (documented * 100)
            .checked_div(coverage.items)
            .unwrap_or(100);
        format!(
            "{documented} of {} module-level items ({percent}%): {} by rustdoc, {} by reference docs",
            coverage.items, coverage.rustdoc, coverage.reference
        )
    }
}

pub fn run(project: &Project) -> Result<Outcome, Error> {
    let docs = artifacts::document(project)?;
    let (generated, stale) = artifacts::staleness(project, &docs)?;
    let mut failed = !stale.is_empty();
    for doc in &docs {
        let health = measure(project, doc)?;
        println!("{} {}", doc.surface.package, doc.surface.version);
        for (label, value) in health.lines() {
            println!("  {label:<11} {value}");
        }
        failed |= health.deny > 0;
    }
    println!("generated {generated} files, {} stale", stale.len());
    Ok(if failed {
        Outcome::Failed
    } else {
        Outcome::Clean
    })
}

pub fn block(project: &Project, doc: &Doc<'_>) -> Result<String, String> {
    let health = measure(project, doc).map_err(|error| error.to_string())?;
    let rows: Vec<Vec<String>> = health
        .lines()
        .into_iter()
        .map(|(label, value)| vec![label.to_owned(), value])
        .collect();
    Ok(table(
        &[("Measure", Align::Left), ("Value", Align::Left)],
        &rows,
    ))
}
