use super::common::{collect_repo_entries, sort_ranked_paths};
use crate::impls::text_codec::decode_text_file;
use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_read_path,
};
use crate::spec::{ToolCategory, ToolDescriptor, ToolPermissionTier, ToolRisk, ToolUsageGuidance};
use agent_core::{ToolExecutionContext, ToolExecutionPolicy, ToolIdentity, ToolSpec};
use agent_protocol::SearchWorkspaceHit;
use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use nucleo::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher, Utf32Str};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex;

pub struct SearchWorkspaceTool;

impl SearchWorkspaceTool {
    pub fn descriptor() -> ToolDescriptor {
        ToolDescriptor::new_with_guidance(
            ToolCategory::RepositoryExploration,
            ToolRisk::Low,
            ToolPermissionTier::ReadOnly,
            vec!["explore", "edit", "verify", "repo"],
            ToolUsageGuidance {
                selection_priority: 30,
                preferred_for: vec![
                    "first step of bug investigation",
                    "finding likely files before reading code",
                ],
                follow_up_hint: Some("open the strongest hit with `read_file`; if several hits look plausible, issue multiple `read_file` calls in parallel before editing"),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "search_workspace".to_string(),
                identity: ToolIdentity::built_in("search_workspace"),
                description: "Search repository files or text through one structured entry point. Use mode=`files` for likely path matches and mode=`text` for symbol or string matches. Reuse `session_id` to refine an existing search instead of restarting from scratch. The goal is to make the next `read_file` call obvious.".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "operation": {
                            "type": "string",
                            "enum": ["search", "close"]
                        },
                        "session_id": { "type": "string" },
                        "mode": {
                            "type": "string",
                            "enum": ["files", "text"]
                        },
                        "query": { "type": "string" },
                        "pattern": { "type": "string" },
                        "path_scope": { "type": "string" },
                        "case_sensitive": { "type": "boolean" },
                        "context_lines": { "type": "integer", "minimum": 0, "maximum": 8 },
                        "max_results": { "type": "integer", "minimum": 1 },
                        "offset": { "type": "integer", "minimum": 0 }
                    }
                }),
                mutating: false,
                execution_policy: ToolExecutionPolicy::ParallelSafe,
                requires_approval: false,
                item_kind: agent_protocol::TurnItemKind::ToolCall,
                delta_kind: agent_protocol::TurnItemDeltaKind::ToolOutput,
                approval_reason: None,
            },
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SearchMode {
    Files,
    Text,
}

impl SearchMode {
    fn parse(value: Option<&str>) -> Option<Self> {
        match value {
            Some("files") => Some(Self::Files),
            Some("text") => Some(Self::Text),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
struct SearchSession {
    mode: SearchMode,
    query: String,
    path_scope: Option<String>,
    case_sensitive: bool,
    context_lines: usize,
}

#[derive(Default)]
struct SearchWorkspaceSessionStore {
    next_id: AtomicU64,
    sessions: Mutex<HashMap<String, SearchSession>>,
}

impl SearchWorkspaceSessionStore {
    fn new() -> Self {
        Self::default()
    }

    fn allocate_id(&self, conversation_id: &str) -> String {
        let next = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("search:{}:{next}", conversation_id)
    }

    async fn insert(&self, session_id: String, session: SearchSession) {
        self.sessions.lock().await.insert(session_id, session);
    }

    async fn get(&self, session_id: &str) -> Option<SearchSession> {
        self.sessions.lock().await.get(session_id).cloned()
    }

    async fn remove(&self, session_id: &str) -> Option<SearchSession> {
        self.sessions.lock().await.remove(session_id)
    }
}

#[derive(Debug, Deserialize)]
struct SearchWorkspaceArgs {
    #[serde(default)]
    operation: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default)]
    path_scope: Option<String>,
    #[serde(default)]
    case_sensitive: Option<bool>,
    #[serde(default)]
    context_lines: Option<usize>,
    #[serde(default)]
    max_results: Option<usize>,
    #[serde(default)]
    offset: Option<usize>,
}

pub(crate) struct SearchWorkspaceLocalTool {
    sessions: Arc<SearchWorkspaceSessionStore>,
}

impl SearchWorkspaceLocalTool {
    pub(crate) fn new() -> Self {
        Self {
            sessions: Arc::new(SearchWorkspaceSessionStore::new()),
        }
    }
}

#[async_trait]
impl LocalTool for SearchWorkspaceLocalTool {
    fn spec(&self) -> ToolSpec {
        SearchWorkspaceTool::descriptor().spec
    }

