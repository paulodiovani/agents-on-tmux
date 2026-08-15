use std::collections::HashMap;
use std::fmt::Display;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

static NERD_FONT_ENABLED: AtomicBool = AtomicBool::new(false);
static FONT_AWESOME_ENABLED: AtomicBool = AtomicBool::new(false);

/// Generic glyphs, drawn for anything without an icon of its own. An icon that
/// keeps one of these is not specific enough to win over the other font.
const NERD_FONT_DEFAULT: &str = "\u{ee0d}";
const FONT_AWESOME_DEFAULT: &str = "\u{f544}";

/// Agents the UI has no icon for.
static DEFAULT_ICON: LazyLock<Icon> =
    LazyLock::new(|| Icon::new(NERD_FONT_DEFAULT, FONT_AWESOME_DEFAULT, "[ag]"));

/// Plain (non-agent) windows: a terminal glyph.
static WINDOW_ICON: LazyLock<Icon> = LazyLock::new(|| Icon::new("\u{e795}", "\u{f120}", "[w]"));

/// Window activity marker: a bell. Both fonts carry it at the same codepoint.
static NOTIFICATION_ICON: LazyLock<Icon> = LazyLock::new(|| Icon::new("\u{f0f3}", "\u{f0f3}", "!"));

/// How each agent is drawn, keyed by the command the backend identifies it by.
/// Icons are a decision of this interface, not a property of the agent, so they
/// live here rather than in the agent registry.
static AGENT_ICONS: LazyLock<HashMap<&str, Icon>> = LazyLock::new(|| {
    HashMap::from([
        ("aider", Icon::new("\u{e669}", "\u{f544}", "[ai]")), //  
        ("claude", Icon::new("\u{ee0d}", "\u{e861}", "[cc]")), //  
        ("codex", Icon::new("\u{ee0d}", "\u{e7cf}", "[cx]")), //  
        ("copilot", Icon::new("\u{f09b}", "\u{f09b}", "[cp]")), //  
        ("cursor", Icon::new("\u{ee0d}", "\u{f544}", "[cu]")), //  
        ("devin", Icon::new("\u{ee0d}", "\u{f544}", "[dv]")), //  
        ("hermes", Icon::new("\u{ee0d}", "\u{f544}", "[hm]")), //  
        ("opencode", Icon::new("\u{ee0d}", "\u{f544}", "[oc]")), //  
        ("pi", Icon::new("\u{e22c}", "\u{f544}", "[pi]")),    //  
    ])
});

/// An icon in its three flavors: Nerd Font, Font Awesome, and a plain text tag
/// for terminals with neither font.
#[derive(Debug, Clone)]
pub struct Icon {
    nf_icon: String,
    fa_icon: String,
    txt_icon: String,
}

impl Display for Icon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let nerd_font = NERD_FONT_ENABLED.load(Ordering::Relaxed);
        let font_awesome = FONT_AWESOME_ENABLED.load(Ordering::Relaxed);

        let icon = match (nerd_font, font_awesome) {
            (true, false) => &self.nf_icon,
            (false, true) => &self.fa_icon,
            (true, true) if self.nf_icon != NERD_FONT_DEFAULT => &self.nf_icon,
            (true, true) if self.fa_icon != FONT_AWESOME_DEFAULT => &self.fa_icon,
            (true, true) => &self.nf_icon,
            _ => &self.txt_icon,
        };

        write!(f, "{}", icon)
    }
}

impl Icon {
    fn new(nf_icon: &str, fa_icon: &str, txt_icon: &str) -> Icon {
        Self {
            nf_icon: nf_icon.to_string(),
            fa_icon: fa_icon.to_string(),
            txt_icon: txt_icon.to_string(),
        }
    }
}

/// Returns the icon of the agent running the given command.
pub fn agent_icon(command: &str) -> &'static Icon {
    AGENT_ICONS.get(command).unwrap_or(&DEFAULT_ICON)
}

/// Returns the icon shown for plain (non-agent) windows.
pub fn window_icon() -> &'static Icon {
    &WINDOW_ICON
}

/// Returns the icon shown when a window has pending activity.
pub fn notification_icon() -> &'static Icon {
    &NOTIFICATION_ICON
}

pub fn set_icon_fonts(nerd_font: bool, font_awesome: bool) {
    NERD_FONT_ENABLED.store(nerd_font, Ordering::Relaxed);
    FONT_AWESOME_ENABLED.store(font_awesome, Ordering::Relaxed);
}

