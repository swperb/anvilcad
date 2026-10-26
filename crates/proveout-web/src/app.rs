//! Routes and handlers.
//!
//! Anyone may run the deterministic check and the profile check; they cost nothing and
//! send nothing to a third party. An AI review needs an account, the acceptance of the
//! current disclosures, and credits on the user's plan. Reports are kept in memory for
//! an hour after they are made so the user can save them; nothing is written to disk
//! unless the user saves it.

use crate::billing::{self, Action, StripeConfig};
use crate::db::{Db, Kind, User};
use crate::util::{RateLimiter, iso, now, random_bytes, random_hex, sha256_hex};
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use proveout_core::ai::{self, DrawingInput, ReviewInput};
use proveout_core::check::{PostInput, check};
use proveout_core::machine::{MachineProfile, presets};
use proveout_core::plan::{Plan, ReviewLevel, can_review};
use proveout_core::profile::profile;
use proveout_core::report::{InputsMeta, Report, build, render_html, render_markdown};
use proveout_core::{APP_NAME, DISCLOSURE_VERSION, NO_GUARANTEE_DISCLOSURE, PRIVACY_DISCLOSURE};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Sends one Messages API request body and returns (status, response body).
pub type Sender = Arc<dyn Fn(&str) -> Result<(u16, String), String> + Send + Sync>;

pub const SESSION_COOKIE: &str = "po_session";
pub const SESSION_TTL: i64 = 30 * 86_400;
pub const PENDING_TTL: i64 = 3600;
pub const MAX_BODY: usize = 40 * 1024 * 1024;

pub struct Config {
    /// Public origin, for example `https://proveout.example`, without a trailing slash.
    pub public_url: String,
    pub secure_cookies: bool,
    pub trust_proxy: bool,
    /// AI reviews allowed across all users per 24 hours: a spending brake.
    pub daily_review_limit: u32,
    pub stripe: Option<StripeConfig>,
}

struct Pending {
    owner: Option<i64>,
    at: i64,
    report: Report,
}

pub struct AppState {
    pub db: Db,
    pub cfg: Config,
    /// `None` when no API key is configured: AI reviews are then unavailable.
    pub sender: Option<Sender>,
    pending: Mutex<HashMap<String, Pending>>,
    limiter: RateLimiter,
}

impl AppState {
    pub fn new(db: Db, cfg: Config, sender: Option<Sender>) -> AppState {
        AppState { db, cfg, sender, pending: Mutex::new(HashMap::new()), limiter: RateLimiter::default() }
    }
}

type St = State<Arc<AppState>>;

/// An error shown to the user as `{"error": "..."}`.
pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

fn internal(msg: impl std::fmt::Display) -> ApiError {
    eprintln!("internal error: {msg}");
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, "something went wrong on our side".into())
}

impl From<proveout_core::Error> for ApiError {
    fn from(e: proveout_core::Error) -> ApiError {
        match e {
            proveout_core::Error::TooLarge(m) => ApiError(StatusCode::PAYLOAD_TOO_LARGE, m),
            proveout_core::Error::Service(m) => ApiError(StatusCode::BAD_GATEWAY, m),
            other => bad(other.to_string()),
        }
    }
}

type ApiResult<T> = Result<T, ApiError>;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(page_index))
        .route("/app.js", get(asset_js))
        .route("/anonymize.js", get(asset_anon))
        .route("/style.css", get(asset_css))
        .route("/terms", get(page_terms))
        .route("/api/meta", get(meta))
        .route("/api/check", post(api_check))
        .route("/api/profile", post(api_profile))
        .route("/api/review", post(api_review))
        .route("/api/signup", post(signup))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/me", get(me).delete(delete_me))
        .route("/api/reports", get(list_reports).post(save_report))
        .route("/api/reports/{id}", get(get_report).delete(delete_report))
        .route("/api/reports/{id}/download", get(download_report))
        .route("/api/pending/{id}/download", get(download_pending))
        .route("/api/projects", get(list_projects).post(save_project))
        .route("/api/projects/{id}", get(get_project).delete(delete_project))
        .route("/api/machines", get(list_machines).post(save_machine))
        .route("/api/machines/{id}", get(get_machine).delete(delete_machine))
        .route("/api/billing/checkout", post(checkout))
        .route("/api/billing/webhook", post(webhook))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(state)
}

/// Security headers on every response, and a same-origin check on every request
/// that changes something (the webhook is checked by signature instead).
async fn guard(State(s): St, req: Request, next: Next) -> Response {
    let unsafe_method = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if unsafe_method && req.uri().path() != "/api/billing/webhook" {
        let h = req.headers();
        let origin_ok = match h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
            Some(o) => o == s.cfg.public_url,
            None => true,
        };
        let json = h
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("application/json"));
        if !origin_ok || !json {
            return ApiError(StatusCode::FORBIDDEN, "cross-site or non-JSON request refused".into())
                .into_response();
        }
    }
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    let set = |h: &mut HeaderMap, k: header::HeaderName, v: &'static str| {
        h.insert(k, HeaderValue::from_static(v));
    };
    if !h.contains_key(header::CONTENT_SECURITY_POLICY) {
        set(
            h,
            header::CONTENT_SECURITY_POLICY,
            "default-src 'self'; img-src 'self' data: blob:; style-src 'self'; script-src 'self'; connect-src 'self'; \
         frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        );
    }
    set(h, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set(h, header::REFERRER_POLICY, "same-origin");
    set(h, header::X_FRAME_OPTIONS, "DENY");
    if s.cfg.secure_cookies {
        set(h, header::STRICT_TRANSPORT_SECURITY, "max-age=31536000");
    }
    resp
}

fn client_ip(s: &AppState, headers: &HeaderMap, addr: SocketAddr) -> IpAddr {
    if s.cfg.trust_proxy
        && let Some(ip) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .and_then(|v| v.trim().parse().ok())
    {
        return ip;
    }
    addr.ip()
}

fn limit(s: &AppState, ip: IpAddr, action: &'static str, max: u32, window: i64) -> ApiResult<()> {
    if s.limiter.allow(ip, action, max, window) {
        Ok(())
    } else {
        Err(ApiError(StatusCode::TOO_MANY_REQUESTS, "too many requests; wait a minute and try again".into()))
    }
}

// ----- Static pages -----

const INDEX: &str = include_str!("../static/index.html");
const APP_JS: &str = include_str!("../static/app.js");
const ANON_JS: &str = include_str!("../static/anonymize.js");
const CSS: &str = include_str!("../static/style.css");

async fn page_index() -> Html<&'static str> {
    Html(INDEX)
}

