/// Contract for agent identification.
pub trait Agent {
    #[allow(dead_code)]
    fn name(&self) -> &str;
    fn command(&self) -> &str;
    #[allow(dead_code)]
    fn icon(&self) -> &str;
}

/// Generic agent implementation with stored properties.
#[derive(Debug, Clone)]
pub struct GenericAgent {
    #[allow(dead_code)]
    name: String,
    command: String,
    #[allow(dead_code)]
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

/// Registry of known agents.
pub struct Agents {
    agents: Vec<GenericAgent>,
}

impl Agents {
    pub fn new() -> Self {
        let agents = vec![
            GenericAgent::new("Claude Code", "claude", "🤖"),
            GenericAgent::new("OpenCode", "opencode", "🤖"),
            GenericAgent::new("Pi", "pi", "🤖"),
            GenericAgent::new("Codex", "codex", "🤖"),
            GenericAgent::new("Devin", "devin", "🤖"),
            GenericAgent::new("Hermes", "hermes", "🤖"),
            GenericAgent::new("Aider", "aider", "🤖"),
            GenericAgent::new("Cursor", "cursor", "🤖"),
        ];
        Self { agents }
    }

    pub fn is_agent(command: &str) -> bool {
        Self::new().agents.iter().any(|a| a.command() == command)
    }

    #[allow(dead_code)]
    pub fn icon(&self, command: &str) -> Option<&str> {
        self.agents
            .iter()
            .find(|a| a.command() == command)
            .map(|a| a.icon())
    }
}

impl Default for Agents {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agents_new() {
        let agents = Agents::new();
        assert_eq!(agents.agents.len(), 8);
    }

    #[test]
    fn test_is_agent_known() {
        assert!(Agents::is_agent("claude"));
        assert!(Agents::is_agent("opencode"));
        assert!(Agents::is_agent("pi"));
        assert!(Agents::is_agent("codex"));
        assert!(Agents::is_agent("devin"));
        assert!(Agents::is_agent("hermes"));
        assert!(Agents::is_agent("aider"));
        assert!(Agents::is_agent("cursor"));
    }

    #[test]
    fn test_is_agent_unknown() {
        assert!(!Agents::is_agent("bash"));
        assert!(!Agents::is_agent("zsh"));
        assert!(!Agents::is_agent("vim"));
        assert!(!Agents::is_agent(""));
    }

    #[test]
    fn test_icon_known() {
        let agents = Agents::new();
        assert_eq!(agents.icon("claude"), Some("🤖"));
        assert_eq!(agents.icon("opencode"), Some("🤖"));
        assert_eq!(agents.icon("pi"), Some("🤖"));
    }

    #[test]
    fn test_icon_unknown() {
        let agents = Agents::new();
        assert_eq!(agents.icon("bash"), None);
        assert_eq!(agents.icon(""), None);
    }

    #[test]
    fn test_generic_agent_trait() {
        let agent = GenericAgent::new("Test Agent", "test", "🧪");
        assert_eq!(agent.name(), "Test Agent");
        assert_eq!(agent.command(), "test");
        assert_eq!(agent.icon(), "🧪");
    }

    #[test]
    fn test_agents_default() {
        let agents = Agents::default();
        assert_eq!(agents.agents.len(), 8);
    }
}
