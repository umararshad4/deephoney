use std::sync::{Arc, Mutex};

use engine::{Config, Event, Mode, Provider, Request, Researcher, research::DeepResearch, store::Store};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
    time::{Duration, timeout},
};

const FIRST: &str = "Meridian's first public release shipped during March 2024.";
const SECOND: &str = "Meridian became available in September 2025 following an audit delay.";

#[derive(Clone, Copy)]
enum Scenario {
    Investigation,
    FailedSearch,
    NoEvidence,
    GapFailure,
    EmptyReport,
    RedTeam,
    Saturation,
    LongDocument,
}

struct HttpRequest {
    target: String,
    body: Value,
    authorization: Option<String>,
}

struct Fixture {
    base: String,
    requests: Arc<Mutex<Vec<HttpRequest>>>,
    task: JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Fixture {
    async fn new(scenario: Scenario) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        let origin = base.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let header_end = loop {
                    let mut chunk = [0; 4096];
                    let n = socket.read(&mut chunk).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                    if let Some(i) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                        break i + 4;
                    }
                    assert!(bytes.len() < 1_048_576);
                };
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let target = headers.lines().next().unwrap().split_whitespace().nth(1).unwrap().to_owned();
                let length = headers
                    .lines()
                    .filter_map(|s| s.split_once(':'))
                    .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                    .map(|(_, v)| v.trim().parse::<usize>().unwrap())
                    .unwrap_or(0);
                let authorization = headers
                    .lines()
                    .filter_map(|line| line.split_once(':'))
                    .find(|(name, _)| name.eq_ignore_ascii_case("authorization"))
                    .map(|(_, value)| value.trim().to_owned());
                while bytes.len() < header_end + length {
                    let mut chunk = [0; 4096];
                    let n = socket.read(&mut chunk).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&chunk[..n]);
                }
                let body = serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap_or(Value::Null);
                let request = HttpRequest { target, body, authorization };
                let (status, content_type, body) = response(&request, &origin, scenario);
                log.lock().unwrap().push(request);
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            }
        });
        Self { base, requests, task }
    }

    fn config(&self, max_pages: usize) -> Config {
        Config {
            provider: Provider::OpenAi,
            llm_base: self.base.clone(),
            strong_model: "fixture-planner".into(),
            fast_model: "fixture-reader".into(),
            searxng_url: self.base.clone(),
            max_pages,
            ..Config::default()
        }
    }
}

