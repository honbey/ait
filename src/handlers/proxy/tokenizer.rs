/// Normal paths will be overwritten by the upstream usage precise value;
/// this fallback value is only used if the connection is interrupted.
pub(crate) fn count_prompt_tokens(body: &serde_json::Value) -> Option<i64> {
    let msgs = body.get("messages")?.as_array()?;
    let len: usize = msgs
        .iter()
        .filter_map(|m| m.get("content"))
        .filter_map(|c| c.as_str())
        .map(|s| s.len())
        .sum();
    if len == 0 { None } else { Some(len as i64 / 3) }
}
