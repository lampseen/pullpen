//! Where a token comes from (PLAN.md §1, ADR-012): the environment, then a
//! token `pullpen setup` stored, then the forge's own CLI, then a configured
//! command. Nothing here writes a token anywhere, and nothing logs one.

use std::process::Command;

use pullpen_core::Forge;

use crate::{Error, Result};

/// `env` is a parameter so tests can supply one; the binary passes
/// `std::env::var`.
pub fn token(
    kind: Forge,
    host: &str,
    stored: Option<&str>,
    token_cmd: Option<&str>,
    env: impl Fn(&str) -> Option<String>,
) -> Result<String> {
    found(kind, host, stored, token_cmd, env).map(|(_, t)| t)
}

/// Where a token came from, which decides what renews it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Env(&'static str),
    Stored,
    Cli,
    Cmd,
}

impl Source {
    /// What to do when the forge rejects a token from here.
    pub fn renew(self, kind: Forge, host: &str) -> String {
        let cli = if kind == Forge::GitHub { "gh" } else { "glab" };
        match self {
            Source::Env(var) => format!("replace the token in ${var}"),
            Source::Stored => format!("run `pullpen setup {host}`"),
            Source::Cli => format!("run `{cli} auth login --hostname {host}`"),
            Source::Cmd => "make token_cmd in config.toml print a token that works".into(),
        }
    }
}

/// The token, and where it was found.
pub fn found(
    kind: Forge,
    host: &str,
    stored: Option<&str>,
    token_cmd: Option<&str>,
    env: impl Fn(&str) -> Option<String>,
) -> Result<(Source, String)> {
    let vars: &[&'static str] = match kind {
        Forge::GitHub => &["PULLPEN_GITHUB_TOKEN", "GITHUB_TOKEN"],
        Forge::GitLab => &["PULLPEN_GITLAB_TOKEN", "GITLAB_TOKEN"],
    };
    if let Some((v, t)) = vars.iter().find_map(|v| env(v).filter(|t| !t.trim().is_empty()).map(|t| (*v, t))) {
        return Ok((Source::Env(v), t.trim().to_string()));
    }
    if let Some(t) = stored.filter(|t| !t.is_empty()) {
        return Ok((Source::Stored, t.to_string()));
    }
    let from_cli = match kind {
        Forge::GitHub => output("gh", &["auth", "token", "--hostname", host]),
        // glab prints the token on stderr, as "Token: …".
        Forge::GitLab => output("glab", &["auth", "status", "--hostname", host, "-t"]).and_then(|o| {
            o.lines()
                .find_map(|l| l.trim().strip_prefix("✓ Token: ").or(l.trim().strip_prefix("Token: ")).map(String::from))
        }),
    };
    if let Some(t) = from_cli.filter(|t| !t.is_empty() && !t.contains(' ')) {
        return Ok((Source::Cli, t));
    }
    if let Some(cmd) = token_cmd
        && let Some(t) = shell(cmd).map(|o| o.trim().to_string()).filter(|t| !t.is_empty())
    {
        return Ok((Source::Cmd, t));
    }
    Err(Error::Auth(host.into()))
}

/// `token_cmd` is a shell line, so it runs through the platform's shell.
fn shell(cmd: &str) -> Option<String> {
    if cfg!(windows) { output("cmd", &["/C", cmd]) } else { output("sh", &["-c", cmd]) }
}

/// A command's output, stdout and stderr together, or `None` if it could not
/// run or failed. Failure here is normal (the CLI may not be installed), so it
/// is not an error.
fn output(bin: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(bin).args(args).output().ok()?;
    o.status.success().then(|| {
        let mut s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if s.is_empty() {
            s = String::from_utf8_lossy(&o.stderr).trim().to_string();
        }
        s
    })
}
