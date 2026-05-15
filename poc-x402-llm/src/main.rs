mod cache;
mod config;
mod error;
mod facilitator;
mod provider;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};

use x402::codec::{decode_header, encode_header, HEADER_PAYMENT_SIGNATURE};
use x402::exact_evm::ExactEvmEip3009Payload;
use x402::facilitator::{FacilitatorRequest, FacilitatorTransport};
use x402::payment::{Extra, PaymentPayload, PaymentRequired, PaymentRequirements, Resource};

use crate::cache::TtlCache;
use crate::config::Config;
use crate::error::AppError;
use crate::facilitator::ReqwestFacilitator;
use crate::provider::build_upstream_body;

#[derive(Default)]
struct ChannelState {
    charged: u128,
    balance: u128,
    total_claimed: u128,
    // Latest client-signed voucher, retained for off-path claim.
    config: Option<x402::batch_settlement::WireChannelConfig>,
    last_max: String,
    last_sig: String,
}

#[derive(Clone)]
struct AppState {
    cfg: Arc<Config>,
    facilitator: Arc<ReqwestFacilitator>,
    http: reqwest::Client,
    cache: Arc<TtlCache>,
    channels: Arc<Mutex<HashMap<String, ChannelState>>>,
}

#[tokio::main]
async fn main() {
    let cfg = Config::load().expect("load configuration");
    let bind = cfg.server.bind.clone();
    eprintln!(
        "mode: settlement={} verify={} ttl={}s",
        cfg.runtime.settlement, cfg.runtime.verify_mode, cfg.runtime.verify_cache_ttl_secs
    );
    let state = AppState {
        facilitator: Arc::new(ReqwestFacilitator::new(cfg.facilitator.url.clone())),
        http: reqwest::Client::new(),
        cache: Arc::new(TtlCache::new()),
        channels: Arc::new(Mutex::new(HashMap::new())),
        cfg: Arc::new(cfg),
    };

    let app = Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/admin/claim-settle", post(admin_claim_settle))
        .with_state(state.clone());

    let claim_secs = state.cfg.runtime.claim_interval_secs;
    if claim_secs > 0 {
        let bg = state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(claim_secs)).await;
                let summary = run_claim_settle(&bg).await;
                eprintln!("periodic claim/settle: {summary}");
            }
        });
    }

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .expect("bind listener");
    eprintln!("listening on {bind}");
    axum::serve(listener, app).await.expect("serve");
}

fn requirements(cfg: &Config) -> PaymentRequirements {
    PaymentRequirements {
        scheme: cfg.payment.scheme.clone(),
        network: cfg.payment.network.clone(),
        amount: cfg.payment.amount.clone(),
        asset: cfg.payment.asset.clone(),
        pay_to: cfg.payment.pay_to.clone(),
        max_timeout_seconds: cfg.payment.max_timeout_seconds,
        extra: Some(Extra {
            asset_transfer_method: None,
            name: cfg.payment.asset_name.clone(),
            version: cfg.payment.asset_version.clone(),
            receiver_authorizer: cfg.payment.receiver_authorizer.clone(),
            withdraw_delay: cfg.payment.withdraw_delay,
        }),
    }
}