async fn asset_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], APP_JS)
}

async fn asset_anon() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], ANON_JS)
}

async fn asset_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], CSS)
}

async fn page_terms() -> Html<String> {
    let e = proveout_core::report::esc;
    Html(format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" \
         content=\"width=device-width,initial-scale=1\"><title>{app} disclosures</title><link rel=\"stylesheet\" \
         href=\"/style.css\"></head><body><main class=\"narrow\"><h1>{app} disclosures</h1><p>Version {v}.</p>\
         <h2>Privacy when you use the AI review</h2><p>{p}</p><h2>No guarantee</h2><p>{g}</p>\
         <h2>What this site stores</h2><p>Your e-mail address, a hash of your password, your plan, your monthly \
         credit use, and a log of each AI review (time, level, model, token counts and the disclosure version you \
         accepted, but not its content). Reports, projects and machine profiles are stored only when you save \
         them, and you can delete them, or your whole account, at any time. Unsaved reports are kept in memory \
         for an hour so you can save them, then discarded.</p><p><a href=\"/\">Back</a></p></main></body></html>",
        app = e(APP_NAME),
        v = e(DISCLOSURE_VERSION),
        p = e(PRIVACY_DISCLOSURE),
        g = e(NO_GUARANTEE_DISCLOSURE),
    ))
}

// ----- Accounts -----

fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|c| {
            let (k, v) = c.trim().split_once('=')?;
            (k == SESSION_COOKIE && v.len() == 64 && v.chars().all(|c| c.is_ascii_hexdigit()))
                .then(|| v.to_string())
        })
}

fn current_user(s: &AppState, headers: &HeaderMap) -> ApiResult<Option<User>> {
    let Some(tok) = cookie_token(headers) else { return Ok(None) };
    let Some(uid) = s.db.session_user(&sha256_hex(&tok)).map_err(internal)? else { return Ok(None) };
    s.db.user(uid).map_err(internal)
}

fn require_user(s: &AppState, headers: &HeaderMap) -> ApiResult<User> {
    current_user(s, headers)?.ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "sign in first".into()))
}

fn session_cookie(s: &AppState, token: &str, max_age: i64) -> String {
    format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{}",
        if s.cfg.secure_cookies { "; Secure" } else { "" }
    )
}

#[derive(Deserialize)]
struct Credentials {
    email: String,
    password: String,
}

fn valid_email(e: &str) -> bool {
    let Some((local, domain)) = e.split_once('@') else { return false };
    e.len() <= 254
        && !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !e.chars().any(|c| c.is_whitespace() || c.is_control())
}

