use super::common::{collect_repo_entries, sort_ranked_paths};
use crate::impls::result_format::{finalize, push_fact, push_list_section, push_section};
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
                    "rapidly testing multiple codebase hypotheses before choosing edits",
                ],
                follow_up_hint: Some("treat the first search as a coverage pass: open the strongest 2-3 plausible hits with `read_file` in parallel before editing"),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "search_workspace".to_string(),
                identity: ToolIdentity::built_in("search_workspace"),
                description: "Search repository files or text through one structured entry point. Use mode=`files` for likely path matches and mode=`text` for symbol, string, or UI wording matches. Treat the first search as a coverage pass across multiple plausible files, then reuse `session_id` to refine instead of restarting from scratch. The goal is to make the next 1-3 `read_file` calls obvious.".to_string(),
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
    top_files: Vec<RankedTextFileSummary>,
    rendered: Vec<String>,
    hits: Vec<SearchWorkspaceHit>,
    match_count: usize,
    file_count: usize,
    truncated: bool,
    next_offset: Option<usize>,
}

#[derive(Clone)]
struct RankedTextFileSummary {
    path: String,
    file_score: u32,
    match_count: usize,
    top_line: Option<usize>,
    top_match_kind: &'static str,
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
    fallback_match: bool,
}

#[derive(Clone)]
struct RankedTextFile {
    file_score: u32,
    path: String,
    match_count: usize,
    hits: Vec<RankedTextHit>,
}

#[derive(Clone, Copy)]
struct TextMatchAssessment {
    strict_match: bool,
    fallback_match: bool,
    phrase_match: bool,
    definition_match: bool,
    matched_terms: usize,
}

#[derive(Clone, Debug, Default)]
struct QueryIntent {
    required_terms: Vec<String>,
    support_terms: Vec<String>,
    preferred_path_terms: Vec<String>,
    preferred_line_terms: Vec<String>,
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
        let query_terms = derive_query_terms(&query_for_search);
        let query_intent = derive_query_intent(&query_terms);
        let mut grouped_hits: HashMap<String, Vec<RankedTextHit>> = HashMap::new();
        let mut strict_match_count = 0usize;
        let mut strict_file_hits = BTreeSet::new();

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
                let assessment = assess_text_match(
                    line,
                    &query_for_search,
                    &query_terms,
                    &query_intent,
                    &path_lower,
                    &file_name_lower,
                    case_sensitive,
                );
                if !assessment.strict_match && !assessment.fallback_match {
                    continue;
                }
                match_count += 1;
                file_hits.insert(entry.relative_path.clone());
                if assessment.strict_match {
                    strict_match_count += 1;
                    strict_file_hits.insert(entry.relative_path.clone());
                }
                let preview = render_match(&entry.relative_path, index, &lines, context_lines);
                let (score, match_kind) = rank_text_hit(
                    &query_lower,
                    &query_terms,
                    &path_lower,
                    &file_name_lower,
                    line,
                    index,
                    assessment,
                    &query_intent,
                );
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
                        fallback_match: assessment.fallback_match && !assessment.strict_match,
                    });
            }
        }

        let prefer_strict_only = strict_match_count >= 3 || strict_file_hits.len() >= 2;

        let mut ranked_files = grouped_hits
            .into_iter()
            .filter_map(|(path, mut hits)| {
                if prefer_strict_only {
                    hits.retain(|hit| !hit.fallback_match);
                    if hits.is_empty() {
                        return None;
                    }
                }
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
                Some(RankedTextFile {
                    file_score,
                    path,
                    match_count,
                    hits,
                })
            })
            .collect::<Vec<_>>();
        ranked_files.sort_by(|left, right| {
            right
                .file_score
                .cmp(&left.file_score)
                .then_with(|| right.match_count.cmp(&left.match_count))
                .then_with(|| left.path.cmp(&right.path))
        });
        let top_files = ranked_files
            .iter()
            .take(5)
            .map(|file| RankedTextFileSummary {
                path: file.path.clone(),
                file_score: file.file_score,
                match_count: file.match_count,
                top_line: file.hits.first().map(|hit| hit.line),
                top_match_kind: file
                    .hits
                    .first()
                    .map(|hit| hit.match_kind)
                    .unwrap_or("text"),
            })
            .collect::<Vec<_>>();

        let mut flattened = Vec::new();
        for hit_index in 0..3 {
            for file in &ranked_files {
                if let Some(hit) = file.hits.get(hit_index) {
                    flattened.push(hit.clone());
                }
            }
        }
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
            top_files,
            rendered,
            hits,
            match_count,
            file_count: if prefer_strict_only {
                strict_file_hits.len()
            } else {
                file_hits.len()
            },
            truncated: total_ranked_hits > offset.saturating_add(max_results),
            next_offset: (total_ranked_hits > offset.saturating_add(max_results))
                .then_some(offset.saturating_add(max_results)),
        })
    })
    .await?
}

