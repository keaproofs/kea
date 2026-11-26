//! The HTTP routes.
//!
//! - `POST /v1/asks` asks about a contract. A proven report for the same code is answered at
//!   once and free; only new code starts a paid job, within per-requester and daily limits.
//! - `GET /v1/asks/{id}` follows a job; `GET /v1/contracts/{address}` reads a report;
//!   `GET /v1/contracts/{address}/test` is the test file, so anyone can re-run the proof;
//!   `GET /v1/contracts` lists recent reports; `GET /badge/{address}.svg` is the badge.

use crate::badge::Badge;
use crate::config::Config;
use crate::store::{JobState, NewJob, Report, Store, day_start, now};
use crate::view;
use alloy_primitives::{Address, B256, keccak256};
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use kea_chain::Rpc;
use serde::Deserialize;
use serde_json::{Value, json};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};
use tower_http::cors::{AllowOrigin, CorsLayer};

/// The most characters a question may have.
const MAX_QUESTION: usize = 280;

/// What every handler shares.
pub struct App {
    /// Reports and jobs.
    pub store: Store,
    /// Settings.
    pub config: Config,
    /// The chain, for what code runs at an address.
    pub rpc: Rpc,
    /// Wakes the worker when a job is queued.
    pub wake: Notify,
    /// Held while one ask passes the limits and joins the queue, so two at once cannot both take
    /// the last place a limit has.
    pub admission: Mutex<()>,
}

/// An error as the API shows it: a status and one sentence, never internals.
#[derive(Debug)]
pub struct Problem {
    status: StatusCode,
    message: String,
}

impl Problem {
    fn new(status: StatusCode, message: &str) -> Self {
        Self { status, message: message.into() }
    }
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        (self.status, Json(json!({"error": self.message}))).into_response()
    }
}

impl From<anyhow::Error> for Problem {
    fn from(error: anyhow::Error) -> Self {
        eprintln!("api: {error:#}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "something went wrong on our side")
    }
}

/// The router, with CORS for the configured site origins and small request bodies.
pub fn router(app: Arc<App>) -> Router {
    let origins: Vec<HeaderValue> = app.config.origins.iter().filter_map(|origin| origin.parse().ok()).collect();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::CONTENT_TYPE]);
    Router::new()
        .route("/v1/health", get(|| async { Json(json!({"ok": true})) }))
        .route("/v1/asks", post(ask))
        .route("/v1/asks/{id}", get(job))
        .route("/v1/contracts", get(recent))
        .route("/v1/contracts/{address}", get(contract))
        .route("/v1/contracts/{address}/test", get(test_file))
        .route("/badge/{file}", get(badge))
        .route("/feed.xml", get(feed_all))
        .route("/feed/{file}", get(feed_one))
        .layer(cors)
        .layer(DefaultBodyLimit::max(4096))
        .with_state(app)
}

fn parse_address(text: &str) -> Result<Address, Problem> {
    text.trim().parse().map_err(|_| Problem::new(StatusCode::BAD_REQUEST, "that is not a contract address"))
}

/// The address as the store keys it.
fn store_key(address: Address) -> String {
    format!("{address:#x}")
}

/// A question with control characters dropped and spaces collapsed; none when empty.
fn clean_question(question: Option<String>) -> Result<Option<String>, Problem> {
    let Some(question) = question else { return Ok(None) };
    let words: Vec<&str> =
        question.split(|c: char| c.is_whitespace() || c.is_control()).filter(|w| !w.is_empty()).collect();
    let cleaned = words.join(" ");
    if cleaned.chars().count() > MAX_QUESTION {
        return Err(Problem::new(StatusCode::BAD_REQUEST, "a question can be at most 280 characters"));
    }
    Ok((!cleaned.is_empty()).then_some(cleaned))
}

/// The implementation a proxy points at, and the keccak of the code that runs; none when no code
/// runs at the address.
///
/// # Errors
/// When the chain does not answer.
pub(crate) async fn identity(rpc: &Rpc, address: Address) -> anyhow::Result<Option<(Option<Address>, B256)>> {
    let proxy = kea_sources::proxy::resolve(rpc, address).await?;
    let runs = proxy.as_ref().map_or(address, |proxy| proxy.implementation);
    let code = rpc.code(runs).await?;
    Ok((!code.is_empty()).then(|| (proxy.map(|proxy| proxy.implementation), keccak256(&code))))
}