/// Runs `test` with the icon fonts toggled. The enabled fonts are global, so
/// every test that depends on them has to go through this lock.
#[cfg(test)]
fn with_icon_fonts<T>(nerd_font: bool, font_awesome: bool, test: impl FnOnce() -> T) -> T {
    use std::sync::Mutex;
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    let _guard = ENV_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    set_icon_fonts(nerd_font, font_awesome);
    let result = test();
    set_icon_fonts(false, false);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::agents::{Agent, agents};

    /// A custom icon in both fonts.
    fn custom() -> Icon {
        Icon::new("\u{e669}", "\u{e861}", "[tt]")
    }

    #[test]
    fn test_icon_without_icon_fonts() {
        assert_eq!(
            with_icon_fonts(false, false, || custom().to_string()),
            "[tt]"
        );
    }

    #[test]
    fn test_icon_with_nerd_font() {
        assert_eq!(
            with_icon_fonts(true, false, || custom().to_string()),
            "\u{e669}"
        );
    }

    #[test]
    fn test_icon_with_font_awesome() {
        assert_eq!(
            with_icon_fonts(false, true, || custom().to_string()),
            "\u{e861}"
        );
    }

    #[test]
    fn test_nerd_font_default_still_wins_over_font_awesome_default() {
        let icon = Icon::new(NERD_FONT_DEFAULT, FONT_AWESOME_DEFAULT, "[tt]");
        assert_eq!(
            with_icon_fonts(true, true, || icon.to_string()),
            NERD_FONT_DEFAULT
        );
    }

    #[test]
    fn test_custom_nerd_font_wins_when_both_enabled() {
        let icon = Icon::new("\u{e669}", FONT_AWESOME_DEFAULT, "[tt]");
        assert_eq!(with_icon_fonts(true, true, || icon.to_string()), "\u{e669}");
    }

    #[test]
    fn test_custom_font_awesome_wins_over_the_nerd_font_default() {
        let icon = Icon::new(NERD_FONT_DEFAULT, "\u{e861}", "[tt]");
        assert_eq!(with_icon_fonts(true, true, || icon.to_string()), "\u{e861}");
    }

    #[test]
    fn test_every_agent_has_an_icon() {
        // The agent registry and this table live in different modules; nothing
        // but this test keeps them in sync.
        for agent in agents() {
            assert!(
                AGENT_ICONS.contains_key(agent.command()),
                "{} has no icon",
                agent.name()
            );
        }
    }

    #[test]
    fn test_unknown_agent_falls_back_to_the_default_icon() {
        assert_eq!(
            with_icon_fonts(false, false, || agent_icon("nonesuch").to_string()),
            "[ag]"
        );
    }

    #[test]
    fn test_agent_icons_have_unique_text_fallbacks() {
        // Without icon fonts every agent must still be identifiable.
        let mut tags: Vec<String> = with_icon_fonts(false, false, || {
            agents()
                .iter()
                .map(|agent| agent_icon(agent.command()).to_string())
                .collect()
        });
        for tag in &tags {
            assert!(tag.starts_with('[') && tag.ends_with(']'), "{tag:?}");
        }
        tags.sort();
        let count = tags.len();
        tags.dedup();
        assert_eq!(tags.len(), count);
    }

    #[test]
    fn test_window_icon() {
        assert_eq!(
            with_icon_fonts(true, false, || window_icon().to_string()),
            "\u{e795}"
        );
        assert_eq!(
            with_icon_fonts(false, true, || window_icon().to_string()),
            "\u{f120}"
        );
        assert_eq!(
            with_icon_fonts(false, false, || window_icon().to_string()),
            "[w]"
        );
    }

    #[test]
    fn test_notification_icon() {
        assert_eq!(
            with_icon_fonts(true, false, || notification_icon().to_string()),
            "\u{f0f3}"
        );
        assert_eq!(
            with_icon_fonts(false, true, || notification_icon().to_string()),
            "\u{f0f3}"
        );
        // Both fonts enabled: the two glyphs are identical, so either arm is correct.
        assert_eq!(
            with_icon_fonts(true, true, || notification_icon().to_string()),
            "\u{f0f3}"
        );
        assert_eq!(
            with_icon_fonts(false, false, || notification_icon().to_string()),
            "!"
        );
    }
}
