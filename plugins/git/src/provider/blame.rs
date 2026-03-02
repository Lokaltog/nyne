//! Blame view — git author and timestamp for each line.

git_template_view!(BlameView, |repo, path| repo.blame(path));