async fn hash_password(pw: String) -> ApiResult<String> {
    tokio::task::spawn_blocking(move || {
        use argon2::password_hash::{PasswordHasher, SaltString};
        let salt_bytes = random_bytes::<16>()?;
        let salt = SaltString::encode_b64(&salt_bytes).map_err(|e| e.to_string())?;
        argon2::Argon2::default()
            .hash_password(pw.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(internal)?
    .map_err(internal)
}

async fn check_password(pw: String, stored: String) -> bool {
    tokio::task::spawn_blocking(move || {
        use argon2::password_hash::{PasswordHash, PasswordVerifier};
        PasswordHash::new(&stored)
            .map(|h| argon2::Argon2::default().verify_password(pw.as_bytes(), &h).is_ok())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

async fn start_session(s: &AppState, uid: i64) -> ApiResult<Response> {
    let token = random_hex(32).map_err(internal)?;
    s.db.create_session(&sha256_hex(&token), uid, SESSION_TTL).map_err(internal)?;
    let user = s.db.user(uid).map_err(internal)?;
    let mut resp = Json(json!({"user": user})).into_response();
    let c = HeaderValue::from_str(&session_cookie(s, &token, SESSION_TTL)).map_err(internal)?;
    resp.headers_mut().insert(header::SET_COOKIE, c);
    Ok(resp)
}

async fn signup(
    State(s): St,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(c): Json<Credentials>,
) -> ApiResult<Response> {
    limit(&s, client_ip(&s, &headers, addr), "signup", 5, 3600)?;
    let email = c.email.trim().to_ascii_lowercase();
    if !valid_email(&email) {
        return Err(bad("enter a valid e-mail address"));
    }
    if c.password.chars().count() < 10 || c.password.len() > 256 {
        return Err(bad("use a password of at least 10 characters"));
    }
    let hash = hash_password(c.password).await?;
    match s.db.create_user(&email, &hash).map_err(internal)? {
        Some(uid) => start_session(&s, uid).await,
        None => Err(ApiError(StatusCode::CONFLICT, "an account with this e-mail already exists".into())),
    }
}

async fn login(
    State(s): St,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(c): Json<Credentials>,
) -> ApiResult<Response> {
    limit(&s, client_ip(&s, &headers, addr), "login", 10, 600)?;
    let email = c.email.trim().to_ascii_lowercase();
    let user = s.db.user_by_email(&email).map_err(internal)?;
    // Hash even when the user does not exist, so timing does not reveal accounts.
    let stored = user.as_ref().map(|u| u.pw.clone()).unwrap_or_else(|| {
        "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$7mN0Yx1GL0T7cmbmVAp8i3ZsZSxJ0bYB8mWl2T0PGkA".into()
    });
    let ok = check_password(c.password, stored).await;
    match (user, ok) {
        (Some(u), true) => start_session(&s, u.id).await,
        _ => Err(ApiError(StatusCode::UNAUTHORIZED, "wrong e-mail or password".into())),
    }
}

async fn logout(State(s): St, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(t) = cookie_token(&headers) {
        s.db.delete_session(&sha256_hex(&t)).map_err(internal)?;
    }
    let mut resp = Json(json!({"ok": true})).into_response();
    let c = HeaderValue::from_str(&session_cookie(&s, "", 0)).map_err(internal)?;
    resp.headers_mut().insert(header::SET_COOKIE, c);
    Ok(resp)
}

async fn me(State(s): St, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let Some(u) = current_user(&s, &headers)? else { return Ok(Json(json!({"user": null}))) };
    let info = u.plan.info();
    Ok(Json(json!({
        "user": u,
        "plan": info,
        "credits_used": s.db.credits_used(u.id).map_err(internal)?,
        "storage_used": s.db.storage_used(u.id).map_err(internal)?,
    })))
}

async fn delete_me(State(s): St, headers: HeaderMap) -> ApiResult<Response> {
    let u = require_user(&s, &headers)?;
    s.db.delete_user(u.id).map_err(internal)?;
    let mut resp = Json(json!({"ok": true})).into_response();
    let c = HeaderValue::from_str(&session_cookie(&s, "", 0)).map_err(internal)?;
    resp.headers_mut().insert(header::SET_COOKIE, c);
    Ok(resp)
}

// ----- Metadata -----

async fn meta(State(s): St) -> Json<Value> {
    let levels: Vec<Value> = ReviewLevel::ALL
        .iter()
        .map(|l| {
            json!({"id": l, "label": l.label(), "description": l.description(), "credits": l.credits(),
                   "max_program_kb": l.max_program_bytes() / 1024})
        })
        .collect();
    let plans: Vec<Value> = Plan::ALL.iter().map(|p| json!(p.info())).collect();
    let presets: Vec<Value> = presets().into_iter().map(|(id, p)| json!({"id": id, "profile": p})).collect();
    Json(json!({
        "app": APP_NAME,
        "disclosure_version": DISCLOSURE_VERSION,
        "privacy": PRIVACY_DISCLOSURE,
        "no_guarantee": NO_GUARANTEE_DISCLOSURE,
        "levels": levels,
        "plans": plans,
        "presets": presets,
        "ai_available": s.sender.is_some(),
        "billing_available": s.cfg.stripe.is_some(),
        "drawing_types": ai::DRAWING_TYPES,
    }))
}

// ----- Inputs shared by the check, profile and review -----

#[derive(Deserialize, Default)]
struct MachineChoice {
    preset: Option<String>,
    profile: Option<MachineProfile>,
    saved: Option<String>,
}

#[derive(Deserialize, Default)]
struct PostUpload {
    #[serde(default)]
    file_name: String,
    #[serde(default)]
    text: String,
}

fn resolve_machine(s: &AppState, user: Option<&User>, m: &MachineChoice) -> ApiResult<MachineProfile> {
    if let Some(p) = &m.profile {
        p.validate()?;
        return Ok(p.clone());
    }
    if let Some(id) = &m.saved {
        let u =
            user.ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "sign in to use saved machines".into()))?;
        let data =
            s.db.get(Kind::Machine, u.id, id)
                .map_err(internal)?
                .ok_or_else(|| bad("no such saved machine"))?;
        let v: Value = serde_json::from_str(&data).map_err(internal)?;
        return serde_json::from_value(v["profile"].clone())
            .map_err(|_| bad("the saved machine is unreadable"));
    }
    let id = m.preset.as_deref().unwrap_or("vmc-fanuc");
    proveout_core::machine::preset(id).ok_or_else(|| bad("no such machine preset"))
}

fn resolve_post(p: &Option<PostUpload>) -> ApiResult<PostInput> {
    match p {
        Some(p) if !p.text.trim().is_empty() => Ok(PostInput::from_upload(&p.file_name, &p.text)?),
        _ => Ok(PostInput::None),
    }
}

fn store_pending(s: &AppState, owner: Option<i64>, mut report: Report) -> ApiResult<Value> {
    let id = random_hex(12).map_err(internal)?;
    report.id = id.clone();
    report.created = iso(now());
    let html = render_html(&report);
    let markdown = render_markdown(&report);
    let out = json!({"pending_id": id, "report": report, "html": html, "markdown": markdown});
    let mut p = s.pending.lock().map_err(|_| internal("pending lock"))?;
    let t = now();
    p.retain(|_, v| t - v.at < PENDING_TTL);
    if p.len() > 5000 {
        // Drop the oldest so memory stays bounded.
        if let Some(k) = p.iter().min_by_key(|(_, v)| v.at).map(|(k, _)| k.clone()) {
            p.remove(&k);
        }
    }
    p.insert(id, Pending { owner, at: t, report });
    Ok(out)
}

// ----- Deterministic check and profile -----

#[derive(Deserialize)]
struct CheckBody {
    #[serde(default)]
    title: String,
    gcode: String,
    #[serde(default)]
    machine: MachineChoice,
    #[serde(default)]
    post: Option<PostUpload>,
}

async fn api_check(
    State(s): St,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(b): Json<CheckBody>,
) -> ApiResult<Json<Value>> {
    limit(&s, client_ip(&s, &headers, addr), "check", 60, 60)?;
    let user = current_user(&s, &headers)?;
    let machine = resolve_machine(&s, user.as_ref(), &b.machine)?;
    let post = resolve_post(&b.post)?;
    let st = s.clone();
    let out = tokio::task::spawn_blocking(move || -> ApiResult<Value> {
        let det = check(&b.gcode, &machine, &post)?;
        let meta = InputsMeta { post: post.label(), ..InputsMeta::default() };
        let report = build(&b.title, &machine, &b.gcode, det, None, None, meta);
        store_pending(&st, user.map(|u| u.id), report)
    })
    .await
    .map_err(internal)??;
    Ok(Json(out))
}

#[derive(Deserialize)]
struct ProfileBody {
    #[serde(default)]
    machine: MachineChoice,
    #[serde(default)]
    post: Option<PostUpload>,
    #[serde(default)]
    known_good: String,
}

async fn api_profile(
    State(s): St,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(b): Json<ProfileBody>,
) -> ApiResult<Json<Value>> {
    limit(&s, client_ip(&s, &headers, addr), "check", 60, 60)?;
    let user = current_user(&s, &headers)?;
    let machine = resolve_machine(&s, user.as_ref(), &b.machine)?;
    let post = resolve_post(&b.post)?;
    let r = tokio::task::spawn_blocking(move || profile(&machine, &post, Some(&b.known_good)))
        .await
        .map_err(internal)?;
    Ok(Json(json!({"profile": r, "no_guarantee": NO_GUARANTEE_DISCLOSURE})))
}

// ----- AI review -----

#[derive(Deserialize)]
struct Acceptance {
    privacy: bool,
    no_guarantee: bool,
    version: String,
}

#[derive(Deserialize)]
struct ReviewBody {
    #[serde(default)]
    title: String,
    gcode: String,
    #[serde(default)]
    drawing: DrawingInput,
    #[serde(default)]
    job_notes: String,
    #[serde(default)]
    machine: MachineChoice,
    #[serde(default)]
    post: Option<PostUpload>,
    level: ReviewLevel,
    #[serde(default)]
    anonymized: bool,
    accept: Acceptance,
}

async fn api_review(
    State(s): St,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(b): Json<ReviewBody>,
) -> ApiResult<Json<Value>> {
    let user = require_user(&s, &headers)?;
    limit(&s, client_ip(&s, &headers, addr), "review", 6, 60)?;
    if !(b.accept.privacy && b.accept.no_guarantee) || b.accept.version != DISCLOSURE_VERSION {
        return Err(ApiError(
            StatusCode::PRECONDITION_REQUIRED,
            "accept the current privacy and no-guarantee disclosures first".into(),
        ));
    }
    let Some(sender) = s.sender.clone() else {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "AI review is not configured on this server".into(),
        ));
    };
    let plan = user.plan;
    let used = s.db.credits_used(user.id).map_err(internal)?;
    can_review(plan, b.level, used).map_err(|m| ApiError(StatusCode::PAYMENT_REQUIRED, m))?;
    if s.db.reviews_last_day().map_err(internal)? >= s.cfg.daily_review_limit {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "the site's daily review limit is reached".into(),
        ));
    }
    let machine = resolve_machine(&s, Some(&user), &b.machine)?;
    let post = resolve_post(&b.post)?;
    // Build the request before taking credits: input errors cost nothing.
    let det = check(&b.gcode, &machine, &post)?;
    let req = ai::build_request(&ReviewInput {
        gcode: &b.gcode,
        drawing: &b.drawing,
        machine: &machine,
        post: &post,
        deterministic: &det,
        job_notes: &b.job_notes,
        level: b.level,
        anonymized: b.anonymized,
    })?
    .to_string();
    let credits = b.level.credits();
    if !s.db.reserve_credits(user.id, credits, plan.info().credits_per_month).map_err(internal)? {
        return Err(ApiError(StatusCode::PAYMENT_REQUIRED, "not enough credits left this month".into()));
    }
    let lines = det.summary.lines;
    let reply = tokio::task::spawn_blocking(move || sender(&req)).await.map_err(internal)?;
    let parsed = match reply {
        Ok((status, body)) => ai::parse_reply(status, &body, lines),
        Err(e) => Err(proveout_core::Error::Service(e)),
    };
    let level_s = format!("{:?}", b.level).to_lowercase();
    let review = match parsed {
        Ok(r) => r,
        Err(e) => {
            s.db.refund_credits(user.id, credits).map_err(internal)?;
            s.db.log_review(user.id, &level_s, b.level.model(), 0, 0, false, DISCLOSURE_VERSION)
                .map_err(internal)?;
            return Err(e.into());
        }
    };
    s.db.log_review(
        user.id,
        &level_s,
        &review.model,
        review.usage.input_tokens,
        review.usage.output_tokens,
        true,
        DISCLOSURE_VERSION,
    )
    .map_err(internal)?;
    let meta = InputsMeta {
        post: post.label(),
        anonymized: b.anonymized,
        drawing_file: b.drawing.file.as_ref().map(|f| f.name.chars().take(120).collect()),
        drawing_notes: !b.drawing.notes.trim().is_empty(),
        ..InputsMeta::default()
    };
    let report = build(&b.title, &machine, &b.gcode, det, Some(review), Some(b.level), meta);
    Ok(Json(store_pending(&s, Some(user.id), report)?))
}

