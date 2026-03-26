//! Blame view — git author and timestamp for each line.

use super::history::HistoryQueries as _;

git_template_view!(BlameView, |repo, path| repo.blame(path));
