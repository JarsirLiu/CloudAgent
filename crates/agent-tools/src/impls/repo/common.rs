use anyhow::Result;
use ignore::WalkBuilder;
use std::cmp::Reverse;
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

pub(crate) fn rank_file_match(
    relative_path: &str,
    file_name: &str,
    pattern: &str,
) -> Option<usize> {
    let normalized_pattern = normalize_for_match(pattern);
    if normalized_pattern.is_empty() {
        return None;
    }

    let normalized_name = normalize_for_match(file_name);
    let normalized_path = normalize_for_match(relative_path);
    let normalized_stem = Path::new(file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(normalize_for_match)
        .unwrap_or_default();

    if normalized_name == normalized_pattern {
        return Some(1_200 + normalized_pattern.len());
    }
    if normalized_stem == normalized_pattern {
        return Some(1_100 + normalized_pattern.len());
    }
    if wildcard_match(&normalized_pattern, &normalized_name) {
        return Some(1_000 + normalized_pattern.len());
    }
    if wildcard_match(&normalized_pattern, &normalized_path) {
        return Some(950 + normalized_pattern.len());
    }
    if normalized_name.starts_with(&normalized_pattern) {
        return Some(900 + normalized_pattern.len());
    }
    if normalized_stem.starts_with(&normalized_pattern) {
        return Some(860 + normalized_pattern.len());
    }
    if path_segment_matches(&normalized_path, &normalized_pattern) {
        return Some(820 + normalized_pattern.len());
    }
    if normalized_name.contains(&normalized_pattern) {
        return Some(760 + normalized_pattern.len());
    }
    if normalized_path.contains(&normalized_pattern) {
        return Some(700 + normalized_pattern.len());
    }
    if let Some(score) = subsequence_score(&normalized_name, &normalized_pattern) {
        return Some(500 + score);
    }
    if let Some(score) = subsequence_score(&normalized_path, &normalized_pattern) {
        return Some(300 + score);
    }

    None
}

pub(crate) fn sort_ranked_paths(matches: &mut [(usize, String)]) {
    matches.sort_by_key(|(score, path)| (Reverse(*score), path.clone()));
}

fn normalize_for_match(input: &str) -> String {
    input.trim().to_ascii_lowercase()
}

fn path_segment_matches(path: &str, pattern: &str) -> bool {
    path.split('/').any(|segment| segment == pattern)
}

fn subsequence_score(text: &str, pattern: &str) -> Option<usize> {
    if pattern.is_empty() {
        return None;
    }
    let mut score = 0_usize;
    let mut last_match_idx = None;
    let mut chars = text.char_indices();

    for needle in pattern.chars() {
        let Some((idx, _)) = chars.find(|(_, candidate)| *candidate == needle) else {
            return None;
        };
        score += 1;
        if let Some(previous_idx) = last_match_idx {
            if idx == previous_idx + needle.len_utf8() {
                score += 3;
            }
        }
        if idx == 0
            || matches!(
                text[..idx].chars().last(),
                Some('/') | Some('_') | Some('-') | Some('.')
            )
        {
            score += 4;
        }
        last_match_idx = Some(idx);
    }

    Some(score + pattern.len())
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = text.chars().collect();
    let mut dp = vec![vec![false; s.len() + 1]; p.len() + 1];
    dp[0][0] = true;
    for i in 1..=p.len() {
        if p[i - 1] == '*' {
            dp[i][0] = dp[i - 1][0];
        }
    }
    for i in 1..=p.len() {
        for j in 1..=s.len() {
            dp[i][j] = match p[i - 1] {
                '*' => dp[i - 1][j] || dp[i][j - 1],
                '?' => dp[i - 1][j - 1],
                c => dp[i - 1][j - 1] && c == s[j - 1],
            };
        }
    }
    dp[p.len()][s.len()]
}
