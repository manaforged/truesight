use serde::Deserialize;

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum Level {
    Allow,
    Warn,
    Deny,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", default)]
pub struct LintLevels {
    pub duplicate_path: Level,
    pub glob_reexport: Level,
    pub reexport_only_module: Level,
    pub no_task: Level,
    pub undocumented: Level,
    pub stale_doc: Level,
    pub unknown_doc_path: Level,
}

impl Default for LintLevels {
    fn default() -> Self {
        Self {
            duplicate_path: Level::Warn,
            glob_reexport: Level::Warn,
            reexport_only_module: Level::Warn,
            no_task: Level::Allow,
            undocumented: Level::Allow,
            stale_doc: Level::Allow,
            unknown_doc_path: Level::Allow,
        }
    }
}

#[derive(Deserialize, Clone, Copy, Default)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct LintOverrides {
    duplicate_path: Option<Level>,
    glob_reexport: Option<Level>,
    reexport_only_module: Option<Level>,
    no_task: Option<Level>,
    undocumented: Option<Level>,
    stale_doc: Option<Level>,
    unknown_doc_path: Option<Level>,
}

impl LintLevels {
    pub fn with(self, over: LintOverrides) -> Self {
        Self {
            duplicate_path: over.duplicate_path.unwrap_or(self.duplicate_path),
            glob_reexport: over.glob_reexport.unwrap_or(self.glob_reexport),
            reexport_only_module: over
                .reexport_only_module
                .unwrap_or(self.reexport_only_module),
            no_task: over.no_task.unwrap_or(self.no_task),
            undocumented: over.undocumented.unwrap_or(self.undocumented),
            stale_doc: over.stale_doc.unwrap_or(self.stale_doc),
            unknown_doc_path: over.unknown_doc_path.unwrap_or(self.unknown_doc_path),
        }
    }
}