// ----- Saved reports, projects and machines -----

fn check_storage(s: &AppState, u: &User, adding: usize) -> ApiResult<()> {
    let used = s.db.storage_used(u.id).map_err(internal)?;
    let cap = u.plan.info().storage_bytes;
    if used + adding as u64 > cap {
        return Err(ApiError(
            StatusCode::PAYMENT_REQUIRED,
            format!(
                "this needs {} KB and {} KB of your {} MB are left",
                adding / 1024,
                cap.saturating_sub(used) / 1024,
                cap >> 20
            ),
        ));
    }
    Ok(())
}

fn valid_id(id: &str) -> ApiResult<()> {
    if id.len() <= 64 && !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(bad("bad id"))
    }
}

#[derive(Deserialize)]
struct SaveReport {
    pending_id: String,
    #[serde(default)]
    project_id: Option<String>,
}

async fn save_report(State(s): St, headers: HeaderMap, Json(b): Json<SaveReport>) -> ApiResult<Json<Value>> {
    let u = require_user(&s, &headers)?;
    valid_id(&b.pending_id)?;
    let report = {
        let p = s.pending.lock().map_err(|_| internal("pending lock"))?;
        match p.get(&b.pending_id) {
            Some(x) if x.owner.is_none() || x.owner == Some(u.id) => x.report.clone(),
            _ => {
                return Err(ApiError(
                    StatusCode::NOT_FOUND,
                    "that report has expired; run the check again".into(),
                ));
            }
        }
    };
    if let Some(pid) = &b.project_id {
        valid_id(pid)?;
        if s.db.get(Kind::Project, u.id, pid).map_err(internal)?.is_none() {
            return Err(bad("no such project"));
        }
    }
    let data = serde_json::to_string(&report).map_err(internal)?;
    check_storage(&s, &u, data.len())?;
    s.db.insert(
        Kind::Report,
        &report.id,
        u.id,
        &report.title,
        &data,
        Some(report.score.percent),
        b.project_id.as_deref(),
    )
    .map_err(|e| if e.contains("UNIQUE") { bad("this report is already saved") } else { internal(e) })?;
    Ok(Json(json!({"id": report.id})))
}

