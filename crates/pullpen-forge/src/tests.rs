use super::*;

#[test]
fn agents_are_listed_logins_bots_and_bot_suffixes() {
    let a = Agents(Agents::default_logins());
    assert!(a.is_agent("claude", false));
    assert!(a.is_agent("Dependabot", false));
    assert!(a.is_agent("renovate[bot]", false));
    assert!(a.is_agent("some-app[bot]", false));
    assert!(a.is_agent("ci-helper", true), "the forge's own bot flag counts");
    assert!(!a.is_agent("mara", false));
}

#[test]
fn only_retryable_failures_are_transient() {
    assert!(Error::RateLimited { retry_in: 5 }.transient());
    assert!(Error::Server { host: "h".into(), status: 502 }.transient());
    assert!(Error::Network("reset".into()).transient());
    assert!(!Error::Refused("no".into()).transient());
    assert!(!Error::Auth("h".into()).transient());
    assert!(!Error::Rejected { host: "h".into(), why: "expired".into() }.transient());
}

/// GitLab's answer to an expired token, as it sent it.
#[tokio::test]
async fn a_rejected_token_keeps_the_forges_reason() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::any())
        .respond_with(wiremock::ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": "invalid_token",
            "error_description": "Token is expired. You can either do re-authorization or token refresh."
        })))
        .mount(&server)
        .await;
    let err = http::send("gitlab.com", http::client().get(server.uri())).await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "gitlab.com rejected the token: Token is expired. You can either do re-authorization or token refresh."
    );
}

#[test]
fn the_fix_names_where_the_token_came_from() {
    use pullpen_core::Forge::{GitHub, GitLab};
    let env = |k: &str| (k == "GITLAB_TOKEN").then(|| "t".to_string());
    let (from, _) = auth::found(GitLab, "gitlab.com", Some("stored"), None, env).unwrap();
    assert_eq!(from.renew(GitLab, "gitlab.com"), "replace the token in $GITLAB_TOKEN");
    let (from, _) = auth::found(GitLab, "gitlab.com", Some("stored"), None, |_| None).unwrap();
    assert_eq!(from.renew(GitLab, "gitlab.com"), "run `pullpen setup gitlab.com`");
    assert_eq!(auth::Source::Cli.renew(GitHub, "ghe.example"), "run `gh auth login --hostname ghe.example`");
    assert_eq!(auth::Source::Cli.renew(GitLab, "gitlab.com"), "run `glab auth login --hostname gitlab.com`");
}

#[test]
fn tokens_come_from_the_environment_first() {
    let env = |k: &str| (k == "GITHUB_TOKEN").then(|| " ghp_x \n".to_string());
    assert_eq!(auth::token(pullpen_core::Forge::GitHub, "github.com", None, None, env).unwrap(), "ghp_x");
    let env = |k: &str| match k {
        "PULLPEN_GITLAB_TOKEN" => Some("first".to_string()),
        "GITLAB_TOKEN" => Some("second".to_string()),
        _ => None,
    };
    assert_eq!(auth::token(pullpen_core::Forge::GitLab, "gitlab.com", None, None, env).unwrap(), "first");
}

#[test]
fn a_stored_token_comes_after_the_environment_and_before_the_cli() {
    let none = |_: &str| None;
    let t = auth::token(pullpen_core::Forge::GitLab, "nowhere.invalid", Some("stored"), Some("echo from-cmd"), none)
        .unwrap();
    assert_eq!(t, "stored");
    let env = |k: &str| (k == "GITLAB_TOKEN").then(|| "env".to_string());
    assert_eq!(auth::token(pullpen_core::Forge::GitLab, "nowhere.invalid", Some("stored"), None, env).unwrap(), "env");
}

#[test]
fn a_configured_command_is_the_last_resort() {
    let none = |_: &str| None;
    let t = auth::token(pullpen_core::Forge::GitLab, "nowhere.invalid", None, Some("echo from-cmd"), none).unwrap();
    assert_eq!(t, "from-cmd");
}

#[tokio::test]
async fn a_network_failure_says_why_and_never_where() {
    // A port nothing listens on: taken, then given back.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let req = http::client().put(format!("http://127.0.0.1:{port}/repos/o/r?token=s3cret"));
    let Err(Error::Network(why)) = http::send("github.com", req).await else { panic!("it cannot have connected") };
    let cause = why.strip_prefix("github.com: error sending request: ").unwrap_or_else(|| panic!("no cause: {why}"));
    assert!(!cause.is_empty());
    assert!(!why.contains("s3cret") && !why.contains("127.0.0.1"), "{why}");
}
