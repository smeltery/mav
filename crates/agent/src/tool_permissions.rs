use crate::AgentTool;
use crate::tools::TerminalTool;
use agent_settings::{AgentSettings, CompiledRegex, ToolPermissions, ToolRules};
use settings::ToolPermissionMode;
use shell_command_parser::{
    TerminalCommandValidation, extract_commands, validate_terminal_command,
};
use std::path::{Component, Path};
use std::sync::LazyLock;
use util::shell::ShellKind;

mod decision;
mod hardcoded;
mod paths;
#[cfg(test)]
mod tests;

pub use decision::{ToolPermissionDecision, decide_permission_from_settings};
pub use paths::{
    decide_permission_for_path, decide_permission_for_paths, most_restrictive, normalize_path,
};

use hardcoded::{check_hardcoded_security_rules, INVALID_TERMINAL_COMMAND_MESSAGE};