async fn list_reports(State(s): St, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let u = require_user(&s, &headers)?;
    Ok(Json(json!({"items": s.db.list(Kind::Report, u.id).map_err(internal)?})))
}

fn load_report(s: &AppState, u: &User, id: &str) -> ApiResult<Report> {
    valid_id(id)?;
    let data =
        s.db.get(Kind::Report, u.id, id)
            .map_err(internal)?
            .ok_or(ApiError(StatusCode::NOT_FOUND, "not found".into()))?;
    serde_json::from_str(&data).map_err(internal)
}

async fn get_report(State(s): St, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let u = require_user(&s, &headers)?;
    let r = load_report(&s, &u, &id)?;
    Ok(Json(json!({"report": r, "html": render_html(&r), "markdown": render_markdown(&r)})))
}

async fn delete_report(State(s): St, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let u = require_user(&s, &headers)?;
    valid_id(&id)?;
    Ok(Json(json!({"deleted": s.db.delete(Kind::Report, u.id, &id).map_err(internal)?})))
}

#[derive(Deserialize)]
struct Format {
    format: Option<String>,
    /// `1` to open the HTML in the browser instead of saving it.
    inline: Option<u8>,
}

fn download(r: &Report, f: &Format) -> ApiResult<Response> {
    let format = f.format.as_deref();
    let stem: String = r
        .title
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .take(60)
        .collect();
    let (body, ctype, ext) = match format.unwrap_or("html") {
        "html" => (render_html(r), "text/html; charset=utf-8", "html"),
        "md" => (render_markdown(r), "text/markdown; charset=utf-8", "md"),
        "json" => (serde_json::to_string_pretty(r).map_err(internal)?, "application/json", "json"),
        _ => return Err(bad("format must be html, md or json")),
    };
    let how = if f.inline == Some(1) && ext == "html" { "inline" } else { "attachment" };
    let disp = format!("{how}; filename=\"proveout-{stem}-{}.{ext}\"", r.id);
    Ok((
        [
            (header::CONTENT_TYPE, ctype.to_string()),
            (header::CONTENT_DISPOSITION, disp),
            // A downloaded HTML report opened from our origin runs no scripts.
            (header::CONTENT_SECURITY_POLICY, "default-src 'none'; style-src 'unsafe-inline'".to_string()),
        ],
        body,
    )
        .into_response())
}

async fn download_report(
    State(s): St,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(f): Query<Format>,
) -> ApiResult<Response> {
    let u = require_user(&s, &headers)?;
    let r = load_report(&s, &u, &id)?;
    download(&r, &f)
}