fn response(req: &HttpRequest, base: &str, scenario: Scenario) -> (&'static str, &'static str, String) {
    let ok = |value: Value| ("200 OK", "application/json", value.to_string());
    if req.target.starts_with("/search?") {
        let url = url::Url::parse(&format!("{base}{}", req.target)).unwrap();
        let q = url.query_pairs().find(|(k, _)| k == "q").unwrap().1.into_owned();
        if q.starts_with("broken") {
            return ("400 Bad Request", "text/plain", "Search unavailable".into());
        }
        if q == "empty" {
            return ok(json!({"results":[]}));
        }
        let page = if q.starts_with("repeat") {
            q.as_str()
        } else if q == "counterevidence" {
            "second"
        } else {
            "first"
        };
        return ok(json!({"results":[
            {"url":format!("{base}/{page}#one"),"title":page,"content":"Search snippets are not evidence."},
            {"url":format!("{base}/{page}#two"),"title":page,"content":"Duplicate result."}
        ]}));
    }
    if matches!(scenario, Scenario::RedTeam) && req.target == "/first" {
        return (
            "200 OK",
            "text/html",
            format!(
                "<html><body><article><p>{FIRST}</p><a href='/stale1.txt'>Record one</a><a href='/stale2.txt'>Record two</a><a href='/stale3.txt'>Record three</a></article></body></html>"
            ),
        );
    }
    if req.target.starts_with("/stale") || req.target.starts_with("/repeat") {
        return ("200 OK", "text/plain", FIRST.into());
    }
    if matches!(scenario, Scenario::LongDocument) && req.target == "/first" {
        return ("200 OK", "text/plain", format!("{}\n{FIRST}", "背景资料 ".repeat(8_000)));
    }
    if req.target == "/first" || req.target == "/second" {
        let text = if req.target == "/first" { FIRST } else { SECOND };
        return ("200 OK", "text/plain", text.into());
    }
    assert_eq!(req.target, "/chat/completions");
    let system = req.body["messages"][0]["content"].as_str().unwrap();
    let user = req.body["messages"][1]["content"].as_str().unwrap();
    let plan = |query: Value| {
        json!({"done":false,"lenses":[{
            "name":"Timeline","question":"When did Meridian launch?","sources":["searxng"],"queries":query
        }]})
        .to_string()
    };
    let repeat_plan = |index| {
        json!({"done":false,"lenses":[{
        "name":format!("Perspective {index}"),"question":"When did Meridian launch?","sources":["searxng"],"queries":[format!("repeat{index}")]
    }]}).to_string()
    };
    let content = if matches!(scenario, Scenario::Saturation) && system.contains("RESEARCH GAPS") {
        repeat_plan(if user.contains("repeat1") { 2 } else { 1 })
    } else if matches!(scenario, Scenario::Saturation) && system.contains("RESEARCH PLAN") {
        repeat_plan(0)
    } else if system.contains("RESEARCH RED TEAM") {
        plan(json!(["counterevidence"]))
    } else if system.contains("RESEARCH GAPS") {
        if matches!(scenario, Scenario::GapFailure) { "not valid JSON".into() } else { plan(json!(["counterevidence"])) }
    } else if system.contains("RESEARCH PLAN") {
        if matches!(scenario, Scenario::FailedSearch) { plan(json!(["broken", "launch"])) } else { plan(json!(["launch"])) }
    } else if system.contains("EXTRACT CLAIMS") {
        let text = if user.contains(SECOND) { SECOND } else { FIRST };
        let claims = if matches!(scenario, Scenario::NoEvidence) {
            json!([{"text":"Invented outcome", "quote":"This quote does not exist on the page."}])
        } else {
            json!([
                {"text":text,"quote":text},
                {"text":"Another invented outcome", "quote":"This quote does not exist on the page."}
            ])
        };
        json!({"claims":claims}).to_string()
    } else if system.contains("REVIEW EVIDENCE") {
        if matches!(scenario, Scenario::Investigation | Scenario::RedTeam) {
            json!({"contradictions":[[0,1]],"verdicts":[
                {"claim":0,"verdict":"unresolved","reason":"Conflicting dated reports","evidence":[0,1]},
                {"claim":1,"verdict":"unresolved","reason":"Conflicting dated reports","evidence":[0,1]}
            ]})
            .to_string()
        } else {
            json!({"contradictions":[],"verdicts":[]}).to_string()
        }
    } else if system.contains("VERIFY REPORT STATEMENTS") {
        json!({"supported":[0,1]}).to_string()
    } else {
        assert!(system.contains("You write the final report"), "Unexpected model prompt: {system}");
        if matches!(scenario, Scenario::EmptyReport) {
            String::new()
        } else if matches!(scenario, Scenario::Investigation | Scenario::RedTeam) {
            json!({"summary":[{"text":"Sources disagree about the launch date.","claims":[0,1]}],"findings":[{"text":"Sources disagree about the launch date.","claims":[0,1]}],"contested":[]}).to_string()
        } else {
            { let text = if user.contains(SECOND) { SECOND } else { FIRST };
            json!({"summary":[{"text":text,"claims":[0]}],"findings":[{"text":text,"claims":[0]}],"contested":[]}).to_string() }
        }
    };
    ok(json!({"choices":[{"message":{"content":content}}],"usage":{"prompt_tokens":11,"completion_tokens":7}}))
}

fn request(depth: usize, breadth: usize) -> Request {
    Request { topic: "When did Meridian launch?".into(), mode: Mode::Normal, depth, breadth, plan: None }
}

#[tokio::test]
async fn researches_gaps_and_saves_a_cited_report() {
    let fixture = Fixture::new(Scenario::Investigation).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let report = timeout(Duration::from_secs(20), DeepResearch.run(fixture.config(2), request(2, 1), tx)).await.unwrap().unwrap();
    assert_eq!(report.ledger.sources.len(), 2);
    assert_eq!(report.ledger.claims.len(), 2, "ungrounded model claims must be dropped");
    assert_eq!(report.ledger.total_voices(), 2);
    assert_eq!(report.ledger.claims[0].contradicts, vec![1]);
    assert_eq!(report.ledger.claims[1].contradicts, vec![0]);
    assert!(report.markdown.contains("[1, 2]"), "{}", report.markdown);
    assert!(!report.markdown.contains("999"));
    assert!(report.markdown.contains("## Sources"));
    let mut queries = 0;
    let mut pages = 0;
    let mut claims = 0;
    let mut last_stats = None;
    while let Ok(event) = rx.try_recv() {
        match event {
            Event::Query { .. } => queries += 1,
            Event::Page { .. } => pages += 1,
            Event::Claim { .. } => claims += 1,
            Event::Stats { pages, sources, voices, claims } => last_stats = Some((pages, sources, voices, claims)),
            _ => {}
        }
    }
    assert_eq!((queries, pages, claims), (2, 2, 2));
    assert_eq!(last_stats, Some((2, 2, 2, 2)));
    let log = fixture.requests.lock().unwrap();
    assert_eq!(log.iter().filter(|r| r.target == "/first").count(), 1);
    assert_eq!(log.iter().filter(|r| r.target == "/second").count(), 1);
    assert!(log.iter().filter(|r| r.target == "/chat/completions").any(|r| r.body["model"] == "fixture-reader"));
    let store = Store::memory().unwrap();
    let id = store.save(&report).unwrap();
    let restored = store.load(id).unwrap();
    assert_eq!(restored.markdown, report.markdown);
    assert_eq!(restored.ledger.claims[0].contradicts, vec![1]);
}

