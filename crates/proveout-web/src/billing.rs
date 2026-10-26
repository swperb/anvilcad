//! Paid plans through Stripe Checkout, if the operator configures it.
//!
//! Set `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET`, `STRIPE_PRICE_PRO` and
//! `STRIPE_PRICE_SHOP` to turn it on. Without them the site runs with free accounts
//! only, and an operator can set a plan by hand with `proveout set-plan`.
//!
//! A webhook event is accepted only with a valid `Stripe-Signature` (HMAC-SHA256 of
//! `timestamp.body` with the endpoint secret, within five minutes). Card details never
//! touch this server: Stripe's hosted checkout collects them.

use crate::util::{ct_eq, form_encode, hex, hmac_sha256};
use proveout_core::plan::Plan;
use serde_json::Value;
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct StripeConfig {
    pub secret_key: String,
    pub webhook_secret: String,
    pub price_pro: String,
    pub price_shop: String,
}

impl StripeConfig {
    pub fn from_env() -> Option<StripeConfig> {
        let get = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        Some(StripeConfig {
            secret_key: get("STRIPE_SECRET_KEY")?,
            webhook_secret: get("STRIPE_WEBHOOK_SECRET")?,
            price_pro: get("STRIPE_PRICE_PRO")?,
            price_shop: get("STRIPE_PRICE_SHOP")?,
        })
    }

    fn price(&self, plan: Plan) -> Option<&str> {
        match plan {
            Plan::Pro => Some(&self.price_pro),
            Plan::Shop => Some(&self.price_shop),
            Plan::Free => None,
        }
    }
}

/// Creates a hosted checkout session and returns its URL.
pub fn checkout_url(
    cfg: &StripeConfig,
    plan: Plan,
    user_id: i64,
    email: &str,
    customer: Option<&str>,
    base: &str,
) -> Result<String, String> {
    let price = cfg.price(plan).ok_or("the free plan needs no checkout")?;
    let uid = user_id.to_string();
    let success = format!("{base}/?billing=success");
    let cancel = format!("{base}/?billing=cancelled");
    // A returning customer keeps their Stripe customer record.
    let who = match customer {
        Some(c) => ("customer", c),
        None => ("customer_email", email),
    };
    let body = form_encode(&[
        ("mode", "subscription"),
        ("line_items[0][price]", price),
        ("line_items[0][quantity]", "1"),
        ("success_url", &success),
        ("cancel_url", &cancel),
        ("client_reference_id", &uid),
        who,
        ("metadata[plan]", plan.as_str()),
        ("subscription_data[metadata][plan]", plan.as_str()),
    ]);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .into();
    let resp = agent
        .post("https://api.stripe.com/v1/checkout/sessions")
        .header("authorization", &format!("Bearer {}", cfg.secret_key))
        .header("content-type", "application/x-www-form-urlencoded")
        .send(body)
        .map_err(|e| format!("billing request failed: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp
        .into_body()
        .with_config()
        .limit(1 << 20)
        .read_to_string()
        .map_err(|e| format!("billing response unreadable: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|_| "billing response is not JSON".to_string())?;
    if status != 200 {
        let msg = v.pointer("/error/message").and_then(Value::as_str).unwrap_or("unknown error");
        return Err(format!("billing error {status}: {msg}"));
    }
    v.get("url")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "billing response has no URL".into())
}

/// Checks a `Stripe-Signature` header against the raw body.
pub fn verify_signature(secret: &str, header: &str, body: &[u8], at: i64) -> bool {
    let mut t: Option<i64> = None;
    let mut sigs = Vec::new();
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", v)) => t = v.parse().ok(),
            Some(("v1", v)) => sigs.push(v.to_ascii_lowercase()),
            _ => {}
        }
    }
    let Some(t) = t else { return false };
    if (at - t).abs() > 300 {
        return false;
    }
    let mut msg = format!("{t}.").into_bytes();
    msg.extend_from_slice(body);
    let want = hex(&hmac_sha256(secret.as_bytes(), &msg));
    sigs.iter().any(|s| ct_eq(s.as_bytes(), want.as_bytes()))
}

/// What a webhook event asks for.
#[derive(Debug, PartialEq)]
pub enum Action {
    /// Checkout finished: (user id, plan, customer id).
    Activate(i64, Plan, String),
    /// Subscription ended: customer id.
    Cancel(String),
    Ignore,
}

pub fn parse_event(body: &[u8]) -> Action {
    let Ok(v) = serde_json::from_slice::<Value>(body) else { return Action::Ignore };
    let obj = v.pointer("/data/object").cloned().unwrap_or(Value::Null);
    let str_at = |p: &str| obj.pointer(p).and_then(Value::as_str).map(str::to_string);
    match v.get("type").and_then(Value::as_str) {
        Some("checkout.session.completed") => {
            let uid = str_at("/client_reference_id").and_then(|s| s.parse().ok());
            let plan = str_at("/metadata/plan").and_then(|p| Plan::parse(&p));
            match (uid, plan, str_at("/customer")) {
                (Some(u), Some(p), Some(c)) if p != Plan::Free => Action::Activate(u, p, c),
                _ => Action::Ignore,
            }
        }
        Some("customer.subscription.deleted") => {
            str_at("/customer").map(Action::Cancel).unwrap_or(Action::Ignore)
        }
        Some("customer.subscription.updated") => match str_at("/status").as_deref() {
            Some("canceled" | "unpaid" | "incomplete_expired") => {
                str_at("/customer").map(Action::Cancel).unwrap_or(Action::Ignore)
            }
            _ => Action::Ignore,
        },
        _ => Action::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures() {
        let body = br#"{"type":"x"}"#;
        let mut msg = b"1700000000.".to_vec();
        msg.extend_from_slice(body);
        let sig = hex(&hmac_sha256(b"whsec_test", &msg));
        let header = format!("t=1700000000,v1=deadbeef,v1={sig}");
        assert!(verify_signature("whsec_test", &header, body, 1_700_000_100));
        assert!(!verify_signature("whsec_test", &header, body, 1_700_000_400));
        assert!(!verify_signature("whsec_other", &header, body, 1_700_000_100));
        assert!(!verify_signature("whsec_test", &header, br#"{"type":"y"}"#, 1_700_000_100));
        assert!(!verify_signature("whsec_test", "v1=abc", body, 1_700_000_100));
    }

    #[test]
    fn events() {
        let done = br#"{"type":"checkout.session.completed","data":{"object":{"client_reference_id":"7","customer":"cus_1","metadata":{"plan":"pro"}}}}"#;
        assert_eq!(parse_event(done), Action::Activate(7, Plan::Pro, "cus_1".into()));
        let gone = br#"{"type":"customer.subscription.deleted","data":{"object":{"customer":"cus_1"}}}"#;
        assert_eq!(parse_event(gone), Action::Cancel("cus_1".into()));
        let free = br#"{"type":"checkout.session.completed","data":{"object":{"client_reference_id":"7","customer":"c","metadata":{"plan":"free"}}}}"#;
        assert_eq!(parse_event(free), Action::Ignore);
        assert_eq!(parse_event(b"junk"), Action::Ignore);
    }
}
