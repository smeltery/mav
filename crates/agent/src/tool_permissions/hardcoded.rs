use super::*;

pub(super) const HARDCODED_SECURITY_DENIAL_MESSAGE: &str = "Blocked by built-in security rule. This operation is considered too \
     harmful to be allowed, and cannot be overridden by settings.";
pub(super) const INVALID_TERMINAL_COMMAND_MESSAGE: &str = "The terminal command could not be approved because terminal does not \
     allow shell substitutions or interpolations in permission-protected commands. Forbidden examples include $VAR, \
     ${VAR}, $(...), backticks, $((...)), <(...), and >(...). Resolve those values before calling terminal, or ask \
     the user for the literal value to use.";

/// Security rules that are always enforced and cannot be overridden by any setting.
/// These protect against catastrophic operations like wiping filesystems.
pub struct HardcodedSecurityRules {
    pub terminal_deny: Vec<CompiledRegex>,
}

pub static HARDCODED_SECURITY_RULES: LazyLock<HardcodedSecurityRules> = LazyLock::new(|| {
    // Flag group matches any short flags (-rf, -rfv, -v, etc.) or long flags (--recursive, --force, etc.)
    // This ensures extra flags like -rfv, -v -rf, --recursive --force don't bypass the rules.
    const FLAGS: &str = r"(--[a-zA-Z0-9][-a-zA-Z0-9_]*(=[^\s]*)?\s+|-[a-zA-Z]+\s+)*";
    // Trailing flags that may appear after the path operand (GNU rm accepts flags after operands)
    const TRAILING_FLAGS: &str = r"(\s+--[a-zA-Z0-9][-a-zA-Z0-9_]*(=[^\s]*)?|\s+-[a-zA-Z]+)*\s*";

    HardcodedSecurityRules {
        terminal_deny: vec![
            // Recursive deletion of root - "rm -rf /", "rm -rfv /", "rm -rf /*", "rm / -rf"
            CompiledRegex::new(
                &format!(r"\brm\s+{FLAGS}(--\s+)?/\*?{TRAILING_FLAGS}$"),
                false,
            )
            .expect("hardcoded regex should compile"),
            // Recursive deletion of home - "rm -rf ~" or "rm -rf ~/" or "rm -rf ~/*" or "rm ~ -rf" (but not ~/subdir)
            CompiledRegex::new(
                &format!(r"\brm\s+{FLAGS}(--\s+)?~/?\*?{TRAILING_FLAGS}$"),
                false,
            )
            .expect("hardcoded regex should compile"),
            // Recursive deletion of home via $HOME - "rm -rf $HOME" or "rm -rf ${HOME}" or "rm $HOME -rf" or with /*
            CompiledRegex::new(
                &format!(r"\brm\s+{FLAGS}(--\s+)?(\$HOME|\$\{{HOME\}})/?(\*)?{TRAILING_FLAGS}$"),
                false,
            )
            .expect("hardcoded regex should compile"),
            // Recursive deletion of current directory - "rm -rf ." or "rm -rf ./" or "rm -rf ./*" or "rm . -rf"
            CompiledRegex::new(
                &format!(r"\brm\s+{FLAGS}(--\s+)?\./?\*?{TRAILING_FLAGS}$"),
                false,
            )
            .expect("hardcoded regex should compile"),
            // Recursive deletion of parent directory - "rm -rf .." or "rm -rf ../" or "rm -rf ../*" or "rm .. -rf"
            CompiledRegex::new(
                &format!(r"\brm\s+{FLAGS}(--\s+)?\.\./?\*?{TRAILING_FLAGS}$"),
                false,
            )
            .expect("hardcoded regex should compile"),
        ],
    }
});

