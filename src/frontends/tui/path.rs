use std::path::{Component, Path};

use unicode_width::UnicodeWidthStr;

const ELLIPSIS: &str = "\u{2026}"; // …

/// Splits a path into joinable parts. The first part is `~` when the path lives
/// under `home`, an empty string for an absolute path (so joining with `/` yields
/// the leading slash), or the first component of a relative path.
fn to_parts(path: &Path, home: Option<&Path>) -> Vec<String> {
    let relative_to_home = home
        .filter(|home| !home.as_os_str().is_empty())
        .and_then(|home| path.strip_prefix(home).ok());

    let mut parts: Vec<String> = Vec::new();
    match relative_to_home {
        Some(rest) => {
            parts.push("~".to_string());
            parts.extend(components(rest));
        }
        None => {
            if path.is_absolute() {
                parts.push(String::new());
            }
            parts.extend(components(path));
        }
    }

    if parts.is_empty() {
        parts.push(path.to_string_lossy().into_owned());
    }
    // The filesystem root has no components of its own.
    if path.is_absolute() && parts.len() == 1 && parts[0].is_empty() {
        parts[0] = "/".to_string();
    }
    parts
}

/// Returns the named components of a path, dropping the root and `.` segments.
fn components(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            Component::ParentDir => Some("..".to_string()),
            Component::CurDir | Component::RootDir | Component::Prefix(_) => None,
        })
        .collect()
}

/// Collapses a component to its first character, fish style. Dotfiles keep the
/// dot plus the following character, so `.config` becomes `.c`.
fn initial(part: &str) -> String {
    let mut chars = part.chars();
    match chars.next() {
        Some('.') => match chars.next() {
            Some(next) => format!(".{next}"),
            None => ".".to_string(),
        },
        Some(first) => first.to_string(),
        None => String::new(),
    }
}

/// Collapses every ancestor to its initial, keeping the last component whole.
fn abbreviate(parts: &[String]) -> Vec<String> {
    let last = parts.len() - 1;
    parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            if index == last || part == "~" || part == ".." || part == "/" || part.is_empty() {
                part.clone()
            } else {
                initial(part)
            }
        })
        .collect()
}

fn join(parts: &[String]) -> String {
    match parts {
        [only] => only.clone(),
        _ => parts.join("/"),
    }
}

fn fits(text: &str, max_cols: usize) -> bool {
    text.width() <= max_cols
}

fn shorten_path_with_home(path: &Path, home: Option<&Path>, max_cols: usize) -> String {
    if max_cols == 0 {
        return String::new();
    }

    let full = path.to_string_lossy().into_owned();
    if fits(&full, max_cols) {
        return full;
    }

    let parts = to_parts(path, home);
    let with_tilde = join(&parts);
    if fits(&with_tilde, max_cols) {
        return with_tilde;
    }

    let abbreviated = abbreviate(&parts);
    let short = join(&abbreviated);
    if fits(&short, max_cols) {
        return short;
    }

    let last = abbreviated.len() - 1;
    for kept in (0..last).rev() {
        let mut candidate: Vec<String> = abbreviated[..kept].to_vec();
        candidate.push(ELLIPSIS.to_string());
        candidate.push(abbreviated[last].clone());
        let text = join(&candidate);
        if fits(&text, max_cols) {
            return text;
        }
    }

    abbreviated[last].clone()
}

