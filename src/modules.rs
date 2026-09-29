use std::collections::{HashMap, HashSet};
use std::ops::AddAssign;

use public_api::rustdoc_types::Id;

use crate::markdown::Anchors;
use crate::surface::{Entry, Kind, Surface};

const OTHER: &str = "Implementations";
const SECTIONS: [(&str, &[Kind]); 10] = [
    ("Structs", &[Kind::Struct]),
    ("Enums", &[Kind::Enum]),
    ("Unions", &[Kind::Union]),
    ("Traits", &[Kind::Trait, Kind::TraitAlias]),
    ("Functions", &[Kind::Fn]),
    ("Type aliases", &[Kind::Type]),
    ("Constants", &[Kind::Const]),
    ("Statics", &[Kind::Static]),
    ("Macros", &[Kind::Macro]),
    ("Re-exports", &[Kind::Use, Kind::ExternCrate]),
];

pub struct Module<'a> {
    pub path: &'a str,
    pub groups: Vec<Group<'a>>,
    counts: Counts,
}

pub struct Group<'a> {
    pub name: &'a str,
    pub head: Option<&'a Entry>,
    pub entries: Vec<&'a Entry>,
}

#[derive(Default, Clone, Copy)]
pub struct Counts {
    pub items: usize,
    pub types: usize,
    pub functions: usize,
    pub methods: usize,
    pub fields: usize,
    pub variants: usize,
    pub constants: usize,
    pub statics: usize,
    pub macros: usize,
    pub others: usize,
    pub aliases: usize,
    pub trait_impls: usize,
    pub inherent_impls: usize,
}

struct Census<'a> {
    canonical: HashMap<Id, &'a str>,
    modules: HashSet<&'a str>,
    counted: HashSet<(Id, &'a str)>,
}

pub fn of(surface: &Surface) -> Vec<Module<'_>> {
    let paths = module_paths(surface);
    let mut modules: Vec<Module<'_>> = paths
        .iter()
        .map(|&path| Module {
            path,
            groups: Vec::new(),
            counts: Counts::default(),
        })
        .collect();
    for entry in surface
        .entries
        .iter()
        .filter(|entry| entry.kind != Kind::Mod)
    {
        let (index, name) = place(surface, &paths, entry);
        if let Some(module) = modules.get_mut(index) {
            module.add(name, entry);
        }
    }
    let mut census = Census::of(surface, &paths);
    for module in &mut modules {
        module.find_heads();
        module.counts = census.count(&module.groups);
    }
    modules
}

fn module_paths(surface: &Surface) -> Vec<&str> {
    let mut paths: Vec<&str> = surface
        .entries
        .iter()
        .filter(|entry| entry.kind == Kind::Mod)
        .map(|entry| entry.path.as_str())
        .collect();
    if paths.is_empty() {
        paths.push(surface.name.as_str());
    }
    paths.sort_unstable();
    paths.dedup();
    paths
}

fn place<'a>(surface: &Surface, modules: &[&'a str], entry: &'a Entry) -> (usize, &'a str) {
    let path = entry.path.as_str();
    let owner = modules
        .iter()
        .enumerate()
        .filter_map(|(index, module)| {
            path.strip_prefix(*module)
                .filter(|rest| rest.is_empty() || rest.starts_with("::"))
                .map(|rest| (index, module.len(), rest))
        })
        .max_by_key(|(_, length, _)| *length);
    match owner {
        Some((index, _, rest)) => {
            let rest = rest.trim_start_matches("::");
            (index, rest.split("::").next().unwrap_or(rest))
        }
        None => {
            let root = modules
                .iter()
                .position(|module| *module == surface.name)
                .unwrap_or(0);
            let subject = if entry.kind == Kind::Impl {
                path
            } else {
                path.rsplit_once("::").map_or(path, |(head, _)| head)
            };
            (root, subject)
        }
    }
}

pub fn within(module: &str, group: &str, path: &str) -> bool {
    let head = format!("{module}::{group}");
    path == head || path.starts_with(&format!("{head}::"))
}

pub fn reexport_only<'m, 'a>(modules: &'m [Module<'a>]) -> impl Iterator<Item = &'m Module<'a>> {
    modules
        .iter()
        .filter(|module| module.counts.reexport_only())
}

impl Group<'_> {
    pub fn kind(&self) -> String {
        self.head
            .map_or_else(|| Kind::Impl.label(), |head| head.kind.label())
    }
}