fn build_file_search_output(
    session_id: &str,
    resolved: &ResolvedSearchRequest,
    search: &FileSearchResult,
) -> String {
    let displayed = search.hits.iter().take(10).collect::<Vec<_>>();
    let summary = if displayed.is_empty() {
        format!("Found no file matches for `{}`.", resolved.query)
    } else {
        format!(
            "Found {} file matches for `{}`; showing {}.",
            search.total_matches,
            resolved.query,
            displayed.len()
        )
    };
    let mut sections = Vec::new();
    push_fact(&mut sections, "Session", session_id.to_string());
    push_fact(&mut sections, "Mode", "files");
    push_fact(&mut sections, "Query", resolved.query.clone());
    if let Some(path_scope) = resolved.path_scope.as_deref() {
        push_fact(&mut sections, "Path scope", path_scope.to_string());
    }
    push_fact(
        &mut sections,
        "Showing",
        format!("{} of {}", displayed.len(), search.total_matches),
    );
    let hits = displayed
        .iter()
        .map(|hit| {
            let rank = hit.rank.unwrap_or_default();
            let score = hit.score.unwrap_or_default();
            let match_kind = hit.match_kind.as_deref().unwrap_or("path");
            format!(
                "{rank}. {} [match_kind={match_kind}, score={score}]",
                hit.path
            )
        })
        .collect::<Vec<_>>();
    if hits.is_empty() {
        push_section(
            &mut sections,
            "Guidance",
            "Try a broader pattern or set `path_scope` before opening files.",
        );
    } else {
        push_list_section(&mut sections, "Top hits", &hits);
    }
    let next_step = if hits.is_empty() {
        Some("adjust the pattern or set `path_scope`, then rerun `search_workspace`")
    } else {
        Some("open the strongest 2-3 plausible hits with `read_file` in parallel before editing")
    };
    finalize(summary, sections, next_step)
}

fn build_text_search_output(
    session_id: &str,
    resolved: &ResolvedSearchRequest,
    search: &TextSearchResult,
) -> String {
    let summary = if search.rendered.is_empty() {
        format!("Found no text matches for `{}`.", resolved.query)
    } else {
        format!(
            "Found {} text matches in {} files for `{}`.",
            search.match_count, search.file_count, resolved.query
        )
    };
    let mut sections = Vec::new();
    push_fact(&mut sections, "Session", session_id.to_string());
    push_fact(&mut sections, "Mode", "text");
    push_fact(&mut sections, "Query", resolved.query.clone());
    if let Some(path_scope) = resolved.path_scope.as_deref() {
        push_fact(&mut sections, "Path scope", path_scope.to_string());
    }
    push_fact(
        &mut sections,
        "Showing",
        format!("{} of {} matches", search.hits.len(), search.match_count),
    );
    if search.rendered.is_empty() {
        push_section(
            &mut sections,
            "Guidance",
            "Try a broader query or widen `path_scope` before reading files.",
        );
    } else {
        let top_files = search
            .top_files
            .iter()
            .map(|file| {
                let top_line = file
                    .top_line
                    .map(|line| format!("line {line}"))
                    .unwrap_or_else(|| "line ?".to_string());
                format!(
                    "{} [file_score={}, matches={}, top_hit={} {}]",
                    file.path, file.file_score, file.match_count, file.top_match_kind, top_line
                )
            })
            .collect::<Vec<_>>();
        push_list_section(&mut sections, "Top files", &top_files);
        push_section(&mut sections, "Matches", search.rendered.join("\n\n"));
    }
    let next_step = if search.rendered.is_empty() {
        Some("adjust the query or widen `path_scope`, then rerun `search_workspace`")
    } else if search.truncated {
        Some(
            "open the strongest 2-3 files with `read_file`, or continue this search with `next_offset` for more matches",
        )
    } else {
        Some("open the strongest 2-3 plausible files with `read_file` in parallel before editing")
    };
    finalize(summary, sections, next_step)
}

