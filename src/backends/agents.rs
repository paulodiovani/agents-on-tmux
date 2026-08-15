use std::sync::LazyLock;

/// Contract for agent identification.
pub trait Agent {
    fn name(&self) -> &str;
    fn command(&self) -> &str;
}

/// Generic agent implementation with stored properties.
#[derive(Debug, Clone)]
pub struct GenericAgent {
    command: String,
    name: String,
}

impl GenericAgent {
    fn new(name: &str, command: &str) -> Self {
        Self {
            command: command.to_string(),
            name: name.to_string(),
        }
    }
}

static AGENTS: LazyLock<Vec<GenericAgent>> = LazyLock::new(|| {
    vec![
        GenericAgent::new("Aider", "aider"),
        GenericAgent::new("Claude", "claude"),
        GenericAgent::new("Codex", "codex"),
        GenericAgent::new("Copilot", "copilot"),
        GenericAgent::new("Cursor", "cursor"),
        GenericAgent::new("Devin", "devin"),
        GenericAgent::new("Hermes", "hermes"),
        GenericAgent::new("OpenCode", "opencode"),
        GenericAgent::new("Pi", "pi"),
    ]
});

impl Agent for GenericAgent {
    fn name(&self) -> &str {
        &self.name
    }

    fn command(&self) -> &str {
        &self.command
    }
}

/// Returns every known agent. Only the tests that check the frontend's icon
/// table against this registry need the whole list.
#[cfg(test)]
pub fn agents() -> &'static [GenericAgent] {
    &AGENTS
}

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
        assert_eq!(agent.name(), "Claude");
        assert_eq!(agent.command(), "claude");
    }

    #[test]
    fn test_generic_agent_trait() {
        let agent = GenericAgent::new("Test Agent", "test");
        assert_eq!(agent.name(), "Test Agent");
        assert_eq!(agent.command(), "test");
    }

    #[test]
    fn test_agents_lists_every_agent() {
        assert_eq!(agents().len(), AGENTS.len());
        assert!(agents().iter().any(|agent| agent.command() == "claude"));
    }

    #[test]
    fn test_agent_commands_are_unique() {
        let mut commands: Vec<&str> = agents().iter().map(|agent| agent.command()).collect();
        commands.sort();
        let count = commands.len();
        commands.dedup();
        assert_eq!(commands.len(), count);
    }
}
