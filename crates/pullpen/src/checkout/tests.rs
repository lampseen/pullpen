use std::fs;

use super::*;

/// A "forge" (a bare repo with a PR ref) and a clone of it, in a fresh
/// temporary directory.
fn setup(tag: &str) -> (PathBuf, PathBuf, PrId) {
    let root = std::env::temp_dir().join(format!("pullpen-co-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let (forge, work, clone) = (root.join("acme/api.git"), root.join("work"), root.join("api"));
    fs::create_dir_all(&forge).unwrap();
    fs::create_dir_all(&work).unwrap();
    let g = |d: &Path, a: &[&str]| git(d, a).unwrap();
    g(&forge, &["init", "--quiet", "--bare"]);
    g(&work, &["init", "--quiet", "-b", "main"]);
    g(&work, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--quiet", "--allow-empty", "-m", "base"]);
    g(&work, &["remote", "add", "origin", forge.to_str().unwrap()]);
    g(&work, &["push", "--quiet", "origin", "main"]);
    g(&work, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--quiet", "--allow-empty", "-m", "the PR"]);
    g(&work, &["push", "--quiet", "origin", "HEAD:refs/pull/7/head"]);
    g(&root, &["clone", "--quiet", forge.to_str().unwrap(), clone.to_str().unwrap()]);
    let id = PrId { forge: Forge::GitHub, host: "github.com".into(), repo: "acme/api".into(), number: 7 };
    (root, clone, id)
}

fn head_subject(dir: &Path) -> String {
    git(dir, &["log", "-1", "--format=%s"]).unwrap().trim().to_string()
}

#[test]
fn checks_out_the_pr_head_found_by_remote() {
    let (root, clone, id) = setup("plain");
    let msg = checkout(&id, "feature/x", &HashMap::new(), false, &clone).unwrap();
    assert!(msg.starts_with("in "), "{msg}");
    assert_eq!(head_subject(&clone), "the PR");
    assert_eq!(git(&clone, &["branch", "--show-current"]).unwrap().trim(), "feature/x");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_local_branch_with_its_own_commits_is_never_reset() {
    let (root, clone, id) = setup("diverged");
    git(&clone, &["switch", "--quiet", "-c", "feature/x"]).unwrap();
    git(&clone, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--quiet", "--allow-empty", "-m", "mine"])
        .unwrap();
    git(&clone, &["switch", "--quiet", "main"]).unwrap();
    let err = checkout(&id, "feature/x", &HashMap::new(), false, &clone).unwrap_err();
    assert!(err.contains("left as it was"), "{err}");
    assert_eq!(git(&clone, &["log", "-1", "--format=%s", "feature/x"]).unwrap().trim(), "mine");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn worktree_mode_adds_a_worktree_beside_the_clone() {
    let (root, clone, id) = setup("worktree");
    let repos = HashMap::from([("acme/api".to_string(), clone.to_string_lossy().to_string())]);
    let msg = checkout(&id, "feature/x", &repos, true, Path::new("/")).unwrap();
    assert!(msg.contains("worktree"), "{msg}");
    assert_eq!(head_subject(&root.join("api-feature-x")), "the PR");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn no_known_clone_says_how_to_add_one() {
    let id = PrId { forge: Forge::GitLab, host: "gitlab.com".into(), repo: "platform/runner".into(), number: 1 };
    let err = checkout(&id, "b", &HashMap::new(), false, Path::new("/")).unwrap_err();
    assert!(err.contains("[repos]"), "{err}");
}

#[test]
fn a_remote_that_is_a_windows_path_still_names_its_repo() {
    let (root, clone, id) = setup("winpath");
    git(&clone, &["remote", "add", "win", r"C:\src\acme\api.git"]).unwrap();
    git(&clone, &["remote", "remove", "origin"]).unwrap();
    assert_eq!(remote_for(&clone, &id.repo).as_deref(), Some("win"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_remote_url_names_its_host_and_repo_however_it_is_spelled() {
    let want = Some(("gitlab.example.com".to_string(), "platform/infra/runner".to_string()));
    for url in [
        "git@gitlab.example.com:platform/infra/runner.git",
        "https://gitlab.example.com/platform/infra/runner",
        "https://oauth2:t0ken@gitlab.example.com/platform/infra/runner.git/",
        "ssh://git@gitlab.example.com:2222/platform/infra/runner.git",
    ] {
        assert_eq!(host_and_repo(url), want, "{url}");
    }
    for path in ["/srv/git/acme/api.git", r"C:\src\acme\api.git", r"C:\Temp\x/acme/api.git", "C:/src/acme/api.git"] {
        assert_eq!(host_and_repo(path), None, "a path on disk is no forge: {path}");
    }
}

#[test]
fn here_reads_the_branch_its_remote_and_its_newest_commit() {
    let (root, clone, _) = setup("here");
    let g = |a: &[&str]| git(&clone, a).unwrap();
    assert!(here(&clone).unwrap_err().contains("not a forge URL"));
    g(&["remote", "set-url", "origin", "git@github.com:acme/api.git"]);
    g(&["switch", "--quiet", "-c", "feat/retry"]);
    g(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "--quiet",
        "--allow-empty",
        "-m",
        "Retry on 502\n\nThe runner gave up.",
    ]);
    let h = here(&clone).unwrap();
    assert_eq!((h.host.as_str(), h.repo.as_str(), h.head.as_str()), ("github.com", "acme/api", "feat/retry"));
    assert_eq!((h.title.as_str(), h.body.as_str()), ("Retry on 502", "The runner gave up."));
    g(&["switch", "--quiet", "--detach"]);
    assert_eq!(here(&clone).err().as_deref(), Some("not on a branch"));
    fs::remove_dir_all(root).unwrap();
}
