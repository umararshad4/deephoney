//! Recovery at the real HTTP boundary, with no external provider calls.
use engine::{Config, net::Net};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    task::JoinHandle,
};

struct Fixture {
    url: String,
    calls: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn start_fixture(reset_first: bool, statuses: Vec<u16>, retry_after: Option<&str>, partial_first: bool) -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let header = retry_after.map(|value| format!("Retry-After: {value}\r\n")).unwrap_or_default();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { break };
            let mut buffer = [0; 8192];
            if socket.read(&mut buffer).await.unwrap_or(0) == 0 {
                continue;
            }
            let index = observed.fetch_add(1, Ordering::SeqCst);
            if reset_first && index == 0 {
                continue;
            }
            if partial_first && index == statuses.iter().position(|status| *status == 200).unwrap_or(0) {
                let _ = socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 1000\r\nConnection: close\r\n\r\npartial")
                    .await;
                continue;
            }
            let status = statuses[index.min(statuses.len() - 1)];
            let body = r#"{"results":[{"url":"https://example.org/source","title":"A recovered public document with useful evidence.","content":"Recorded reference"}]}"#;
            let response = format!(
                "HTTP/1.1 {status} Fixture\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n{header}Connection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });
    Fixture { url, calls, task }
}

#[tokio::test]
async fn temporary_transport_failure_recovers_a_get() {
    let fixture = start_fixture(true, vec![200], None, false).await;
    let net = Net::new(&Config::default());
    let page = net.page(&fixture.url).await.expect("temporary dropped connection should recover");
    assert!(page.text.contains("recovered public document"));
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 2);
    assert_eq!((net.stats().requests, net.stats().retries, net.stats().recovered), (2, 1, 1));
    assert!(page.warnings.iter().any(|w| w.contains("retry")), "recovery must be visible");
}

#[tokio::test]
async fn temporary_statuses_recover_but_attempts_are_bounded() {
    let fixture = start_fixture(false, vec![500, 504, 200], Some("0"), false).await;
    let net = Net::new(&Config::default());
    assert!(net.page(&fixture.url).await.is_ok());
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 3);
    let failed = start_fixture(false, vec![503], Some("0"), false).await;
    assert!(net.page(&failed.url).await.is_err());
    assert_eq!(failed.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn permanent_status_and_generic_post_are_not_retried() {
    for (status, post) in [(304, false), (403, false), (404, false), (503, true)] {
        let fixture = start_fixture(false, vec![status], Some("0"), false).await;
        let net = Net::new(&Config::default());
        let request = if post { net.http.post(&fixture.url).body("do not replay") } else { net.http.get(&fixture.url) };
        assert!(net.send(request).await.is_err());
        assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn cancelling_backoff_stops_future_requests() {
    let fixture = start_fixture(false, vec![429, 200], Some("10"), false).await;
    let net = Net::new(&Config::default());
    assert!(tokio::time::timeout(std::time::Duration::from_millis(150), net.page(&fixture.url)).await.is_err());
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn long_retry_after_is_not_shortened_and_retried_early() {
    let fixture = start_fixture(false, vec![429, 200], Some("120"), false).await;
    let net = Net::new(&Config::default());
    let error = net.page(&fixture.url).await.unwrap_err();
    assert!(error.to_string().contains("Retry-After"));
    assert_eq!(fixture.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn interrupted_page_and_search_bodies_restart_within_the_same_budget() {
    let fixture = start_fixture(false, vec![200], None, true).await;
    let net = Net::new(&Config::default());
    assert!(net.page(&fixture.url).await.unwrap().text.contains("recovered public document"));
    assert_eq!((net.stats().requests, net.stats().retries, net.stats().recovered), (2, 1, 1));
    let search = start_fixture(false, vec![200], None, true).await;
    let cfg = Config { searxng_url: search.url.trim_end_matches('/').into(), ..Default::default() };
    let net = Net::new(&cfg);
    let hits = engine::sources::Source::Searx.search(&net, &cfg, "record").await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!((net.stats().requests, net.stats().retries, net.stats().recovered), (2, 1, 1));
}

#[tokio::test]
async fn headers_and_partial_bodies_share_three_total_attempts() {
    let fixture = start_fixture(false, vec![503, 200, 200], Some("0"), true).await;
    let net = Net::new(&Config::default());
    assert!(net.page(&fixture.url).await.is_ok());
    assert_eq!((net.stats().requests, net.stats().retries, net.stats().recovered), (3, 2, 1));
    let failed = start_fixture(false, vec![200, 503], Some("0"), true).await;
    let net = Net::new(&Config::default());
    assert!(net.page(&failed.url).await.is_err());
    assert_eq!((net.stats().requests, net.stats().retries, net.stats().recovered), (3, 2, 0));
}
