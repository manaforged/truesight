use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;

use clap::ValueEnum;
use public_api::PublicItem;
use public_api::rustdoc_types::{
    Crate as Rustdoc, Deprecation, Id, Item, ItemEnum, ItemKind, Visibility,
};

use crate::config::Project;
use crate::error::Error;
use crate::features;
use crate::package::Crate;
use crate::path::item_path;
use crate::rustdoc::{self, Build};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, ValueEnum)]
pub enum Kind {
    Mod,
    Use,
    Struct,
    Union,
    Enum,
    Variant,
    Field,
    Fn,
    Trait,
    TraitAlias,
    Impl,
    Type,
    Const,
    Static,
    Macro,
    ExternType,
    ExternCrate,
    Primitive,
    Other,
}

impl Kind {
    pub fn of(inner: &ItemEnum) -> Self {
        match inner {
            ItemEnum::Module(_) => Self::Mod,
            ItemEnum::ExternCrate { .. } => Self::ExternCrate,
            ItemEnum::Use(_) => Self::Use,
            ItemEnum::Union(_) => Self::Union,
            ItemEnum::Struct(_) => Self::Struct,
            ItemEnum::StructField(_) => Self::Field,
            ItemEnum::Enum(_) => Self::Enum,
            ItemEnum::Variant(_) => Self::Variant,
            ItemEnum::Function(_) => Self::Fn,
            ItemEnum::Trait(_) => Self::Trait,
            ItemEnum::TraitAlias(_) => Self::TraitAlias,
            ItemEnum::Impl(_) => Self::Impl,
            ItemEnum::TypeAlias(_) | ItemEnum::AssocType { .. } => Self::Type,
            ItemEnum::Constant { .. } | ItemEnum::AssocConst { .. } => Self::Const,
            ItemEnum::Static(_) => Self::Static,
            ItemEnum::ExternType => Self::ExternType,
            ItemEnum::Macro(_) | ItemEnum::ProcMacro(_) => Self::Macro,
            ItemEnum::Primitive(_) => Self::Primitive,
        }
    }

    pub fn is_type(self) -> bool {
        matches!(
            self,
            Self::Struct | Self::Union | Self::Enum | Self::Trait | Self::TraitAlias | Self::Type
        )
    }

    pub fn label(self) -> String {
        self.to_possible_value().map_or_else(
            || String::from("other"),
            |value| value.get_name().to_owned(),
        )
    }
}

#[derive(Clone)]
pub struct Location {
    pub file: PathBuf,
    pub line: usize,
}

#[derive(Clone)]
pub struct Entry {
    pub line: String,
    pub path: String,
    pub kind: Kind,
    pub id: Id,
    pub trait_impl: bool,
    pub definition: Option<String>,
    pub gates: Vec<String>,
    pub location: Option<Location>,
    pub summary: Option<String>,
    pub deprecation: Option<Deprecation>,
}

impl Entry {
    pub fn note(&self) -> Option<String> {
        let note = self.deprecation.as_ref()?.note.as_deref()?;
        let note = note.split_whitespace().collect::<Vec<_>>().join(" ");
        (!note.is_empty()).then_some(note)
    }
}

pub struct Listing {
    pub entries: Vec<Entry>,
    pub format: u32,
    pub globs: Vec<String>,
}

impl Listing {
    pub fn build(project: &Project, krate: &Crate, features: &[String]) -> Result<Self, Error> {
        rustdoc::build(project, krate, features, false).map(|build| Self::of(&build))
    }

    fn of(build: &Build) -> Self {
        let rustdoc = &build.rustdoc;
        let index = &rustdoc.index;
        let mut seen = HashSet::new();
        let entries = build
            .api
            .items()
            .filter_map(|item| {
                let found = index.get(&item.id());
                let kind = found.map_or(Kind::Other, |found| Kind::of(&found.inner));
                let parent = item.parent_id().and_then(|parent| index.get(&parent));
                let trait_impl = is_trait_impl(found) || is_trait_impl(parent);
                if trait_impl && kind == Kind::Fn {
                    return None;
                }
                let line = item.to_string();
                seen.insert(line.clone()).then(|| Entry {
                    path: item_path(&line),
                    line,
                    kind,
                    id: item.id(),
                    trait_impl,
                    definition: definition(rustdoc, item),
                    gates: Vec::new(),
                    location: found
                        .and_then(|found| found.span.as_ref())
                        .map(|span| Location {
                            file: span.filename.clone(),
                            line: span.begin.0,
                        }),
                    summary: found
                        .and_then(|found| found.docs.as_deref())
                        .and_then(|docs| docs.lines().map(str::trim).find(|text| !text.is_empty()))
                        .map(str::to_owned),
                    deprecation: found.and_then(|found| found.deprecation.clone()),
                })
            })
            .collect();
        Self {
            entries,
            format: rustdoc.format_version,
            globs: globs(rustdoc),
        }
    }
}

