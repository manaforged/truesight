use std::path::{Path, PathBuf};

use cargo_metadata::semver::Version;
use cargo_metadata::{Metadata, MetadataCommand, Package, TargetKind};

use crate::Outcome;
use crate::artifacts::{self, write};
use crate::config::{
    BOOK_CONFIG, CONFIG_FILE, DEFAULT_TOOLCHAIN, Project, SUMMARY, VERSION_SLOT, canonical,
    current_dir, normal, read_book, read_optional,
};
use crate::error::Error;
use crate::history;
use crate::markdown::relative;
use crate::package::readme_of;
use crate::pages;

const DOCS: &str = "docs";
const BOOK_DIRS: [&str; 4] = ["", DOCS, "book", "doc"];
const DEFAULT_SOURCE: &str = "src";
const INTRODUCTION: &str = "introduction.md";
const MARKER: &str = "<!-- truesight:";

pub fn run() -> Result<Outcome, Error> {
    let cwd = canonical(&current_dir()?)?;
    let metadata = MetadataCommand::new().current_dir(&cwd).no_deps().exec()?;
    let root = metadata.workspace_root.clone().into_std_path_buf();
    let config = root.join(CONFIG_FILE);
    if config.exists() {
        return Err(Error::ConfigExists(config));
    }
    let libraries = library_packages(&metadata)?;
    let names: Vec<String> = libraries
        .iter()
        .map(|package| package.name.to_string())
        .collect();
    let found = find_book(&root)?;
    let planned = found.clone().unwrap_or_else(|| PathBuf::from(DOCS));
    refuse_written_pages(&root.join(planned), &names)?;
    let per_crate = per_crate_tags(&root, &libraries)?;
    let book = set_up_book(&root, &libraries, &names, found)?;
    let text = config_text(&relative(&root, &root.join(book)), &names, per_crate);
    write(&config, &text)?;
    println!("wrote {}\n\n{text}", config.display());
    let outcome = artifacts::sync(&Project::load(None)?)?;
    println!(
        "\nRun `cargo truesight check` before you push. It exits 1 when a generated file is stale."
    );
    Ok(outcome)
}

fn refuse_written_pages(book: &Path, names: &[String]) -> Result<(), Error> {
    for name in names {
        if let Some(page) = artifacts::pages_under(&pages::dir(book, name))?
            .into_iter()
            .next()
        {
            return Err(Error::PagesExist(page));
        }
    }
    Ok(())
}

fn set_up_book(
    root: &Path,
    libraries: &[&Package],
    names: &[String],
    found: Option<PathBuf>,
) -> Result<PathBuf, Error> {
    let several = names.len() > 1;
    let readmes: Vec<(String, PathBuf)> = libraries
        .iter()
        .filter_map(|package| readme_of(package).map(|path| (package.name.to_string(), path)))
        .collect();
    let book = match found {
        Some(book) => book,
        None => scaffold(root, names, &readmes)?,
    };
    add_pages(&root.join(&book).join(SUMMARY), names, several)?;
    for (name, readme) in &readmes {
        add_readme_block(readme, name, several)?;
    }
    Ok(book)
}

fn library_packages(metadata: &Metadata) -> Result<Vec<&Package>, Error> {
    let packages = metadata.workspace_packages();
    let libraries: Vec<&Package> = packages
        .iter()
        .copied()
        .filter(|package| {
            package
                .targets
                .iter()
                .any(|target| target.is_kind(TargetKind::Lib) || target.is_kind(TargetKind::RLib))
        })
        .collect();
    if libraries.is_empty() {
        return Err(packages.first().map_or(Error::NoPackage, |package| {
            Error::NoLibrary(package.name.to_string())
        }));
    }
    Ok(libraries)
}

fn per_crate_tags(root: &Path, libraries: &[&Package]) -> Result<bool, Error> {
    if libraries.len() < 2 {
        return Ok(false);
    }
    let tags = history::merged_tags(root)?;
    let per_package = |tag: &String| {
        libraries
            .iter()
            .any(|package| tag.starts_with(&format!("{}-v", package.name)))
    };
    if tags.iter().any(per_package) {
        return Ok(true);
    }
    let shared = |tag: &String| {
        tag.strip_prefix('v')
            .is_some_and(|version| Version::parse(version).is_ok())
    };
    if tags.iter().any(shared) {
        return Ok(false);
    }
    let first = &libraries[0].version;
    Ok(libraries.iter().any(|package| &package.version != first))
}