#[tokio::test]
async fn a_failed_search_warns_and_other_queries_still_work() {
    let fixture = Fixture::new(Scenario::FailedSearch).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let report = timeout(Duration::from_secs(20), DeepResearch.run(fixture.config(1), request(1, 2), tx)).await.unwrap().unwrap();
    assert_eq!(report.ledger.claims.len(), 1);
    assert!(std::iter::from_fn(|| rx.try_recv().ok()).any(|e| matches!(e, Event::Warn(_))));
}

#[tokio::test]
async fn no_grounded_evidence_returns_an_error_without_writing_a_report() {
    let fixture = Fixture::new(Scenario::NoEvidence).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = timeout(Duration::from_secs(20), DeepResearch.run(fixture.config(1), request(1, 1), tx)).await.unwrap();
    assert!(result.is_err());
    assert!(
        !fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| { r.body["messages"][0]["content"].as_str().is_some_and(|s| s.contains("You write the final report")) })
    );
}

#[tokio::test]
async fn a_failed_gap_plan_keeps_collected_evidence() {
    let fixture = Fixture::new(Scenario::GapFailure).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let report = timeout(Duration::from_secs(20), DeepResearch.run(fixture.config(2), request(2, 1), tx)).await.unwrap().unwrap();
    assert_eq!(report.ledger.claims.len(), 1);
    assert!(report.markdown.contains("## Sources"));
    assert!(std::iter::from_fn(|| rx.try_recv().ok()).any(|e| matches!(e, Event::Warn(_))));
}

#[tokio::test]
async fn an_empty_model_response_cannot_be_saved_as_a_finished_report() {
    let fixture = Fixture::new(Scenario::EmptyReport).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = timeout(Duration::from_secs(20), DeepResearch.run(fixture.config(1), request(1, 1), tx)).await.unwrap();
    assert!(result.is_err(), "an empty model response must fail report writing");
}

#[tokio::test]
async fn planning_spends_no_search_or_page_requests() {
    let fixture = Fixture::new(Scenario::FailedSearch).await;
    let plan = DeepResearch.plan(fixture.config(2), request(2, 2)).await.unwrap();
    assert_eq!(plan.lenses.len(), 1);
    assert!(fixture.requests.lock().unwrap().iter().all(|r| r.target == "/chat/completions"));
}

#[tokio::test]
async fn the_edited_plan_runs_without_replacing_it_with_a_model_plan() {
    let fixture = Fixture::new(Scenario::GapFailure).await;
    let mut req = request(1, 1);
    req.plan = Some(engine::ResearchPlan {
        done: false,
        lenses: vec![engine::Lens {
            name: "Edited timeline".into(),
            question: "Check the alternate date".into(),
            sources: vec!["searxng".into()],
            queries: vec!["counterevidence".into()],
        }],
    });
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let report = DeepResearch.run(fixture.config(1), req, tx).await.unwrap();
    assert_eq!(report.ledger.claims[0].text, SECOND);
    assert!(
        !fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| { r.body["messages"][0]["content"].as_str().is_some_and(|s| s.contains("RESEARCH PLAN")) })
    );
}

#[tokio::test]
async fn closing_the_event_receiver_cancels_an_inflight_model_request() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 4096];
        let _ = stream.read(&mut bytes).await.unwrap();
        let _ = accepted_tx.send(());
        std::future::pending::<()>().await;
    });
    let cfg = Config { provider: Provider::OpenAi, llm_base: format!("http://{address}"), ..Config::default() };
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let research = tokio::spawn(DeepResearch.run(cfg, request(1, 1), tx));
    timeout(Duration::from_secs(3), accepted_rx).await.unwrap().unwrap();
    drop(rx);
    let result = timeout(Duration::from_secs(1), research).await.unwrap().unwrap();
    server.abort();
    assert!(result.is_err());
}

