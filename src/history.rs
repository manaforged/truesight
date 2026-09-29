use std::path::Path;
use std::process::Command;

use cargo_metadata::semver::Version;

use crate::config::{Crate, Project, VERSION_SLOT};
use crate::diff::{Change, Paths};
use crate::error::Error;
use crate::markdown::relative;

const UNRELEASED: &str = "Unreleased";

pub struct Section {
    pub title: String,
    pub body: Body,
}

pub enum Body {
    First(usize),
    Changes(Paths),
}

pub struct History {
    pub sections: Vec<Section>,
    pub tracked: bool,
    pub shallow: bool,
}

pub fn load(project: &Project, krate: &Crate, current: &str) -> Result<History, Error> {
    let tracked = succeeds(&project.root, &["rev-parse", "--is-inside-work-tree"]);
    let shallow =
        tracked && git(&project.root, &["rev-parse", "--is-shallow-repository"])?.trim() == "true";
    let releases = if tracked {
        releases(project, krate)?
    } else {
        Vec::new()
    };
    let mut points: Vec<(String, String)> = Vec::new();
    let mut released = false;
    for (version, tag) in releases {
        let Some(spine) = spine_at(project, krate, &tag)? else {
            continue;
        };
        let version = version.to_string();
        released |= version == krate.version;
        points.push((version, spine));
    }
    let changed_since_release = points.last().is_none_or(|(_, spine)| spine != current);
    if !released {
        points.push((krate.version.clone(), current.to_owned()));
    } else if changed_since_release {
        points.push((String::from(UNRELEASED), current.to_owned()));
    }
    let mut sections: Vec<Section> = points
        .iter()
        .enumerate()
        .map(|(index, (title, spine))| {
            let before = index
                .checked_sub(1)
                .and_then(|previous| points.get(previous));
            let body = match before {
                None => Body::First(spine.lines().count()),
                Some((_, before)) => Body::Changes(Paths::of(&Change::between(before, spine))),
            };
            Section {
                title: title.clone(),
                body,
            }
        })
        .collect();
    sections.reverse();
    Ok(History {
        sections,
        tracked,
        shallow,
    })
}

pub fn spine_at(
    project: &Project,
    krate: &Crate,
    reference: &str,
) -> Result<Option<String>, Error> {
    let path = format!("./{}", relative(&project.root, &krate.spine));
    let listed = git(
        &project.root,
        &["ls-tree", "--name-only", reference, "--", &path],
    )?;
    if listed.trim().is_empty() {
        return Ok(None);
    }
    git(&project.root, &["show", &format!("{reference}:{path}")]).map(Some)
}

fn releases(project: &Project, krate: &Crate) -> Result<Vec<(Version, String)>, Error> {
    let tags = merged_tags(&project.root)?;
    let (prefix, suffix) = krate
        .tag
        .split_once(VERSION_SLOT)
        .unwrap_or((krate.tag.as_str(), ""));
    let mut found: Vec<(Version, String)> = tags
        .iter()
        .filter_map(|tag| {
            let version = tag.strip_prefix(prefix)?.strip_suffix(suffix)?;
            Version::parse(version)
                .ok()
                .map(|version| (version, tag.to_owned()))
        })
        .collect();
    found.sort();
    Ok(found)
}

pub fn merged_tags(root: &Path) -> Result<Vec<String>, Error> {
    if !succeeds(root, &["rev-parse", "--verify", "--quiet", "HEAD"]) {
        return Ok(Vec::new());
    }
    let tags = git(root, &["tag", "--merged", "HEAD"])?;
    Ok(tags.lines().map(str::to_owned).collect())
}

fn command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .arg("-C")
        .arg(root)
        .args(args);
    command
}

fn succeeds(root: &Path, args: &[&str]) -> bool {
    command(root, args)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn git(root: &Path, args: &[&str]) -> Result<String, Error> {
    let output = command(root, args).output().map_err(|source| Error::Io {
        path: root.to_owned(),
        source,
    })?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(Error::Git {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}
