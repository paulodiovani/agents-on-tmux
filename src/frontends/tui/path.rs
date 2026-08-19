use std::path::{Component, Path};

use unicode_width::UnicodeWidthStr;

const ELLIPSIS: &str = "\u{2026}"; // …
const MIN_TRUNCATED_WIDTH: usize = 3;

/// Splits a path into joinable parts, the first being `~` under `home` or an empty
/// string when absolute, so that joining with `/` restores the leading slash.
fn to_parts(path: &Path, home: Option<&Path>) -> Vec<String> {
    let under_home = home
        .filter(|home| !home.as_os_str().is_empty())
        .and_then(|home| path.strip_prefix(home).ok());

    let root = match (under_home.is_some(), path.is_absolute()) {
        (true, _) => Some("~".to_string()),
        (false, true) => Some(String::new()),
        (false, false) => None,
    };
    let named = under_home
        .unwrap_or(path)
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            Component::ParentDir => Some("..".to_string()),
            Component::CurDir | Component::RootDir | Component::Prefix(_) => None,
        });

    let mut parts: Vec<String> = root.into_iter().chain(named).collect();
    if parts.is_empty() {
        parts.push(path.to_string_lossy().into_owned());
    }
    // The filesystem root has no components of its own.
    if path.is_absolute() && parts == [""] {
        parts[0] = "/".to_string();
    }
    parts
}

/// Collapses a component to its first character, fish style. Dotfiles keep the
/// dot plus the following character, so `.config` becomes `.c`.
fn initial(part: &str) -> String {
    let keep = if part.starts_with('.') { 2 } else { 1 };
    part.chars().take(keep).collect()
}

/// Collapses every ancestor to its initial, keeping the last component whole.
fn abbreviate(parts: &[String]) -> Vec<String> {
    let leaf = parts.len() - 1;
    parts
        .iter()
        .enumerate()
        .map(|(index, part)| match index {
            index if index == leaf => part.clone(),
            _ => initial(part),
        })
        .collect()
}

/// The path written progressively shorter, widest first, down to the last component
/// on its own. Never empty, and the last component is never cut.
fn candidates(path: &Path, home: Option<&Path>) -> Vec<String> {
    let parts = to_parts(path, home);
    let abbreviated = abbreviate(&parts);
    let leaf = abbreviated.len() - 1;

    let mut candidates = vec![
        path.to_string_lossy().into_owned(),
        parts.join("/"),
        abbreviated.join("/"),
    ];
    candidates.extend((0..leaf).rev().map(|kept| {
        let mut shortened = abbreviated[..kept].to_vec();
        shortened.push(ELLIPSIS.to_string());
        shortened.push(abbreviated[leaf].clone());
        shortened.join("/")
    }));
    let full_leaf = &parts[parts.len() - 1];
    candidates.push(full_leaf.clone());
    let chars: Vec<char> = full_leaf.chars().collect();
    for suffix_len in (1..chars.len()).rev() {
        let suffix: String = chars[chars.len() - suffix_len..].iter().collect();
        let truncated = format!("..{}", suffix);
        if truncated.width() > MIN_TRUNCATED_WIDTH {
            candidates.push(truncated);
        }
    }
    let initial_char = initial(full_leaf);
    candidates.push(initial_char.to_uppercase());
    candidates
}

fn shorten_path_with_home(path: &Path, home: Option<&Path>, max_cols: usize) -> String {
    if max_cols == 0 {
        return String::new();
    }

    let candidates = candidates(path, home);
    candidates
        .iter()
        .find(|candidate| candidate.width() <= max_cols)
        .or_else(|| candidates.last())
        .cloned()
        .unwrap_or_default()
}

/// Shortens a directory path to at most `max_cols` display columns: `~`, then
/// fish-style ancestors, then a middle ellipsis. The last component is never cut.
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
    fn test_last_component_truncated_from_left_when_too_long() {
        assert_eq!(shorten("/home/user/Development/agents-on-tmux", 5), "..mux");
    }

    #[test]
    fn test_left_truncation_progression() {
        assert_eq!(shorten("/home/user/foobar", 6), "foobar");
        assert_eq!(shorten("/home/user/foobar", 5), "..bar");
        assert_eq!(shorten("/home/user/foobar", 4), "..ar");
        assert_eq!(shorten("/home/user/foobar", 1), "F");
    }

    #[test]
    fn test_abbreviated_leaf_fallback() {
        assert_eq!(shorten("/home/user/foobar", 2), "F");
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
