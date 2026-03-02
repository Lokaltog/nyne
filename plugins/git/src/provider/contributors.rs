//! Contributors view — authors ranked by commit count.

git_template_view!(ContributorsView, |repo, path| repo.contributors(path));
