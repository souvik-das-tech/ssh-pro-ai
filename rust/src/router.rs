//! Auto model choice: pick a fast or a smart model for each request, locally and instantly.
//!
//! No AI call decides this (that would cost the tokens and seconds Auto is meant to save): a keyword
//! classifier that knows English, Banglish and Hinglish sends quick look-ups and chat to the fast
//! model, and troubleshooting, security, changes and long multi-step requests to the smart one.

use std::sync::LazyLock;

use regex::{Regex, RegexBuilder};

pub const AUTO: &str = "auto";

/// (reason shown to the user, pattern). Checked in order; the first hit wins.
const SMART: &[(&str, &str)] = &[
    ("security check", r"\b(cve-?\d*|vulnerab\w*|secur\w*|exploit\w*|malware|hack\w*|audit\w*|breach|rootkit|suspicious|patch(ed|es)?|harden\w*|brute.?force|intrusion)\b"),
    ("troubleshooting", r"\b(why|keno|kyu|kyon|kyun|debug\w*|troubleshoot\w*|diagnos\w*|investigat\w*|root.?cause|error\w*|fail\w*|crash\w*|broken|not working|down|slow\w*|hang\w*|timeout|502|503|504|500|oom|leak\w*|problem|somossa|issue|kharap|chol(che|ch?e) ?na|kaj kor(che|ch?e) ?na|nahi? chal|kaam nahi?|thik ?(koro|kar)|fix\w*)\b"),
    ("change to servers", r"\b(install\w*|uninstall\w*|upgrad\w*|updat\w*|deploy\w*|migrat\w*|restor\w*|backup\w*|configur\w*|setup|set up|enable|disable|restart\w*|reboot|delete|remov\w*|clean\w*|kill|rollback|renew\w*|rotate|creat\w*|add|chang\w*|edit|upload\w*|download\w*|import\w*|dump|kor[eo] dao|kar ?do|bana(o|do)|hatao|muchh?o)\b"),
    ("analysis", r"\b(analy[sz]\w*|compar\w*|optimi[sz]\w*|recommend\w*|plan|strategy|explain|best way|tune|tuning|capacity|bottleneck|review)\b"),
    ("complex setup", r"\b(nginx|apache|ssl|tls|certbot|firewall|ufw|iptables|docker.?compose|kubernetes|k8s|mysql|mariadb|postgres\w*|redis|replication|cron\w*|systemd|selinux)\b.*\b(config\w*|rule\w*|set\w*|chang\w*|fix\w*|move|switch)\b"),
];

static SMART_RE: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    SMART
        .iter()
        .map(|(why, p)| (*why, RegexBuilder::new(p).case_insensitive(true).build().unwrap()))
        .collect()
});

pub const LONG_REQUEST: usize = 220; // characters; long requests usually carry several steps or conditions

/// Defaults when the user has not picked models for Auto: the first available match wins.
pub const FAST_PREFERENCE: &[&str] =
    &[r"gemini.*flash.*low", r"gemini.*flash.*medium", r"chatgpt:gpt-5\.6-luna", r"flash", r"luna"];
pub const SMART_PREFERENCE: &[&str] =
    &[r"chatgpt:gpt-6", r"claude-sonnet", r"claude-opus", r"gemini.*flash.*high", r"terra"];

/// ("fast" | "smart", reason) for one user request.
pub fn classify(prompt: &str) -> (&'static str, &'static str) {
    let text = prompt.trim();
    for (why, pattern) in SMART_RE.iter() {
        if pattern.is_match(text) {
            return ("smart", why);
        }
    }
    if text.chars().count() > LONG_REQUEST || text.matches('\n').count() >= 3 {
        return ("smart", "long request");
    }
    ("fast", "quick check")
}

pub fn pick_default<'a, I>(available: I, preference: &[&str], avoid: Option<&str>) -> Option<String>
where
    I: IntoIterator<Item = &'a String>,
{
    let ids: Vec<&String> = available.into_iter().filter(|m| m.as_str() != AUTO).collect();
    for pattern in preference {
        let re = RegexBuilder::new(pattern).case_insensitive(true).build().unwrap();
        for model in &ids {
            if Some(model.as_str()) != avoid && re.is_match(model) {
                return Some((*model).clone());
            }
        }
    }
    ids.iter()
        .find(|m| Some(m.as_str()) != avoid)
        .or(ids.first())
        .map(|m| (*m).clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_requests() {
        assert_eq!(classify("check disk space").0, "fast");
        assert_eq!(classify("free memory?").0, "fast");
        assert_eq!(classify("why is nginx slow").0, "smart");
        assert_eq!(classify("is there any CVE for php").0, "smart");
        assert_eq!(classify("install node everywhere").0, "smart");
        assert_eq!(classify("server ta kaj korche na").0, "smart");
        assert_eq!(classify(&"x ".repeat(150)).0, "smart");
    }

    #[test]
    fn picks_defaults() {
        let ids: Vec<String> = ["claude-sonnet-5", "gemini-3.8-flash-low", "gemini-3.8-flash-high"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(pick_default(&ids, FAST_PREFERENCE, None).as_deref(), Some("gemini-3.8-flash-low"));
        assert_eq!(pick_default(&ids, SMART_PREFERENCE, None).as_deref(), Some("claude-sonnet-5"));
        assert_eq!(
            pick_default(&ids, SMART_PREFERENCE, Some("claude-sonnet-5")).as_deref(),
            Some("gemini-3.8-flash-high")
        );
    }
}