#[tokio::test]
async fn red_team_reserves_pages_for_counterevidence_ahead_of_old_leads() {
    let fixture = Fixture::new(Scenario::RedTeam).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut req = request(1, 1);
    req.mode = Mode::Adversarial;
    let report = timeout(Duration::from_secs(25), DeepResearch.run(fixture.config(4), req, tx)).await.unwrap().unwrap();
    assert!(report.ledger.claims.iter().any(|c| c.text == SECOND));
    assert!(std::iter::from_fn(|| rx.try_recv().ok()).any(|e| matches!(e, Event::Status(s) if s.contains("Debating contested"))));
    let log = fixture.requests.lock().unwrap();
    assert!(log.iter().any(|r| r.target == "/second"));
    assert_eq!(log.iter().filter(|r| r.target.starts_with("/stale")).count(), 2);
    assert_eq!(log.iter().filter(|r| r.target == "/first" || r.target == "/second" || r.target.starts_with("/stale")).count(), 4);
}

#[tokio::test]
async fn two_rounds_of_repeated_evidence_stop_before_maximum_depth() {
    let fixture = Fixture::new(Scenario::Saturation).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let report = timeout(Duration::from_secs(20), DeepResearch.run(fixture.config(20), request(5, 1), tx)).await.unwrap().unwrap();
    assert_eq!(report.ledger.claims.len(), 1);
    assert_eq!(report.ledger.total_voices(), 1);
    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(events.iter().any(|e| matches!(e, Event::Status(s) if s.contains("Research saturated"))));
    let rounds: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::Round { round, new_claims, .. } => Some((*round, *new_claims)),
            _ => None,
        })
        .collect();
    assert_eq!(rounds, vec![(1, 1), (2, 0), (3, 0)]);
    assert_eq!(fixture.requests.lock().unwrap().iter().filter(|r| r.target.starts_with("/search?")).count(), 3);
}

#[tokio::test]
async fn evidence_after_fourteen_thousand_unicode_characters_reaches_the_report() {
    let fixture = Fixture::new(Scenario::LongDocument).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let report = timeout(Duration::from_secs(30), DeepResearch.run(fixture.config(1), request(1, 1), tx)).await.unwrap().unwrap();
    assert_eq!(report.ledger.claims.len(), 1);
    assert_eq!(report.ledger.claims[0].text, FIRST);
    assert!(report.markdown.contains(FIRST), "late evidence must appear in report provenance");
    let requests = fixture.requests.lock().unwrap();
    let reads: Vec<_> =
        requests.iter().filter(|r| r.body["messages"][0]["content"].as_str().unwrap_or("").contains("EXTRACT CLAIMS")).collect();
    assert!(reads.len() >= 4, "all retained sections must be read");
    assert!(reads.iter().any(|r| { r.body["messages"][1]["content"].as_str().unwrap().contains(FIRST) }));
    assert!(reads.iter().any(|r| { !r.body["messages"][1]["content"].as_str().unwrap().contains(FIRST) }));
}

#[tokio::test]
async fn independent_verifier_receives_review_calls_without_the_writer_credential() {
    let writer = Fixture::new(Scenario::FailedSearch).await;
    let verifier = Fixture::new(Scenario::FailedSearch).await;
    let cfg = Config {
        llm_key: "writer-fixture-secret".into(),
        verifier_provider: Provider::OpenAi,
        verifier_base: verifier.base.clone(),
        verifier_model: "fixture-independent-verifier".into(),
        ..writer.config(1)
    };
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let report = timeout(Duration::from_secs(30), DeepResearch.run(cfg, request(1, 2), tx)).await.unwrap().unwrap();
    let writer_log = writer.requests.lock().unwrap();
    let verifier_log = verifier.requests.lock().unwrap();
    let writer_calls: Vec<_> = writer_log.iter().filter(|r| r.target == "/chat/completions").collect();
    assert!(!writer_calls.is_empty());
    assert!(writer_calls.iter().all(|r| r.authorization.as_deref() == Some("Bearer writer-fixture-secret")));
    assert!(!writer_calls.iter().any(|r| { r.body["messages"][0]["content"].as_str().unwrap_or("").contains("VERIFY REPORT STATEMENTS") }));
    assert!(verifier_log.iter().any(|r| { r.body["messages"][0]["content"].as_str().unwrap_or("").contains("VERIFY REPORT STATEMENTS") }));
    assert!(verifier_log.iter().any(|r| { r.body["messages"][0]["content"].as_str().unwrap_or("").contains("REVIEW EVIDENCE") }));
    assert!(verifier_log.iter().all(|r| r.authorization.is_none()), "a different endpoint cannot inherit the writer key");
    assert!(verifier_log.iter().all(|r| r.body["model"] == "fixture-independent-verifier"));
    let model_calls = writer_calls.len() + verifier_log.len();
    assert_eq!(report.metrics.model_calls, model_calls);
    assert_eq!(report.metrics.input_tokens, Some(model_calls as u64 * 11));
    assert_eq!(report.metrics.output_tokens, Some(model_calls as u64 * 7));
    assert_eq!(report.metrics.page_attempts, 1);
    assert_eq!(report.metrics.search_requests, 2);
    assert!(report.metrics.elapsed_ms > 0);
    assert!(report.metrics.separate_verifier);
    assert_eq!(report.metrics.verifier_model, "fixture-independent-verifier");
    let store = Store::memory().unwrap();
    let saved = store.save(&report).unwrap();
    assert_eq!(store.load(saved).unwrap().metrics.model_calls, model_calls);
}

