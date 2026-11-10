//! Where a shell pane's working directory sits relative to the project (or
//! worktree) it belongs to, so a title shown *inside* that project doesn't
//! repeat the project's own path.

use std::path::{Path, PathBuf};

/// A working directory's position under a root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShellPlace {
    /// The directory is the root itself.
    Root,
    /// The directory is below the root; the string is `./relative/path`.
    Inside(String),
}

/// Reads `title` as a directory (absolute, or `~`-relative with `home`) and
/// places it under `root`. `None` when the title isn't a directory or lies
/// outside the root — such a title should be shown as it is.
pub fn shell_place(title: &str, root: &Path, home: Option<&Path>) -> Option<ShellPlace> {
    let title = title.trim();
    let path: PathBuf = if title == "~" {
        home?.to_path_buf()
    } else if let Some(rest) = title.strip_prefix("~/") {
        home?.join(rest)
    } else if title.starts_with('/') {
        PathBuf::from(title)
    } else {
        return None;
    };
    let rel = path.strip_prefix(root).ok()?;
    if rel.as_os_str().is_empty() {
        Some(ShellPlace::Root)
    } else {
        Some(ShellPlace::Inside(format!("./{}", rel.display())))
    }
}

#[cfg(test)]
mod tests {
    use super::{ShellPlace, shell_place};
    use std::path::Path;

    const HOME: &str = "/home/dev";

    fn place(title: &str, root: &str) -> Option<ShellPlace> {
        shell_place(title, Path::new(root), Some(Path::new(HOME)))
    }

    #[test]
    fn the_project_root_is_root_however_it_is_spelled() {
        assert_eq!(
            place("~/code/app", "/home/dev/code/app"),
            Some(ShellPlace::Root)
        );
        assert_eq!(
            place("/home/dev/code/app", "/home/dev/code/app"),
            Some(ShellPlace::Root)
        );
        // A prompt's `user@host: dir` leaves a leading space; roots may end in `/`.
        assert_eq!(
            place(" ~/code/app/", "/home/dev/code/app/"),
            Some(ShellPlace::Root)
        );
        assert_eq!(place("~", HOME), Some(ShellPlace::Root));
    }

    #[test]
    fn a_subfolder_is_shown_relative_to_the_root() {
        assert_eq!(
            place("~/code/app/server/src", "/home/dev/code/app"),
            Some(ShellPlace::Inside("./server/src".into()))
        );
    }

    #[test]
    fn directories_outside_the_root_and_non_paths_are_left_alone() {
        assert_eq!(place("~/code/other", "/home/dev/code/app"), None);
        // A sibling that merely shares a name prefix is not inside.
        assert_eq!(place("~/code/app-old", "/home/dev/code/app"), None);
        assert_eq!(place("vim notes.md", "/home/dev/code/app"), None);
        assert_eq!(place("Fix the /login route", "/home/dev/code/app"), None);
    }

    #[test]
    fn a_tilde_title_without_a_known_home_is_left_alone() {
        assert_eq!(
            shell_place("~/code/app", Path::new("/home/dev/code/app"), None),
            None
        );
    }
}