    async fn invoke(
        &self,
        invocation: LocalToolInvocation,
        ctx: &ToolExecutionContext,
    ) -> Result<ToolInvocationOutput> {
        let args: SearchWorkspaceArgs = invocation.payload.parse_arguments()?;
        if matches!(args.operation.as_deref(), Some("close")) {
            let session_id = args
                .session_id
                .as_deref()
                .ok_or_else(|| anyhow!("`session_id` is required when operation=`close`"))?;
            let removed = self.sessions.remove(session_id).await;
            let closed_mode = removed
                .as_ref()
                .map(|session| match session.mode {
                    SearchMode::Files => agent_protocol::SearchWorkspaceMode::Files,
                    SearchMode::Text => agent_protocol::SearchWorkspaceMode::Text,
                })
                .unwrap_or(agent_protocol::SearchWorkspaceMode::Text);
            let message = if removed.is_some() {
                format!("Closed search session `{session_id}`.")
            } else {
                format!("Search session `{session_id}` was not found.")
            };
            return Ok(ToolInvocationOutput {
                content: message,
                structured: Some(agent_protocol::StructuredToolResult::SearchWorkspace {
                    session_id: session_id.to_string(),
                    operation: agent_protocol::SearchWorkspaceOperation::Close,
                    mode: closed_mode,
                    status: if removed.is_some() {
                        agent_protocol::SearchWorkspaceStatus::Closed
                    } else {
                        agent_protocol::SearchWorkspaceStatus::NotFound
                    },
                    query: String::new(),
                    path_scope: None,
                    case_sensitive: false,
                    context_lines: 0,
                    max_results: 0,
                    offset: 0,
                    file_count: 0,
                    match_count: 0,
                    truncated: false,
                    next_offset: None,
                    hits: Vec::new(),
                }),
            });
        }

        let mut resolved = self.resolve_request(args, &ctx.conversation_id).await?;
        let session_id = resolved.session_id.clone();
        let content = match resolved.mode {
            SearchMode::Files => {
                let search = run_file_search(
                    resolved.query.as_str(),
                    resolved.path_scope.as_deref(),
                    resolved.case_sensitive,
                    resolved.max_results,
                    resolved.offset,
                    ctx,
                )
                .await?;
                resolved.result_count = search.total_matches;
                resolved.match_count = search.total_matches;
                resolved.truncated =
                    search.total_matches > resolved.offset.saturating_add(resolved.max_results);
                resolved.next_offset = (search.total_matches
                    > resolved.offset.saturating_add(resolved.max_results))
                .then_some(resolved.offset.saturating_add(search.hits.len()));
                resolved.hits = search.hits.clone();
                build_file_search_output(&session_id, &resolved, &search)
            }
            SearchMode::Text => {
                let search = run_text_search(
                    resolved.query.as_str(),
                    resolved.path_scope.as_deref(),
                    resolved.case_sensitive,
                    resolved.context_lines,
                    resolved.max_results,
                    resolved.offset,
                    ctx,
                )
                .await?;
                resolved.result_count = search.file_count;
                resolved.match_count = search.match_count;
                resolved.truncated = search.truncated;
                resolved.next_offset = search.next_offset;
                resolved.hits = search.hits.clone();
                build_text_search_output(&session_id, &resolved, &search)
            }
        };

        self.sessions
            .insert(
                session_id.clone(),
                SearchSession {
                    mode: resolved.mode,
                    query: resolved.query.clone(),
                    path_scope: resolved.path_scope.clone(),
                    case_sensitive: resolved.case_sensitive,
                    context_lines: resolved.context_lines,
                },
            )
            .await;

        Ok(ToolInvocationOutput {
            content,
            structured: Some(agent_protocol::StructuredToolResult::SearchWorkspace {
                session_id,
                operation: agent_protocol::SearchWorkspaceOperation::Search,
                mode: match resolved.mode {
                    SearchMode::Files => agent_protocol::SearchWorkspaceMode::Files,
                    SearchMode::Text => agent_protocol::SearchWorkspaceMode::Text,
                },
                status: agent_protocol::SearchWorkspaceStatus::Active,
                query: resolved.query,
                path_scope: resolved.path_scope,
                case_sensitive: resolved.case_sensitive,
                context_lines: resolved.context_lines,
                max_results: resolved.max_results,
                offset: resolved.offset,
                file_count: resolved.result_count,
                match_count: resolved.match_count,
                truncated: resolved.truncated,
                next_offset: resolved.next_offset,
                hits: resolved.hits,
            }),
        })
    }
}

struct ResolvedSearchRequest {
    session_id: String,
    mode: SearchMode,
    query: String,
    path_scope: Option<String>,
    case_sensitive: bool,
    context_lines: usize,
    max_results: usize,
    offset: usize,
    result_count: usize,
    match_count: usize,
    truncated: bool,
    next_offset: Option<usize>,
    hits: Vec<SearchWorkspaceHit>,
}

impl SearchWorkspaceLocalTool {
    async fn resolve_request(
        &self,
        args: SearchWorkspaceArgs,
        conversation_id: &str,
    ) -> Result<ResolvedSearchRequest> {
        let existing = if let Some(session_id) = args.session_id.as_deref() {
            self.sessions
                .get(session_id)
                .await
                .ok_or_else(|| anyhow!("search session `{session_id}` was not found"))?
        } else {
            SearchSession {
                mode: SearchMode::parse(args.mode.as_deref())
                    .ok_or_else(|| anyhow!("`mode` must be `files` or `text`"))?,
                query: String::new(),
                path_scope: None,
                case_sensitive: false,
                context_lines: 0,
            }
        };

        let mode = SearchMode::parse(args.mode.as_deref()).unwrap_or(existing.mode);
        let query = resolve_query(
            mode,
            args.query.as_deref(),
            args.pattern.as_deref(),
            &existing,
        )?;
        let path_scope = args.path_scope.or(existing.path_scope.clone());
        let case_sensitive = args.case_sensitive.unwrap_or(existing.case_sensitive);
        let context_lines = args.context_lines.unwrap_or(existing.context_lines).min(8);
        let max_results = args.max_results.unwrap_or(100).clamp(1, 500);
        let offset = args.offset.unwrap_or(0);
        let session_id = args
            .session_id
            .unwrap_or_else(|| self.sessions.allocate_id(conversation_id));

        Ok(ResolvedSearchRequest {
            session_id,
            mode,
            query,
            path_scope,
            case_sensitive,
            context_lines,
            max_results,
            offset,
            result_count: 0,
            match_count: 0,
            truncated: false,
            next_offset: None,
            hits: Vec::new(),
        })
    }
}

fn resolve_query(
    mode: SearchMode,
    query: Option<&str>,
    pattern: Option<&str>,
    existing: &SearchSession,
) -> Result<String> {
    let raw = match mode {
        SearchMode::Files => pattern.or(query),
        SearchMode::Text => query.or(pattern),
    }
    .unwrap_or(existing.query.as_str())
    .trim()
    .to_string();
    if raw.is_empty() {
        bail!("a non-empty search query is required");
    }
    Ok(raw)
}

struct FileSearchResult {
    hits: Vec<SearchWorkspaceHit>,
    total_matches: usize,
}

#[derive(Clone)]
struct RankedFileHit {
    path: String,
    score: u32,
    indices: Option<Vec<u32>>,
    match_kind: &'static str,
}

async fn run_file_search(
    pattern: &str,
    path_scope: Option<&str>,
    case_sensitive: bool,
    max_results: usize,
    offset: usize,
    ctx: &ToolExecutionContext,
) -> Result<FileSearchResult> {
    let root = resolve_read_path(&ctx.workspace_root, path_scope)?;
    let workspace_root = ctx.workspace_root.clone();
    let query = if case_sensitive {
        pattern.to_string()
    } else {
        pattern.to_ascii_lowercase()
    };
    let matches = tokio::task::spawn_blocking(move || -> Result<Vec<RankedFileHit>> {
        let entries = collect_repo_entries(&workspace_root, &root)?;
        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let pattern = Pattern::new(
            &query,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let mut ranked = entries
            .into_iter()
            .filter_map(|entry| {
                let candidate_name = if case_sensitive {
                    entry.file_name.clone()
                } else {
                    entry.file_name.to_ascii_lowercase()
                };
                let candidate_path = if case_sensitive {
                    entry.relative_path.clone()
                } else {
                    entry.relative_path.to_ascii_lowercase()
                };
                let mut name_buf = Vec::new();
                let mut path_buf = Vec::new();
                let name_haystack: Utf32Str<'_> = Utf32Str::new(&candidate_name, &mut name_buf);
                let path_haystack: Utf32Str<'_> = Utf32Str::new(&candidate_path, &mut path_buf);
                let path_score = pattern.score(path_haystack, &mut matcher);
                let name_score = pattern.score(name_haystack, &mut matcher);
                let (score, match_kind, indices_source) = match (path_score, name_score) {
                    (Some(path_score), Some(name_score)) if name_score >= path_score => {
                        (name_score, "file_name", candidate_name.as_str())
                    }
                    (Some(path_score), Some(_)) | (Some(path_score), None) => {
                        (path_score, "path", candidate_path.as_str())
                    }
                    (None, Some(name_score)) => (name_score, "file_name", candidate_name.as_str()),
                    (None, None) => return None,
                };
                let indices = fuzzy_match_indices(indices_source, &query);
                Some(RankedFileHit {
                    path: entry.relative_path,
                    score,
                    indices,
                    match_kind,
                })
            })
            .collect::<Vec<_>>();
        sort_ranked_paths(&mut ranked, |hit| hit.score, |hit| hit.path.as_str());
        Ok(ranked)
    })
    .await??;

    let total_matches = matches.len();
    let hits = matches
        .into_iter()
        .skip(offset)
        .take(max_results)
        .enumerate()
        .map(|(index, hit)| SearchWorkspaceHit {
            path: hit.path.clone(),
            line: None,
            preview: hit.path,
            score: Some(hit.score),
            file_score: Some(hit.score),
            file_match_count: Some(1),
            rank: Some(offset + index + 1),
            indices: hit.indices,
            match_kind: Some(hit.match_kind.to_string()),
        })
        .collect::<Vec<_>>();
    Ok(FileSearchResult {
        hits,
        total_matches,
    })
}

struct TextSearchResult {
    rendered: Vec<String>,
    hits: Vec<SearchWorkspaceHit>,
    match_count: usize,
    file_count: usize,
    truncated: bool,
    next_offset: Option<usize>,
}

#[derive(Clone)]
struct RankedTextHit {
    path: String,
    line: usize,
    preview: String,
    score: u32,
    file_score: u32,
    file_match_count: usize,
    rank: usize,
    match_kind: &'static str,
}

#[derive(Clone)]
struct RankedTextFile {
    file_score: u32,
    path: String,
    match_count: usize,
    hits: Vec<RankedTextHit>,
}

async fn run_text_search(
    query: &str,
    path_scope: Option<&str>,
    case_sensitive: bool,
    context_lines: usize,
    max_results: usize,
    offset: usize,
    ctx: &ToolExecutionContext,
) -> Result<TextSearchResult> {
    let search_root = resolve_read_path(&ctx.workspace_root, path_scope)?;
    let workspace_root = ctx.workspace_root.clone();
    let query_for_search = query.to_string();

    tokio::task::spawn_blocking(move || -> Result<TextSearchResult> {
        let entries = collect_repo_entries(&workspace_root, &search_root)?;
        let mut file_hits = BTreeSet::new();
        let mut match_count = 0usize;
        let query_lower = query_for_search.to_ascii_lowercase();
        let mut grouped_hits: HashMap<String, Vec<RankedTextHit>> = HashMap::new();

        for entry in entries {
            let path = workspace_root.join(&entry.relative_path);
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if bytes.contains(&0) {
                continue;
            }
            let Ok(decoded) = decode_text_file(&bytes) else {
                continue;
            };
            let text = decoded.text;
            let lines = text.lines().collect::<Vec<_>>();
            let path_lower = entry.relative_path.to_ascii_lowercase();
            let file_name_lower = entry.file_name.to_ascii_lowercase();
            for (index, line) in lines.iter().enumerate() {
                if !line_matches(line, &query_for_search, case_sensitive) {
                    continue;
                }
                match_count += 1;
                file_hits.insert(entry.relative_path.clone());
                let preview = render_match(&entry.relative_path, index, &lines, context_lines);
                let (score, match_kind) =
                    rank_text_hit(&query_lower, &path_lower, &file_name_lower, line, index);
                grouped_hits
                    .entry(entry.relative_path.clone())
                    .or_default()
                    .push(RankedTextHit {
                        path: entry.relative_path.clone(),
                        line: index + 1,
                        preview,
                        score,
                        file_score: 0,
                        file_match_count: 0,
                        rank: 0,
                        match_kind,
                    });
            }
        }

        let mut ranked_files = grouped_hits
            .into_iter()
            .map(|(path, mut hits)| {
                hits.sort_by(|left, right| {
                    right
                        .score
                        .cmp(&left.score)
                        .then_with(|| left.line.cmp(&right.line))
                });
                let top_score = hits.first().map(|hit| hit.score).unwrap_or_default();
                let file_score =
                    top_score + (hits.len().min(8) as u32 * 20) + short_path_bonus(path.as_str());
                let match_count = hits.len();
                for hit in &mut hits {
                    hit.file_score = file_score;
                    hit.file_match_count = match_count;
                }
                RankedTextFile {
                    file_score,
                    path,
                    match_count,
                    hits,
                }
            })
            .collect::<Vec<_>>();
        ranked_files.sort_by(|left, right| {
            right
                .file_score
                .cmp(&left.file_score)
                .then_with(|| right.match_count.cmp(&left.match_count))
                .then_with(|| left.path.cmp(&right.path))
        });

        let mut flattened = Vec::new();
        for file in ranked_files {
            for hit in file.hits.into_iter().take(3) {
                flattened.push(hit);
            }
        }
        flattened.sort_by(|left, right| {
            right
                .file_score
                .cmp(&left.file_score)
                .then_with(|| right.score.cmp(&left.score))
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.line.cmp(&right.line))
        });
        for (index, hit) in flattened.iter_mut().enumerate() {
            hit.rank = index + 1;
        }

        let total_ranked_hits = flattened.len();
        let paged = flattened
            .into_iter()
            .skip(offset)
            .take(max_results)
            .collect::<Vec<_>>();
        let rendered = paged
            .iter()
            .map(|hit| hit.preview.clone())
            .collect::<Vec<_>>();
        let hits = paged
            .into_iter()
            .map(|hit| SearchWorkspaceHit {
                path: hit.path,
                line: Some(hit.line),
                preview: hit.preview,
                score: Some(hit.score),
                file_score: Some(hit.file_score),
                file_match_count: Some(hit.file_match_count),
                rank: Some(hit.rank),
                indices: None,
                match_kind: Some(hit.match_kind.to_string()),
            })
            .collect::<Vec<_>>();

        Ok(TextSearchResult {
            rendered,
            hits,
            match_count,
            file_count: file_hits.len(),
            truncated: total_ranked_hits > offset.saturating_add(max_results),
            next_offset: (total_ranked_hits > offset.saturating_add(max_results))
                .then_some(offset.saturating_add(max_results)),
        })
    })
    .await?
}

fn build_file_search_output(
    _session_id: &str,
    _resolved: &ResolvedSearchRequest,
    search: &FileSearchResult,
) -> String {
    let displayed = search
        .hits
        .iter()
        .take(10)
        .map(|hit| hit.path.clone())
        .collect::<Vec<_>>();
    let mut sections = Vec::new();
    if displayed.is_empty() {
        sections.push("No files found. Try a broader pattern or set path_scope.".to_string());
    } else {
        sections.push(format!(
            "Top {} matches (showing {} of {}):",
            displayed.len(),
            displayed.len(),
            search.total_matches
        ));
        sections.push(displayed.join("\n"));
        if search.hits.len() > displayed.len() {
            sections.push(format!(
                "\n…and {} more",
                search.hits.len().saturating_sub(displayed.len())
            ));
        }
    }
    sections.join("\n")
}

fn build_text_search_output(
    _session_id: &str,
    resolved: &ResolvedSearchRequest,
    search: &TextSearchResult,
) -> String {
    let mut sections = Vec::new();
    if search.rendered.is_empty() {
        sections.push(format!(
            "No matches found for `{}`{}.",
            resolved.query,
            resolved
                .path_scope
                .as_deref()
                .map(|scope| format!(" under {scope}"))
                .unwrap_or_default()
        ));
    } else {
        sections.push(format!(
            "Found {} matches in {} files:",
            search.match_count, search.file_count
        ));
        sections.push(search.rendered.join("\n\n"));
        if search.truncated {
            sections.push(format!(
                "\n…and {} more matches",
                search.match_count.saturating_sub(resolved.max_results)
            ));
        }
    }
    sections.join("\n")
}

fn line_matches(line: &str, query: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        line.contains(query)
    } else {
        line.to_ascii_lowercase()
            .contains(&query.to_ascii_lowercase())
    }
}

