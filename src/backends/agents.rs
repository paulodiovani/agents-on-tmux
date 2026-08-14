use std::fmt::Display;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// Contract for agent identification.
pub trait Agent {
    fn name(&self) -> &str;
    fn command(&self) -> &str;
    fn icon(&self) -> &AgentIcon;
}

static NERD_FONT_ENABLED: AtomicBool = AtomicBool::new(false);
static FONT_AWESOME_ENABLED: AtomicBool = AtomicBool::new(false);

const NERD_FONT_DEFAULT: &str = "\u{ee0d}"; //
const FONT_AWESOME_DEFAULT: &str = "\u{f544}"; // 

/// Icon version used by agents, from nerd font, font awesome, or a plain text
/// tag when neither icon font is available.
#[derive(Debug, Clone)]
pub struct AgentIcon {
    nf_icon: String,
    fa_icon: String,
    txt_icon: String,
}

impl Display for AgentIcon {
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

impl AgentIcon {
    fn new(nf_icon: &str, fa_icon: &str, txt_icon: &str) -> AgentIcon {
        Self {
            nf_icon: nf_icon.to_string(),
            fa_icon: fa_icon.to_string(),
            txt_icon: txt_icon.to_string(),
        }
    }
}

/// Generic agent implementation with stored properties.
#[derive(Debug, Clone)]
pub struct GenericAgent {
    command: String,
    icon: AgentIcon,
    name: String,
}

impl GenericAgent {
    fn new(name: &str, command: &str, icon: AgentIcon) -> Self {
        Self {
            command: command.to_string(),
            icon,
            name: name.to_string(),
        }
    }
}

static AGENTS: LazyLock<Vec<GenericAgent>> = LazyLock::new(|| {
    vec![
        GenericAgent::new(
            "Aider",
            "aider",
            AgentIcon::new("\u{e669}", "\u{f544}", "[ai]"),
        ), //  
        GenericAgent::new(
            "Claude",
            "claude",
            AgentIcon::new("\u{ee0d}", "\u{e861}", "[cc]"),
        ), //  
        GenericAgent::new(
            "Codex",
            "codex",
            AgentIcon::new("\u{ee0d}", "\u{e7cf}", "[cx]"),
        ), //  
        GenericAgent::new(
            "Copilot",
            "copilot",
            AgentIcon::new("\u{f09b}", "\u{f09b}", "[cp]"),
        ), //  
        GenericAgent::new(
            "Cursor",
            "cursor",
            AgentIcon::new("\u{ee0d}", "\u{f544}", "[cu]"),
        ), //  
        GenericAgent::new(
            "Devin",
            "devin",
            AgentIcon::new("\u{ee0d}", "\u{f544}", "[dv]"),
        ), //  
        GenericAgent::new(
            "Hermes",
            "hermes",
            AgentIcon::new("\u{ee0d}", "\u{f544}", "[hm]"),
        ), //  
        GenericAgent::new(
            "OpenCode",
            "opencode",
            AgentIcon::new("\u{ee0d}", "\u{f544}", "[oc]"),
        ), //  
        GenericAgent::new("Pi", "pi", AgentIcon::new("\u{e22c}", "\u{f544}", "[pi]")), //  
    ]
});

/// Icon for plain (non-agent) windows: a terminal glyph, or `[w]` as text fallback.
static WINDOW_ICON: LazyLock<AgentIcon> =
    LazyLock::new(|| AgentIcon::new("\u{e795}", "\u{f120}", "[w]"));

/// Icon for the window activity marker: a bell, or `!` as text fallback.
static NOTIFICATION_ICON: LazyLock<AgentIcon> =
    LazyLock::new(|| AgentIcon::new("\u{f0f3}", "\u{f0f3}", "!"));

impl Agent for GenericAgent {
    fn name(&self) -> &str {
        &self.name
    }

    fn command(&self) -> &str {
        &self.command
    }

    fn icon(&self) -> &AgentIcon {
        &self.icon
    }
}

pub fn is_agent(command: &str) -> Option<GenericAgent> {
    AGENTS.iter().find(|a| a.command() == command).cloned()
}

/// Returns the icon shown for plain (non-agent) windows.
pub fn window_icon() -> &'static AgentIcon {
    &WINDOW_ICON
}

/// Returns the icon shown when a window has pending activity.
pub fn notification_icon() -> &'static AgentIcon {
    &NOTIFICATION_ICON
}

