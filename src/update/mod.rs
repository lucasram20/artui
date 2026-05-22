//! Background self-update check.
//!
//! Modeled on opencode's `autoupdate: notify` and Codex's silent-on-launch
//! check. We never auto-install — only fetch the latest GitHub release tag,
//! compare semver, and surface a banner when the bump is severe enough per
//! the user's `[updates]` config.
//!
//! Default policy is `major` only: a `feat:` patch (v0.0.5 → v0.1.0 minor)
//! and a `fix:` (v0.1.0 → v0.1.1 patch) stay silent. A breaking change
//! (v0.1.0 → v1.0.0 major) shows the banner.

use std::time::Duration;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NotifyLevel {
    /// Silence all update banners.
    Off,
    /// Banner on major bump only (default — matches the user's request).
    #[default]
    Major,
    /// Banner on major or minor bump.
    Minor,
    /// Banner on any newer version, including patches.
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Same,
    Patch,
    Minor,
    Major,
}

impl Severity {
    fn satisfies(self, level: NotifyLevel) -> bool {
        match (level, self) {
            (NotifyLevel::Off, _) => false,
            (_, Severity::Same) => false,
            (NotifyLevel::Major, Severity::Major) => true,
            (NotifyLevel::Major, _) => false,
            (NotifyLevel::Minor, Severity::Major | Severity::Minor) => true,
            (NotifyLevel::Minor, _) => false,
            (NotifyLevel::All, _) => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub severity: Severity,
    pub release_url: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SemVer {
    major: u64,
    minor: u64,
    patch: u64,
}

fn parse_semver(raw: &str) -> Option<SemVer> {
    let trimmed = raw.trim().trim_start_matches('v');
    let mut iter = trimmed.split(['.', '-']).take(3);
    let major = iter.next()?.parse().ok()?;
    let minor = iter.next()?.parse().ok()?;
    let patch = iter.next()?.parse().ok()?;
    Some(SemVer {
        major,
        minor,
        patch,
    })
}

fn classify(current: SemVer, latest: SemVer) -> Severity {
    if latest.major > current.major {
        Severity::Major
    } else if latest.major == current.major && latest.minor > current.minor {
        Severity::Minor
    } else if latest == current {
        Severity::Same
    } else if latest.major == current.major
        && latest.minor == current.minor
        && latest.patch > current.patch
    {
        Severity::Patch
    } else {
        // Older or pre-release — treat as same so we don't downgrade.
        Severity::Same
    }
}

#[derive(Debug, Deserialize)]
struct LatestRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
}

/// Fetch the latest GitHub release for `repo` (owner/name) and decide whether
/// it should surface as a banner given `level` and `current_version`.
pub async fn check_for_update(
    repo: &str,
    current_version: &str,
    level: NotifyLevel,
    timeout: Duration,
) -> Option<UpdateInfo> {
    if level == NotifyLevel::Off {
        return None;
    }
    let current = parse_semver(current_version)?;
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(format!("artui/{current_version}"))
        .build()
        .ok()?;
    let response = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let release: LatestRelease = response.json().await.ok()?;
    let latest = parse_semver(&release.tag_name)?;
    let severity = classify(current, latest);
    if !severity.satisfies(level) {
        return None;
    }
    let url = if release.html_url.is_empty() {
        format!("https://github.com/{repo}/releases/latest")
    } else {
        release.html_url
    };
    Some(UpdateInfo {
        current: current_version.to_owned(),
        latest: release.tag_name,
        severity,
        release_url: url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_parse_handles_leading_v_and_prerelease() {
        assert_eq!(
            parse_semver("v1.2.3"),
            Some(SemVer {
                major: 1,
                minor: 2,
                patch: 3
            })
        );
        assert_eq!(
            parse_semver("0.0.1"),
            Some(SemVer {
                major: 0,
                minor: 0,
                patch: 1
            })
        );
        assert_eq!(
            parse_semver("v2.0.0-rc.1"),
            Some(SemVer {
                major: 2,
                minor: 0,
                patch: 0
            })
        );
        assert_eq!(parse_semver("not-a-version"), None);
    }

    #[test]
    fn classify_detects_each_severity() {
        let cur = SemVer {
            major: 0,
            minor: 0,
            patch: 1,
        };
        assert_eq!(
            classify(
                cur,
                SemVer {
                    major: 0,
                    minor: 0,
                    patch: 1
                }
            ),
            Severity::Same
        );
        assert_eq!(
            classify(
                cur,
                SemVer {
                    major: 0,
                    minor: 0,
                    patch: 2
                }
            ),
            Severity::Patch
        );
        assert_eq!(
            classify(
                cur,
                SemVer {
                    major: 0,
                    minor: 1,
                    patch: 0
                }
            ),
            Severity::Minor
        );
        assert_eq!(
            classify(
                cur,
                SemVer {
                    major: 1,
                    minor: 0,
                    patch: 0
                }
            ),
            Severity::Major
        );
        // Older release should not downgrade.
        assert_eq!(
            classify(
                SemVer {
                    major: 1,
                    minor: 0,
                    patch: 0
                },
                SemVer {
                    major: 0,
                    minor: 9,
                    patch: 0
                }
            ),
            Severity::Same
        );
    }

    #[test]
    fn major_level_silences_minor_and_patch() {
        assert!(Severity::Major.satisfies(NotifyLevel::Major));
        assert!(!Severity::Minor.satisfies(NotifyLevel::Major));
        assert!(!Severity::Patch.satisfies(NotifyLevel::Major));
    }

    #[test]
    fn minor_level_passes_major_and_minor_only() {
        assert!(Severity::Major.satisfies(NotifyLevel::Minor));
        assert!(Severity::Minor.satisfies(NotifyLevel::Minor));
        assert!(!Severity::Patch.satisfies(NotifyLevel::Minor));
    }

    #[test]
    fn all_level_passes_everything_except_same() {
        assert!(Severity::Major.satisfies(NotifyLevel::All));
        assert!(Severity::Minor.satisfies(NotifyLevel::All));
        assert!(Severity::Patch.satisfies(NotifyLevel::All));
        assert!(!Severity::Same.satisfies(NotifyLevel::All));
    }

    #[test]
    fn off_level_silences_everything() {
        assert!(!Severity::Major.satisfies(NotifyLevel::Off));
        assert!(!Severity::Minor.satisfies(NotifyLevel::Off));
        assert!(!Severity::Patch.satisfies(NotifyLevel::Off));
    }
}
