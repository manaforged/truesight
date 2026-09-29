use std::collections::HashSet;

use public_api::rustdoc_types::{Crate as Rustdoc, Id, Item, ItemEnum, Visibility};

use crate::surface::{Kind, Location};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Use,
    Glob,
    Module,
}

pub struct Site {
    pub via: Via,
    pub location: Location,
}

pub struct Clash {
    pub module: String,
    pub name: String,
    pub location: Option<Location>,
}

pub struct Tree<'a> {
    rustdoc: &'a Rustdoc,
}

enum Step<'a> {
    Direct(&'a Item),
    Use(&'a Item),
    Glob(&'a Item),
}

fn location(item: &Item) -> Option<Location> {
    item.span.as_ref().map(|span| Location {
        file: span.filename.clone(),
        line: span.begin.0,
    })
}

fn site(item: &Item, via: Via) -> Option<Site> {
    location(item).map(|location| Site { via, location })
}

fn visible(item: &Item) -> bool {
    matches!(item.visibility, Visibility::Public) || matches!(item.inner, ItemEnum::Variant(_))
}

fn named_step<'a>(child: &'a Item, name: &str) -> Option<Step<'a>> {
    match &child.inner {
        ItemEnum::Use(import) if import.is_glob => None,
        ItemEnum::Use(import) => (import.name == name).then_some(Step::Use(child)),
        _ => (child.name.as_deref() == Some(name)).then_some(Step::Direct(child)),
    }
}

impl<'a> Tree<'a> {
    pub fn new(rustdoc: &'a Rustdoc) -> Self {
        Self { rustdoc }
    }

    fn get(&self, id: &Id) -> Option<&'a Item> {
        self.rustdoc.index.get(id)
    }

    fn target(&self, item: &'a Item) -> Option<&'a Item> {
        match &item.inner {
            ItemEnum::Use(import) => import.id.as_ref().and_then(|id| self.get(id)),
            _ => None,
        }
    }

    fn children(&self, item: &'a Item) -> Vec<&'a Item> {
        let ids: &[Id] = match &item.inner {
            ItemEnum::Module(module) => &module.items,
            ItemEnum::Enum(inner) => &inner.variants,
            _ => &[],
        };
        ids.iter()
            .filter_map(|id| self.get(id))
            .filter(|child| visible(child))
            .collect()
    }

    fn kind_of(&self, step: &Step<'a>) -> Option<Kind> {
        match *step {
            Step::Direct(item) => Some(Kind::of(&item.inner)),
            Step::Use(item) | Step::Glob(item) => {
                self.target(item).map(|target| Kind::of(&target.inner))
            }
        }
    }

    fn step(
        &self,
        parent: &'a Item,
        name: &str,
        want: Kind,
        seen: &mut HashSet<Id>,
    ) -> Option<Step<'a>> {
        let children = self.children(parent);
        let mut direct: Vec<Step<'a>> = children
            .iter()
            .filter_map(|&child| named_step(child, name))
            .collect();
        if !direct.is_empty() {
            let chosen = direct
                .iter()
                .position(|step| self.kind_of(step) == Some(want))
                .unwrap_or(0);
            return Some(direct.swap_remove(chosen));
        }
        children.into_iter().find_map(|child| {
            let ItemEnum::Use(import) = &child.inner else {
                return None;
            };
            let target = self.target(child).filter(|_| import.is_glob)?;
            if !seen.insert(target.id) {
                return None;
            }
            self.step(target, name, want, seen)
                .map(|_| Step::Glob(child))
        })
    }

    pub fn origin(&self, path: &str, own: &str, kind: Kind) -> Option<Site> {
        let names: Vec<&str> = path.split("::").skip(1).collect();
        let mut parent = self.get(&self.rustdoc.root)?;
        for (index, name) in names.iter().enumerate() {
            let want = if index + 1 == names.len() {
                kind
            } else {
                Kind::Mod
            };
            match self.step(parent, name, want, &mut HashSet::new())? {
                Step::Use(item) => return site(item, Via::Use),
                Step::Glob(item) => return site(item, Via::Glob),
                Step::Direct(item) if matches!(item.inner, ItemEnum::Module(_)) => parent = item,
                Step::Direct(_) => break,
            }
        }
        self.declared(path, own)
    }

    fn declared(&self, path: &str, own: &str) -> Option<Site> {
        let shared = path
            .split("::")
            .zip(own.split("::"))
            .take_while(|(left, right)| left == right)
            .count();
        let segments: Vec<&str> = path.split("::").collect();
        let names = segments.get(1..=shared).filter(|names| !names.is_empty())?;
        let mut item = self.get(&self.rustdoc.root)?;
        for name in names {
            match self.step(item, name, Kind::Mod, &mut HashSet::new())? {
                Step::Direct(next) => item = next,
                Step::Use(_) | Step::Glob(_) => return None,
            }
        }
        if matches!(item.inner, ItemEnum::Module(_)) {
            site(item, Via::Module)
        } else {
            None
        }
    }

    pub fn clashes(&self) -> Vec<Clash> {
        let mut found = Vec::new();
        for module in self.rustdoc.index.values() {
            let ItemEnum::Module(inner) = &module.inner else {
                continue;
            };
            if module.crate_id != 0 {
                continue;
            }
            let children: Vec<&Item> = inner.items.iter().filter_map(|id| self.get(id)).collect();
            let hidden: HashSet<&str> = children
                .iter()
                .filter_map(|&child| self.hidden_module(child))
                .collect();
            for child in children {
                if let ItemEnum::Use(import) = &child.inner
                    && !import.is_glob
                    && matches!(child.visibility, Visibility::Public)
                    && hidden.contains(import.name.as_str())
                    && self
                        .target(child)
                        .is_some_and(|target| matches!(target.inner, ItemEnum::Function(_)))
                {
                    found.push(Clash {
                        module: self.path_of(module),
                        name: import.name.clone(),
                        location: location(child),
                    });
                }
            }
        }
        found
    }

    fn hidden_module(&self, child: &'a Item) -> Option<&'a str> {
        let (module, name) = match &child.inner {
            ItemEnum::Module(_) => (child, child.name.as_deref()?),
            ItemEnum::Use(import) if !import.is_glob => (self.target(child)?, import.name.as_str()),
            _ => return None,
        };
        let hidden = matches!(module.inner, ItemEnum::Module(_))
            && !matches!(module.visibility, Visibility::Public);
        hidden.then_some(name)
    }

    fn path_of(&self, module: &Item) -> String {
        self.rustdoc.paths.get(&module.id).map_or_else(
            || module.name.clone().unwrap_or_default(),
            |summary| summary.path.join("::"),
        )
    }
}