fn config_text(book: &str, names: &[String], per_crate: bool) -> String {
    let mut text = format!(
        "toolchain = {}\nbook = {}\n",
        toml_string(DEFAULT_TOOLCHAIN),
        toml_string(book)
    );
    for name in names {
        text.push_str(&format!("\n[[crate]]\npackage = {}\n", toml_string(name)));
        if per_crate {
            let tag = format!("{name}-v{VERSION_SLOT}");
            text.push_str(&format!("tag = {}\n", toml_string(&tag)));
        }
    }
    text
}

fn toml_string(text: &str) -> String {
    toml::Value::from(text).to_string()
}

fn find_book(root: &Path) -> Result<Option<PathBuf>, Error> {
    for dir in BOOK_DIRS {
        let Some(section) = read_book(&root.join(dir))? else {
            continue;
        };
        let source = section.src.unwrap_or_else(|| PathBuf::from(DEFAULT_SOURCE));
        return Ok(Some(normal(&Path::new(dir).join(source))));
    }
    Ok(None)
}

fn scaffold(
    root: &Path,
    names: &[String],
    readmes: &[(String, PathBuf)],
) -> Result<PathBuf, Error> {
    let docs = root.join(DOCS);
    let title = match names {
        [name] => name.clone(),
        _ => root.file_name().map_or_else(
            || String::from("API reference"),
            |name| name.to_string_lossy().into_owned(),
        ),
    };
    write(
        &docs.join(BOOK_CONFIG),
        &format!(
            "[book]\ntitle = {}\nsrc = \".\"\nlanguage = \"en\"\n\n[build]\nbuild-dir = \"../target/book\"\ncreate-missing = false\n",
            toml_string(&title)
        ),
    )?;
    let readme = Some(root.join("README.md"))
        .filter(|path| path.is_file())
        .or_else(|| readmes.first().map(|(_, path)| path.clone()));
    let introduction = match readme {
        Some(path) => format!("{{{{#include {}}}}}\n", relative(&docs, &path)),
        None => format!("# {title}\n"),
    };
    write_new(&docs.join(INTRODUCTION), &introduction)?;
    write_new(
        &docs.join(SUMMARY),
        &format!("# Summary\n\n[Introduction]({INTRODUCTION})\n"),
    )?;
    Ok(PathBuf::from(DOCS))
}

fn add_pages(summary: &Path, names: &[String], several: bool) -> Result<(), Error> {
    let mut text = read_optional(summary)?.unwrap_or_else(|| String::from("# Summary\n"));
    if text.contains("<!-- truesight:pages") {
        return Ok(());
    }
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("\n# API reference\n");
    for name in names {
        let package = if several {
            format!(" {name}")
        } else {
            String::new()
        };
        text.push_str(&format!(
            "\n<!-- truesight:pages{package} -->\n<!-- /truesight -->\n"
        ));
    }
    write(summary, &text)
}

fn add_readme_block(readme: &Path, name: &str, several: bool) -> Result<(), Error> {
    let Some(text) = read_optional(readme)? else {
        return Ok(());
    };
    if text.contains(MARKER) {
        return Ok(());
    }
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let package = if several {
        format!(" {name}")
    } else {
        String::new()
    };
    let block = format!("## API\n\n<!-- truesight:surface{package} -->\n<!-- /truesight -->\n\n")
        .replace('\n', newline);
    let at = text
        .find(&format!("{newline}## License"))
        .map_or(text.len(), |index| index + newline.len());
    let (head, tail) = text.split_at(at);
    let separator = if head.is_empty() || head.ends_with(&format!("{newline}{newline}")) {
        String::new()
    } else if head.ends_with(newline) {
        newline.to_owned()
    } else {
        format!("{newline}{newline}")
    };
    write(readme, &format!("{head}{separator}{block}{tail}"))
}

fn write_new(path: &Path, content: &str) -> Result<(), Error> {
    if path.exists() {
        return Ok(());
    }
    write(path, content)
}
