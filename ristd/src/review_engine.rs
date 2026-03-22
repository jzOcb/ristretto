//! Cross-model review request building and response parsing.

use std::path::PathBuf;

use rist_shared::{AgentType, SessionId};

/// Maps source agents to preferred reviewer agents and parses structured review output.
#[derive(Debug, Clone)]
pub struct ReviewEngine {
    /// Maps agent types to their review counterparts.
    review_pairs: Vec<(AgentType, AgentType)>,
}

/// Input for generating a review prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRequest {
    /// Session that produced the changes being reviewed.
    pub source_agent: SessionId,
    /// Agent family that produced the changes.
    pub source_type: AgentType,
    /// Diff under review.
    pub diff: String,
    /// Human-readable task summary.
    pub task_description: String,
    /// Files touched by the change.
    pub file_list: Vec<PathBuf>,
}

/// Parsed result returned by a reviewer agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewResult {
    /// Reviewer agent family, when known.
    pub reviewer_type: AgentType,
    /// Overall review verdict.
    pub verdict: ReviewVerdict,
    /// Structured comments extracted from the review text.
    pub comments: Vec<ReviewComment>,
    /// Any suggested fix actions that were called out explicitly.
    pub suggested_fixes: Vec<String>,
}

/// High-level review disposition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewVerdict {
    /// The changes are acceptable as-is.
    Approved,
    /// The changes need fixes before merging.
    RequestChanges,
    /// The changes need discussion before proceeding.
    NeedsDiscussion,
}

/// Structured comment extracted from reviewer output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewComment {
    /// File referenced by the comment.
    pub file: PathBuf,
    /// Optional line reference in the file.
    pub line: Option<usize>,
    /// Severity level for the comment.
    pub severity: CommentSeverity,
    /// Human-readable message.
    pub message: String,
}

/// Severity assigned to a review comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentSeverity {
    /// Defect or must-fix issue.
    Error,
    /// Important but not necessarily blocking issue.
    Warning,
    /// Informational note.
    Info,
    /// Minor polish suggestion.
    Nit,
}

impl ReviewEngine {
    /// Creates a review engine with the default cross-model pairings.
    #[must_use]
    pub fn new() -> Self {
        let review_pairs = vec![
            (AgentType::Claude, AgentType::Codex),
            (AgentType::Codex, AgentType::Claude),
            (AgentType::Gemini, AgentType::Claude),
        ];
        Self { review_pairs }
    }

    /// Returns the default reviewer agent type for `source_type`.
    #[must_use]
    pub fn reviewer_for(&self, source_type: &AgentType) -> AgentType {
        self.review_pairs
            .iter()
            .find_map(|(source, reviewer)| (source == source_type).then(|| reviewer.clone()))
            .unwrap_or(AgentType::Claude)
    }

    /// Generates a structured review prompt from `request`.
    #[must_use]
    pub fn build_review_prompt(&self, request: &ReviewRequest) -> String {
        let reviewer = self.reviewer_for(&request.source_type);
        let files = if request.file_list.is_empty() {
            "- No file list provided".to_owned()
        } else {
            request
                .file_list
                .iter()
                .map(|path| format!("- {}", path.display()))
                .collect::<Vec<_>>()
                .join("\n")
        };

        format!(
            "You are performing an adversarial cross-model review.\n\
Reviewer type: {:?}\n\
Source agent: {:?} ({})\n\
Task: {}\n\n\
Files in scope:\n{}\n\n\
Check the change for:\n\
- Correctness: logic errors, edge cases\n\
- Performance: unnecessary allocations, O(n^2) work that could be O(n)\n\
- Security: input validation, unsafe operations\n\
- Style: naming, documentation, error handling\n\
- Architecture: separation of concerns, API design\n\n\
Reply using this format:\n\
VERDICT: APPROVED | CHANGES REQUESTED | NEEDS DISCUSSION\n\
ERROR|WARNING|INFO|NIT: <message> in <file>:<line>\n\
SUGGESTED FIX: <fix>\n\n\
Diff:\n{}",
            reviewer,
            request.source_type,
            request.source_agent.0,
            request.task_description,
            files,
            request.diff
        )
    }

    /// Parses reviewer output into a structured review result.
    #[must_use]
    pub fn parse_review_output(&self, output: &str) -> ReviewResult {
        let reviewer_type = parse_reviewer_type(output).unwrap_or(AgentType::Unknown);
        let verdict = parse_verdict(output);
        let comments = output
            .lines()
            .filter_map(parse_comment_line)
            .collect::<Vec<_>>();
        let suggested_fixes = output
            .lines()
            .filter_map(|line| {
                line.split_once(':').and_then(|(prefix, rest)| {
                    prefix
                        .trim()
                        .eq_ignore_ascii_case("suggested fix")
                        .then(|| rest.trim().to_owned())
                })
            })
            .filter(|line| !line.is_empty())
            .collect();

        ReviewResult {
            reviewer_type,
            verdict,
            comments,
            suggested_fixes,
        }
    }

    /// Returns true when the review result blocks merging.
    #[must_use]
    pub fn needs_changes(&self, result: &ReviewResult) -> bool {
        result.verdict == ReviewVerdict::RequestChanges
            || result
                .comments
                .iter()
                .any(|comment| comment.severity == CommentSeverity::Error)
    }
}

