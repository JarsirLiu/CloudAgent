#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PowerShellAccess {
    ReadOnly,
    Unknown,
}

pub(super) fn classify_segment(segment: &str) -> PowerShellAccess {
    let normalized = segment.trim().to_ascii_lowercase();
    let normalized = normalized.as_str();
    if normalized.is_empty() {
        return PowerShellAccess::Unknown;
    }

    match parse_expression(normalized) {
        Some(Expression::Pipeline(commands)) if !commands.is_empty() => {
            if commands.iter().all(CommandInvocation::is_read_only) {
                PowerShellAccess::ReadOnly
            } else {
                PowerShellAccess::Unknown
            }
        }
        Some(Expression::Assignment { value, .. }) if value.is_read_only() => {
            PowerShellAccess::ReadOnly
        }
        Some(expr) if expr.is_read_only() => PowerShellAccess::ReadOnly,
        _ => PowerShellAccess::Unknown,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Expression<'a> {
    Assignment {
        variable: &'a str,
        value: Box<Expression<'a>>,
    },
    Pipeline(Vec<CommandInvocation<'a>>),
    Value(ValueExpr<'a>),
}

impl Expression<'_> {
    fn is_read_only(&self) -> bool {
        match self {
            Self::Assignment { value, .. } => value.is_read_only(),
            Self::Pipeline(commands) => commands.iter().all(CommandInvocation::is_read_only),
            Self::Value(value) => value.is_read_only(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ValueExpr<'a> {
    Variable {
        name: &'a str,
        suffixes: Vec<Suffix<'a>>,
    },
    Grouped {
        inner: Box<Expression<'a>>,
        suffixes: Vec<Suffix<'a>>,
    },
    Joined {
        left: Box<ValueExpr<'a>>,
        right: &'a str,
    },
}

impl ValueExpr<'_> {
    fn is_read_only(&self) -> bool {
        match self {
            Self::Variable { suffixes, .. } => suffixes.iter().all(Suffix::is_read_only),
            Self::Grouped { inner, suffixes } => {
                inner.is_read_only() && suffixes.iter().all(Suffix::is_read_only)
            }
            Self::Joined { left, right } => left.is_read_only() && is_safe_literal(right),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Suffix<'a> {
    Member(&'a str),
    Index(&'a str),
}

impl Suffix<'_> {
    fn is_read_only(&self) -> bool {
        match self {
            Self::Member(member) => !member.trim().is_empty(),
            Self::Index(index) => !index.trim().is_empty(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CommandInvocation<'a> {
    name: &'a str,
    args: Vec<&'a str>,
}

impl CommandInvocation<'_> {
    fn is_read_only(&self) -> bool {
        is_safe_cmdlet(self.name)
            && self.args.iter().all(|arg| {
                !contains_powershell_redirection(arg)
                    && !contains_powershell_call_operator(arg)
                    && !contains_executable_expression(arg)
            })
    }
}

fn parse_expression(input: &str) -> Option<Expression<'_>> {
    let trimmed = input.trim();
    if let Some((variable, rhs)) = split_assignment(trimmed) {
        return Some(Expression::Assignment {
            variable,
            value: Box::new(parse_expression(rhs)?),
        });
    }

    let pipeline_parts = split_top_level(trimmed, '|');
    if pipeline_parts.len() > 1 {
        let commands = pipeline_parts
            .into_iter()
            .map(parse_command_invocation)
            .collect::<Option<Vec<_>>>()?;
        return Some(Expression::Pipeline(commands));
    }

    if let Some(value) = parse_value_expr(trimmed) {
        return Some(Expression::Value(value));
    }

    parse_command_invocation(trimmed).map(|command| Expression::Pipeline(vec![command]))
}

fn parse_value_expr(input: &str) -> Option<ValueExpr<'_>> {
    let trimmed = input.trim();
    if let Some((left, right)) = split_join_operator(trimmed) {
        return Some(ValueExpr::Joined {
            left: Box::new(parse_value_expr(left)?),
            right,
        });
    }

    if let Some(value) = parse_grouped_value(trimmed) {
        return Some(value);
    }

    parse_variable_value(trimmed)
}

fn parse_grouped_value(input: &str) -> Option<ValueExpr<'_>> {
    if !input.starts_with('(') {
        return None;
    }

    let close_index = find_matching_pair(input, '(', ')')?;
    let inner = input[1..close_index].trim();
    let suffix_text = input[close_index + 1..].trim();
    Some(ValueExpr::Grouped {
        inner: Box::new(parse_expression(inner)?),
        suffixes: parse_suffixes(suffix_text)?,
    })
}

fn parse_variable_value(input: &str) -> Option<ValueExpr<'_>> {
    let trimmed = input.trim();
    if !trimmed.starts_with('$') {
        return None;
    }

    let name_end = trimmed
        .char_indices()
        .find_map(|(index, ch)| {
            (index > 0 && !matches!(ch, '_' | ':') && !ch.is_ascii_alphanumeric()).then_some(index)
        })
        .unwrap_or(trimmed.len());
    let (name, suffix_text) = trimmed.split_at(name_end);
    if name.len() <= 1 {
        return None;
    }

    Some(ValueExpr::Variable {
        name,
        suffixes: parse_suffixes(suffix_text.trim())?,
    })
}

fn parse_suffixes(mut input: &str) -> Option<Vec<Suffix<'_>>> {
    let mut suffixes = Vec::new();
    while !input.trim().is_empty() {
        input = input.trim_start();
        if let Some(rest) = input.strip_prefix('.') {
            let end = rest
                .char_indices()
                .find_map(|(index, ch)| {
                    (!matches!(ch, '_' | '-') && !ch.is_ascii_alphanumeric()).then_some(index)
                })
                .unwrap_or(rest.len());
            let member = rest[..end].trim();
            if member.is_empty() {
                return None;
            }
            suffixes.push(Suffix::Member(member));
            input = &rest[end..];
            continue;
        }

        if input.starts_with('[') {
            let close_index = find_matching_pair(input, '[', ']')?;
            let index = input[1..close_index].trim();
            if index.is_empty() {
                return None;
            }
            suffixes.push(Suffix::Index(index));
            input = &input[close_index + 1..];
            continue;
        }

        return None;
    }
    Some(suffixes)
}

fn parse_command_invocation(input: &str) -> Option<CommandInvocation<'_>> {
    let tokens = tokenize_powershell(input)?;
    let (name, args) = tokens.split_first()?;
    Some(CommandInvocation {
        name,
        args: args.to_vec(),
    })
}

fn tokenize_powershell(input: &str) -> Option<Vec<&str>> {
    let mut tokens = Vec::new();
    let mut token_start = None;
    let mut quote: Option<char> = None;

    for (index, ch) in input.char_indices() {
        match quote {
            Some(active) => {
                if ch == active {
                    quote = None;
                }
            }
            None if ch == '"' || ch == '\'' => {
                token_start.get_or_insert(index);
                quote = Some(ch);
            }
            None if ch.is_whitespace() => {
                if let Some(start) = token_start.take() {
                    tokens.push(input[start..index].trim());
                }
            }
            None => {
                token_start.get_or_insert(index);
            }
        }
    }

    if quote.is_some() {
        return None;
    }

    if let Some(start) = token_start {
        tokens.push(input[start..].trim());
    }

    (!tokens.is_empty()).then_some(tokens)
}

fn split_assignment(input: &str) -> Option<(&str, &str)> {
    let (lhs, rhs) = split_top_level_once(input, '=')?;
    let lhs = lhs.trim();
    let rhs = rhs.trim();
    if !lhs.starts_with('$') || rhs.is_empty() || lhs.contains("==") || lhs.contains("!=") {
        return None;
    }
    Some((lhs, rhs))
}

fn split_join_operator(input: &str) -> Option<(&str, &str)> {
    let normalized = " -join ";
    let index = find_top_level_substring(input, normalized)?;
    let left = input[..index].trim();
    let right = input[index + normalized.len()..].trim();
    (!left.is_empty() && !right.is_empty()).then_some((left, right))
}

fn split_top_level(input: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut quote: Option<char> = None;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;

    for (index, ch) in input.char_indices() {
        match quote {
            Some(active) if ch == active => {
                quote = None;
            }
            Some(_) => {}
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch == '(' => paren_depth = paren_depth.saturating_add(1),
            None if ch == ')' => paren_depth = paren_depth.saturating_sub(1),
            None if ch == '[' => bracket_depth = bracket_depth.saturating_add(1),
            None if ch == ']' => bracket_depth = bracket_depth.saturating_sub(1),
            None if ch == delimiter && paren_depth == 0 && bracket_depth == 0 => {
                parts.push(input[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }

    parts.push(input[start..].trim());
    parts
}

fn split_top_level_once(input: &str, delimiter: char) -> Option<(&str, &str)> {
    let mut quote: Option<char> = None;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;

    for (index, ch) in input.char_indices() {
        match quote {
            Some(active) if ch == active => {
                quote = None;
            }
            Some(_) => {}
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch == '(' => paren_depth = paren_depth.saturating_add(1),
            None if ch == ')' => paren_depth = paren_depth.saturating_sub(1),
            None if ch == '[' => bracket_depth = bracket_depth.saturating_add(1),
            None if ch == ']' => bracket_depth = bracket_depth.saturating_sub(1),
            None if ch == delimiter && paren_depth == 0 && bracket_depth == 0 => {
                return Some((&input[..index], &input[index + ch.len_utf8()..]));
            }
            _ => {}
        }
    }

    None
}

fn find_top_level_substring(input: &str, needle: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;

    for (index, ch) in input.char_indices() {
        match quote {
            Some(active) => {
                if ch == active {
                    quote = None;
                }
                continue;
            }
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                continue;
            }
            None if ch == '(' => {
                paren_depth = paren_depth.saturating_add(1);
                continue;
            }
            None if ch == ')' => {
                paren_depth = paren_depth.saturating_sub(1);
                continue;
            }
            None if ch == '[' => {
                bracket_depth = bracket_depth.saturating_add(1);
                continue;
            }
            None if ch == ']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }

        if quote.is_none()
            && paren_depth == 0
            && bracket_depth == 0
            && input[index..].starts_with(needle)
        {
            return Some(index);
        }
    }

    None
}

fn find_matching_pair(input: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;

    for (index, ch) in input.char_indices() {
        match quote {
            Some(active) if ch == active => {
                quote = None;
            }
            Some(_) => {}
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch == open => depth = depth.saturating_add(1),
            None if ch == close => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

fn is_safe_cmdlet(name: &str) -> bool {
    matches!(
        name,
        "pwd"
            | "ls"
            | "dir"
            | "cat"
            | "type"
            | "head"
            | "tail"
            | "find"
            | "tree"
            | "rg"
            | "grep"
            | "fd"
            | "findstr"
            | "select-string"
            | "get-childitem"
            | "get-content"
            | "measure-object"
            | "where-object"
            | "sort-object"
            | "select-object"
    )
}

fn is_safe_literal(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && (trimmed.starts_with('"')
            || trimmed.starts_with('\'')
            || trimmed.chars().all(|ch| {
                ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '\\' | '/')
            }))
}

fn contains_powershell_redirection(value: &str) -> bool {
    value.contains('>') || value.contains(">>")
}

fn contains_powershell_call_operator(value: &str) -> bool {
    value.trim() == "&" || value.contains("& ")
}

fn contains_executable_expression(value: &str) -> bool {
    let normalized = value.trim();
    normalized.contains("invoke-expression")
        || normalized.contains("start-process")
        || normalized.contains("new-object")
}

#[cfg(test)]
mod tests {
    use super::{PowerShellAccess, classify_segment};

    #[test]
    fn classifies_readonly_segments() {
        let assignment_segment = "$lines = Get-Content packages/core/src/agents/runtime/agent-core.ts; $lines[630..730] -join \"`n\""
            .split(';')
            .next()
            .expect("assignment segment");
        assert_eq!(
            classify_segment(assignment_segment),
            PowerShellAccess::ReadOnly
        );
        assert_eq!(
            classify_segment("$lines[630..730] -join \"`n\""),
            PowerShellAccess::ReadOnly
        );
        assert_eq!(
            classify_segment("(Get-Content \"D:\\x\").Count"),
            PowerShellAccess::ReadOnly
        );
        assert_eq!(
            classify_segment("(Get-Content \"D:\\x\" | Measure-Object -Line).Lines"),
            PowerShellAccess::ReadOnly
        );
        assert_eq!(
            classify_segment("(Get-ChildItem \"D:\\x\" -File).Count"),
            PowerShellAccess::ReadOnly
        );
        assert_eq!(
            classify_segment("$content = Get-Content \"D:\\x\" -Raw"),
            PowerShellAccess::ReadOnly
        );
        assert_eq!(
            classify_segment("$content.Length"),
            PowerShellAccess::ReadOnly
        );
    }

    #[test]
    fn rejects_mutating_or_unsupported_segments() {
        assert_eq!(
            classify_segment("Set-Content out.txt hi"),
            PowerShellAccess::Unknown
        );
        assert_eq!(
            classify_segment("(Invoke-Expression \"Get-Content x\").Count"),
            PowerShellAccess::Unknown
        );
        assert_eq!(
            classify_segment(
                "$text = Get-Content file -Raw; Set-Content file $text"
                    .split(';')
                    .nth(1)
                    .expect("mutating segment")
            ),
            PowerShellAccess::Unknown
        );
    }
}