fn assess_text_match(
    line: &str,
    query: &str,
    query_terms: &[String],
    query_intent: &QueryIntent,
    path_lower: &str,
    file_name_lower: &str,
    case_sensitive: bool,
) -> TextMatchAssessment {
    let normalized_query = if case_sensitive {
        query.to_string()
    } else {
        query.to_ascii_lowercase()
    };
    let line_cmp = if case_sensitive {
        line.to_string()
    } else {
        line.to_ascii_lowercase()
    };
    let phrase_match = line_cmp.contains(&normalized_query)
        || path_lower.contains(&normalized_query)
        || file_name_lower.contains(&normalized_query);
    let definition_match = line_looks_like_definition(&line_cmp, &normalized_query);
    let matched_terms = query_terms
        .iter()
        .filter(|term| !term.is_empty() && line_cmp.contains(term.as_str()))
        .count();
    let required_terms_matched = query_intent
        .required_terms
        .iter()
        .filter(|term| line_cmp.contains(term.as_str()))
        .count();
    let support_terms_matched = query_intent
        .support_terms
        .iter()
        .filter(|term| line_cmp.contains(term.as_str()))
        .count();
    let strict_match = phrase_match || definition_match;
    let intent_match = !query_intent.required_terms.is_empty()
        && required_terms_matched >= query_intent.required_terms.len().min(2)
        && support_terms_matched >= 1;
    let fallback_match =
        !strict_match && (intent_match || (query_terms.len() >= 2 && matched_terms >= 2));
    TextMatchAssessment {
        strict_match,
        fallback_match,
        phrase_match,
        definition_match,
        matched_terms,
    }
}

fn derive_query_terms(query: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut seen = BTreeSet::new();
    let normalized = query.trim().to_ascii_lowercase();
    if !normalized.is_empty() && seen.insert(normalized.clone()) {
        terms.push(normalized);
    }
    for raw in query
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
    {
        let lowered = raw.to_ascii_lowercase();
        if lowered.len() >= 3 && seen.insert(lowered.clone()) {
            terms.push(lowered);
        }
    }

    let camel_parts = split_camel_case(query);
    for part in camel_parts {
        let lowered = part.to_ascii_lowercase();
        if lowered.len() >= 3 && seen.insert(lowered.clone()) {
            terms.push(lowered);
        }
    }
    terms
}

fn derive_query_intent(query_terms: &[String]) -> QueryIntent {
    let mut intent = QueryIntent::default();
    let has_tab = query_terms.iter().any(|term| term == "tab");
    let has_completion = query_terms
        .iter()
        .any(|term| term.contains("completion") || term.contains("autocomplete"));
    let has_space = query_terms
        .iter()
        .any(|term| term == "space" || term == "whitespace");
    let has_insert = query_terms
        .iter()
        .any(|term| term.contains("insert") || term.contains("append"));

    if has_tab && has_completion {
        intent.required_terms = vec!["tab".to_string(), "completion".to_string()];
        intent.support_terms.extend([
            "accept".to_string(),
            "selected".to_string(),
            "suggestion".to_string(),
            "insertion".to_string(),
            "key".to_string(),
        ]);
        intent.preferred_path_terms.extend([
            "completion".to_string(),
            "composer".to_string(),
            "input".to_string(),
            "chat".to_string(),
        ]);
        intent.preferred_line_terms.extend([
            "accept_selected_completion".to_string(),
            "keycode::tab".to_string(),
            "suggestion".to_string(),
            "insert".to_string(),
        ]);
    }

    if has_space || has_insert {
        intent.required_terms.push("space".to_string());
        intent.support_terms.extend([
            "insert".to_string(),
            "append".to_string(),
            "trim".to_string(),
            "push".to_string(),
            "\" \"".to_string(),
        ]);
        intent.preferred_line_terms.extend([
            "push(' ')".to_string(),
            "push_str(\" \")".to_string(),
            "trim".to_string(),
        ]);
    }

    dedupe_strings(&mut intent.required_terms);
    dedupe_strings(&mut intent.support_terms);
    dedupe_strings(&mut intent.preferred_path_terms);
    dedupe_strings(&mut intent.preferred_line_terms);
    intent
}

