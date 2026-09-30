use std::path::{Component, Path, PathBuf};

use cargo_metadata::{Metadata, MetadataCommand};
use serde::Deserialize;

use crate::blocks;
use crate::error::Error;
use crate::levels::LintLevels;
use crate::package::{Crate, CrateEntry, resolve, spine_of};
use crate::rustdoc;

pub const CONFIG_FILE: &str = "truesight.toml";
pub const BOOK_CONFIG: &str = "book.toml";
pub const SUMMARY: &str = "SUMMARY.md";
pub const CHANGELOG: &str = "CHANGELOG.md";
pub const DEFAULT_TOOLCHAIN: &str = "nightly-2026-09-16";
pub const VERSION_SLOT: &str = "{version}";

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct ConfigFile {
    toolchain: Option<String>,
    book: Option<PathBuf>,
    #[serde(default)]
    markdown: Vec<PathBuf>,
    #[serde(default)]
    lint: LintLevels,
    #[serde(default, rename = "crate")]
    crates: Vec<CrateEntry>,
}

pub struct Project {
    pub root: PathBuf,
    pub toolchain: String,
    pub target_dir: PathBuf,
    pub book: Option<PathBuf>,
    pub markdown: Vec<PathBuf>,
    pub crates: Vec<Crate>,
    pub skipped: Vec<String>,
    pub spines: Vec<(String, PathBuf)>,
}

impl Project {
    pub fn load(only: Option<&str>) -> Result<Self, Error> {
        let cwd = canonical(&current_dir()?)?;
        let found = cwd
            .ancestors()
            .map(|dir| dir.join(CONFIG_FILE))
            .find(|path| path.is_file());
        let file = match &found {
            Some(path) => read_config(path)?,
            None => ConfigFile::default(),
        };
        let metadata = MetadataCommand::new().current_dir(&cwd).no_deps().exec()?;
        let root = root_of(found.as_deref(), &metadata)?;
        let (entries, skipped) = entries(file.crates, only, &metadata, &cwd)?;
        let spines = entries
            .iter()
            .chain(&skipped)
            .filter_map(|entry| spine_of(&metadata, &root, entry))
            .collect();
        let skipped = skipped.into_iter().map(|entry| entry.package).collect();
        let crates = entries
            .into_iter()
            .map(|entry| resolve(&metadata, &root, entry, file.lint))
            .collect::<Result<Vec<_>, _>>()?;
        let book = file
            .book
            .as_deref()
            .map(|dir| book_dir(&root, dir))
            .transpose()?;
        let markdown = blocks::files(&root, &file.markdown, book.as_deref(), &crates)?;
        let toolchain = file
            .toolchain
            .unwrap_or_else(|| DEFAULT_TOOLCHAIN.to_owned());
        rustdoc::ensure_toolchain(&toolchain)?;
        Ok(Self {
            toolchain,
            target_dir: metadata
                .target_directory
                .join("truesight")
                .into_std_path_buf(),
            book,
            markdown,
            crates,
            skipped,
            spines,
            root,
        })
    }

    pub fn show(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .display()
            .to_string()
    }
}

pub fn current_dir() -> Result<PathBuf, Error> {
    std::env::current_dir().map_err(|source| Error::Io {
        path: PathBuf::from("."),
        source,
    })
}

pub fn canonical(path: &Path) -> Result<PathBuf, Error> {
    std::fs::canonicalize(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })
}

pub fn normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub fn read_optional(path: &Path) -> Result<Option<String>, Error> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Io {
            path: path.to_owned(),
            source,
        }),
    }
}

fn read_config(path: &Path) -> Result<ConfigFile, Error> {
    let text = std::fs::read_to_string(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| Error::Toml {
        path: path.to_owned(),
        source: Box::new(source),
    })
}

#[derive(Deserialize, Default)]
struct BookFile {
    #[serde(default)]
    book: BookSection,
}

#[derive(Deserialize, Default)]
pub struct BookSection {
    pub title: Option<String>,
    pub src: Option<PathBuf>,
}

pub fn read_book(dir: &Path) -> Result<Option<BookSection>, Error> {
    let path = dir.join(BOOK_CONFIG);
    let Some(text) = read_optional(&path)? else {
        return Ok(None);
    };
    let file: BookFile = toml::from_str(&text).map_err(|source| Error::Toml {
        path,
        source: Box::new(source),
    })?;
    Ok(Some(file.book))
}

fn root_of(found: Option<&Path>, metadata: &Metadata) -> Result<PathBuf, Error> {
    match found.and_then(Path::parent) {
        Some(dir) => canonical(dir),
        None => canonical(metadata.workspace_root.as_std_path()),
    }
}

fn entries(
    configured: Vec<CrateEntry>,
    only: Option<&str>,
    metadata: &Metadata,
    cwd: &Path,
) -> Result<(Vec<CrateEntry>, Vec<CrateEntry>), Error> {
    if !configured.is_empty() {
        return select(configured, only);
    }
    let name = match only {
        Some(name) => name.to_owned(),
        None => nearest_package(metadata, cwd)?,
    };
    Ok((vec![CrateEntry::named(name)], Vec::new()))
}

fn select(
    mut entries: Vec<CrateEntry>,
    only: Option<&str>,
) -> Result<(Vec<CrateEntry>, Vec<CrateEntry>), Error> {
    let Some(name) = only else {
        return Ok((entries, Vec::new()));
    };
    let position = entries
        .iter()
        .position(|entry| entry.package == name)
        .ok_or_else(|| Error::UnknownPackage {
            name: name.to_owned(),
            known: entries
                .iter()
                .map(|entry| format!("`{}`", entry.package))
                .collect::<Vec<_>>()
                .join(", "),
        })?;
    let entry = entries.remove(position);
    let skipped = entries
        .into_iter()
        .filter(|other| other.package != name)
        .collect();
    Ok((vec![entry], skipped))
}

fn book_dir(root: &Path, configured: &Path) -> Result<PathBuf, Error> {
    let dir = normal(&root.join(configured));
    if dir.join(SUMMARY).is_file() {
        Ok(dir)
    } else {
        Err(Error::NoSummary(configured.to_owned()))
    }
}

fn nearest_package(metadata: &Metadata, cwd: &Path) -> Result<String, Error> {
    metadata
        .packages
        .iter()
        .filter_map(|package| {
            package
                .manifest_path
                .parent()
                .map(|dir| (dir.as_std_path(), package))
        })
        .filter(|(dir, _)| cwd.starts_with(dir))
        .max_by_key(|(dir, _)| dir.components().count())
        .map(|(_, package)| package.name.to_string())
        .ok_or(Error::NoPackage)
}
