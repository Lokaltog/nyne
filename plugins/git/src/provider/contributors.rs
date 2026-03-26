//! Contributors view — authors ranked by commit count.

use super::history::HistoryQueries as _;

git_template_view!(ContributorsView, |repo, path| repo.contributors(path));