/// Checks if input matches any hardcoded security rules that cannot be bypassed.
/// Returns a Deny decision if blocked, None otherwise.
pub(super) fn check_hardcoded_security_rules(
    tool_name: &str,
    inputs: &[String],
    shell_kind: ShellKind,
) -> Option<ToolPermissionDecision> {
    // Currently only terminal tool has hardcoded rules
    if tool_name != TerminalTool::NAME {
        return None;
    }

    let rules = &*HARDCODED_SECURITY_RULES;
    let terminal_patterns = &rules.terminal_deny;

    for input in inputs {
        // First: check the original input as-is (and its path-normalized form)
        if matches_hardcoded_patterns(input, terminal_patterns) {
            return Some(ToolPermissionDecision::Deny(
                HARDCODED_SECURITY_DENIAL_MESSAGE.into(),
            ));
        }

        // Second: parse and check individual sub-commands (for chained commands)
        if shell_kind.supports_posix_chaining() {
            if let Some(commands) = extract_commands(input) {
                for command in &commands {
                    if matches_hardcoded_patterns(command, terminal_patterns) {
                        return Some(ToolPermissionDecision::Deny(
                            HARDCODED_SECURITY_DENIAL_MESSAGE.into(),
                        ));
                    }
                }
            }
        }
    }

    None
}

/// Checks a single command against hardcoded patterns, both as-is and with
/// path arguments normalized (to catch traversal bypasses like `rm -rf /tmp/../../`
/// and multi-path bypasses like `rm -rf /tmp /`).
fn matches_hardcoded_patterns(command: &str, patterns: &[CompiledRegex]) -> bool {
    for pattern in patterns {
        if pattern.is_match(command) {
            return true;
        }
    }

    for expanded in expand_rm_to_single_path_commands(command) {
        for pattern in patterns {
            if pattern.is_match(&expanded) {
                return true;
            }
        }
    }

    false
}

/// For rm commands, expands multi-path arguments into individual single-path
/// commands with normalized paths. This catches both traversal bypasses like
/// `rm -rf /tmp/../../` and multi-path bypasses like `rm -rf /tmp /`.
fn expand_rm_to_single_path_commands(command: &str) -> Vec<String> {
    let trimmed = command.trim();

    let first_token = trimmed.split_whitespace().next();
    if !first_token.is_some_and(|t| t.eq_ignore_ascii_case("rm")) {
        return vec![];
    }

    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    let mut flags = Vec::new();
    let mut paths = Vec::new();
    let mut past_double_dash = false;

    for part in parts.iter().skip(1) {
        if !past_double_dash && *part == "--" {
            past_double_dash = true;
            flags.push(*part);
            continue;
        }
        if !past_double_dash && part.starts_with('-') {
            flags.push(*part);
        } else {
            paths.push(*part);
        }
    }

    let flags_str = if flags.is_empty() {
        String::new()
    } else {
        format!("{} ", flags.join(" "))
    };

    let mut results = Vec::new();
    for path in &paths {
        if path.starts_with('$') {
            let home_prefix = if path.starts_with("${HOME}") {
                Some("${HOME}")
            } else if path.starts_with("$HOME") {
                Some("$HOME")
            } else {
                None
            };

            if let Some(prefix) = home_prefix {
                let suffix = &path[prefix.len()..];
                if suffix.is_empty() {
                    results.push(format!("rm {flags_str}{path}"));
                } else if suffix.starts_with('/') {
                    let normalized_suffix = normalize_path(suffix);
                    let reconstructed = if normalized_suffix == "/" {
                        prefix.to_string()
                    } else {
                        format!("{prefix}{normalized_suffix}")
                    };
                    results.push(format!("rm {flags_str}{reconstructed}"));
                } else {
                    results.push(format!("rm {flags_str}{path}"));
                }
            } else {
                results.push(format!("rm {flags_str}{path}"));
            }
            continue;
        }

        let mut normalized = normalize_path(path);
        if normalized.is_empty() && !Path::new(path).has_root() {
            normalized = ".".to_string();
        }

        results.push(format!("rm {flags_str}{normalized}"));
    }

    results
}