async fn download_pending(
    State(s): St,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(f): Query<Format>,
) -> ApiResult<Response> {
    valid_id(&id)?;
    let user = current_user(&s, &headers)?;
    let r = {
        let p = s.pending.lock().map_err(|_| internal("pending lock"))?;
        match p.get(&id) {
            Some(x) if x.owner.is_none() || x.owner == user.as_ref().map(|u| u.id) => x.report.clone(),
            _ => return Err(ApiError(StatusCode::NOT_FOUND, "that report has expired".into())),
        }
    };
    download(&r, &f)
}

#[derive(Deserialize)]
struct SaveNamed {
    name: String,
    data: Value,
}

fn save_named(s: &AppState, u: &User, kind: Kind, b: SaveNamed) -> ApiResult<Json<Value>> {
    let name: String = b.name.chars().filter(|c| !c.is_control()).take(120).collect();
    if name.trim().is_empty() {
        return Err(bad("give it a name"));
    }
    let data = serde_json::to_string(&b.data).map_err(internal)?;
    check_storage(s, u, data.len())?;
    let id = random_hex(12).map_err(internal)?;
    s.db.insert(kind, &id, u.id, &name, &data, None, None).map_err(internal)?;
    Ok(Json(json!({"id": id})))
}

async fn save_project(State(s): St, headers: HeaderMap, Json(b): Json<SaveNamed>) -> ApiResult<Json<Value>> {
    let u = require_user(&s, &headers)?;
    save_named(&s, &u, Kind::Project, b)
}

async fn save_machine(State(s): St, headers: HeaderMap, Json(b): Json<SaveNamed>) -> ApiResult<Json<Value>> {
    let u = require_user(&s, &headers)?;
    let p: MachineProfile = serde_json::from_value(b.data.get("profile").cloned().unwrap_or(Value::Null))
        .map_err(|e| bad(format!("bad profile: {e}")))?;
    p.validate()?;
    save_named(&s, &u, Kind::Machine, b)
}

async fn list_projects(State(s): St, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let u = require_user(&s, &headers)?;
    Ok(Json(json!({"items": s.db.list(Kind::Project, u.id).map_err(internal)?})))
}

async fn list_machines(State(s): St, headers: HeaderMap) -> ApiResult<Json<Value>> {
    let u = require_user(&s, &headers)?;
    Ok(Json(json!({"items": s.db.list(Kind::Machine, u.id).map_err(internal)?})))
}

fn get_named(s: &AppState, headers: &HeaderMap, kind: Kind, id: &str) -> ApiResult<Json<Value>> {
    let u = require_user(s, headers)?;
    valid_id(id)?;
    let data =
        s.db.get(kind, u.id, id)
            .map_err(internal)?
            .ok_or(ApiError(StatusCode::NOT_FOUND, "not found".into()))?;
    Ok(Json(json!({"id": id, "data": serde_json::from_str::<Value>(&data).map_err(internal)?})))
}

async fn get_project(State(s): St, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    get_named(&s, &headers, Kind::Project, &id)
}

async fn get_machine(State(s): St, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    get_named(&s, &headers, Kind::Machine, &id)
}

async fn delete_project(State(s): St, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let u = require_user(&s, &headers)?;
    valid_id(&id)?;
    Ok(Json(json!({"deleted": s.db.delete(Kind::Project, u.id, &id).map_err(internal)?})))
}

async fn delete_machine(State(s): St, headers: HeaderMap, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let u = require_user(&s, &headers)?;
    valid_id(&id)?;
    Ok(Json(json!({"deleted": s.db.delete(Kind::Machine, u.id, &id).map_err(internal)?})))
}

// ----- Billing -----

#[derive(Deserialize)]
struct CheckoutBody {
    plan: Plan,
}

async fn checkout(State(s): St, headers: HeaderMap, Json(b): Json<CheckoutBody>) -> ApiResult<Json<Value>> {
    let u = require_user(&s, &headers)?;
    let Some(cfg) = s.cfg.stripe.clone() else {
        return Err(ApiError(
            StatusCode::SERVICE_UNAVAILABLE,
            "paid plans are not set up on this server yet".into(),
        ));
    };
    let base = s.cfg.public_url.clone();
    let url = tokio::task::spawn_blocking(move || {
        billing::checkout_url(&cfg, b.plan, u.id, &u.email, u.stripe_customer.as_deref(), &base)
    })
    .await
    .map_err(internal)?
    .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(json!({"url": url})))
}

async fn webhook(State(s): St, headers: HeaderMap, body: Bytes) -> ApiResult<StatusCode> {
    let Some(cfg) = &s.cfg.stripe else { return Ok(StatusCode::NOT_FOUND) };
    let sig = headers.get("stripe-signature").and_then(|v| v.to_str().ok()).unwrap_or("");
    if !billing::verify_signature(&cfg.webhook_secret, sig, &body, now()) {
        return Err(ApiError(StatusCode::BAD_REQUEST, "bad signature".into()));
    }
    match billing::parse_event(&body) {
        Action::Activate(uid, plan, customer) => {
            s.db.set_plan(uid, plan, Some(&customer)).map_err(internal)?
        }
        Action::Cancel(customer) => {
            s.db.set_plan_by_customer(&customer, Plan::Free).map_err(internal)?;
        }
        Action::Ignore => {}
    }
    Ok(StatusCode::OK)
}

