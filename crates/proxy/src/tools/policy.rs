use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyAction {
    Allow,       // auto-execute server-side
    Deny,        // reject the tool call, return error to LLM
    PassThrough, // pass to client for execution
}

#[derive(Debug, Clone)]
pub struct PolicyRule {
    pub tool_name: String,
    pub action: PolicyAction,
    pub timeout: Option<Duration>,
    pub max_concurrency: Option<usize>,
}

impl PolicyRule {
    fn matches(&self, name: &str) -> bool {
        if let Some(prefix) = self.tool_name.strip_suffix('*') {
            name.starts_with(prefix)
        } else {
            self.tool_name == name
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolExecutionPolicy {
    pub default_action: PolicyAction,
    pub rules: Vec<PolicyRule>,
}

impl ToolExecutionPolicy {
    pub fn resolve(&self, tool_name: &str) -> PolicyAction {
        for rule in &self.rules {
            if rule.matches(tool_name) {
                return rule.action;
            }
        }
        self.default_action
    }

    pub fn find_rule(&self, tool_name: &str) -> Option<&PolicyRule> {
        self.rules.iter().find(|r| r.matches(tool_name))
    }
}

impl Default for ToolExecutionPolicy {
    fn default() -> Self {
        Self {
            default_action: PolicyAction::PassThrough,
            rules: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_uses_passthrough() {
        let policy = ToolExecutionPolicy::default();
        assert_eq!(policy.default_action, PolicyAction::PassThrough);
        assert_eq!(policy.resolve("any_tool"), PolicyAction::PassThrough);
    }

    #[test]
    fn exact_match_rule_works() {
        let policy = ToolExecutionPolicy {
            default_action: PolicyAction::PassThrough,
            rules: vec![PolicyRule {
                tool_name: "read_file".to_string(),
                action: PolicyAction::Allow,
                timeout: None,
                max_concurrency: None,
            }],
        };
        assert_eq!(policy.resolve("read_file"), PolicyAction::Allow);
        // Non-matching tool falls back to default
        assert_eq!(policy.resolve("write_file"), PolicyAction::PassThrough);
    }

    #[test]
    fn glob_pattern_rule_trailing_wildcard_works() {
        let policy = ToolExecutionPolicy {
            default_action: PolicyAction::PassThrough,
            rules: vec![PolicyRule {
                tool_name: "fs_*".to_string(),
                action: PolicyAction::Allow,
                timeout: None,
                max_concurrency: None,
            }],
        };
        assert_eq!(policy.resolve("fs_read"), PolicyAction::Allow);
        assert_eq!(policy.resolve("fs_write"), PolicyAction::Allow);
        assert_eq!(policy.resolve("fs_"), PolicyAction::Allow);
        // Non-matching prefix falls back to default
        assert_eq!(policy.resolve("net_fetch"), PolicyAction::PassThrough);
    }

    #[test]
    fn deny_action_blocks_a_tool() {
        let policy = ToolExecutionPolicy {
            default_action: PolicyAction::PassThrough,
            rules: vec![PolicyRule {
                tool_name: "delete_*".to_string(),
                action: PolicyAction::Deny,
                timeout: None,
                max_concurrency: None,
            }],
        };
        assert_eq!(policy.resolve("delete_file"), PolicyAction::Deny);
        assert_eq!(policy.resolve("delete_db"), PolicyAction::Deny);
        assert_eq!(policy.resolve("read_file"), PolicyAction::PassThrough);
    }

    #[test]
    fn timeout_override_per_tool_via_find_rule() {
        let timeout = Duration::from_secs(30);
        let policy = ToolExecutionPolicy {
            default_action: PolicyAction::Allow,
            rules: vec![
                PolicyRule {
                    tool_name: "slow_tool".to_string(),
                    action: PolicyAction::Allow,
                    timeout: Some(timeout),
                    max_concurrency: None,
                },
                PolicyRule {
                    tool_name: "fast_tool".to_string(),
                    action: PolicyAction::Allow,
                    timeout: None,
                    max_concurrency: None,
                },
            ],
        };
        let rule = policy.find_rule("slow_tool").expect("rule should exist");
        assert_eq!(rule.timeout, Some(timeout));

        let rule = policy.find_rule("fast_tool").expect("rule should exist");
        assert_eq!(rule.timeout, None);

        // No rule for unknown tool
        assert!(policy.find_rule("unknown_tool").is_none());
    }
}
