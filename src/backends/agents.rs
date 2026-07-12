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

#[derive(Debug, Clone)]
pub struct AgentIcon {
    nf_icon: String,
    fa_icon: String,
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
            _ => "",
        };

        write!(f, "{}", icon)
    }
}

impl AgentIcon {
    fn new(nf_icon: &str, fa_icon: &str) -> AgentIcon {
        Self {
            nf_icon: nf_icon.to_string(),
            fa_icon: fa_icon.to_string(),
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
        GenericAgent::new("Aider", "aider", AgentIcon::new("\u{e669}", "\u{f544}")), //  
        GenericAgent::new(
            "Claude Code",
            "claude",
            AgentIcon::new("\u{ee0d}", "\u{e861}"),
        ), //  
        GenericAgent::new("Codex", "codex", AgentIcon::new("\u{ee0d}", "\u{e7cf}")), //  
        GenericAgent::new("Copilot", "copilot", AgentIcon::new("\u{f09b}", "\u{f09b}")), //  
        GenericAgent::new("Cursor", "cursor", AgentIcon::new("\u{ee0d}", "\u{f544}")), //  
        GenericAgent::new("Devin", "devin", AgentIcon::new("\u{ee0d}", "\u{f544}")), //  
        GenericAgent::new("Hermes", "hermes", AgentIcon::new("\u{ee0d}", "\u{f544}")), //  
        GenericAgent::new(
            "OpenCode",
            "opencode",
            AgentIcon::new("\u{ee0d}", "\u{f544}"),
        ), //  
        GenericAgent::new("Pi", "pi", AgentIcon::new("\u{e22c}", "\u{f544}")),       //  
    ]
});

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
        assert_eq!(agent.name(), "Claude Code");
        assert_eq!(agent.command(), "claude");
    }

    #[test]
    fn test_generic_agent_trait() {
        let agent = GenericAgent::new("Test Agent", "test", AgentIcon::new("nf", "fa"));
        assert_eq!(agent.name(), "Test Agent");
        assert_eq!(agent.command(), "test");
        assert_eq!(
            with_icon_fonts(true, false, || agent.icon().to_string()),
            "nf"
        );
    }

    #[test]
    fn test_agent_icon_without_icon_fonts() {
        let icon = AgentIcon::new("\u{e669}", "\u{f544}"); //  
        assert_eq!(with_icon_fonts(false, false, || icon.to_string()), "");
    }

    #[test]
    fn test_agent_icon_with_nerd_font_custom() {
        let icon = AgentIcon::new("\u{e669}", "\u{f544}"); //  
        assert_eq!(
            with_icon_fonts(true, false, || icon.to_string()),
            "\u{e669}" // 
        );
    }

    #[test]
    fn test_agent_icon_with_nerd_font_default() {
        let icon = AgentIcon::new("\u{ee0d}", "\u{e861}"); //  
        assert_eq!(
            with_icon_fonts(true, false, || icon.to_string()),
            "\u{ee0d}" // 
        );
    }

    #[test]
    fn test_agent_icon_with_font_awesome_custom() {
        let icon = AgentIcon::new("\u{ee0d}", "\u{e861}"); //  
        assert_eq!(
            with_icon_fonts(false, true, || icon.to_string()),
            "\u{e861}" // 
        );
    }

    #[test]
    fn test_agent_icon_with_font_awesome_default() {
        let icon = AgentIcon::new("\u{e669}", "\u{f544}"); //  
        assert_eq!(
            with_icon_fonts(false, true, || icon.to_string()),
            "\u{f544}" // 
        );
    }

    #[test]
    fn test_agent_icon_prefers_nerd_font_custom_when_both_enabled() {
        let icon = AgentIcon::new("\u{e669}", "\u{f544}"); //  
        assert_eq!(with_icon_fonts(true, true, || icon.to_string()), "\u{e669}"); // 
    }

    #[test]
    fn test_agent_icon_prefers_nerd_font_default_when_both_enabled() {
        let icon = AgentIcon::new("\u{ee0d}", "\u{f544}"); //  
        assert_eq!(with_icon_fonts(true, true, || icon.to_string()), "\u{ee0d}"); // 
    }
}