fn fuzzy_match_indices(haystack: &str, needle: &str) -> Option<Vec<u32>> {
    let lowered_haystack = haystack.to_ascii_lowercase();
    let lowered_needle = needle.to_ascii_lowercase();
    let mut search_from = 0usize;
    let mut indices = Vec::new();
    for needle_char in lowered_needle.chars() {
        let slice = lowered_haystack.get(search_from..)?;
        let relative = slice.find(needle_char)?;
        let absolute = search_from + relative;
        indices.push(u32::try_from(absolute).ok()?);
        search_from = absolute.saturating_add(1);
    }
    Some(indices)
}

fn rank_text_hit(
    query_lower: &str,
    path_lower: &str,
    file_name_lower: &str,
    line: &str,
    line_index: usize,
) -> (u32, &'static str) {
    let line_lower = line.to_ascii_lowercase();
    let mut score = 100u32;
    let mut match_kind = "text";
    if file_name_lower.contains(query_lower) {
        score += 140;
        match_kind = "file_name";
    }
    if path_lower.contains(query_lower) {
        score += 80;
        if match_kind == "text" {
            match_kind = "path";
        }
    }
    if line_lower.contains(&format!("fn {query_lower}"))
        || line_lower.contains(&format!("class {query_lower}"))
        || line_lower.contains(&format!("struct {query_lower}"))
        || line_lower.contains(&format!("enum {query_lower}"))
    {
        score += 120;
        match_kind = "definition";
    }
    if line_lower.trim_start().starts_with(query_lower) {
        score += 30;
    }
    score += short_path_bonus(path_lower);
    score += u32::try_from(line.len().min(120))
        .unwrap_or(120)
        .saturating_sub(20);
    score = score.saturating_sub(u32::try_from(line_index.min(200)).unwrap_or(200) / 4);
    (score, match_kind)
}

fn short_path_bonus(path: &str) -> u32 {
    let segments = path.matches('/').count() as u32;
    40u32.saturating_sub(segments * 6)
}

fn render_match(path: &str, line_index: usize, lines: &[&str], context_lines: usize) -> String {
    if context_lines == 0 {
        return format!(
            "{path}:{}: {}",
            line_index + 1,
            lines[line_index].trim_end()
        );
    }

    let start = line_index.saturating_sub(context_lines);
    let end = (line_index + context_lines + 1).min(lines.len());
    let mut out = vec![format!("{path}:{}", line_index + 1)];
    for (offset, line) in lines[start..end].iter().enumerate() {
        let actual = start + offset;
        let marker = if actual == line_index { ">" } else { " " };
        out.push(format!("{marker} {:>6}  {}", actual + 1, line.trim_end()));
    }
    out.join("\n")
}
