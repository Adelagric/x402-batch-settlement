//! Concrete reqwest transport for the x402 facilitator. The core crate
//! is transport-agnostic; this is where HTTP actually happens.

use x402::facilitator::{FacilitatorRequest, FacilitatorTransport, SettleResponse, VerifyResponse};
use x402::Error as X402Error;

pub struct ReqwestFacilitator {
    client: reqwest::Client,
    base_url: String,
}

impl ReqwestFacilitator {
    pub fn new(base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
        }
    }

    async fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        req: &FacilitatorRequest,
    ) -> Result<T, X402Error> {
        let url = format!("{}{}", self.base_url.trim_end_matches('/'), path);
        let resp = self
            .client
            .post(&url)
            .json(req)
            .send()
            .await
            .map_err(|e| X402Error::Facilitator(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(X402Error::Facilitator(format!(
                "{path} returned {}",
                resp.status()
            )));
        }
        resp.json::<T>()
            .await
            .map_err(|e| X402Error::Facilitator(e.to_string()))
    }
}

impl FacilitatorTransport for ReqwestFacilitator {
    async fn verify(&self, req: &FacilitatorRequest) -> Result<VerifyResponse, X402Error> {
        self.post("/verify", req).await
    }

    async fn settle(&self, req: &FacilitatorRequest) -> Result<SettleResponse, X402Error> {
        self.post("/settle", req).await
    }
}
