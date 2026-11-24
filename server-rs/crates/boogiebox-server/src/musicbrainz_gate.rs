//! One pace for every MusicBrainz request the server makes. MusicBrainz allows about one request a second per IP
//! address and answers 503 to anything faster, so the background passes (track and artist tags, artist ids) and
//! what a user is waiting for (Refresh Metadata, artist and album About) share one queue. A user's request goes
//! before the background passes' and is sent again (twice at most) if MusicBrainz still says 503.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::LazyLock;
use std::time::Duration;

use tokio::sync::Mutex;
use tokio::time::Instant;

/// Who is waiting for the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// Someone has a page open (About, Refresh Metadata).
    Foreground,
    /// A background pass.
    Background,
}

/// Time between two MusicBrainz requests (`BOOGIEBOX_MUSICBRAINZ_DELAY_MS`, default 1.1 s; 0 in unit tests).
pub fn interval() -> Duration {
    let default_ms = if cfg!(test) { 0 } else { 1100 };
    Duration::from_millis(
        std::env::var("BOOGIEBOX_MUSICBRAINZ_DELAY_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default_ms),
    )
}

/// The queue: when the next request may go, and how many foreground requests are waiting.
pub struct Gate {
    next: Mutex<Option<Instant>>,
    foreground_waiting: AtomicUsize,
}

struct Waiting<'a>(&'a AtomicUsize);

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Default for Gate {
    fn default() -> Self {
        Self::new()
    }
}

impl Gate {
    pub fn new() -> Self {
        Self {
            next: Mutex::new(None),
            foreground_waiting: AtomicUsize::new(0),
        }
    }

    /// Waits for this caller's turn and books the next slot `interval` later. A background caller steps aside
    /// whenever a foreground one is waiting.
    pub async fn wait(&self, priority: Priority, interval: Duration) {
        let _waiting = (priority == Priority::Foreground).then(|| {
            self.foreground_waiting.fetch_add(1, Ordering::SeqCst);
            Waiting(&self.foreground_waiting)
        });
        loop {
            if priority == Priority::Background && self.foreground_waiting() {
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
            let mut next = self.next.lock().await;
            if let Some(at) = *next {
                tokio::time::sleep_until(at).await;
            }
            // A foreground request that came in while this one slept goes first.
            if priority == Priority::Background && self.foreground_waiting() {
                continue;
            }
            *next = Some(Instant::now() + interval);
            return;
        }
    }

    fn foreground_waiting(&self) -> bool {
        self.foreground_waiting.load(Ordering::SeqCst) > 0
    }
}

static GATE: LazyLock<Gate> = LazyLock::new(Gate::new);

/// Sends a MusicBrainz request in its turn. A foreground request that gets 429/503 waits another turn and is
/// sent again, [`FOREGROUND_RETRIES`] times at most.
pub async fn send(
    request: reqwest::RequestBuilder,
    priority: Priority,
) -> reqwest::Result<reqwest::Response> {
    let retries = match priority {
        Priority::Foreground => FOREGROUND_RETRIES,
        Priority::Background => 0,
    };
    let mut request = request;
    for _ in 0..retries {
        let Some(again) = request.try_clone() else {
            break;
        };
        GATE.wait(priority, interval()).await;
        let response = request.send().await?;
        if !matches!(response.status().as_u16(), 429 | 503) {
            return Ok(response);
        }
        request = again;
    }
    GATE.wait(priority, interval()).await;
    request.send().await
}

/// How often a foreground request is sent again after a 429/503 (MusicBrainz also limits all BoogieBox servers
/// together by their user agent, so one turn in the queue isn't always enough).
const FOREGROUND_RETRIES: usize = 2;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const STEP: Duration = Duration::from_millis(1100);

    #[tokio::test(start_paused = true)]
    async fn requests_are_spaced_by_the_interval() {
        let gate = Gate::new();
        let start = Instant::now();
        gate.wait(Priority::Background, STEP).await;
        assert_eq!(
            Instant::now() - start,
            Duration::ZERO,
            "the first one goes at once"
        );
        gate.wait(Priority::Background, STEP).await;
        gate.wait(Priority::Foreground, STEP).await;
        assert_eq!(Instant::now() - start, STEP * 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_foreground_request_goes_before_waiting_background_ones() {
        let gate = Arc::new(Gate::new());
        gate.wait(Priority::Background, STEP).await; // the slot after this one is 1.1 s away
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut tasks = Vec::new();
        for name in ["background 1", "background 2"] {
            let (gate, order) = (gate.clone(), order.clone());
            tasks.push(tokio::spawn(async move {
                gate.wait(Priority::Background, STEP).await;
                order.lock().unwrap().push(name);
            }));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        let (fg_gate, fg_order) = (gate.clone(), order.clone());
        tasks.push(tokio::spawn(async move {
            fg_gate.wait(Priority::Foreground, STEP).await;
            fg_order.lock().unwrap().push("foreground");
        }));
        for task in tasks {
            task.await.unwrap();
        }
        let order = order.lock().unwrap();
        assert_eq!(order[0], "foreground", "{order:?}");
        assert_eq!(order.len(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn a_dropped_wait_leaves_the_slot_and_the_count_alone() {
        let gate = Gate::new();
        gate.wait(Priority::Background, STEP).await;
        let waiting = tokio::time::timeout(
            Duration::from_millis(100),
            gate.wait(Priority::Foreground, STEP),
        )
        .await;
        assert!(waiting.is_err(), "still waiting for the slot");
        assert!(!gate.foreground_waiting());
        let start = Instant::now();
        gate.wait(Priority::Background, STEP).await;
        assert_eq!(Instant::now() - start, Duration::from_millis(1000));
    }
    #[tokio::test]
    async fn a_foreground_request_is_sent_again_after_a_503_and_a_background_one_is_not() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/busy"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/busy"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let client = reqwest::Client::new();
        let url = format!("{}/busy", server.uri());
        let response = send(client.get(&url), Priority::Foreground).await.unwrap();
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(server.received_requests().await.unwrap().len(), 3);

        server.reset().await;
        Mock::given(method("GET"))
            .and(path("/busy"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let response = send(client.get(&url), Priority::Background).await.unwrap();
        assert_eq!(response.status().as_u16(), 503);
        let response = send(client.get(&url), Priority::Foreground).await.unwrap();
        assert_eq!(
            response.status().as_u16(),
            503,
            "gives up after the retries"
        );
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1 + 1 + FOREGROUND_RETRIES
        );
    }
}
