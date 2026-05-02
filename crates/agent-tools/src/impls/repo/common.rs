use anyhow::Result;
use ignore::WalkBuilder;
use std::path::Path;

pub(crate) const DEFAULT_IGNORED_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "dist",
    "build",
    "target",
    "target-verify",
    ".next",
    ".nuxt",
    ".turbo",
    ".cache",
    "coverage",
    ".venv",
    "venv",
    "__pycache__",
];

#[derive(Clone, Debug)]
pub(crate) struct RepoEntry {
    pub(crate) relative_path: String,
    pub(crate) file_name: String,
}

pub(crate) fn collect_repo_entries(
    workspace_root: &Path,
    search_root: &Path,
) -> Result<Vec<RepoEntry>> {
    let workspace_root = workspace_root
        .canonicalize()
        .unwrap_or_else(|_| workspace_root.to_path_buf());
    let search_root = search_root
        .canonicalize()
        .unwrap_or_else(|_| search_root.to_path_buf());

    let mut builder = WalkBuilder::new(&search_root);
    builder
        .hidden(false)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .parents(true)
        .follow_links(false);
    builder.filter_entry(|entry| {
        let name = entry.file_name().to_string_lossy();
        !DEFAULT_IGNORED_DIRS.contains(&name.as_ref())
    });

    let walker = builder.build();
    let mut entries = Vec::new();

    for result in walker {
        let entry = match result {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let path = entry.path();
        if path == search_root || !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        let Ok(relative) = path.strip_prefix(&workspace_root) else {
            continue;
        };
        let relative_path = relative.to_string_lossy().replace('\\', "/");
        let file_name = path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| relative_path.clone());
        entries.push(RepoEntry {
            relative_path,
            file_name,
        });
    }

    Ok(entries)
}

pub(crate) fn sort_ranked_paths(matches: &mut [(usize, String)]) {
    matches.sort_by(|(score_a, path_a), (score_b, path_b)| {
        score_b.cmp(score_a).then_with(|| path_a.cmp(path_b))
    });
}