/// 402 with the Base64 `PAYMENT-REQUIRED` challenge header.
fn challenge(cfg: &Config, reason: &str) -> Response {
    let body = PaymentRequired {
        x402_version: x402::X402_VERSION,
        error: Some(reason.to_string()),
        accepts: vec![requirements(cfg)],
        resource: Some(Resource {
            url: cfg.payment.resource_url.clone(),
            description: None,
            mime_type: Some("application/json".into()),
        }),
    };
    match encode_header(&body) {
        Ok(encoded) => match HeaderValue::from_str(&encoded) {
            Ok(value) => {
                let mut headers = HeaderMap::new();
                headers.insert(HeaderName::from_static("payment-required"), value);
                (StatusCode::PAYMENT_REQUIRED, headers).into_response()
            }
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        },
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Cheap, always-on structural guard. Never skipped, even on a verify
/// cache hit: it ensures the presented payment targets this resource's
/// requirements before any trust is extended.
fn local_matches(
    payment: &PaymentPayload,
    req: &PaymentRequirements,
    eip: Option<&ExactEvmEip3009Payload>,
) -> bool {
    let a = &payment.accepted;
    let base = a.scheme == req.scheme
        && a.network == req.network
        && a.asset.eq_ignore_ascii_case(&req.asset)
        && a.pay_to.eq_ignore_ascii_case(&req.pay_to)
        && a.amount == req.amount;
    let amount_ok = req.amount.parse::<u128>().is_ok();
    let auth_ok = match eip {
        Some(e) => {
            e.authorization.value == req.amount
                && e.authorization.to.eq_ignore_ascii_case(&req.pay_to)
        }
        None => true,
    };
    base && amount_ok && auth_ok
}

async fn chat_completions(State(app): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    let started = Instant::now();
    let cfg = app.cfg.as_ref();
    let cached_mode = cfg.runtime.verify_mode == "cached";
    let async_settle = cfg.runtime.settlement == "async";
    let ttl = Duration::from_secs(cfg.runtime.verify_cache_ttl_secs);

    let signature = match headers.get(HEADER_PAYMENT_SIGNATURE) {
        None => return challenge(cfg, "PAYMENT-SIGNATURE header is required"),
        Some(value) => match value.to_str() {
            Ok(s) => s.to_owned(),
            Err(_) => return challenge(cfg, "invalid PAYMENT-SIGNATURE header"),
        },
    };

    let payment: PaymentPayload = match decode_header(&signature) {
        Ok(p) => p,
        Err(e) => return AppError::from(e).into_response(),
    };
    if payment.check_version().is_err() {
        return challenge(cfg, "unsupported x402 version");
    }

    if payment.accepted.scheme == x402::batch_settlement::SCHEME_BATCH_SETTLEMENT {
        return handle_batch_settlement(&app, payment, &body, started).await;
    }

    let eip = ExactEvmEip3009Payload::from_payment(&payment).ok();
    let req = requirements(cfg);
    if !local_matches(&payment, &req, eip.as_ref()) {
        return challenge(cfg, "payment does not match requirements");
    }
    let payer = eip.as_ref().map(|e| e.authorization.from.clone());
    let freq = FacilitatorRequest::new(payment, req);

    // --- verify (hot path) ---
    let t_verify = Instant::now();
    let mut was_cached = false;
    if cached_mode {
        if let Some(p) = payer.as_ref() {
            if app.cache.fresh(p, ttl) {
                was_cached = true;
            }
        }
    }
    if !was_cached {
        match app.facilitator.verify(&freq).await {
            Ok(v) if v.is_valid => {
                if cached_mode {
                    if let Some(p) = payer.as_ref() {
                        app.cache.record(p, ttl);
                    }
                }
            }
            Ok(v) => {
                let reason = if v.invalid_reason.is_empty() {
                    "payment verification failed"
                } else {
                    v.invalid_reason.as_str()
                };
                return challenge(cfg, reason);
            }
            Err(e) => return AppError::from(e).into_response(),
        }
    }
    let verify_us = t_verify.elapsed().as_micros();

    // --- upstream (off the payment overhead) ---
    let upstream_body =
        match build_upstream_body(&body, &cfg.upstream.model, cfg.upstream.max_tokens) {
            Ok(v) => v,
            Err(e) => return e.into_response(),
        };
    let t_upstream = Instant::now();
    let mut builder = app
        .http
        .post(cfg.upstream.base_url.as_str())
        .header("authorization", format!("Bearer {}", cfg.upstream.api_key))
        .json(&upstream_body);
    if let Some(version) = &cfg.upstream.api_version {
        builder = builder.header("x-api-version", version);
    }
    let upstream_resp = match builder.send().await {
        Ok(r) => r,
        Err(e) => return AppError::Upstream(e.to_string()).into_response(),
    };
    if !upstream_resp.status().is_success() {
        // Payment verified but resource not delivered: never settle.
        let status = upstream_resp.status();
        let body = upstream_resp.text().await.unwrap_or_default();
        let snippet: String = body.chars().take(200).collect();
        eprintln!("upstream failure: {status}: {snippet}");
        return AppError::Upstream(format!("upstream {status}: {snippet}")).into_response();
    }
    let payload = match upstream_resp.bytes().await {
        Ok(b) => b.to_vec(),
        Err(e) => return AppError::Upstream(e.to_string()).into_response(),
    };
    let upstream_us = t_upstream.elapsed().as_micros();

    // --- settle ---
    let mut settle_us: u128 = 0;
    let mut settlement: Option<x402::SettlementResponse> = None;
    if async_settle {
        let fac = app.facilitator.clone();
        let fr = freq.clone();
        tokio::spawn(async move {
            if let Err(e) = fac.settle(&fr).await {
                eprintln!("async settle failed (reconciliation needed): {e}");
            }
        });
    } else {
        let t_settle = Instant::now();
        match app.facilitator.settle(&freq).await {
            Ok(s) => settlement = Some(s),
            Err(e) => return AppError::from(e).into_response(),
        }
        settle_us = t_settle.elapsed().as_micros();
    }

    let total_us = started.elapsed().as_micros();
    let overhead_us = total_us.saturating_sub(upstream_us);

    let mut response = (StatusCode::OK, payload).into_response();
    let h = response.headers_mut();
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Some(s) = &settlement {
        if let Ok(encoded) = encode_header(s) {
            if let Ok(value) = HeaderValue::from_str(&encoded) {
                h.insert(HeaderName::from_static("payment-response"), value);
            }
        }
    }
    set_str(h, "x-verify-cached", if was_cached { "true" } else { "false" });
    set_str(h, "x-settlement", &cfg.runtime.settlement);
    set_us(h, "x-verify-us", verify_us);
    set_us(h, "x-upstream-us", upstream_us);
    set_us(h, "x-settle-us", settle_us);
    set_us(h, "x-overhead-us", overhead_us);
    set_us(h, "x-total-us", total_us);
    response
}

fn set_us(headers: &mut HeaderMap, name: &'static str, micros: u128) {
    if let Ok(value) = HeaderValue::from_str(&micros.to_string()) {
        headers.insert(HeaderName::from_static(name), value);
    }
}

fn set_str(headers: &mut HeaderMap, name: &'static str, v: &str) {
    if let Ok(value) = HeaderValue::from_str(v) {
        headers.insert(HeaderName::from_static(name), value);
    }
}

/// channel-manager correctness core (off the hot path): aggregate the
/// latest signed voucher of every channel with unclaimed value into a
/// single `claim`, then `settle` receiver+token. Idempotent: a channel
/// is only claimed when `charged > total_claimed`, and `total_claimed`
/// is reconciled to the claimed ceiling on success, so a re-trigger is
/// a no-op (no double-claim). Receiver authorizer is delegated to the
/// facilitator (no server signature). Long-tail (refund, recovery,
/// withdrawal monitoring, multi-strategy) is intentionally out of
/// scope — see docs/DECISIONS.md D12.
async fn run_claim_settle(app: &AppState) -> serde_json::Value {
    use x402::batch_settlement::{ClaimPayload, SettlePayload, VoucherClaim, VoucherClaimInner};

    let req = requirements(app.cfg.as_ref());

    let claimable: Vec<(String, x402::batch_settlement::WireChannelConfig, String, String)> = {
        let chans = app.channels.lock().unwrap();
        chans
            .iter()
            .filter_map(|(cid, s)| match &s.config {
                Some(c) if s.charged > s.total_claimed && !s.last_sig.is_empty() => {
                    Some((cid.clone(), c.clone(), s.last_max.clone(), s.last_sig.clone()))
                }
                _ => None,
            })
            .collect()
    };
    if claimable.is_empty() {
        return serde_json::json!({"claimedChannels": 0, "reason": "nothing to claim"});
    }

    let claims: Vec<VoucherClaim> = claimable
        .iter()
        .map(|(_, cfg, max, sig)| VoucherClaim {
            voucher: VoucherClaimInner {
                channel: cfg.clone(),
                max_claimable_amount: max.clone(),
            },
            signature: sig.clone(),
            total_claimed: max.clone(),
        })
        .collect();

    let claim_pp = PaymentPayload {
        x402_version: x402::X402_VERSION,
        resource: None,
        accepted: req.clone(),
        payload: serde_json::to_value(ClaimPayload::new(claims)).unwrap_or(serde_json::Value::Null),
    };
    let claim_resp = match app
        .facilitator
        .settle(&FacilitatorRequest::new(claim_pp, req.clone()))
        .await
    {
        Ok(r) => r,
        Err(e) => return serde_json::json!({"error": format!("claim failed: {e}")}),
    };
    if !claim_resp.success {
        return serde_json::json!({
            "error": "claim rejected",
            "reason": claim_resp.error_reason,
            "detail": claim_resp.error_message,
        });
    }

    let settle_pp = PaymentPayload {
        x402_version: x402::X402_VERSION,
        resource: None,
        accepted: req.clone(),
        payload: serde_json::to_value(SettlePayload::new(req.pay_to.clone(), req.asset.clone()))
            .unwrap_or(serde_json::Value::Null),
    };
    let settle_resp = match app
        .facilitator
        .settle(&FacilitatorRequest::new(settle_pp, req.clone()))
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return serde_json::json!({
                "claimTx": claim_resp.transaction,
                "error": format!("settle failed: {e}")
            })
        }
    };

    if !settle_resp.success {
        return serde_json::json!({
            "claimTx": claim_resp.transaction,
            "error": "settle rejected",
            "reason": settle_resp.error_reason,
            "detail": settle_resp.error_message,
        });
    }

    {
        let mut chans = app.channels.lock().unwrap();
        for (cid, _, max, _) in &claimable {
            if let (Some(s), Ok(m)) = (chans.get_mut(cid), max.parse::<u128>()) {
                s.total_claimed = m;
            }
        }
    }

    serde_json::json!({
        "claimedChannels": claimable.len(),
        "claimTx": claim_resp.transaction,
        "settleTx": settle_resp.transaction,
        "settledAmount": settle_resp.amount,
    })
}