impl<'a> Module<'a> {
    fn add(&mut self, name: &'a str, entry: &'a Entry) {
        match self.groups.iter_mut().find(|group| group.name == name) {
            Some(group) => group.entries.push(entry),
            None => self.groups.push(Group {
                name,
                head: None,
                entries: vec![entry],
            }),
        }
    }

    fn find_heads(&mut self) {
        for group in &mut self.groups {
            let full = format!("{}::{}", self.path, group.name);
            group.head = group
                .entries
                .iter()
                .copied()
                .find(|entry| entry.kind != Kind::Impl && entry.path == full);
        }
    }

    pub fn layout(&self) -> Vec<(&'static str, Vec<&Group<'a>>)> {
        let mut sections: Vec<(&'static str, Vec<&Group<'a>>)> = SECTIONS
            .iter()
            .map(|(title, _)| (*title, Vec::new()))
            .chain([(OTHER, Vec::new())])
            .collect();
        for group in &self.groups {
            let index = group
                .head
                .and_then(|head| {
                    SECTIONS
                        .iter()
                        .position(|(_, kinds)| kinds.contains(&head.kind))
                })
                .unwrap_or(SECTIONS.len());
            if let Some((_, groups)) = sections.get_mut(index) {
                groups.push(group);
            }
        }
        sections.retain(|(_, groups)| !groups.is_empty());
        sections
    }

    pub fn anchors(&self) -> HashMap<&'a str, String> {
        let mut ids = Anchors::default();
        ids.next(self.path);
        let mut found = HashMap::new();
        for (title, groups) in self.layout() {
            ids.next(title);
            for group in groups {
                found.insert(group.name, ids.next(group.name));
            }
        }
        found
    }

    pub fn counts(&self) -> Counts {
        self.counts
    }
}

impl<'a> Census<'a> {
    fn of(surface: &'a Surface, modules: &[&'a str]) -> Self {
        Self {
            canonical: surface
                .named()
                .into_iter()
                .filter(|named| !matches!(named.kind, Kind::Mod | Kind::ExternCrate))
                .filter_map(|named| Some((named.id, named.paths.first().copied()?)))
                .collect(),
            modules: modules.iter().copied().collect(),
            counted: HashSet::new(),
        }
    }

    fn count(&mut self, groups: &[Group<'a>]) -> Counts {
        let mut counts = Counts::default();
        for entry in groups
            .iter()
            .flat_map(|group| group.entries.iter().copied())
        {
            if entry.trait_impl {
                counts.trait_impls += 1;
            } else if entry.kind == Kind::Impl {
                counts.inherent_impls += 1;
            } else if let Some(&canonical) = self.canonical.get(&entry.id)
                && self.counted.insert((entry.id, entry.path.as_str()))
            {
                if entry.path == canonical {
                    self.classify(&mut counts, entry);
                } else {
                    counts.aliases += 1;
                }
            }
        }
        counts
    }

    fn classify(&self, counts: &mut Counts, entry: &Entry) {
        let free = entry
            .path
            .rsplit_once("::")
            .is_some_and(|(parent, _)| self.modules.contains(parent));
        counts.items += 1;
        let slot = match entry.kind {
            Kind::Fn if free => &mut counts.functions,
            Kind::Fn => &mut counts.methods,
            Kind::Field => &mut counts.fields,
            Kind::Variant => &mut counts.variants,
            Kind::Const => &mut counts.constants,
            Kind::Static => &mut counts.statics,
            Kind::Macro => &mut counts.macros,
            kind if kind.is_type() => &mut counts.types,
            _ => &mut counts.others,
        };
        *slot += 1;
    }
}

impl Counts {
    pub fn total(modules: &[Module<'_>]) -> Self {
        let mut total = Self::default();
        for module in modules {
            total += module.counts;
        }
        total
    }

    pub fn paths(&self) -> usize {
        self.items + self.aliases
    }

    pub fn reexport_only(&self) -> bool {
        self.items == 0 && self.aliases > 0
    }
}

impl AddAssign for Counts {
    fn add_assign(&mut self, other: Self) {
        self.items += other.items;
        self.types += other.types;
        self.functions += other.functions;
        self.methods += other.methods;
        self.fields += other.fields;
        self.variants += other.variants;
        self.constants += other.constants;
        self.statics += other.statics;
        self.macros += other.macros;
        self.others += other.others;
        self.aliases += other.aliases;
        self.trait_impls += other.trait_impls;
        self.inherent_impls += other.inherent_impls;
    }
}