fn supplied_queries(queries: &[&str]) -> Request {
    let mut req = request(1, 6);
    req.plan = Some(engine::ResearchPlan {
        done: false,
        lenses: queries
            .chunks(6)
            .enumerate()
            .map(|(i, chunk)| engine::Lens {
                name: format!("Adapter reliability {i}"),
                question: "When did Meridian launch?".into(),
                sources: vec!["searxng".into()],
                queries: chunk.iter().map(|q| (*q).into()).collect(),
            })
            .collect(),
    });
    req
}

#[tokio::test]
async fn repeated_adapter_failure_pauses_remaining_queries_only_for_this_run() {
    let fixture = Fixture::new(Scenario::FailedSearch).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let req = supplied_queries(&[
        "broken1", "broken2", "broken3", "broken4", "broken5", "broken6", "broken7", "broken8", "broken9", "broken10", "broken11",
        "broken12",
    ]);
    assert!(DeepResearch.run(fixture.config(1), req, tx).await.is_err());
    let queries = fixture.requests.lock().unwrap().iter().filter(|r| r.target.starts_with("/search?")).count();
    assert!(queries <= 6, "remaining failing queries should be skipped; made {queries}");
    assert!(std::iter::from_fn(|| rx.try_recv().ok()).any(|e| matches!(e, Event::Warn(s) if s.contains("paused for this investigation"))));
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let report = DeepResearch.run(fixture.config(1), supplied_queries(&["launch"]), tx).await.unwrap();
    assert_eq!(report.metrics.search_requests, 1);
    assert_eq!(report.ledger.claims.len(), 1);
}

#[tokio::test]
async fn successful_empty_search_resets_adapter_failures_and_keeps_later_queries() {
    let fixture = Fixture::new(Scenario::FailedSearch).await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let report = DeepResearch
        .run(fixture.config(1), supplied_queries(&["broken1", "broken2", "empty", "broken3", "broken4", "launch"]), tx)
        .await
        .unwrap();
    assert_eq!(report.metrics.search_requests, 6);
    assert_eq!(report.ledger.claims.len(), 1);
    assert!(!std::iter::from_fn(|| rx.try_recv().ok()).any(|e| matches!(e, Event::Warn(s) if s.contains("paused for this investigation"))));
}

#[tokio::test]
async fn search_pause_is_retained_in_saved_report() {
    let fixture = Fixture::new(Scenario::FailedSearch).await;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let report = DeepResearch
        .run(
            fixture.config(1),
            supplied_queries(&[
                "launch", "broken1", "broken2", "broken3", "broken4", "broken5", "broken6", "broken7", "broken8", "broken9", "broken10",
                "broken11",
            ]),
            tx,
        )
        .await
        .unwrap();
    assert!(report.metrics.search_requests <= 7);
    assert!(report.markdown.contains("Reduced search coverage: searxng paused"));
    let store = Store::memory().unwrap();
    let id = store.save(&report).unwrap();
    assert!(store.load(id).unwrap().markdown.contains("remaining queries were skipped"));
}

#[tokio::test]
async fn closing_receiver_cancels_an_inflight_search_without_waiting_for_retry_budget() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 4096];
        let _ = socket.read(&mut bytes).await.unwrap();
        let _ = accepted_tx.send(());
        std::future::pending::<()>().await;
    });
    let cfg = Config { provider: Provider::OpenAi, llm_base: base.clone(), searxng_url: base, ..Config::default() };
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let research = tokio::spawn(DeepResearch.run(cfg, supplied_queries(&["launch"]), tx));
    timeout(Duration::from_secs(3), accepted_rx).await.unwrap().unwrap();
    drop(rx);
    assert!(timeout(Duration::from_secs(1), research).await.unwrap().unwrap().is_err());
    server.abort();
}
