//! An app straight from GitHub: `kode-sim install https://github.com/owner/repo`.
//!
//! The link is read here; the repository is cloned (shallow, with its
//! submodules) into the cache, `<cache>/github/<owner>/<repo>`, and that
//! folder is what gets built. A second install of the same link fetches
//! again and takes the newest commit of the same branch or tag.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A repository on GitHub, and the branch or tag to take (its default
/// branch when there is none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    pub owner: String,
    pub name: String,
    pub rev: Option<String>,
}

impl Repo {
    pub fn url(&self) -> String {
        format!("https://github.com/{}/{}.git", self.owner, self.name)
    }
}

/// What GitHub allows in an owner's or a repository's name.
fn plain(s: &str) -> bool {
    !s.is_empty() && s != "." && s != ".." && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

/// The repository a link names: `https://github.com/<owner>/<repo>`, with
/// or without `.git`, `/tree/<branch or tag>` or a trailing `/`, and the same
/// without `https://`. Anything else is not a link (it stays a path).
pub fn parse(arg: &str) -> Option<Repo> {
    let rest = arg.strip_prefix("https://").or_else(|| arg.strip_prefix("http://")).unwrap_or(arg);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let rest = rest.strip_prefix("github.com/")?;
    let rest = rest.trim_end_matches('/');
    let mut parts = rest.splitn(3, '/');
    let owner = parts.next()?;
    let name = parts.next()?;
    let name = name.strip_suffix(".git").unwrap_or(name);
    if !plain(owner) || !plain(name) {
        return None;
    }
    let rev = match parts.next() {
        None => None,
        Some(tail) => {
            let r = tail.strip_prefix("tree/")?;
            if r.is_empty() || r.starts_with('-') || r.contains("..") || r.chars().any(|c| c.is_whitespace()) {
                return None;
            }
            Some(r.to_string())
        }
    };
    Some(Repo { owner: owner.to_string(), name: name.to_string(), rev })
}

/// Where the clone of `repo` lives in `cache`.
pub fn dir_in(cache: &Path, repo: &Repo) -> PathBuf {
    cache.join("github").join(&repo.owner).join(&repo.name)
}

fn git(dir: Option<&Path>, args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new("git");
    cmd.args(["-c", "advice.detachedHead=false"]);
    if let Some(d) = dir {
        cmd.arg("-C").arg(d);
    }
    let out = cmd
        .args(args)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    Err(format!("git {} failed: {last}", args.first().unwrap_or(&"")))
}

/// Clone `url` (at `rev`, or its default branch) into `dir`, or, when it is
/// there already, fetch the same again and reset to it. The folder is the
/// project to build.
pub fn fetch_from(url: &str, rev: Option<&str>, dir: &Path) -> Result<PathBuf, String> {
    if dir.join(".git").exists() {
        let want = rev.unwrap_or("HEAD");
        git(Some(dir), &["fetch", "-q", "--depth", "1", "origin", want])?;
        git(Some(dir), &["reset", "-q", "--hard", "FETCH_HEAD"])?;
        git(Some(dir), &["clean", "-q", "-fdx"])?;
    } else {
        if dir.exists() {
            std::fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let d = dir.to_string_lossy();
        let mut args = vec!["clone", "-q", "--depth", "1"];
        if let Some(r) = rev {
            args.extend(["--branch", r]);
        }
        args.extend([url, &d]);
        git(None, &args)?;
    }
    git(Some(dir), &["submodule", "update", "-q", "--init", "--recursive", "--depth", "1"])?;
    Ok(dir.to_path_buf())
}

/// The app at `repo`, cloned or brought up to date in kode-sim's cache.
pub fn fetch(repo: &Repo) -> Result<PathBuf, String> {
    let dir = dir_in(&crate::emu::cache_dir(), repo);
    let what = match &repo.rev {
        Some(r) => format!("{}/{} at {r}", repo.owner, repo.name),
        None => format!("{}/{}", repo.owner, repo.name),
    };
    eprintln!("kode-sim: fetching {what} from GitHub into {} ...", dir.display());
    fetch_from(&repo.url(), repo.rev.as_deref(), &dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(owner: &str, name: &str, rev: Option<&str>) -> Option<Repo> {
        Some(Repo { owner: owner.into(), name: name.into(), rev: rev.map(Into::into) })
    }

    #[test]
    fn a_github_link_names_its_repository() {
        let want = repo("kodediy", "kode-app-template", None);
        for link in [
            "https://github.com/kodediy/kode-app-template",
            "https://github.com/kodediy/kode-app-template/",
            "https://github.com/kodediy/kode-app-template.git",
            "http://github.com/kodediy/kode-app-template",
            "https://www.github.com/kodediy/kode-app-template",
            "github.com/kodediy/kode-app-template",
        ] {
            assert_eq!(parse(link), want, "{link}");
        }
    }

    #[test]
    fn a_tree_link_names_its_branch_or_tag() {
        assert_eq!(parse("https://github.com/a/b/tree/v1.2.0"), repo("a", "b", Some("v1.2.0")));
        assert_eq!(parse("https://github.com/a/b/tree/feat/x/"), repo("a", "b", Some("feat/x")));
    }

    #[test]
    fn paths_and_other_links_stay_paths() {
        for arg in [
            "my-app",
            "./my-app",
            "/home/me/github.com/a/b",
            "~/code/kode-app",
            "app.bin",
            "https://gitlab.com/a/b",
            "https://github.com/a",
            "https://github.com/a/b/blob/main/README.md",
            "https://github.com/a/b/tree/",
            "https://github.com/a/b/tree/--upload-pack=x",
            "https://github.com/../b",
            "https://github.com/a b/c",
        ] {
            assert_eq!(parse(arg), None, "{arg}");
        }
    }

    #[test]
    fn the_clone_lives_in_the_cache_by_owner_and_name() {
        let r = parse("https://github.com/kodediy/kode-app-template").unwrap();
        assert_eq!(dir_in(Path::new("/c"), &r), PathBuf::from("/c/github/kodediy/kode-app-template"));
        assert_eq!(r.url(), "https://github.com/kodediy/kode-app-template.git");
    }

    fn sh(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "init.defaultBranch=main",
                "-c",
                "protocol.file.allow=always",
            ])
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    #[test]
    fn a_second_fetch_takes_the_newest_commit_of_the_same_branch() {
        let tmp = std::env::temp_dir().join(format!("kode-sim-github-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let work = tmp.join("work");
        std::fs::create_dir_all(&work).unwrap();
        sh(&work, &["init", "-q"]);
        std::fs::write(work.join("CMakeLists.txt"), "one").unwrap();
        sh(&work, &["add", "."]);
        sh(&work, &["commit", "-q", "-m", "one"]);
        sh(&work, &["tag", "v1"]);
        let url = format!("file://{}", work.display());

        let dir = tmp.join("cache/github/o/app");
        assert_eq!(fetch_from(&url, None, &dir).unwrap(), dir);
        assert_eq!(std::fs::read_to_string(dir.join("CMakeLists.txt")).unwrap(), "one");

        std::fs::write(work.join("CMakeLists.txt"), "two").unwrap();
        sh(&work, &["commit", "-q", "-am", "two"]);
        std::fs::write(dir.join("left-over"), "x").unwrap();
        fetch_from(&url, None, &dir).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("CMakeLists.txt")).unwrap(), "two");
        assert!(!dir.join("left-over").exists(), "what the last build left goes");

        let tagged = tmp.join("cache/github/o/app-v1");
        fetch_from(&url, Some("v1"), &tagged).unwrap();
        assert_eq!(std::fs::read_to_string(tagged.join("CMakeLists.txt")).unwrap(), "one");

        let err = fetch_from(&url, Some("no-such-branch"), &tmp.join("cache/github/o/x")).unwrap_err();
        assert!(err.starts_with("git clone failed"), "{err}");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