/// The real sender: one HTTPS request to the Messages API.
pub fn api_sender(key: String) -> Sender {
    use anvil_cam::ai::{Transport, UreqTransport};
    Arc::new(move |body: &str| {
        let mut headers: Vec<(&str, &str)> = ai::headers().to_vec();
        headers.push(("x-api-key", key.as_str()));
        UreqTransport.post(ai::ENDPOINT, &headers, body, Duration::from_secs(600)).map_err(|e| e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use http_body_util::BodyExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::ServiceExt;

    const PROG: &str = "O1001 (BRACKET)\n(T1 D=10 FLAT END MILL)\nG21 G90 G54\nT1 M6\nS8000 M3\nG0 X0 Y0\n\
                        G43 H1 Z10\nG1 Z-3 F250\nG1 X50 F1000\nG0 Z10\nM30\n";

    fn reply() -> String {
        let inner = json!({
            "summary": "Mills a 50 mm slot.", "units": "mm", "confidence_percent": 80, "confidence_reason": "simple",
            "findings": [{"severity": "warning", "category": "feeds", "lines": [8], "message": "plunge is fast", "suggestion": "ramp"}],
            "drawing_checks": [{"feature": "Slot depth", "drawing_value": "3", "program_value": "3", "status": "match", "lines": [8], "note": ""}],
            "setup": {"part": "Bracket", "material": "6061", "stock": "60x40x20", "workholding": "vise", "program_zero": "top left",
                      "operations": [], "tools": [], "offsets": [], "checklist": []},
            "additions": [], "questions": []
        });
        json!({"model": "claude-sonnet-5-5", "content": [{"type": "text", "text": inner.to_string()}],
               "stop_reason": "end_turn", "usage": {"input_tokens": 5000, "output_tokens": 900}})
        .to_string()
    }

    fn app(calls: Arc<AtomicUsize>) -> (Router, Arc<AppState>) {
        let sender: Sender = Arc::new(move |body: &str| {
            calls.fetch_add(1, Ordering::SeqCst);
            let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
            assert_eq!(v["model"], "claude-sonnet-5-5");
            Ok((200, reply()))
        });
        let cfg = Config {
            public_url: "http://localhost:8080".into(),
            secure_cookies: false,
            trust_proxy: false,
            daily_review_limit: 100,
            stripe: None,
        };
        let st = Arc::new(AppState::new(Db::open(":memory:").unwrap(), cfg, Some(sender)));
        (router(st.clone()), st)
    }

    async fn call(
        app: &Router,
        method: &str,
        uri: &str,
        cookie: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, HeaderMap, Value) {
        let mut b = HttpRequest::builder().method(method).uri(uri);
        if let Some(c) = cookie {
            b = b.header("cookie", c);
        }
        let req = match body {
            Some(v) => b.header("content-type", "application/json").body(Body::from(v.to_string())).unwrap(),
            None => b.body(Body::empty()).unwrap(),
        };
        let mut req = req;
        req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 9))));
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let v =
            serde_json::from_slice(&bytes).unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into()));
        (status, headers, v)
    }

    fn cookie_of(h: &HeaderMap) -> String {
        h.get("set-cookie").unwrap().to_str().unwrap().split(';').next().unwrap().to_string()
    }

    #[tokio::test]
    async fn anonymous_check_profile_and_pages() {
        let (app, _) = app(Arc::new(AtomicUsize::new(0)));
        let (st, h, _) = call(&app, "GET", "/", None, None).await;
        assert_eq!(st, StatusCode::OK);
        assert!(h.get("content-security-policy").is_some());
        let (st, _, v) = call(
            &app,
            "POST",
            "/api/check",
            None,
            Some(json!({"gcode": PROG, "machine": {"preset": "vmc-fanuc"}})),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        assert_eq!(v["report"]["score"]["percent"], 84);
        assert!(v["html"].as_str().unwrap().contains("setup sheet"));
        let pid = v["pending_id"].as_str().unwrap().to_string();
        let (st, _, _) =
            call(&app, "GET", &format!("/api/pending/{pid}/download?format=md"), None, None).await;
        assert_eq!(st, StatusCode::OK);
        let (st, _, v) = call(
            &app,
            "POST",
            "/api/profile",
            None,
            Some(json!({"machine": {"preset": "vmc-haas"}, "known_good": PROG})),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        assert!(v["profile"]["checks"].as_array().unwrap().len() >= 5);
        // Reviews need an account.
        let (st, _, _) = call(
            &app,
            "POST",
            "/api/review",
            None,
            Some(json!({"gcode": PROG, "level": "standard",
            "accept": {"privacy": true, "no_guarantee": true, "version": DISCLOSURE_VERSION}})),
        )
        .await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn cross_site_and_non_json_posts_are_refused() {
        let (app, _) = app(Arc::new(AtomicUsize::new(0)));
        let mut req = HttpRequest::builder()
            .method("POST")
            .uri("/api/logout")
            .header("origin", "https://evil.example")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap();
        req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 9))));
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::FORBIDDEN);
        let mut req =
            HttpRequest::builder().method("POST").uri("/api/logout").body(Body::from("a=b")).unwrap();
        req.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 9))));
        assert_eq!(app.oneshot(req).await.unwrap().status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn signup_review_save_download_and_quota() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (app, st) = app(calls.clone());
        let (s, h, v) = call(
            &app,
            "POST",
            "/api/signup",
            None,
            Some(json!({"email": "Mach@Shop.example", "password": "correct horse"})),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let c = cookie_of(&h);
        let (_, _, v) = call(&app, "GET", "/api/me", Some(&c), None).await;
        assert_eq!(v["user"]["email"], "mach@shop.example");
        assert_eq!(v["user"]["plan"], "free");

        let body = |level: &str, version: &str| {
            json!({
                "title": "Bracket", "gcode": PROG, "level": level, "machine": {"preset": "vmc-fanuc"},
                "drawing": {"notes": "slot 50 x 3 deep"}, "anonymized": true,
                "accept": {"privacy": true, "no_guarantee": true, "version": version}
            })
        };
        // Old disclosure version: refused before anything is sent.
        let (s, _, _) =
            call(&app, "POST", "/api/review", Some(&c), Some(body("standard", "2020-01-01"))).await;
        assert_eq!(s, StatusCode::PRECONDITION_REQUIRED);
        // High needs a paid plan.
        let (s, _, _) =
            call(&app, "POST", "/api/review", Some(&c), Some(body("high", DISCLOSURE_VERSION))).await;
        assert_eq!(s, StatusCode::PAYMENT_REQUIRED);
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let (s, _, v) =
            call(&app, "POST", "/api/review", Some(&c), Some(body("standard", DISCLOSURE_VERSION))).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        // rules 1.0, drawing 1.0, AI 0.8 -> 0.94, capped at nothing.
        assert_eq!(v["report"]["score"]["percent"], 94);
        assert_eq!(v["report"]["ai"]["findings"][0]["lines"], json!([8]));
        let pid = v["pending_id"].as_str().unwrap().to_string();

        let (s, _, v) = call(&app, "POST", "/api/reports", Some(&c), Some(json!({"pending_id": pid}))).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        let (_, _, v) = call(&app, "GET", "/api/reports", Some(&c), None).await;
        assert_eq!(v["items"][0]["score"], 94);
        let (s, h, v) =
            call(&app, "GET", &format!("/api/reports/{pid}/download?format=html"), Some(&c), None).await;
        assert_eq!(s, StatusCode::OK);
        assert!(h.get("content-disposition").unwrap().to_str().unwrap().contains("proveout-Bracket"));
        assert!(v.as_str().unwrap().contains("Slot depth"));

        // Three free credits: two more reviews, then a 402.
        for _ in 0..2 {
            let (s, _, _) =
                call(&app, "POST", "/api/review", Some(&c), Some(body("standard", DISCLOSURE_VERSION))).await;
            assert_eq!(s, StatusCode::OK);
        }
        let (s, _, _) =
            call(&app, "POST", "/api/review", Some(&c), Some(body("standard", DISCLOSURE_VERSION))).await;
        assert_eq!(s, StatusCode::PAYMENT_REQUIRED);
        assert_eq!(calls.load(Ordering::SeqCst), 3);

        // Another account cannot read the report.
        let (_, h2, _) = call(
            &app,
            "POST",
            "/api/signup",
            None,
            Some(json!({"email": "other@shop.example", "password": "another password"})),
        )
        .await;
        let c2 = cookie_of(&h2);
        let (s, _, _) = call(&app, "GET", &format!("/api/reports/{pid}"), Some(&c2), None).await;
        assert_eq!(s, StatusCode::NOT_FOUND);

        // Plans change through the database (as the webhook or set-plan does).
        let uid = st.db.user_by_email("mach@shop.example").unwrap().unwrap().id;
        st.db.set_plan(uid, Plan::Pro, None).unwrap();
        let (_, _, v) = call(&app, "GET", "/api/me", Some(&c), None).await;
        assert_eq!(v["plan"]["name"], "Pro");

        // Login and logout.
        let (s, _, _) = call(
            &app,
            "POST",
            "/api/login",
            None,
            Some(json!({"email": "mach@shop.example", "password": "wrong password"})),
        )
        .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        let (s, _, _) = call(
            &app,
            "POST",
            "/api/login",
            None,
            Some(json!({"email": "mach@shop.example", "password": "correct horse"})),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        call(&app, "POST", "/api/logout", Some(&c), Some(json!({}))).await;
        let (_, _, v) = call(&app, "GET", "/api/me", Some(&c), None).await;
        assert_eq!(v["user"], Value::Null);
    }

    #[tokio::test]
    async fn failed_reviews_refund_credits() {
        let sender: Sender = Arc::new(|_: &str| {
            Ok((
                529,
                json!({"type": "error", "error": {"type": "overloaded_error", "message": "busy"}})
                    .to_string(),
            ))
        });
        let cfg = Config {
            public_url: "http://x".into(),
            secure_cookies: false,
            trust_proxy: false,
            daily_review_limit: 10,
            stripe: None,
        };
        let st = Arc::new(AppState::new(Db::open(":memory:").unwrap(), cfg, Some(sender)));
        let app = router(st.clone());
        let (_, h, _) = call(
            &app,
            "POST",
            "/api/signup",
            None,
            Some(json!({"email": "a@b.example", "password": "0123456789"})),
        )
        .await;
        let c = cookie_of(&h);
        let (s, _, v) = call(
            &app,
            "POST",
            "/api/review",
            Some(&c),
            Some(json!({"gcode": PROG, "level": "standard",
            "accept": {"privacy": true, "no_guarantee": true, "version": DISCLOSURE_VERSION}})),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_GATEWAY);
        assert!(v["error"].as_str().unwrap().contains("temporary"));
        let (_, _, v) = call(&app, "GET", "/api/me", Some(&c), None).await;
        assert_eq!(v["credits_used"], 0);
    }
}
