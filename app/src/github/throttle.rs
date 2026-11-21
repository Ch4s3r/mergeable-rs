use regex::Regex;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

const NOTIFICATION_TRIGGERING_PATHS: [&str; 17] = [
    "/orgs/{org}/invitations",
    "/orgs/{org}/invitations/{invitation_id}",
    "/orgs/{org}/teams/{team_slug}/discussions",
    "/orgs/{org}/teams/{team_slug}/discussions/{discussion_number}/comments",
    "/repos/{owner}/{repo}/collaborators/{username}",
    "/repos/{owner}/{repo}/commits/{commit_sha}/comments",
    "/repos/{owner}/{repo}/issues",
    "/repos/{owner}/{repo}/issues/{issue_number}/comments",
    "/repos/{owner}/{repo}/pulls",
    "/repos/{owner}/{repo}/pulls/{pull_number}/comments",
    "/repos/{owner}/{repo}/pulls/{pull_number}/comments/{comment_id}/replies",
    "/repos/{owner}/{repo}/pulls/{pull_number}/merge",
    "/repos/{owner}/{repo}/pulls/{pull_number}/requested_reviewers",
    "/repos/{owner}/{repo}/pulls/{pull_number}/reviews",
    "/repos/{owner}/{repo}/releases",
    "/teams/{team_id}/discussions",
    "/teams/{team_id}/discussions/{discussion_number}/comments",
];

static NOTIFICATION_PATH_PATTERN: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let alternatives: Vec<String> = NOTIFICATION_TRIGGERING_PATHS
        .iter()
        .map(|path| {
            let segments: Vec<String> = path
                .split('/')
                .map(|segment| {
                    if segment.starts_with('{') {
                        "(?:.+?)".to_string()
                    } else {
                        regex::escape(segment)
                    }
                })
                .collect();
            format!("(?:{})", segments.join("/"))
        })
        .collect();
    Regex::new(&format!("(?i)^(?:{})[^/]*$", alternatives.join("|"))).ok()
});

struct SpacedLane {
    minimum_interval: Duration,
    next_start: Mutex<Option<Instant>>,
}

impl SpacedLane {
    fn new(minimum_interval: Duration) -> Self {
        SpacedLane {
            minimum_interval,
            next_start: Mutex::new(None),
        }
    }

    async fn wait_turn(&self) {
        let mut next_start = self.next_start.lock().await;
        let now = Instant::now();
        let start = match *next_start {
            Some(scheduled) if scheduled > now => scheduled,
            _ => now,
        };
        *next_start = Some(start + self.minimum_interval);
        drop(next_start);
        tokio::time::sleep_until(start).await;
    }
}

#[derive(Clone)]
pub struct RequestThrottle {
    enabled: bool,
    concurrency: Arc<Semaphore>,
    writes: Arc<SpacedLane>,
    searches: Arc<SpacedLane>,
    notifications: Arc<SpacedLane>,
}

impl RequestThrottle {
    pub fn like_probot(enabled: bool) -> Self {
        RequestThrottle {
            enabled,
            concurrency: Arc::new(Semaphore::new(10)),
            writes: Arc::new(SpacedLane::new(Duration::from_millis(1000))),
            searches: Arc::new(SpacedLane::new(Duration::from_millis(2000))),
            notifications: Arc::new(SpacedLane::new(Duration::from_millis(3000))),
        }
    }

    pub async fn acquire(&self, method: &str, pathname: &str) -> Option<OwnedSemaphorePermit> {
        if !self.enabled {
            return None;
        }
        let is_write = method != "GET" && method != "HEAD";
        if is_write || pathname.starts_with("/graphql") {
            self.writes.wait_turn().await;
        }
        if is_write
            && NOTIFICATION_PATH_PATTERN
                .as_ref()
                .is_some_and(|pattern| pattern.is_match(pathname))
        {
            self.notifications.wait_turn().await;
        }
        if method == "GET" && pathname.starts_with("/search/") {
            self.searches.wait_turn().await;
        }
        Arc::clone(&self.concurrency).acquire_owned().await.ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_comment_creation_is_a_notification_path() {
        let pattern = NOTIFICATION_PATH_PATTERN.as_ref().unwrap();
        assert!(pattern.is_match("/repos/acme/app/issues/1/comments"));
        assert!(!pattern.is_match("/repos/acme/app/check-runs"));
    }
}
