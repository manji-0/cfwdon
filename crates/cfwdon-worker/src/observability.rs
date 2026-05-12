use worker::{console_debug, console_log};

pub(crate) fn observability_started_at_ms() -> f64 {
    js_sys::Date::now()
}

pub(crate) fn observability_duration_ms(started_at_ms: f64) -> u64 {
    (js_sys::Date::now() - started_at_ms).max(0.0).round() as u64
}

pub(crate) fn add_log_message(
    mut payload: serde_json::Value,
    message: impl Into<String>,
) -> serde_json::Value {
    if let Some(object) = payload.as_object_mut() {
        object.insert("message".to_owned(), serde_json::json!(message.into()));
    }
    payload
}

pub(crate) fn log_json_event(mut payload: serde_json::Value) {
    if let Some(object) = payload.as_object_mut() {
        object.insert("source".to_owned(), serde_json::json!("cfwdon"));
    }

    match serde_json::to_string(&payload) {
        Ok(message) => console_log!("{}", message),
        Err(error) => console_log!("cfwdon_observability_log_error={}", error),
    }
}

pub(crate) fn log_json_debug_event(mut payload: serde_json::Value) {
    if let Some(object) = payload.as_object_mut() {
        object.insert("source".to_owned(), serde_json::json!("cfwdon"));
    }

    match serde_json::to_string(&payload) {
        Ok(message) => console_debug!("{}", message),
        Err(error) => console_log!("cfwdon_observability_log_error={}", error),
    }
}

pub(crate) fn log_observed_operation(
    component: &str,
    operation: &str,
    outcome: &str,
    started_at_ms: f64,
    details: serde_json::Value,
) {
    let duration_ms = observability_duration_ms(started_at_ms);
    let message = observed_operation_message(component, operation, outcome, duration_ms, &details);
    let mut payload = serde_json::json!({
        "event": "cfwdon_operation",
        "component": component,
        "operation": operation,
        "outcome": outcome,
        "duration_ms": duration_ms,
        "message": message,
    });

    if let (Some(payload), Some(details)) = (payload.as_object_mut(), details.as_object()) {
        for (key, value) in details {
            payload.insert(key.clone(), value.clone());
        }
    }

    log_json_debug_event(payload);
}

fn observed_operation_message(
    component: &str,
    operation: &str,
    outcome: &str,
    duration_ms: u64,
    details: &serde_json::Value,
) -> String {
    let component = component.to_ascii_uppercase();
    let operation = operation.replace('_', " ");
    let outcome = observed_outcome_label(outcome);

    match component.as_str() {
        "R2" => observed_r2_message(&operation, outcome, duration_ms, details),
        "D1" => observed_d1_message(&operation, outcome, duration_ms, details),
        _ => format!("{component} {operation} {outcome} in {duration_ms}ms"),
    }
}

fn observed_r2_message(
    operation: &str,
    outcome: &str,
    duration_ms: u64,
    details: &serde_json::Value,
) -> String {
    let object_family = detail_str(details, "object_family").unwrap_or("unknown");
    let bytes = detail_u64(details, "bytes")
        .map(|bytes| format!(" ({bytes} bytes)"))
        .unwrap_or_default();
    format!("R2 {operation} {outcome} for {object_family} object{bytes} in {duration_ms}ms")
}

fn observed_d1_message(
    operation: &str,
    outcome: &str,
    duration_ms: u64,
    details: &serde_json::Value,
) -> String {
    let target = detail_str(details, "query_name")
        .or_else(|| detail_str(details, "table"))
        .or_else(|| detail_str(details, "statement_family"))
        .map(|target| format!(" for {target}"))
        .unwrap_or_default();
    format!("D1 {operation} {outcome}{target} in {duration_ms}ms")
}

fn observed_outcome_label(outcome: &str) -> &str {
    match outcome {
        "ok" | "OK" | "success" | "hit" => "completed",
        "miss" => "missed",
        "error" | "ERROR" | "failed" => "failed",
        value => value,
    }
}

fn detail_str<'a>(details: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    details.get(key).and_then(serde_json::Value::as_str)
}

fn detail_u64(details: &serde_json::Value, key: &str) -> Option<u64> {
    details.get(key).and_then(serde_json::Value::as_u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_operation_message_describes_r2_operation() {
        let details = serde_json::json!({
            "object_family": "media",
            "bytes": 1234,
        });

        assert_eq!(
            observed_operation_message("r2", "put", "ok", 42, &details),
            "R2 put completed for media object (1234 bytes) in 42ms"
        );
    }

    #[test]
    fn observed_operation_message_describes_d1_operation_target() {
        let details = serde_json::json!({
            "query_name": "load account by id",
        });

        assert_eq!(
            observed_operation_message("d1", "first", "OK", 7, &details),
            "D1 first completed for load account by id in 7ms"
        );
    }

    #[test]
    fn add_log_message_keeps_existing_fields() {
        let payload = add_log_message(
            serde_json::json!({
                "event": "api_request",
                "status": 200,
            }),
            "API request GET /api/v1/instance completed with HTTP 200 in 12ms",
        );

        assert_eq!(payload["event"], "api_request");
        assert_eq!(payload["status"], 200);
        assert_eq!(
            payload["message"],
            "API request GET /api/v1/instance completed with HTTP 200 in 12ms"
        );
    }
}