fn split_camel_case(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut prev_is_lower = false;
    for ch in input.chars() {
        if !ch.is_ascii_alphanumeric() {
            if current.len() >= 3 {
                parts.push(current.clone());
            }
            current.clear();
            prev_is_lower = false;
            continue;
        }
        let is_upper = ch.is_ascii_uppercase();
        if is_upper && prev_is_lower && current.len() >= 3 {
            parts.push(current.clone());
            current.clear();
        }
        prev_is_lower = ch.is_ascii_lowercase();
        current.push(ch);
    }
    if current.len() >= 3 {
        parts.push(current);
    }
    parts
}

fn dedupe_strings(values: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    values.retain(|value| seen.insert(value.clone()));
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

#[allow(clippy::too_many_arguments)]
fn rank_text_hit(
    query_lower: &str,
    query_terms: &[String],
    path_lower: &str,
    file_name_lower: &str,
    line: &str,
    line_index: usize,
    assessment: TextMatchAssessment,
    query_intent: &QueryIntent,
) -> (u32, &'static str) {
    let line_lower = line.to_ascii_lowercase();
    let mut score = 100u32;
    let mut match_kind = "text";
    let exact_term_match = query_terms.iter().any(|term| line_lower.contains(term));
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
    if assessment.definition_match {
        score += 120;
        match_kind = "definition";
    }
    if assessment.phrase_match {
        score += 110;
        if match_kind == "text" {
            match_kind = "phrase";
        }
    }
    if exact_term_match {
        score += 45;
        if match_kind == "text" {
            match_kind = "term";
        }
    }
    if assessment.matched_terms >= 2 {
        score += 55;
        if match_kind == "text" || match_kind == "term" {
            match_kind = "term_cover";
        }
    }
    let preferred_path_matches = query_intent
        .preferred_path_terms
        .iter()
        .filter(|term| {
            path_lower.contains(term.as_str()) || file_name_lower.contains(term.as_str())
        })
        .count();
    if preferred_path_matches > 0 {
        score += 45 * preferred_path_matches.min(3) as u32;
        if match_kind == "text" || match_kind == "term" {
            match_kind = "entrypoint";
        }
    }
    let preferred_line_matches = query_intent
        .preferred_line_terms
        .iter()
        .filter(|term| line_lower.contains(term.as_str()))
        .count();
    if preferred_line_matches > 0 {
        score += 55 * preferred_line_matches.min(3) as u32;
        if match_kind == "text" || match_kind == "term" || match_kind == "term_cover" {
            match_kind = "handler";
        }
    }
    if line_lower.trim_start().starts_with(query_lower) {
        score += 30;
    }
    score += short_path_bonus(path_lower);
    score += u32::try_from(line.len().min(120))
        .unwrap_or(120)
        .saturating_sub(20);
    score = score.saturating_sub(u32::try_from(line_index.min(200)).unwrap_or(200) / 4);
    if assessment.fallback_match {
        score = score.saturating_sub(35);
    }
    (score, match_kind)
}

fn line_looks_like_definition(line_lower: &str, query_lower: &str) -> bool {
    line_lower.contains(&format!("fn {query_lower}"))
        || line_lower.contains(&format!("class {query_lower}"))
        || line_lower.contains(&format!("struct {query_lower}"))
        || line_lower.contains(&format!("enum {query_lower}"))
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
