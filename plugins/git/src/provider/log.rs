//! Git log view — commit history with metadata.

use super::history::HistoryQueries as _;

/// Default cap on log entries (metadata only, so higher than history).
pub(super) const LOG_LIMIT: usize = 200;

git_template_view!(LogView, |repo, path| repo.file_history(path, LOG_LIMIT));