fn is_trait_impl(item: Option<&Item>) -> bool {
    item.is_some_and(|item| matches!(&item.inner, ItemEnum::Impl(block) if block.trait_.is_some()))
}

fn definition(rustdoc: &Rustdoc, item: &PublicItem) -> Option<String> {
    let local = |id: Id| {
        rustdoc
            .paths
            .get(&id)
            .filter(|summary| summary.crate_id == 0)
    };
    if let Some(summary) = local(item.id()) {
        return Some(summary.path.join("::"));
    }
    let owner = local(item.parent_id()?).filter(|summary| summary.kind != ItemKind::Module)?;
    let name = rustdoc.index.get(&item.id())?.name.as_deref()?;
    Some(format!("{}::{name}", owner.path.join("::")))
}

fn globs(rustdoc: &Rustdoc) -> Vec<String> {
    let found: BTreeSet<String> = rustdoc
        .index
        .values()
        .filter(|item| item.crate_id == 0 && matches!(item.visibility, Visibility::Public))
        .filter_map(|item| match &item.inner {
            ItemEnum::Use(import) if import.is_glob => Some(import.source.clone()),
            _ => None,
        })
        .collect();
    found.into_iter().collect()
}

pub struct Surface {
    pub package: String,
    pub name: String,
    pub version: String,
    pub toolchain: String,
    pub format: u32,
    pub features: String,
    pub build_features: Vec<String>,
    pub gates: Vec<String>,
    pub entries: Vec<Entry>,
    pub globs: Vec<String>,
    pub prelude: Option<String>,
}

pub fn load(project: &Project, krate: &Crate) -> Result<Surface, Error> {
    let plan = features::plan(krate)?;
    let listing = features::gated(project, krate, &plan)?;
    Ok(Surface {
        package: krate.package.clone(),
        name: krate.name.clone(),
        version: krate.version.clone(),
        toolchain: project.toolchain.clone(),
        format: listing.format,
        features: label(krate),
        globs: listing.globs,
        build_features: plan.base,
        gates: krate.gates.clone(),
        entries: listing.entries,
        prelude: krate.prelude.clone(),
    })
}

fn label(krate: &Crate) -> String {
    let mut names: Vec<&str> = Vec::new();
    if krate.default_features {
        names.push("default");
    }
    names.extend(krate.features.iter().map(String::as_str));
    if names.is_empty() {
        String::from("none")
    } else {
        names.join(", ")
    }
}

impl Surface {
    pub fn spine(&self) -> String {
        let mut text = String::new();
        for entry in &self.entries {
            text.push_str(&entry.line);
            text.push('\n');
        }
        text
    }

    pub fn rooted(&self, path: &str) -> bool {
        path.strip_prefix(self.name.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"))
    }

    pub fn feature_roots(&self) -> Vec<(&str, Vec<&str>)> {
        self.gates
            .iter()
            .map(|gate| {
                let gated = |entry: &&Entry| entry.kind != Kind::Impl && entry.gates.contains(gate);
                let paths: HashSet<&str> = self
                    .entries
                    .iter()
                    .filter(gated)
                    .map(|entry| entry.path.as_str())
                    .collect();
                let mut roots: Vec<&str> = paths
                    .iter()
                    .copied()
                    .filter(|path| {
                        path.rsplit_once("::")
                            .is_none_or(|(parent, _)| !paths.contains(parent))
                    })
                    .collect();
                roots.sort_unstable();
                (gate.as_str(), roots)
            })
            .collect()
    }
}