async fn admin_claim_settle(State(app): State<AppState>) -> Response {
    (StatusCode::OK, Json(run_claim_settle(&app).await)).into_response()
}

fn hex32(b: &[u8; 32]) -> String {
    let mut s = String::with_capacity(66);
    s.push_str("0x");
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

fn u128_field(v: &serde_json::Value, key: &str) -> Option<u128> {
    let f = v.get(key)?;
    if let Some(s) = f.as_str() {
        s.parse().ok()
    } else {
        f.as_u64().map(|n| n as u128)
    }
}

/// batch-settlement path. `voucher` payloads verify locally (pure
/// crypto, no facilitator, no chain). `deposit` (first request) is
/// verified by the facilitator, which sponsors the gasless on-chain
/// deposit and returns the channel snapshot. Claim/settle are batched
/// off the hot path (Tranche 2 stub — never per request).
async fn handle_batch_settlement(
    app: &AppState,
    payment: PaymentPayload,
    body: &Bytes,
    started: Instant,
) -> Response {
    let cfg = app.cfg.as_ref();
    let req = requirements(cfg);
    let chain_id = match req.network.rsplit(':').next().and_then(|s| s.parse::<u64>().ok()) {
        Some(c) => c,
        None => return challenge(cfg, "unsupported network for batch-settlement"),
    };
    let amount: u128 = match req.amount.parse() {
        Ok(a) => a,
        Err(_) => return challenge(cfg, "bad requirements amount"),
    };
    let bp: x402::batch_settlement::BatchVoucherPayload =
        match serde_json::from_value(payment.payload.clone()) {
            Ok(b) => b,
            Err(e) => return AppError::BadRequest(format!("batch payload: {e}")).into_response(),
        };
    let kind = bp.kind.clone();

    let payer = bp.channel_config.payer.clone();
    let cid_hex;
    let mut pr_tx = String::new();
    let mut pr_amount = String::new();
    let mut snap_balance: u128 = 0;
    let mut snap_claimed: u128 = 0;
    let mut snap_charged: u128 = 0;
    let refund_nonce = "0".to_string();

    let t_verify = Instant::now();
    match kind.as_str() {
        "voucher" => {
            let (cid, max) = match x402::batch_settlement::verify_voucher(&bp, chain_id) {
                Ok(v) => v,
                Err(e) => return challenge(cfg, &format!("voucher invalid: {e}")),
            };
            cid_hex = hex32(&cid);
            let policy = {
                let mut chans = app.channels.lock().unwrap();
                match chans.get_mut(&cid_hex) {
                    None => Err("no channel state; deposit required first"),
                    Some(s) => {
                        if max > s.balance {
                            Err("voucher exceeds channel balance")
                        } else if max <= s.total_claimed {
                            Err("voucher at or below claimed total")
                        } else if max < s.charged.saturating_add(amount) {
                            Err("voucher below cumulative charge")
                        } else {
                            s.charged = max;
                            s.config = Some(bp.channel_config.clone());
                            s.last_max = bp.voucher.max_claimable_amount.clone();
                            s.last_sig = bp.voucher.signature.clone();
                            snap_balance = s.balance;
                            snap_claimed = s.total_claimed;
                            snap_charged = s.charged;
                            Ok(())
                        }
                    }
                }
            };
            if let Err(reason) = policy {
                return challenge(cfg, reason);
            }
        }
        "deposit" => {
            let freq = FacilitatorRequest::new(payment.clone(), req.clone());
            let vr = match app.facilitator.verify(&freq).await {
                Ok(v) => v,
                Err(e) => return AppError::from(e).into_response(),
            };
            if !vr.is_valid {
                let r = if vr.invalid_reason.is_empty() {
                    "deposit verification failed"
                } else {
                    vr.invalid_reason.as_str()
                };
                return challenge(cfg, r);
            }
            // Deposit settle: facilitator submits the gasless on-chain
            // deposit and creates/funds the channel (one-off).
            let settle = match app.facilitator.settle(&freq).await {
                Ok(s) => s,
                Err(e) => return AppError::from(e).into_response(),
            };
            if !settle.success {
                return challenge(
                    cfg,
                    settle.error_reason.as_deref().unwrap_or("deposit settle failed"),
                );
            }
            let cfgp = match bp.channel_config.parse() {
                Ok(c) => c,
                Err(e) => return AppError::BadRequest(e.to_string()).into_response(),
            };
            cid_hex = hex32(&cfgp.channel_id(chain_id));
            let snap = settle
                .extra
                .as_ref()
                .and_then(|x| x.get("channelState").cloned())
                .or_else(|| vr.extra.clone());
            let (mut balance, total_claimed) = match snap.as_ref() {
                Some(x) => (
                    u128_field(x, "balance").unwrap_or(0),
                    u128_field(x, "totalClaimed").unwrap_or(0),
                ),
                None => (0, 0),
            };
            if balance == 0 {
                eprintln!("batch deposit: no channelState balance from facilitator; using sentinel (measurement only)");
                balance = u128::MAX / 2;
            }
            pr_tx = settle.transaction.clone();
            pr_amount = settle.amount.clone().unwrap_or_default();
            snap_balance = balance;
            snap_claimed = total_claimed;
            snap_charged = amount;
            app.channels.lock().unwrap().insert(
                cid_hex.clone(),
                ChannelState {
                    charged: amount,
                    balance,
                    total_claimed,
                    config: Some(bp.channel_config.clone()),
                    last_max: bp.voucher.max_claimable_amount.clone(),
                    last_sig: bp.voucher.signature.clone(),
                },
            );
        }
        other => {
            return challenge(cfg, &format!("unsupported batch-settlement type: {other}"));
        }
    }
    let verify_us = t_verify.elapsed().as_micros();

    let upstream_body =
        match build_upstream_body(body, &cfg.upstream.model, cfg.upstream.max_tokens) {
            Ok(v) => v,
            Err(e) => return e.into_response(),
        };
    let t_upstream = Instant::now();
    let mut builder = app
        .http
        .post(cfg.upstream.base_url.as_str())
        .header("authorization", format!("Bearer {}", cfg.upstream.api_key))
        .json(&upstream_body);
    if let Some(version) = &cfg.upstream.api_version {
        builder = builder.header("x-api-version", version);
    }
    let upstream_resp = match builder.send().await {
        Ok(r) => r,
        Err(e) => return AppError::Upstream(e.to_string()).into_response(),
    };
    if !upstream_resp.status().is_success() {
        let status = upstream_resp.status();
        let bdy = upstream_resp.text().await.unwrap_or_default();
        let snippet: String = bdy.chars().take(200).collect();
        eprintln!("upstream failure: {status}: {snippet}");
        return AppError::Upstream(format!("upstream {status}: {snippet}")).into_response();
    }
    let payload_bytes = match upstream_resp.bytes().await {
        Ok(b) => b.to_vec(),
        Err(e) => return AppError::Upstream(e.to_string()).into_response(),
    };
    let upstream_us = t_upstream.elapsed().as_micros();

    // No settle on the hot path, by construction: vouchers are claimed
    // and settled in batches off-path (Tranche 2).
    let total_us = started.elapsed().as_micros();
    let overhead_us = total_us.saturating_sub(upstream_us);

    let mut response = (StatusCode::OK, payload_bytes).into_response();
    let h = response.headers_mut();
    h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    let pr = serde_json::json!({
        "success": true,
        "transaction": pr_tx,
        "network": req.network,
        "payer": payer,
        "amount": pr_amount,
        "extra": {
            "chargedAmount": amount.to_string(),
            "channelState": {
                "channelId": cid_hex,
                "balance": snap_balance.to_string(),
                "totalClaimed": snap_claimed.to_string(),
                "withdrawRequestedAt": 0,
                "refundNonce": refund_nonce,
                "chargedCumulativeAmount": snap_charged.to_string()
            }
        }
    });
    if let Ok(enc) = encode_header(&pr) {
        if let Ok(v) = HeaderValue::from_str(&enc) {
            h.insert(HeaderName::from_static("payment-response"), v);
        }
    }
    set_str(h, "x-scheme", "batch-settlement");
    set_str(h, "x-batch-kind", &kind);
    set_us(h, "x-verify-us", verify_us);
    set_us(h, "x-upstream-us", upstream_us);
    set_us(h, "x-settle-us", 0);
    set_us(h, "x-overhead-us", overhead_us);
    set_us(h, "x-total-us", total_us);
    response
}
