//! Version parsing and ordering.
//!
//! Debug builds carry a build number after the first hyphen
//! (`2.0.0-65-abc1234`); that number is this project's ordering key, not
//! SemVer build metadata. Real pre-release tags (`1.2.0-beta.11`) fall back to
//! SemVer 2.0.0 identifier precedence, where numeric identifiers compare
//! numerically and a normal version outranks its pre-release.

use std::cmp::Ordering;

/// A parsed version. `build` is `Some` only for dev builds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub build: Option<u64>,
    /// Dot-separated pre-release identifiers, empty for a normal version.
    pub pre: Vec<String>,
    pub text: String,
}

impl Version {
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        let mut groups = text.split('-');
        let core = groups.next()?;
        let mut it = core.split('.');
        let major = it.next()?.parse().ok()?;
        let minor = it.next().unwrap_or("0").parse().ok()?;
        let patch = it.next().unwrap_or("0").parse().ok()?;
        if it.next().is_some() {
            return None;
        }
        let rest: Vec<&str> = groups.collect();
        let second = rest.first().copied();
        let build = second.and_then(|g| g.parse::<u64>().ok());
        // A real pre-release tag has no numeric build number in the first
        // segment; keep its identifiers for SemVer ordering.
        let pre = if build.is_none() && text.contains('-') {
            text.split_once('-')?
                .1
                .split('.')
                .map(str::to_owned)
                .collect()
        } else {
            Vec::new()
        };
        Some(Self {
            major,
            minor,
            patch,
            build,
            pre,
            text: text.to_owned(),
        })
    }

    /// Is `self` newer than `installed`?
    #[must_use]
    pub fn is_newer_than(&self, installed: &Self) -> bool {
        let mine = (self.major, self.minor, self.patch);
        let theirs = (installed.major, installed.minor, installed.patch);
        if mine != theirs {
            return mine > theirs;
        }
        match (self.build, installed.build) {
            (None, Some(_)) => return true,
            (Some(a), Some(b)) => return a > b,
            (Some(_), None) => return false,
            (None, None) => {}
        }
        pre_greater(&self.pre, &installed.pre)
    }
}

fn pre_greater(a: &[String], b: &[String]) -> bool {
    if a.is_empty() {
        return !b.is_empty();
    }
    if b.is_empty() {
        return false;
    }
    for (x, y) in a.iter().zip(b.iter()) {
        match cmp_ident(x, y) {
            Ordering::Equal => {}
            Ordering::Greater => return true,
            Ordering::Less => return false,
        }
    }
    a.len() > b.len()
}

fn cmp_ident(a: &str, b: &str) -> Ordering {
    match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => a.cmp(b),
    }
}