async fn code_identity(rpc: &Rpc, address: Address) -> Result<(Option<Address>, B256), Problem> {
    match identity(rpc, address).await {
        Ok(Some(found)) => Ok(found),
        Ok(None) => Err(Problem::new(StatusCode::UNPROCESSABLE_ENTITY, "there is no contract code at this address")),
        Err(_) => Err(Problem::new(StatusCode::BAD_GATEWAY, "the chain did not answer; try again")),
    }
}

/// Whether one more paid job fits today's budget, counting every open job at its reserve price.
///
/// # Errors
/// When the store fails.
pub(crate) fn within_budget(app: &App) -> anyhow::Result<bool> {
    let open = app.store.open_jobs()?;
    let reserved = f64::from(u32::try_from(open + 1).unwrap_or(u32::MAX)) * app.config.job_reserve_usd;
    Ok(app.store.spent_since(day_start(now()))? + reserved <= app.config.daily_budget_usd)
}

/// A salted hash of who is asking. Behind Cloudflare the client is `CF-Connecting-IP`; trusted
/// only when configured, since anyone can send the header to an open port.
fn requester(config: &Config, headers: &HeaderMap, peer: SocketAddr) -> String {
    let forwarded = config
        .trust_cloudflare
        .then(|| headers.get("cf-connecting-ip").and_then(|value| value.to_str().ok()))
        .flatten()
        .map(str::to_string);
    let client = forwarded.unwrap_or_else(|| peer.ip().to_string());
    let digest = keccak256([config.salt.as_bytes(), b"|", person(&client).as_bytes()].concat());
    format!("{:x}", B256::from(digest)).chars().take(32).collect()
}

/// One person's share of the address space. An IPv6 /64 belongs to one household or one machine,
/// which can use any address in it, so the limit counts the /64.
fn person(client: &str) -> String {
    let Ok(IpAddr::V6(v6)) = client.parse::<IpAddr>() else { return client.to_string() };
    if let Some(v4) = v6.to_ipv4_mapped() {
        return v4.to_string();
    }
    let s = v6.segments();
    format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
}

/// What an ask gets, from what the store already holds.
#[derive(Debug, PartialEq, Eq)]
enum Decision<'a> {
    /// The address's own report, for the code that runs there now.
    Own,
    /// Identical code proved at this other address.
    Twin(&'a str),
    /// A new check, steered by the asker's question or not.
    Check { steered: bool },
}

/// A current report answers every ask: a question or a fresh ask never replaces or steers a
/// published report. Identical code proved elsewhere answers too, unless the asker wants this
/// address proved itself. A question shapes only the first check of code nobody has proved; a
/// re-check of changed code, or a fresh check of proved code, runs unsteered.
fn decide<'a>(
    own: Option<&Report>,
    twin: Option<&'a Report>,
    code_hash: &str,
    fresh: bool,
    question: bool,
) -> Decision<'a> {
    match (own, twin) {
        (Some(report), _) if report.code_hash == code_hash => Decision::Own,
        (None, Some(twin)) if !fresh => Decision::Twin(&twin.address),
        // Changed code, or proved code an asker wants proved here: checked as it is.
        (Some(_), _) | (None, Some(_)) => Decision::Check { steered: false },
        (None, None) => Decision::Check { steered: question },
    }
}

/// What a failed job tells the public: one known reason, or a general line. The full error, which
/// can hold paths and endpoints, stays in the store.
fn public_error(error: &str) -> &'static str {
    let error = error.to_lowercase();
    if error.contains("older than") && error.contains("solc") {
        "this contract's compiler is older than any test library builds with"
    } else if error.contains("no verified source")
        || error.contains("not verified")
        || error.contains("fetching the subject")
    {
        "no verified source was found for this contract on Sourcify or Blockscout"
    } else {
        "the check could not finish; it has been logged"
    }
}

#[derive(Deserialize)]
struct AskBody {
    address: String,
    question: Option<String>,
    /// Prove this address itself even when identical code was proved elsewhere.
    #[serde(default)]
    fresh: bool,
}

