use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

use cargo_metadata::{Metadata, MetadataCommand, Package, TargetKind};
use serde::Deserialize;

use crate::blocks;
use crate::error::Error;
use crate::intent;
use crate::lint::LintLevels;
use crate::rustdoc;

pub const CONFIG_FILE: &str = "truesight.toml";
pub const BOOK_CONFIG: &str = "book.toml";
pub const SUMMARY: &str = "SUMMARY.md";
pub const CHANGELOG: &str = "CHANGELOG.md";
pub const DEFAULT_TOOLCHAIN: &str = "nightly-2026-09-16";
pub const VERSION_SLOT: &str = "{version}";
const DEFAULT_TAG: &str = "v{version}";

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
struct CrateEntry {
    package: String,
    #[serde(default)]
    features: Vec<String>,
    #[serde(default = "default_features")]
    default_features: bool,
    #[serde(default)]
    gates: Vec<String>,
    spine: Option<PathBuf>,
    intent: Option<PathBuf>,
    tag: Option<String>,
    budget: Option<usize>,
    prelude: Option<String>,
}

fn default_features() -> bool {
    true
}

impl CrateEntry {
    fn named(package: String) -> Self {
        Self {
            package,
            features: Vec::new(),
            default_features: true,
            gates: Vec::new(),
            spine: None,
            intent: None,
            tag: None,
            budget: None,
            prelude: None,
        }
    }
}

pub struct Project {
    pub root: PathBuf,
    pub toolchain: String,
    pub target_dir: PathBuf,
    pub book: Option<PathBuf>,
    pub markdown: Vec<PathBuf>,
    pub lint: LintLevels,
    pub crates: Vec<Crate>,
    pub skipped: Vec<String>,
}

pub struct Example {
    pub name: String,
    pub source: PathBuf,
    pub features: Vec<String>,
}

pub struct Crate {
    pub package: String,
    pub name: String,
    pub version: String,
    pub manifest: PathBuf,
    pub feature_table: BTreeMap<String, Vec<String>>,
    pub features: Vec<String>,
    pub default_features: bool,
    pub gates: Vec<String>,
    pub spine: PathBuf,
    pub intent: PathBuf,
    pub tag: String,
    pub readme: Option<PathBuf>,
    pub examples: Vec<Example>,
    pub budget: Option<usize>,
    pub prelude: Option<String>,
    pub library: PathBuf,
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
        let crates = entries
            .into_iter()
            .map(|entry| resolve(&metadata, &root, entry))
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
            lint: file.lint,
            crates,
            skipped,
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
) -> Result<(Vec<CrateEntry>, Vec<String>), Error> {
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
) -> Result<(Vec<CrateEntry>, Vec<String>), Error> {
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
        .map(|other| other.package)
        .filter(|package| package != name)
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

fn resolve(metadata: &Metadata, root: &Path, entry: CrateEntry) -> Result<Crate, Error> {
    let package = metadata
        .packages
        .iter()
        .find(|package| package.name.as_str() == entry.package)
        .ok_or_else(|| Error::NotInWorkspace(entry.package.clone()))?;
    let library = package
        .targets
        .iter()
        .find(|target| target.is_kind(TargetKind::Lib) || target.is_kind(TargetKind::RLib))
        .ok_or_else(|| Error::NoLibrary(entry.package.clone()))?;
    let tag = entry.tag.unwrap_or_else(|| DEFAULT_TAG.to_owned());
    if !tag.contains(VERSION_SLOT) {
        return Err(Error::TagPattern {
            package: entry.package,
            pattern: tag,
        });
    }
    let spine = located(root, entry.spine, &entry.package, "txt");
    let intent = located(root, entry.intent, &entry.package, "toml");
    let name = library.name.replace('-', "_");
    Ok(Crate {
        prelude: entry.prelude.map(|path| intent::qualify(&name, path)),
        library: library.src_path.clone().into_std_path_buf(),
        name,
        version: package.version.to_string(),
        manifest: package.manifest_path.clone().into_std_path_buf(),
        feature_table: package.features.clone(),
        readme: readme_of(package),
        examples: examples_of(package),
        features: entry.features,
        default_features: entry.default_features,
        gates: entry.gates,
        spine,
        intent,
        tag,
        budget: entry.budget,
        package: entry.package,
    })
}

fn located(root: &Path, configured: Option<PathBuf>, package: &str, extension: &str) -> PathBuf {
    let path =
        configured.unwrap_or_else(|| PathBuf::from("api").join(format!("{package}.{extension}")));
    normal(&root.join(path))
}

pub fn readme_of(package: &Package) -> Option<PathBuf> {
    let dir = package.manifest_path.parent()?.as_std_path();
    let name = package.readme.as_ref().map_or_else(
        || PathBuf::from("README.md"),
        |path| path.clone().into_std_path_buf(),
    );
    let path = dir.join(name);
    path.is_file().then_some(path)
}

fn examples_of(package: &Package) -> Vec<Example> {
    let mut examples: Vec<Example> = package
        .targets
        .iter()
        .filter(|target| target.is_kind(TargetKind::Example))
        .map(|target| Example {
            name: target.name.clone(),
            source: target.src_path.clone().into_std_path_buf(),
            features: target.required_features.clone(),
        })
        .collect();
    examples.sort_by(|left, right| left.name.cmp(&right.name));
    examples
}
