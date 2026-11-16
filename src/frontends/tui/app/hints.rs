use crate::backends::logger;
use crate::backends::tmux::Tmux;

/// The nested-session commands worth advertising, with their footer labels.
const HINTED_COMMANDS: [(&str, &str); 5] = [
    ("new-window -c \"#{pane_current_path}\"", "new"),
    ("next-window", "next"),
    ("previous-window", "prev"),
    ("last-window", "last"),
    (
        "command-prompt -I \"#W\" { rename-window \"%%\" }",
        "rename",
    ),
];

/// Resolves the nested-session key sequences to advertise, from the nested
/// bindings and both sessions' prefixes: `<prefix> <parent send-prefix>`
/// when the prefixes collide (the parent eats the first press), otherwise
/// just the nested prefix. Empty when anything cannot be resolved, so the
/// footer falls back to the TUI's own keybindings.
pub fn resolve_tmux_hints(nested: &dyn Tmux, parent: &dyn Tmux) -> Vec<(String, String)> {
    let Some(nested_prefix) = effective_prefix(nested) else {
        return Vec::new();
    };
    let Some(parent_prefix) = effective_prefix(parent) else {
        return Vec::new();
    };
    let keys = match nested.list_keys("prefix") {
        Ok(keys) => keys,
        Err(error) => {
            logger::debug!("app: list-keys nested failed: {error}");
            return Vec::new();
        }
    };

    let prefix_entry = if nested_prefix == parent_prefix {
        let parent_keys = match parent.list_keys("prefix") {
            Ok(keys) => keys,
            Err(error) => {
                logger::debug!("app: list-keys parent failed: {error}");
                return Vec::new();
            }
        };
        let send_prefix = parent_keys
            .iter()
            .find(|b| b.command == "send-prefix")
            .map(|b| b.key.clone())
            .unwrap_or_else(|| parent_prefix.clone());
        format!("{nested_prefix} {send_prefix}")
    } else {
        nested_prefix.clone()
    };

    let binding_for = |command: &str| keys.iter().find(|b| b.command == command).map(|b| &b.key);
    let mut hints = vec![(prefix_entry, "prefix".to_string())];
    hints.extend(HINTED_COMMANDS.iter().filter_map(|(command, label)| {
        Some((binding_for(command)?.clone(), (*label).to_string()))
    }));
    hints
}

/// Effective prefix of a driver's session: its own override, else the
/// server-wide default.
fn effective_prefix(driver: &dyn Tmux) -> Option<String> {
    prefix_option(driver, false).or_else(|| prefix_option(driver, true))
}