async fn ask(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<AskBody>,
) -> Result<(StatusCode, Json<Value>), Problem> {
    let address = parse_address(&body.address)?;
    let question = clean_question(body.question)?;
    let key = store_key(address);
    let (implementation, code_hash) = code_identity(&app.rpc, address).await?;
    let code_hash = format!("{code_hash:#x}");
    let report_url = format!("/v1/contracts/{key}");
    let own = app.store.report(&key)?;
    let twin = if own.is_none() { app.store.twin(&code_hash, &key)? } else { None };
    let steered = match decide(own.as_ref(), twin.as_ref(), &code_hash, body.fresh, question.is_some()) {
        Decision::Own => return Ok((StatusCode::OK, Json(json!({"state": "done", "report": report_url})))),
        Decision::Twin(proved) => {
            return Ok((StatusCode::OK, Json(json!({"state": "done", "twin_of": proved, "report": report_url}))));
        }
        Decision::Check { steered } => steered,
    };
    let _admitting = app.admission.lock().await;
    if let Some(open) = app.store.open_job_for(&key)? {
        return Ok((StatusCode::ACCEPTED, Json(job_json(&app.store, open)?)));
    }
    let who = requester(&app.config, &headers, peer);
    let today = day_start(now());
    if app.store.asks_since(&who, today)? >= app.config.asks_per_day {
        return Err(Problem::new(
            StatusCode::TOO_MANY_REQUESTS,
            "you have used today's checks; proven reports stay free to read, and the limit resets at 00:00 UTC",
        ));
    }
    if app.store.open_jobs()? >= app.config.max_queue {
        return Err(Problem::new(StatusCode::SERVICE_UNAVAILABLE, "the queue is full; try again soon"));
    }
    if !within_budget(&app)? {
        return Err(Problem::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "today's proving budget is spent; it resets at 00:00 UTC",
        ));
    }
    let implementation = implementation.map(store_key);
    let id = app.store.enqueue(&NewJob {
        address: &key,
        question: if steered { question.as_deref() } else { None },
        requester: &who,
        code_hash: &code_hash,
        implementation: implementation.as_deref(),
    })?;
    app.wake.notify_one();
    Ok((StatusCode::ACCEPTED, Json(job_json(&app.store, id)?)))
}

fn job_json(store: &Store, id: i64) -> Result<Value, Problem> {
    let job = store.job(id)?.ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "there is no such job"))?;
    let position = if job.state == JobState::Queued { Some(store.position(id)?) } else { None };
    Ok(json!({
        "job": job.id,
        "address": job.address,
        "state": job.state.as_str(),
        "position": position,
        "error": job.error.as_deref().map(public_error),
        "report": (job.state == JobState::Done).then(|| format!("/v1/contracts/{}", job.address)),
        "created_at": job.created_at,
        "started_at": job.started_at,
        "finished_at": job.finished_at,
    }))
}

async fn job(State(app): State<Arc<App>>, Path(id): Path<i64>) -> Result<Json<Value>, Problem> {
    Ok(Json(job_json(&app.store, id)?))
}

#[derive(Deserialize)]
struct Recent {
    limit: Option<usize>,
}

async fn recent(State(app): State<Arc<App>>, Query(query): Query<Recent>) -> Result<Json<Value>, Problem> {
    let reports = app.store.recent(query.limit.unwrap_or(20).min(50))?;
    let mut listed = Vec::with_capacity(reports.len());
    for report in &reports {
        let mut line = view::listed(report);
        line["stale"] = app.store.stale_since(&report.address)?.is_some().into();
        listed.push(line);
    }
    Ok(Json(json!({"reports": listed})))
}

/// The report for an address, or for identical code proved at another address.
async fn find(app: &App, address: Address) -> Result<(Report, Option<String>), Problem> {
    let key = store_key(address);
    if let Some(report) = app.store.report(&key)? {
        return Ok((report, None));
    }
    let (_, code_hash) = code_identity(&app.rpc, address).await?;
    match app.store.twin(&format!("{code_hash:#x}"), &key)? {
        Some(twin) => {
            let proved = twin.address.clone();
            Ok((twin, Some(proved)))
        }
        None => Err(Problem::new(StatusCode::NOT_FOUND, "this contract has not been checked yet")),
    }
}

async fn contract(State(app): State<Arc<App>>, Path(address): Path<String>) -> Result<Json<Value>, Problem> {
    let address = parse_address(&address)?;
    let (report, twin_of) = find(&app, address).await?;
    let key = store_key(address);
    let mut body = view::report(&report, &key, twin_of.as_deref());
    body["stale_since"] = app.store.stale_since(&report.address)?.into();
    body["events"] = app
        .store
        .events(Some(&report.address), 10)?
        .into_iter()
        .filter(|event| crate::feed::PUBLIC.contains(&event.kind.as_str()))
        .map(|event| json!({"kind": event.kind, "detail": event.detail, "at": event.at}))
        .collect();
    Ok(Json(body))
}

