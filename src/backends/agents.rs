use std::sync::LazyLock;

/// Contract for agent identification.
pub trait Agent {
    fn name(&self) -> &str;
    fn command(&self) -> &str;
    fn icon(&self) -> &str;
}

/// Generic agent implementation with stored properties.
#[derive(Debug, Clone)]
pub struct GenericAgent {
    name: String,
    command: String,
    icon: String,
}

impl GenericAgent {
    fn new(name: &str, command: &str, icon: &str) -> Self {
        Self {
            name: name.to_string(),
            command: command.to_string(),
            icon: icon.to_string(),
        }
    }
}

impl Agent for GenericAgent {
    fn name(&self) -> &str {
        &self.name
    }

    fn command(&self) -> &str {
        &self.command
    }

    fn icon(&self) -> &str {
        &self.icon
    }
}

static AGENTS: LazyLock<Vec<GenericAgent>> = LazyLock::new(|| {
    vec![
        GenericAgent::new("Claude Code", "claude", "🤖"),
        GenericAgent::new("OpenCode", "opencode", "🤖"),
        GenericAgent::new("Pi", "pi", "🤖"),
        GenericAgent::new("Codex", "codex", "🤖"),
        GenericAgent::new("Devin", "devin", "🤖"),
        GenericAgent::new("Hermes", "hermes", "🤖"),
        GenericAgent::new("Aider", "aider", "🤖"),
        GenericAgent::new("Cursor", "cursor", "🤖"),
    ]
});

pub fn is_agent(command: &str) -> Option<GenericAgent> {
    AGENTS.iter().find(|a| a.command() == command).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(agent.icon(), "🤖");
    }

    #[test]
    fn test_generic_agent_trait() {
        let agent = GenericAgent::new("Test Agent", "test", "🧪");
        assert_eq!(agent.name(), "Test Agent");
        assert_eq!(agent.command(), "test");
        assert_eq!(agent.icon(), "🧪");
    }
}
