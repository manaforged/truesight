use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use public_api::rustdoc_types::{Crate as Rustdoc, Id, ItemEnum, Module, Type};
use serde::Deserialize;

use crate::config::{Crate, read_optional};
use crate::error::Error;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct IntentFile {
    #[serde(default, rename = "task")]
    tasks: Vec<Task>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub name: String,
    pub call: String,
    pub owner: Option<String>,
    pub guide: Option<PathBuf>,
}

pub fn load(krate: &Crate) -> Result<Vec<Task>, Error> {
    let Some(text) = read_optional(&krate.intent)? else {
        return Ok(Vec::new());
    };
    let file: IntentFile = toml::from_str(&text).map_err(|source| Error::Toml {
        path: krate.intent.clone(),
        source: Box::new(source),
    })?;
    Ok(file
        .tasks
        .into_iter()
        .map(|task| task.qualified(&krate.name))
        .collect())
}

impl Task {
    fn qualified(self, name: &str) -> Self {
        Self {
            call: qualify(name, self.call),
            owner: self.owner.map(|owner| qualify(name, owner)),
            ..self
        }
    }
}

pub fn qualify(name: &str, path: String) -> String {
    if path == name || path.starts_with(&format!("{name}::")) {
        path
    } else {
        format!("{name}::{path}")
    }
}

pub fn owner_paths(rustdoc: &Rustdoc) -> HashSet<String> {
    let names = names(rustdoc);
    let mut known: HashSet<String> = names.values().flatten().cloned().collect();
    for item in rustdoc.index.values() {
        let ItemEnum::Impl(block) = &item.inner else {
            continue;
        };
        let Type::ResolvedPath(target) = &block.for_ else {
            continue;
        };
        let Some(prefixes) = names.get(&target.id) else {
            continue;
        };
        for name in block
            .items
            .iter()
            .filter_map(|id| rustdoc.index.get(id))
            .filter_map(|member| member.name.as_ref())
        {
            known.extend(prefixes.iter().map(|prefix| format!("{prefix}::{name}")));
        }
    }
    known
}

fn names(rustdoc: &Rustdoc) -> HashMap<Id, Vec<String>> {
    let mut names: HashMap<Id, Vec<String>> = HashMap::new();
    for (id, summary) in &rustdoc.paths {
        if summary.crate_id == 0 {
            names.entry(*id).or_default().push(summary.path.join("::"));
        }
    }
    for (id, item) in &rustdoc.index {
        let ItemEnum::Module(module) = &item.inner else {
            continue;
        };
        let Some(parent) = rustdoc
            .paths
            .get(id)
            .filter(|summary| summary.crate_id == 0)
        else {
            continue;
        };
        let parent = parent.path.join("::");
        for (name, target) in imports(rustdoc, module, &mut HashSet::new()) {
            names
                .entry(target)
                .or_default()
                .push(format!("{parent}::{name}"));
        }
    }
    names
}

fn imports(rustdoc: &Rustdoc, module: &Module, seen: &mut HashSet<Id>) -> Vec<(String, Id)> {
    let mut found = Vec::new();
    for child in module
        .items
        .iter()
        .filter_map(|child| rustdoc.index.get(child))
    {
        let ItemEnum::Use(import) = &child.inner else {
            continue;
        };
        let Some(target) = import.id else {
            continue;
        };
        if !import.is_glob {
            found.push((import.name.clone(), target));
            continue;
        }
        if !seen.insert(target) {
            continue;
        }
        if let Some(ItemEnum::Module(source)) = rustdoc.index.get(&target).map(|item| &item.inner) {
            found.extend(bound(rustdoc, source, seen));
        }
    }
    found
}

fn bound(rustdoc: &Rustdoc, module: &Module, seen: &mut HashSet<Id>) -> Vec<(String, Id)> {
    let mut found = imports(rustdoc, module, seen);
    found.extend(
        module
            .items
            .iter()
            .filter_map(|id| rustdoc.index.get(id))
            .filter(|item| !matches!(item.inner, ItemEnum::Use(_)))
            .filter_map(|item| item.name.clone().map(|name| (name, item.id))),
    );
    found
}