fn atom_response(xml: String) -> Response {
    (
        [(header::CONTENT_TYPE, "application/atom+xml; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=300")],
        xml,
    )
        .into_response()
}

fn public(events: Vec<crate::store::Event>) -> Vec<crate::store::Event> {
    events.into_iter().filter(|event| crate::feed::PUBLIC.contains(&event.kind.as_str())).collect()
}

async fn feed_all(State(app): State<Arc<App>>) -> Result<Response, Problem> {
    let events = public(app.store.events(None, 50)?);
    let store = &app.store;
    let name = |address: &str| store.report(address).ok().flatten().map_or_else(|| address.to_string(), |r| r.name);
    Ok(atom_response(crate::feed::atom(
        "Kea: proved contracts that changed",
        "/feed.xml",
        &app.config.site,
        &events,
        name,
    )))
}

async fn feed_one(State(app): State<Arc<App>>, Path(file): Path<String>) -> Result<Response, Problem> {
    let address = file.strip_suffix(".xml").ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "feeds end in .xml"))?;
    let key = store_key(parse_address(address)?);
    let name = app.store.report(&key)?.map_or_else(|| key.clone(), |report| report.name);
    let events = public(app.store.events(Some(&key), 50)?);
    let title = format!("Kea: changes to {name}");
    Ok(atom_response(crate::feed::atom(&title, &format!("/feed/{key}.xml"), &app.config.site, &events, |_| {
        name.clone()
    })))
}

async fn test_file(State(app): State<Arc<App>>, Path(address): Path<String>) -> Result<Response, Problem> {
    let (report, _) = find(&app, parse_address(&address)?).await?;
    if report.test_source.is_empty() {
        return Err(Problem::new(StatusCode::NOT_FOUND, "this report has no test file"));
    }
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], view::public_test(&report)).into_response())
}

