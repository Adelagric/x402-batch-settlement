//! Upstream adapter. The router is OpenAI-compatible at the edge and
//! the configured upstream is also OpenAI-compatible, so this is a
//! near-passthrough: the inbound request is forwarded as-is with the
//! configured model id and a `max_tokens` default enforced, and the
//! upstream response body is relayed verbatim. A non-OpenAI
//! Messages-style provider would be a separate adapter (see
//! docs/DECISIONS.md, D3/D9).

use serde_json::Value;

use crate::error::AppError;

/// Parse the inbound OpenAI body, override `model` with the configured
/// one, and ensure `max_tokens` is set. `messages` (including any
/// system-role entries) is forwarded unchanged.
pub fn build_upstream_body(
    inbound: &[u8],
    model: &str,
    default_max_tokens: u32,
) -> Result<Value, AppError> {
    let mut v: Value =
        serde_json::from_slice(inbound).map_err(|e| AppError::BadRequest(e.to_string()))?;
    let obj = v
        .as_object_mut()
        .ok_or_else(|| AppError::BadRequest("request body must be a JSON object".into()))?;
    if !obj.get("messages").map(Value::is_array).unwrap_or(false) {
        return Err(AppError::BadRequest("`messages` array is required".into()));
    }
    obj.insert("model".into(), Value::String(model.to_string()));
    if !obj.contains_key("max_tokens") {
        obj.insert("max_tokens".into(), Value::from(default_max_tokens));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_model_and_defaults_max_tokens_keeping_messages() {
        let inbound = br#"{"model":"client-asked","messages":[{"role":"system","content":"s"},{"role":"user","content":"hi"}]}"#;
        let v = build_upstream_body(inbound, "deepseek-chat", 256).unwrap();
        assert_eq!(v["model"], "deepseek-chat");
        assert_eq!(v["max_tokens"], 256);
        assert_eq!(v["messages"].as_array().unwrap().len(), 2);
        assert_eq!(v["messages"][0]["role"], "system");
    }

    #[test]
    fn respects_explicit_max_tokens() {
        let inbound = br#"{"messages":[{"role":"user","content":"hi"}],"max_tokens":42}"#;
        let v = build_upstream_body(inbound, "m", 256).unwrap();
        assert_eq!(v["max_tokens"], 42);
    }

    #[test]
    fn rejects_missing_messages() {
        let inbound = br#"{"model":"x"}"#;
        assert!(build_upstream_body(inbound, "m", 256).is_err());
    }
}
