/// Normal paths will be overwritten by the upstream usage precise value;
/// this fallback value is only used if the connection is interrupted.
pub(crate) fn count_prompt_tokens(body: &serde_json::Value) -> Option<i64> {
    let text = extract_text(body)?;
    Some(text.len() as i64 / 3)
}

fn extract_text(body: &serde_json::Value) -> Option<String> {
    let msgs = body.get("messages")?.as_array()?;
    let text: String = msgs
        .iter()
        .filter_map(|m| m.get("content"))
        .filter_map(|c| c.as_str())
        .collect();
    (!text.is_empty()).then_some(text)
}