impl Default for ReviewEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_reviewer_type(output: &str) -> Option<AgentType> {
    output.lines().find_map(|line| {
        let (prefix, value) = line.split_once(':')?;
        if !prefix.trim().eq_ignore_ascii_case("reviewer type") {
            return None;
        }
        match value.trim().to_ascii_lowercase().as_str() {
            "claude" => Some(AgentType::Claude),
            "codex" => Some(AgentType::Codex),
            "gemini" => Some(AgentType::Gemini),
            custom if !custom.is_empty() => Some(AgentType::Custom(custom.to_owned())),
            _ => None,
        }
    })
}

fn parse_verdict(output: &str) -> ReviewVerdict {
    output
        .lines()
        .find_map(|line| {
            let (prefix, value) = line.split_once(':')?;
            if !prefix.trim().eq_ignore_ascii_case("verdict") {
                return None;
            }
            match value.trim().to_ascii_uppercase().as_str() {
                "APPROVED" => Some(ReviewVerdict::Approved),
                "CHANGES REQUESTED" => Some(ReviewVerdict::RequestChanges),
                "NEEDS DISCUSSION" => Some(ReviewVerdict::NeedsDiscussion),
                _ => Some(ReviewVerdict::RequestChanges),
            }
        })
        .unwrap_or(ReviewVerdict::RequestChanges)
}

fn parse_comment_line(line: &str) -> Option<ReviewComment> {
    let (severity, remainder) = line.split_once(':')?;
    let severity = match severity.trim().to_ascii_uppercase().as_str() {
        "ERROR" => CommentSeverity::Error,
        "WARNING" => CommentSeverity::Warning,
        "INFO" => CommentSeverity::Info,
        "NIT" => CommentSeverity::Nit,
        _ => return None,
    };

    let (message, location) = remainder
        .rsplit_once(" in ")
        .map_or((remainder.trim(), None), |(message, location)| {
            (message.trim(), Some(location.trim()))
        });
    let (file, line) = location
        .and_then(parse_file_reference)
        .unwrap_or_else(|| (PathBuf::from("<unknown>"), None));

    Some(ReviewComment {
        file,
        line,
        severity,
        message: message.to_owned(),
    })
}

fn parse_file_reference(location: &str) -> Option<(PathBuf, Option<usize>)> {
    let location = location
        .strip_prefix("file ")
        .map_or(location, str::trim)
        .trim();
    if let Some((file, line)) = location.rsplit_once(':') {
        let line = line.parse::<usize>().ok();
        return Some((PathBuf::from(file.trim()), line));
    }
    Some((PathBuf::from(location), None))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rist_shared::{AgentType, SessionId};

    use super::{CommentSeverity, ReviewEngine, ReviewRequest, ReviewVerdict};

    #[test]
    fn reviewer_for_returns_cross_model_pairs() {
        let engine = ReviewEngine::new();

        assert_eq!(engine.reviewer_for(&AgentType::Claude), AgentType::Codex);
        assert_eq!(engine.reviewer_for(&AgentType::Codex), AgentType::Claude);
        assert_eq!(engine.reviewer_for(&AgentType::Gemini), AgentType::Claude);
    }

    #[test]
    fn build_review_prompt_includes_review_categories() {
        let engine = ReviewEngine::new();
        let request = ReviewRequest {
            source_agent: SessionId::new(),
            source_type: AgentType::Claude,
            diff: "diff --git a/src/lib.rs b/src/lib.rs".to_owned(),
            task_description: "Implement review engine".to_owned(),
            file_list: vec![PathBuf::from("src/lib.rs")],
        };

        let prompt = engine.build_review_prompt(&request);

        assert!(prompt.contains("Correctness"));
        assert!(prompt.contains("Performance"));
        assert!(prompt.contains("Security"));
        assert!(prompt.contains("Style"));
        assert!(prompt.contains("Architecture"));
    }

    #[test]
    fn parse_review_output_extracts_verdict_and_comments() {
        let engine = ReviewEngine::new();
        let output = "\
Reviewer Type: codex
VERDICT: CHANGES REQUESTED
ERROR: missing retry reset in src/recovery.rs:42
WARNING: this clones too much in file src/review.rs:11
SUGGESTED FIX: reset the retry counter after successful output";

        let result = engine.parse_review_output(output);

        assert_eq!(result.reviewer_type, AgentType::Codex);
        assert_eq!(result.verdict, ReviewVerdict::RequestChanges);
        assert_eq!(result.comments.len(), 2);
        assert_eq!(result.comments[0].severity, CommentSeverity::Error);
        assert_eq!(result.comments[0].line, Some(42));
        assert_eq!(result.suggested_fixes.len(), 1);
    }

    #[test]
    fn needs_changes_treats_nits_as_non_blocking() {
        let engine = ReviewEngine::new();
        let nit_only = engine.parse_review_output("VERDICT: APPROVED\nNIT: rename helper");
        let blocking = engine.parse_review_output("VERDICT: APPROVED\nERROR: broken edge case");

        assert!(!engine.needs_changes(&nit_only));
        assert!(engine.needs_changes(&blocking));
    }

    #[test]
    fn parse_review_output_defaults_garbled_verdict_to_changes_requested() {
        let engine = ReviewEngine::new();

        let missing = engine.parse_review_output("ERROR: broken edge case");
        let garbled = engine.parse_review_output("VERDICT: shrug");

        assert_eq!(missing.verdict, ReviewVerdict::RequestChanges);
        assert_eq!(garbled.verdict, ReviewVerdict::RequestChanges);
    }
}
