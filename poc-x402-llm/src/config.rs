//! Configuration. Loaded from a local untracked TOML file (path from
//! `APP_CONFIG`, default `config.local.toml`) with environment
//! overrides under the `AR_` prefix (`AR_UPSTREAM__API_KEY`, ...).
//! Provider endpoints and secrets never live in the versioned tree.

use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub facilitator: FacilitatorConfig,
    pub payment: PaymentConfig,
    pub upstream: UpstreamConfig,
    #[serde(default)]
    pub runtime: RuntimeConfig,
}

/// POC-2 measurement knobs. `settlement`: `sync` settles on the hot
/// path, `async` spawns it off-path. `verify_mode`: `facilitator`
/// always calls `/verify`, `cached` trusts a payer for
/// `verify_cache_ttl_secs` after a first successful verify.
#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeConfig {
    pub settlement: String,
    pub verify_mode: String,
    pub verify_cache_ttl_secs: u64,
    /// batch-settlement: periodic claim+settle interval (seconds).
    /// 0 disables the background job (admin endpoint still works).
    #[serde(default)]
    pub claim_interval_secs: u64,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            settlement: "sync".into(),
            verify_mode: "facilitator".into(),
            verify_cache_ttl_secs: 30,
            claim_interval_secs: 0,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub bind: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FacilitatorConfig {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaymentConfig {
    pub scheme: String,
    pub network: String,
    pub asset: String,
    pub pay_to: String,
    pub amount: String,
    pub max_timeout_seconds: u64,
    pub asset_name: Option<String>,
    pub asset_version: Option<String>,
    pub resource_url: String,
    pub receiver_authorizer: Option<String>,
    pub withdraw_delay: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpstreamConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
    pub api_version: Option<String>,
    pub max_tokens: u32,
}

impl Config {
    pub fn load() -> Result<Self, Box<figment::Error>> {
        let path = std::env::var("APP_CONFIG").unwrap_or_else(|_| "config.local.toml".into());
        Figment::new()
            .merge(Toml::file(path))
            .merge(Env::prefixed("AR_").split("__"))
            .extract()
            .map_err(Box::new)
    }
}