async fn badge(State(app): State<Arc<App>>, Path(file): Path<String>) -> Result<Response, Problem> {
    let address = file.strip_suffix(".svg").ok_or_else(|| Problem::new(StatusCode::NOT_FOUND, "badges end in .svg"))?;
    // Only the address's own report: a badge is fetched on every page view, and must not cost a
    // chain call each time.
    let key = store_key(parse_address(address)?);
    let badge = match app.store.report(&key)? {
        // Code that changed since it was proved has no proof yet; the old counts would describe
        // code that no longer runs.
        Some(_) if app.store.stale_since(&key)?.is_some() => Badge::Changed,
        Some(report) => {
            let summary = view::summary(&view::score(&report));
            let count = |name: &str| summary[name].as_u64().and_then(|n| usize::try_from(n).ok()).unwrap_or(0);
            Badge::Checked { proven: count("proven"), found: count("found") }
        }
        None => Badge::NotChecked,
    };
    Ok((
        [(header::CONTENT_TYPE, "image/svg+xml; charset=utf-8"), (header::CACHE_CONTROL, "public, max-age=300")],
        badge.svg(),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tests::report;
    use axum::body::Body;
    use axum::extract::connect_info::MockConnectInfo;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use kea_claims::model::Model;
    use tower::ServiceExt;

    const ADDRESS: &str = "0x3c2269811836af69497e5f486a85d7316753cf62";

    fn config() -> Config {
        Config {
            db: "unused".into(),
            work: "unused".into(),
            listen: "127.0.0.1:0".into(),
            // Nothing listens here: a test that reaches the chain fails loudly instead of leaving.
            rpc_url: "http://127.0.0.1:9".into(),
            daily_budget_usd: 5.0,
            job_reserve_usd: 0.6,
            asks_per_day: 3,
            max_queue: 30,
            origins: vec!["http://localhost:5173".into()],
            salt: "s".repeat(32),
            trust_cloudflare: false,
            model: Model::Sonnet,
            attempts: 4,
            job_budget_usd: 1.5,
            watch_minutes: 15,
            site: "https://keaproofs.io".into(),
        }
    }

    fn app() -> Router {
        let config = config();
        let store = Store::in_memory().unwrap();
        store.save_report(&report(ADDRESS, "0xc0de", 1)).unwrap();
        let app = Arc::new(App {
            store,
            rpc: Rpc::new(config.rpc_url.clone()),
            config,
            wake: Notify::new(),
            admission: Mutex::new(()),
        });
        router(app).layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 4000))))
    }

    async fn send(request: Request<Body>) -> (StatusCode, String, Option<String>) {
        let response = app().oneshot(request).await.unwrap();
        let status = response.status();
        let kind = response.headers().get(header::CONTENT_TYPE).map(|v| v.to_str().unwrap().to_string());
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8(body.to_vec()).unwrap(), kind)
    }

    fn post(body: &Value) -> Request<Body> {
        Request::post("/v1/asks")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn bad_input_is_refused_before_anything_is_spent() {
        let (status, body, _) = send(post(&json!({"address": "0x12"}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        let (status, body, _) = send(post(&json!({"address": ADDRESS, "question": "?".repeat(281)}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }

    #[tokio::test]
    async fn reports_badges_and_test_files_are_served_from_the_store() {
        let (status, body, _) =
            send(Request::get(format!("/v1/contracts/{ADDRESS}")).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let report: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(report["name"], "PriceFeed");
        assert_eq!(report["summary"], json!({"proven": 0, "found": 0, "unclear": 0}));
        let (status, body, kind) =
            send(Request::get(format!("/badge/{ADDRESS}.svg")).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(kind.unwrap().starts_with("image/svg+xml") && body.contains("0 proven"));
        let (status, _, _) =
            send(Request::get(format!("/v1/contracts/{ADDRESS}/test")).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "an empty test file is not served");
        let (status, body, _) = send(Request::get("/v1/contracts?limit=5").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains(ADDRESS));
    }

    #[test]
    fn questions_are_cleaned_and_bounded() {
        assert_eq!(
            clean_question(Some("  can the owner\n\tmint?  ".into())).unwrap().as_deref(),
            Some("can the owner mint?")
        );
        assert_eq!(clean_question(Some(" \u{7}\n".into())).unwrap(), None);
        assert!(clean_question(Some("x".repeat(281))).is_err());
    }

    #[test]
    fn requesters_are_salted_hashes_and_the_header_needs_trust() {
        let mut config = config();
        let peer = SocketAddr::from(([10, 0, 0, 1], 1));
        let mut headers = HeaderMap::new();
        headers.insert("cf-connecting-ip", "203.0.113.9".parse().unwrap());
        let direct = requester(&config, &headers, peer);
        assert_eq!(direct.len(), 32);
        assert!(!direct.contains("10.0.0.1"));
        config.trust_cloudflare = true;
        assert_ne!(requester(&config, &headers, peer), direct, "the trusted header names the client");
    }

    #[test]
    fn a_published_report_is_never_replaced_or_steered_by_an_ask() {
        let own = report(ADDRESS, "0xc0de", 1);
        let twin = report("0x00000000000000000000000000000000000000aa", "0xc0de", 1);
        // A current report answers whatever the ask carries.
        for (fresh, question) in [(false, false), (false, true), (true, false), (true, true)] {
            assert_eq!(decide(Some(&own), None, "0xc0de", fresh, question), Decision::Own);
        }
        // Changed code earns a re-check, never a steered one.
        assert_eq!(decide(Some(&own), None, "0xbeef", false, true), Decision::Check { steered: false });
        // Identical code answers unless the asker wants this address proved, and then unsteered.
        assert_eq!(decide(None, Some(&twin), "0xc0de", false, true), Decision::Twin(&twin.address));
        assert_eq!(decide(None, Some(&twin), "0xc0de", true, true), Decision::Check { steered: false });
        // Only code nobody has proved takes the question.
        assert_eq!(decide(None, None, "0xc0de", false, true), Decision::Check { steered: true });
    }

    #[test]
    fn an_ipv6_slash_64_is_one_requester() {
        assert_eq!(person("2001:db8:1:2:aaaa::1"), person("2001:db8:1:2:bbbb:cccc:dddd:9"));
        assert_ne!(person("2001:db8:1:2::1"), person("2001:db8:1:3::1"));
        assert_eq!(person("::ffff:203.0.113.9"), "203.0.113.9", "a mapped IPv4 client stays itself");
        assert_eq!(person("203.0.113.9"), "203.0.113.9");
    }

    #[test]
    fn failures_never_show_internals() {
        assert_eq!(
            public_error("error sending request for url (https://rpc.example/KEY/)"),
            "the check could not finish; it has been logged"
        );
        assert!(public_error("solc 0.5.17 is older than 0.6.10, the oldest").contains("compiler"));
    }
}
