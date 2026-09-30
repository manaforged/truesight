use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::Outcome;
use crate::config::Project;
use crate::error::Error;
use crate::markdown::{code, short};
use crate::modules;
use crate::origin::{Tree, Via};
use crate::package::Crate;
use crate::pages::amount;
use crate::rustdoc;
use crate::surface::{self, Kind, Location, Surface};

#[derive(Default)]
struct Plan {
    files: BTreeMap<PathBuf, Vec<(usize, String)>>,
    loose: Vec<String>,
    groups: usize,
    extra: usize,
    kept: usize,
    reexports: usize,
    clashes: usize,
}

pub fn run(project: &Project) -> Result<Outcome, Error> {
    for (index, krate) in project.crates.iter().enumerate() {
        if index > 0 {
            println!();
        }
        let surface = surface::load(project, krate)?;
        plan(project, krate, &surface)?.print(&surface);
    }
    Ok(Outcome::Clean)
}

fn plan(project: &Project, krate: &Crate, surface: &Surface) -> Result<Plan, Error> {
    let mut plan = Plan::default();
    {
        let public = rustdoc::build(project, krate, &surface.build_features, false)?;
        plan.aliases(surface, &Tree::new(&public.rustdoc));
    }
    plan.reexports(surface);
    let private = rustdoc::build(project, krate, &surface.build_features, true)?;
    plan.clashes(surface, &Tree::new(&private.rustdoc));
    Ok(plan)
}

fn alias_note(surface: &Surface, path: &str, own: &str, via: Option<Via>, kept: bool) -> String {
    let what = match via {
        Some(Via::Use) => "use adds ",
        Some(Via::Glob) => "glob use adds ",
        Some(Via::Module) => "pub mod adds ",
        None => "",
    };
    let tail = if kept { " (prelude, kept)" } else { "" };
    format!(
        "{what}{}, another path to {}{tail}",
        code(short(path, &surface.name)),
        code(short(own, &surface.name))
    )
}

impl Plan {
    fn add(&mut self, location: Option<Location>, note: String) {
        match location {
            Some(location) => self
                .files
                .entry(location.file)
                .or_default()
                .push((location.line, note)),
            None => self.loose.push(note),
        }
    }

    fn aliases(&mut self, surface: &Surface, tree: &Tree<'_>) {
        for named in surface.duplicates() {
            let Some((own, others)) = named.paths.split_first() else {
                continue;
            };
            self.groups += 1;
            for path in others {
                let kept = surface.in_prelude(path);
                if kept {
                    self.kept += 1;
                } else {
                    self.extra += 1;
                }
                let site = tree.origin(path, own, named.kind);
                let note = alias_note(surface, path, own, site.as_ref().map(|site| site.via), kept);
                self.add(site.map(|site| site.location), note);
            }
        }
    }

    fn reexports(&mut self, surface: &Surface) {
        let modules = modules::of(surface);
        for module in modules::reexport_only(&modules) {
            if surface.prelude.as_deref() == Some(module.path) || surface.in_prelude(module.path) {
                continue;
            }
            self.reexports += 1;
            let location = surface
                .entries
                .iter()
                .find(|entry| entry.kind == Kind::Mod && entry.path == module.path)
                .and_then(|entry| entry.location.clone());
            let note = format!(
                "module {} only re-exports items whose own path is elsewhere",
                code(short(module.path, &surface.name))
            );
            self.add(location, note);
        }
    }

    fn clashes(&mut self, surface: &Surface, tree: &Tree<'_>) {
        for clash in tree.clashes() {
            self.clashes += 1;
            let module = format!("{}::{}", clash.module, clash.name);
            let note = format!(
                "use brings fn {} next to module {}, which is not public",
                code(&clash.name),
                code(short(&module, &surface.name))
            );
            self.add(clash.location, note);
        }
    }

    fn print(mut self, surface: &Surface) {
        println!(
            "{}: {} with {} and {}; {}; {}",
            surface.package,
            amount(self.groups, "alias group"),
            amount(self.extra, "extra path"),
            amount(self.kept, "prelude path"),
            amount(self.reexports, "re-export-only module"),
            amount(self.clashes, "name conflict")
        );
        for (file, notes) in &mut self.files {
            notes.sort();
            println!("\n{}", file.display());
            for (line, note) in notes.iter() {
                println!("  {line}: {note}");
            }
        }
        if !self.loose.is_empty() {
            self.loose.sort();
            println!("\nno source line");
            for note in &self.loose {
                println!("  {note}");
            }
        }
    }
}