/// Shortens a directory path to at most `max_cols` display columns, trying, in
/// order: the full path, `~` for the home directory, fish-style abbreviated
/// ancestors, and finally a middle ellipsis. The last component is never cut.
pub fn shorten_path(path: &Path, max_cols: usize) -> String {
    shorten_path_with_home(path, dirs::home_dir().as_deref(), max_cols)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const HOME: &str = "/home/user";

    fn shorten(path: &str, max_cols: usize) -> String {
        shorten_path_with_home(Path::new(path), Some(Path::new(HOME)), max_cols)
    }

    #[test]
    fn test_full_path_when_it_fits() {
        assert_eq!(shorten("/etc/nginx", 20), "/etc/nginx");
    }

    #[test]
    fn test_full_path_at_exact_width() {
        assert_eq!(shorten("/etc/nginx", 10), "/etc/nginx");
    }

    #[test]
    fn test_tilde_substitution() {
        assert_eq!(
            shorten("/home/user/Development/Rust/agents-on-tmux", 35),
            "~/Development/Rust/agents-on-tmux"
        );
    }

    #[test]
    fn test_tilde_for_home_itself() {
        assert_eq!(shorten("/home/user", 3), "~");
    }

    #[test]
    fn test_fish_style_abbreviation() {
        assert_eq!(
            shorten("/home/user/Development/Rust/agents-on-tmux", 25),
            "~/D/R/agents-on-tmux"
        );
    }

    #[test]
    fn test_fish_style_abbreviation_of_absolute_path() {
        assert_eq!(shorten("/var/lib/docker/volumes", 20), "/v/l/d/volumes");
    }

    #[test]
    fn test_middle_ellipsis_keeps_the_head() {
        assert_eq!(
            shorten("/home/user/Development/Rust/Projects/agents-on-tmux", 20),
            "~/D/\u{2026}/agents-on-tmux"
        );
    }

    #[test]
    fn test_middle_ellipsis_drops_more_ancestors_when_needed() {
        assert_eq!(
            shorten("/home/user/a/b/c/d/e/agents-on-tmux", 20),
            "~/a/\u{2026}/agents-on-tmux"
        );
        assert_eq!(
            shorten("/home/user/a/b/c/d/e/agents-on-tmux", 18),
            "~/\u{2026}/agents-on-tmux"
        );
        assert_eq!(
            shorten("/home/user/a/b/c/d/e/agents-on-tmux", 16),
            "\u{2026}/agents-on-tmux"
        );
    }

    #[test]
    fn test_last_component_is_never_truncated() {
        assert_eq!(
            shorten("/home/user/Development/agents-on-tmux", 5),
            "agents-on-tmux"
        );
    }

    #[test]
    fn test_root() {
        assert_eq!(shorten("/", 10), "/");
        assert_eq!(shorten("/", 1), "/");
    }

    #[test]
    fn test_zero_width() {
        assert_eq!(shorten("/home/user/project", 0), "");
    }

    #[test]
    fn test_short_path_is_untouched() {
        assert_eq!(shorten("/tmp", 4), "/tmp");
        assert_eq!(shorten("/tmp", 3), "tmp");
    }

    #[test]
    fn test_relative_path() {
        assert_eq!(shorten("src/frontends/tui", 17), "src/frontends/tui");
        assert_eq!(shorten("src/frontends/tui", 10), "s/f/tui");
    }

    #[test]
    fn test_empty_path() {
        assert_eq!(shorten("", 10), "");
    }

    #[test]
    fn test_dotfile_ancestors_keep_the_dot() {
        assert_eq!(
            shorten("/home/user/.config/nvim/lua/plugins", 20),
            "~/.c/n/l/plugins"
        );
    }

    #[test]
    fn test_parent_dir_components_are_kept() {
        assert_eq!(shorten("../sibling/project", 18), "../sibling/project");
        assert_eq!(shorten("../sibling/project", 12), "../s/project");
    }

    #[test]
    fn test_without_home_no_tilde_substitution() {
        let path = PathBuf::from("/home/user/Development/agents-on-tmux");
        assert_eq!(
            shorten_path_with_home(&path, None, 25),
            "/h/u/D/agents-on-tmux"
        );
    }

    #[test]
    fn test_empty_home_is_ignored() {
        let path = PathBuf::from("/home/user/project");
        assert_eq!(
            shorten_path_with_home(&path, Some(Path::new("")), 18),
            "/home/user/project"
        );
    }

    #[test]
    fn test_wide_characters_are_measured_by_display_width() {
        let path = "/home/user/\u{6587}\u{4ef6}";
        assert_eq!(shorten(path, 15), path);
        assert_eq!(shorten(path, 14), "~/\u{6587}\u{4ef6}");
    }

    #[test]
    fn test_wide_characters_abbreviate_by_first_character() {
        let path = "/home/user/\u{30d7}\u{30ed}\u{30b8}\u{30a7}\u{30af}\u{30c8}/src";
        assert_eq!(shorten(path, 10), "~/\u{30d7}/src");
    }

    #[test]
    fn test_shorten_path_uses_the_real_home() {
        assert_eq!(shorten_path(Path::new("/tmp"), 40), "/tmp");
    }
}
