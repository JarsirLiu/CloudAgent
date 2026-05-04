use super::common::{collect_repo_entries, sort_ranked_paths};
use crate::impls::text_codec::decode_text_file;
use crate::registry::shared::{
    LocalTool, LocalToolInvocation, ToolInvocationOutput, resolve_read_path,
};
use crate::spec::{ToolCategory, ToolDescriptor, ToolPermissionTier, ToolRisk, ToolUsageGuidance};
use agent_core::{ToolExecutionContext, ToolIdentity, ToolSpec};
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
            true,
            vec!["explore", "edit", "verify", "repo"],
            ToolUsageGuidance {
                preferred_for: vec![
                    "first step of bug investigation",
                    "finding likely files before reading code",
                ],
                follow_up_hint: Some("open the strongest hits with `read_files` before editing"),
                ..ToolUsageGuidance::default()
            },
            ToolSpec {
                name: "search_workspace".to_string(),
                identity: ToolIdentity::built_in("search_workspace"),
                description: "Search repository files or text through one structured entry point. Use mode=`files` for likely path matches and mode=`text` for symbol or string matches. Reuse `session_id` to refine an existing search instead of restarting from scratch.".to_string(),
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
    let pattern = if case_sensitive {
        pattern.to_string()
    } else {
        pattern.to_ascii_lowercase()
    };
    let matches = tokio::task::spawn_blocking(move || -> Result<Vec<String>> {
        let entries = collect_repo_entries(&workspace_root, &root)?;
        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let pattern = Pattern::new(
            &pattern,
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
                let score = pattern
                    .score(path_haystack, &mut matcher)
                    .or_else(|| pattern.score(name_haystack, &mut matcher))?;
                Some((
                    usize::try_from(score).unwrap_or(usize::MAX),
                    entry.relative_path,
                ))
            })
            .collect::<Vec<_>>();
        sort_ranked_paths(&mut ranked);
        Ok(ranked.into_iter().map(|(_, path)| path).collect())
    })
    .await??;

    let total_matches = matches.len();
    let hits = matches
        .into_iter()
        .skip(offset)
        .take(max_results)
        .map(|path| SearchWorkspaceHit {
            path: path.clone(),
            line: None,
            preview: path,
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
        let mut rendered = Vec::new();
        let mut hits = Vec::new();
        let mut file_hits = BTreeSet::new();
        let mut match_count = 0usize;

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
            for (index, line) in lines.iter().enumerate() {
                if !line_matches(line, &query_for_search, case_sensitive) {
                    continue;
                }
                match_count += 1;
                file_hits.insert(entry.relative_path.clone());
                if match_count <= offset {
                    continue;
                }
                if rendered.len() >= max_results {
                    continue;
                }
                let preview = render_match(&entry.relative_path, index, &lines, context_lines);
                rendered.push(preview.clone());
                hits.push(SearchWorkspaceHit {
                    path: entry.relative_path.clone(),
                    line: Some(index + 1),
                    preview,
                });
            }
        }

        Ok(TextSearchResult {
            rendered,
            hits,
            match_count,
            file_count: file_hits.len(),
            truncated: match_count > offset.saturating_add(max_results),
            next_offset: (match_count > offset.saturating_add(max_results))
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
