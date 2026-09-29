use clap::Args;

use crate::Outcome;
use crate::config::Project;
use crate::error::Error;
use crate::modules;
use crate::pages;
use crate::surface::{self, Entry, Kind, Surface};

#[derive(Args, Default)]
pub struct ShowArgs {
    #[arg(help = "Print only the items whose path contains PATTERN")]
    pattern: Option<String>,
    #[arg(long, value_enum, help = "Print only the items of this kind")]
    kind: Option<Kind>,
    #[arg(long = "where", help = "Print the source file and line of each item")]
    locate: bool,
}

pub fn run(project: &Project, args: &ShowArgs) -> Result<Outcome, Error> {
    for krate in &project.crates {
        let surface = surface::load(project, krate)?;
        if args.pattern.is_none() && args.kind.is_none() {
            summary(&surface, args.locate);
        } else {
            items(&surface, args);
        }
    }
    Ok(Outcome::Clean)
}

fn summary(surface: &Surface, locate: bool) {
    let modules = modules::of(surface);
    println!(
        "{} {} · features: {} · {} · rustdoc format {}",
        surface.package, surface.version, surface.features, surface.toolchain, surface.format
    );
    println!("{}\n", pages::summary_line(&modules));
    let width = modules
        .iter()
        .flat_map(|module| {
            module
                .groups
                .iter()
                .map(|group| group.name.len() + 2)
                .chain([module.path.len()])
        })
        .max()
        .unwrap_or_default();
    for module in &modules {
        println!(
            "{:<width$}  {:<7}{:>5}",
            module.path,
            "mod",
            module.counts().items
        );
        for group in &module.groups {
            let place = if locate {
                group.head.map(site).unwrap_or_default()
            } else {
                String::new()
            };
            println!(
                "  {:<inner$}  {:<7}{:>5}{place}",
                group.name,
                group.kind(),
                group.entries.len(),
                inner = width.saturating_sub(2)
            );
        }
    }
}

fn items(surface: &Surface, args: &ShowArgs) {
    let matches = surface.entries.iter().filter(|entry| {
        args.kind.is_none_or(|kind| entry.kind == kind)
            && args
                .pattern
                .as_deref()
                .is_none_or(|pattern| entry.path.contains(pattern))
    });
    for entry in matches {
        let place = if args.locate {
            site(entry)
        } else {
            String::new()
        };
        println!("{}{place}", entry.line);
    }
}

fn site(entry: &Entry) -> String {
    entry
        .location
        .as_ref()
        .map(|location| format!("  {}:{}", location.file.display(), location.line))
        .unwrap_or_default()
}