/// The `prefix` option of the session or, when `global`, of the server; `None`
/// when unset, blank or unreadable.
fn prefix_option(driver: &dyn Tmux, global: bool) -> Option<String> {
    match driver.show_options("prefix", global) {
        Ok(value) if !value.is_empty() => Some(value),
        Ok(_) => None,
        Err(error) => {
            let scope = if global { "global" } else { "session" };
            logger::debug!("app: show-options prefix ({scope}) failed: {error}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{new_app, session, test_app};
    use crate::backends::tmux::KeyBinding;
    use crate::frontends::tui::app::App;
    use crate::frontends::tui::fake_tmux::{FakeTmux, default_key_bindings};

    fn hints_app(nested: FakeTmux, parent: FakeTmux) -> App {
        new_app(&nested, &parent, Some("%7"), None)
    }

    #[test]
    fn test_tmux_hints_resolved_when_tracking() {
        let app = hints_app(session(), session());
        assert_eq!(
            app.tmux_hints(),
            [
                ("C-b C-b".to_string(), "prefix".to_string()),
                ("c".to_string(), "new".to_string()),
                ("n".to_string(), "next".to_string()),
                ("p".to_string(), "prev".to_string()),
                ("l".to_string(), "last".to_string()),
                (",".to_string(), "rename".to_string()),
            ]
        );
    }

    #[test]
    fn test_tmux_hints_empty_without_pane_id() {
        let (app, _) = test_app();
        assert!(app.tmux_hints().is_empty());
    }

    #[test]
    fn test_tmux_hints_skip_unbound_commands() {
        let mut nested = session();
        nested.keys = Some(
            default_key_bindings()
                .into_iter()
                .filter(|b| b.command != "last-window")
                .collect(),
        );
        let app = hints_app(nested, session());
        assert_eq!(app.tmux_hints().len(), 5);
        assert!(!app.tmux_hints().iter().any(|(_, label)| label == "last"));
    }

    #[test]
    fn test_tmux_hints_send_prefix_falls_back_to_prefix() {
        let mut nested = session();
        nested.prefix = Some("C-a".to_string());
        let mut parent = session();
        parent.prefix = Some("C-a".to_string());
        parent.keys = Some(
            default_key_bindings()
                .into_iter()
                .filter(|b| b.command != "send-prefix")
                .collect(),
        );
        let app = hints_app(nested, parent);
        assert_eq!(
            app.tmux_hints()[0],
            ("C-a C-a".to_string(), "prefix".to_string())
        );
        assert!(
            app.tmux_hints()
                .iter()
                .any(|(key, label)| key == "c" && label == "new")
        );
    }

    #[test]
    fn test_tmux_hints_use_bound_send_prefix_key() {
        let mut parent = session();
        let mut keys = default_key_bindings();
        keys[0].key = "C-Space".to_string();
        parent.keys = Some(keys);
        let app = hints_app(session(), parent);
        assert_eq!(
            app.tmux_hints()[0],
            ("C-b C-Space".to_string(), "prefix".to_string())
        );
    }

    #[test]
    fn test_tmux_hints_bare_nested_prefix_when_prefixes_differ() {
        let mut parent = session();
        parent.prefix = Some("C-a".to_string());
        let app = hints_app(session(), parent);
        assert_eq!(
            app.tmux_hints()[0],
            ("C-b".to_string(), "prefix".to_string())
        );
        assert_eq!(app.tmux_hints().len(), 6);
    }

    #[test]
    fn test_tmux_hints_match_command_prompt_with_rename() {
        let app = hints_app(session(), session());
        assert!(
            app.tmux_hints()
                .iter()
                .any(|(key, label)| key == "," && label == "rename")
        );
    }

    #[test]
    fn test_tmux_hints_do_not_match_menu_commands() {
        let mut nested = session();
        let mut keys = default_key_bindings();
        keys.push(KeyBinding {
            key: "<".to_string(),
            command: "display-menu -T \"Window menu\" #{window_index} rename-window".to_string(),
        });
        nested.keys = Some(keys);
        let app = hints_app(nested, session());
        let rename_hint = app.tmux_hints().iter().find(|(_, label)| label == "rename");
        assert!(rename_hint.is_some());
        assert_ne!(rename_hint.unwrap().0, "<");
    }

    #[test]
    fn test_tmux_hints_empty_when_nested_list_keys_fails() {
        let mut nested = session();
        nested.keys = None;
        let app = hints_app(nested, session());
        assert!(app.tmux_hints().is_empty());
    }

    #[test]
    fn test_tmux_hints_empty_when_parent_list_keys_fails() {
        let mut parent = session();
        parent.keys = None;
        let app = hints_app(session(), parent);
        assert!(app.tmux_hints().is_empty());
    }

    #[test]
    fn test_tmux_hints_empty_when_nested_prefix_fails() {
        let mut nested = session();
        nested.prefix = None;
        let app = hints_app(nested, session());
        assert!(app.tmux_hints().is_empty());
    }

    #[test]
    fn test_tmux_hints_empty_when_parent_prefix_fails() {
        let mut parent = session();
        parent.prefix = None;
        let app = hints_app(session(), parent);
        assert!(app.tmux_hints().is_empty());
    }

    #[test]
    fn test_tmux_hints_empty_when_parent_prefix_blank() {
        let mut parent = session();
        parent.prefix = Some(String::new());
        let app = hints_app(session(), parent);
        assert!(app.tmux_hints().is_empty());
    }
}
