mod list_agents;
mod list_windows;
mod new_agent;
mod new_window;
mod read_agent;
mod read_window;
mod registry;
mod send_command;
mod tool;

pub use registry::all_tools;
pub use tool::{CallContext, Tool};

#[cfg(test)]
pub use tool::ToolOutput;