pub fn set_icon_fonts(nerd_font: bool, font_awesome: bool) {
    NERD_FONT_ENABLED.store(nerd_font, Ordering::Relaxed);
    FONT_AWESOME_ENABLED.store(font_awesome, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn with_icon_fonts<T>(nerd_font: bool, font_awesome: bool, test: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap();
        set_icon_fonts(nerd_font, font_awesome);
        let result = test();
        set_icon_fonts(false, false);
        result
    }

    #[test]
    fn test_is_agent_known() {
        assert!(is_agent("claude").is_some());
        assert!(is_agent("opencode").is_some());
        assert!(is_agent("pi").is_some());
        assert!(is_agent("codex").is_some());
        assert!(is_agent("devin").is_some());
        assert!(is_agent("hermes").is_some());
        assert!(is_agent("aider").is_some());
        assert!(is_agent("cursor").is_some());
    }

    #[test]
    fn test_is_agent_unknown() {
        assert!(is_agent("bash").is_none());
        assert!(is_agent("zsh").is_none());
        assert!(is_agent("vim").is_none());
        assert!(is_agent("").is_none());
    }

    #[test]
    fn test_is_agent_returns_correct_agent() {
        let agent = is_agent("claude").unwrap();
        assert_eq!(agent.name(), "Claude");
        assert_eq!(agent.command(), "claude");
    }

    #[test]
    fn test_generic_agent_trait() {
        let agent = GenericAgent::new("Test Agent", "test", AgentIcon::new("nf", "fa", "txt"));
        assert_eq!(agent.name(), "Test Agent");
        assert_eq!(agent.command(), "test");
        assert_eq!(
            with_icon_fonts(true, false, || agent.icon().to_string()),
            "nf"
        );
    }

    #[test]
    fn test_agent_icon_without_icon_fonts() {
        let icon = AgentIcon::new("\u{e669}", "\u{f544}", "[ai]"); //  
        assert_eq!(with_icon_fonts(false, false, || icon.to_string()), "[ai]");
    }

    #[test]
    fn test_agent_icon_with_nerd_font_custom() {
        let icon = AgentIcon::new("\u{e669}", "\u{f544}", "[ai]"); //  
        assert_eq!(
            with_icon_fonts(true, false, || icon.to_string()),
            "\u{e669}" // 
        );
    }

    #[test]
    fn test_agent_icon_with_nerd_font_default() {
        let icon = AgentIcon::new("\u{ee0d}", "\u{e861}", "[cc]"); //  
        assert_eq!(
            with_icon_fonts(true, false, || icon.to_string()),
            "\u{ee0d}" // 
        );
    }

    #[test]
    fn test_agent_icon_with_font_awesome_custom() {
        let icon = AgentIcon::new("\u{ee0d}", "\u{e861}", "[cc]"); //  
        assert_eq!(
            with_icon_fonts(false, true, || icon.to_string()),
            "\u{e861}" // 
        );
    }

    #[test]
    fn test_agent_icon_with_font_awesome_default() {
        let icon = AgentIcon::new("\u{e669}", "\u{f544}", "[ai]"); //  
        assert_eq!(
            with_icon_fonts(false, true, || icon.to_string()),
            "\u{f544}" // 
        );
    }

    #[test]
    fn test_agent_icon_prefers_nerd_font_custom_when_both_enabled() {
        let icon = AgentIcon::new("\u{e669}", "\u{f544}", "[ai]"); //  
        assert_eq!(with_icon_fonts(true, true, || icon.to_string()), "\u{e669}"); // 
    }

    #[test]
    fn test_agent_icons_have_text_fallbacks() {
        // Without icon fonts every agent must still be identifiable.
        for agent in AGENTS.iter() {
            let tag = with_icon_fonts(false, false, || agent.icon().to_string());
            assert!(
                tag.starts_with('[') && tag.ends_with(']'),
                "{} has no text fallback tag, got {tag:?}",
                agent.name()
            );
        }
    }

    #[test]
    fn test_agent_text_fallbacks_are_unique() {
        let mut tags: Vec<String> = with_icon_fonts(false, false, || {
            AGENTS.iter().map(|a| a.icon().to_string()).collect()
        });
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

    #[test]
    fn test_agent_icon_prefers_nerd_font_default_when_both_enabled() {
        let icon = AgentIcon::new("\u{ee0d}", "\u{f544}", "[cu]"); //  
        assert_eq!(with_icon_fonts(true, true, || icon.to_string()), "\u{ee0d}"); // 
    }
}
