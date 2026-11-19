//! JSON trace formats normalize spans before resolving their source locations.
use super::*;

use serde_json::Value;
use std::collections::HashMap;

struct Record {
    span: Span,
    label: String,
    start: Option<i128>,
    location: Option<(String, usize)>,
}

pub(super) fn parse(repo: &Repository, source: &str) -> Report {
    let mut report = Report::default();
    let value: Value = match serde_json::from_str(source) {
        Ok(value) => value,
        Err(error) => {
            report.error = format!("Invalid trace JSON: {error}");
            return report;
        }
    };
    let records = if field(&value, "resourceSpans", "resource_spans").is_some()
        || value.get("spans").is_some()
    {
        report.format = "OpenTelemetry OTLP JSON".into();
        otel(&value, &mut report)
    } else if value.get("traceEvents").is_some() || value.is_array() {
        report.format = "Chrome Trace JSON".into();
        chrome(&value, &mut report)
    } else {
        Err(
            "Unsupported trace JSON. Use an OTLP resourceSpans export or Chrome traceEvents array."
                .into(),
        )
    };
    let records = match records {
        Ok(records) => records,
        Err(error) => {
            report.error = error;
            return report;
        }
    };
    let order = match ordered(&records) {
        Ok(order) => order,
        Err(error) => {
            report.error = error;
            return report;
        }
    };
    for index in order {
        let record = &records[index];
        if !report
            .groups
            .iter()
            .any(|group| group.id == record.span.group)
        {
            report.groups.push(Group {
                id: record.span.group.clone(),
                label: record.label.clone(),
            });
        }
        let Some((path, line)) = &record.location else {
            issue(
                &mut report,
                format!(
                    "{}: source location missing; include code.file.path and code.line.number (or Chrome args.file / args.line).",
                    record.span.name
                ),
            );
            continue;
        };
        match Frame::resolve(repo, path, *line) {
            Ok(mut frame) => {
                frame.span = Some(record.span.clone());
                report.frames.push(frame);
            }
            Err(error) => issue(
                &mut report,
                format!("{} · {path}:{line}: {error}", record.span.name),
            ),
        }
    }
    if report.frames.is_empty() {
        report.error = "No matching source spans. Spans need a file and positive line number from the selected snapshot.".into();
    }
    report
}

fn field<'a>(value: &'a Value, camel: &str, snake: &str) -> Option<&'a Value> {
    value.get(camel).or_else(|| value.get(snake))
}
fn scalar(value: &Value) -> &Value {
    [
        "stringValue",
        "intValue",
        "doubleValue",
        "boolValue",
        "string_value",
        "int_value",
        "double_value",
        "bool_value",
    ]
    .into_iter()
    .find_map(|key| value.get(key))
    .unwrap_or(value)
}
fn number(value: &Value) -> Option<u64> {
    let value = scalar(value);
    value.as_u64().or_else(|| value.as_str()?.parse().ok())
}
fn attr<'a>(attributes: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .find_map(|key| {
            attributes.get(*key).or_else(|| {
                attributes
                    .as_array()?
                    .iter()
                    .find(|item| item.get("key").and_then(Value::as_str) == Some(*key))
                    .and_then(|item| item.get("value"))
            })
        })
        .map(scalar)
}
fn code_location(attributes: &Value) -> Option<(String, usize)> {
    let path = attr(
        attributes,
        &["code.file.path", "code.filepath", "file", "filename"],
    )?
    .as_str()?;
    let line = number(attr(
        attributes,
        &["code.line.number", "code.lineno", "line"],
    )?)?;
    Some((path.into(), usize::try_from(line).ok()?))
}
fn issue(report: &mut Report, text: String) {
    if report.issues.len() < 128 {
        report.issues.push(text);
    } else {
        report.ignored += 1;
    }
}
fn push(records: &mut Vec<Record>, record: Record) -> Result<usize, String> {
    if records.len() == 128 {
        return Err("Trace exceeds the 128 span limit; shorten it and build again.".into());
    }
    let index = records.len();
    records.push(record);
    Ok(index)
}
fn name(value: &Value) -> String {
    value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Unnamed span")
        .chars()
        .take(160)
        .collect()
}

fn otel(value: &Value, report: &mut Report) -> Result<Vec<Record>, String> {
    let mut spans = Vec::new();
    if let Some(resources) = field(value, "resourceSpans", "resource_spans") {
        for resource in resources
            .as_array()
            .ok_or("OTLP resourceSpans must be an array.")?
        {
            let scopes = field(resource, "scopeSpans", "scope_spans")
                .or_else(|| resource.get("instrumentationLibrarySpans"))
                .and_then(Value::as_array)
                .ok_or("OTLP scopeSpans must be an array.")?;
            for scope in scopes {
                spans.extend(
                    scope
                        .get("spans")
                        .and_then(Value::as_array)
                        .ok_or("OTLP spans must be an array.")?,
                );
            }
        }
    } else {
        spans.extend(
            value
                .get("spans")
                .and_then(Value::as_array)
                .ok_or("OTLP spans must be an array.")?,
        );
    }
    let mut records = Vec::new();
    for span in spans {
        let name = name(span);
        let (Some(trace), Some(id)) = (
            field(span, "traceId", "trace_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty()),
            field(span, "spanId", "span_id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty()),
        ) else {
            issue(report, format!("{name}: traceId and spanId are required."));
            continue;
        };
        let start_value = field(span, "startTimeUnixNano", "start_time_unix_nano");
        let end_value = field(span, "endTimeUnixNano", "end_time_unix_nano");
        let start = start_value.and_then(number);
        let end = end_value.and_then(number);
        if (start_value.is_some() && start.is_none())
            || (end_value.is_some() && end.is_none())
            || start.zip(end).is_some_and(|(start, end)| end < start)
        {
            issue(report, format!("{name}: invalid nanosecond interval."));
            continue;
        }
        let code = span.get("status").and_then(|status| status.get("code"));
        let error = code.and_then(Value::as_u64) == Some(2)
            || code.and_then(Value::as_str) == Some("STATUS_CODE_ERROR");
        let group = format!("otel:{}", trace.to_ascii_lowercase());
        push(
            &mut records,
            Record {
                label: format!("OpenTelemetry trace {}", trace.to_ascii_lowercase()),
                start: start.map(i128::from),
                location: span.get("attributes").and_then(code_location),
                span: Span {
                    group,
                    name,
                    id: id.to_ascii_lowercase(),
                    parent: field(span, "parentSpanId", "parent_span_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_ascii_lowercase(),
                    duration_ms: start
                        .zip(end)
                        .map(|(start, end)| (end - start) as f64 / 1_000_000.),
                    error,
                },
            },
        )?;
    }
    Ok(records)
}

fn chrome_time(event: &Value, key: &str) -> Option<i128> {
    let value = event.get(key)?.as_f64()?;
    let nanos = value * 1000.;
    (nanos.is_finite() && nanos.abs() < 1e30).then(|| nanos.round() as i128)
}
fn identity(event: &Value, key: &str) -> Option<String> {
    let value = event.get(key)?;
    value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_i64().map(|id| id.to_string()))
        .or_else(|| value.as_u64().map(|id| id.to_string()))
}
fn chrome_location(args: &Value) -> Option<(String, usize)> {
    if let Some(location) = code_location(args) {
        return Some(location);
    }
    if let Some(data) = args.get("data") {
        if let Some(location) = code_location(data) {
            return Some(location);
        }
        // Blink FunctionCall / EvaluateScript trace data uses one-based locations.
        if let (Some(url), Some(line)) = (
            data.get("url").and_then(Value::as_str),
            data.get("lineNumber").and_then(number),
        ) {
            return Some((url.into(), usize::try_from(line).ok()?));
        }
    }
    let call = args
        .get("callFrame")
        .or_else(|| args.get("data")?.get("callFrame"))
        .or_else(|| args.get("data")?.get("stackTrace")?.as_array()?.first())?;
    // DevTools Runtime.CallFrame locations are zero-based.
    let line = number(call.get("lineNumber")?)?.checked_add(1)?;
    Some((
        call.get("url")?.as_str()?.into(),
        usize::try_from(line).ok()?,
    ))
}

fn chrome(value: &Value, report: &mut Report) -> Result<Vec<Record>, String> {
    let events = value
        .as_array()
        .or_else(|| value.get("traceEvents")?.as_array())
        .ok_or("Chrome traceEvents must be an array.")?;
    let mut order = (0..events.len()).collect::<Vec<_>>();
    order.sort_by_key(|&index| {
        (
            chrome_time(&events[index], "ts").unwrap_or(i128::MAX),
            index,
        )
    });
    let mut records: Vec<Record> = Vec::new();
    let mut stacks: HashMap<String, Vec<usize>> = HashMap::new();
    for index in order {
        let event = &events[index];
        let phase = event.get("ph").and_then(Value::as_str).unwrap_or("");
        if !matches!(phase, "X" | "B" | "E" | "b" | "e") {
            report.ignored += 1;
            continue;
        }
        let name = name(event);
        let (Some(pid), Some(tid), Some(start)) = (
            identity(event, "pid"),
            identity(event, "tid"),
            chrome_time(event, "ts"),
        ) else {
            issue(
                report,
                format!("{name}: Chrome duration events require pid, tid and a finite timestamp."),
            );
            continue;
        };
        let (group, label) = if matches!(phase, "b" | "e") {
            let global = event.get("id2").and_then(|id| identity(id, "global"));
            let local = event
                .get("id2")
                .and_then(|id| identity(id, "local"))
                .or_else(|| identity(event, "id"));
            let Some(id) = global.as_ref().or(local.as_ref()) else {
                issue(report, format!("{name}: async event has no id."));
                continue;
            };
            let category = event.get("cat").and_then(Value::as_str).unwrap_or("");
            let scope = event.get("scope").and_then(Value::as_str).unwrap_or("");
            (
                format!(
                    "chrome:async:{}",
                    serde_json::json!([
                        if global.is_some() { "global" } else { &pid },
                        category,
                        scope,
                        id
                    ])
                ),
                format!("Chrome async {category} / {id}"),
            )
        } else {
            (
                format!("chrome:{}", serde_json::json!([pid, tid])),
                format!("Chrome process {pid} / thread {tid}"),
            )
        };
        if matches!(phase, "E" | "e") {
            if let Some(open) = stacks.get_mut(&group).and_then(Vec::pop) {
                let begin = records[open].start.unwrap();
                if start >= begin {
                    records[open].span.duration_ms = Some((start - begin) as f64 / 1_000_000.);
                } else {
                    issue(
                        report,
                        format!("{}: end precedes begin.", records[open].span.name),
                    );
                }
            } else {
                issue(
                    report,
                    format!(
                        "{name}: end event has no matching begin in this thread or async scope."
                    ),
                );
            }
            continue;
        }
        let duration = if phase == "X" {
            let duration = chrome_time(event, "dur")
                .filter(|&value| value >= 0)
                .ok_or_else(|| format!("{name}: complete event needs a nonnegative duration."));
            match duration {
                Ok(duration) => Some(duration as f64 / 1_000_000.),
                Err(error) => {
                    issue(report, error);
                    continue;
                }
            }
        } else {
            None
        };
        let args = event.get("args").unwrap_or(&Value::Null);
        let record = Record {
            label,
            start: Some(start),
            location: chrome_location(args),
            span: Span {
                group: group.clone(),
                name,
                id: String::new(),
                parent: String::new(),
                duration_ms: duration,
                error: args.get("error").and_then(Value::as_bool).unwrap_or(false),
            },
        };
        let record = push(&mut records, record)?;
        if phase != "X" {
            stacks.entry(group).or_default().push(record);
        }
    }
    for stack in stacks.values() {
        for &index in stack {
            issue(
                report,
                format!(
                    "{}: begin event has no matching end; duration unavailable.",
                    records[index].span.name
                ),
            );
        }
    }
    Ok(records)
}

fn ordered(records: &[Record]) -> Result<Vec<usize>, String> {
    let mut ids = HashMap::new();
    for (index, record) in records
        .iter()
        .enumerate()
        .filter(|(_, record)| !record.span.id.is_empty())
    {
        if ids
            .insert((record.span.group.as_str(), record.span.id.as_str()), index)
            .is_some()
        {
            return Err(
                "Duplicate span ID within one trace; cannot build an unambiguous route.".into(),
            );
        }
    }
    let mut order = (0..records.len()).collect::<Vec<_>>();
    order.sort_by_key(|&index| {
        (
            &records[index].span.group,
            records[index].start.unwrap_or(i128::MAX),
            index,
        )
    });
    let mut result = Vec::new();
    let mut state = vec![0; records.len()];
    fn visit(
        index: usize,
        records: &[Record],
        ids: &HashMap<(&str, &str), usize>,
        state: &mut [u8],
        result: &mut Vec<usize>,
    ) -> Result<(), String> {
        if state[index] == 2 {
            return Ok(());
        }
        if state[index] == 1 {
            return Err("Parent span cycle detected; route was not created.".into());
        }
        state[index] = 1;
        let span = &records[index].span;
        if !span.parent.is_empty()
            && let Some(&parent) = ids.get(&(span.group.as_str(), span.parent.as_str()))
        {
            visit(parent, records, ids, state, result)?;
        }
        state[index] = 2;
        result.push(index);
        Ok(())
    }
    for index in order {
        visit(index, records, &ids, &mut state, &mut result)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn span(id: &str, parent: &str, path: &str, line: usize, start: u64, end: u64) -> Value {
        json!({"traceId":"ABC", "spanId":id, "parentSpanId":parent, "name":id,
            "startTimeUnixNano":start.to_string(), "endTimeUnixNano":end.to_string(),
            "attributes":[{"key":"code.file.path","value":{"stringValue":path}},
                {"key":"code.line.number","value":{"intValue":line.to_string()}}]})
    }

    #[test]
    fn otlp_parents_precede_children_and_nanosecond_durations_keep_precision() {
        let start = 1_700_000_000_000_000_000;
        let mut child = span(
            "child",
            "root",
            "/project/lib/demo.rb",
            12,
            start - 10,
            start + 250_000,
        );
        child["status"] = json!({"code":2});
        let source = json!({"resourceSpans":[{"scopeSpans":[{"spans":[child, span("root", "", "src/main.rs", 2, start, start + 1_000_000)]}]}]}).to_string();
        let report = parse(&super::super::tests::repo(), &source);
        assert_eq!(
            report
                .frames
                .iter()
                .map(|frame| frame.line)
                .collect::<Vec<_>>(),
            [2, 12]
        );
        let child = report.frames[1].span.as_ref().unwrap();
        assert_eq!(child.parent, "root");
        assert_eq!(child.duration_ms, Some(0.25001));
        assert!(child.error);
    }

    #[test]
    fn independent_otel_traces_remain_separate_and_missing_code_is_reported() {
        let mut other = span("second", "", "src/app.js", 9, 1, 4);
        other["traceId"] = json!("OTHER");
        let missing = json!({"traceId":"ABC","spanId":"missing","name":"database","startTimeUnixNano":"1","endTimeUnixNano":"2"});
        let source = json!({"resourceSpans":[{"scopeSpans":[{"spans":[span("first", "", "src/main.rs", 2, 1, 5), other, missing]}]}]}).to_string();
        let report = parse(&super::super::tests::repo(), &source);
        assert_eq!(report.groups.len(), 2);
        assert_eq!(report.frames.len(), 2);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("database") && issue.contains("source"))
        );
        assert_ne!(
            report.frames[0].span.as_ref().unwrap().group,
            report.frames[1].span.as_ref().unwrap().group
        );
    }

    #[test]
    fn chrome_complete_and_begin_end_events_pair_per_thread_and_use_microseconds() {
        let source = json!({"traceEvents":[
            {"ph":"E","pid":1,"tid":2,"ts":2000},
            {"ph":"B","pid":1,"tid":2,"ts":1000,"name":"request","args":{"file":"src/main.rs","line":2}},
            {"ph":"X","pid":1,"tid":3,"ts":1000,"dur":250,"name":"worker","args":{"code.filepath":"lib/demo.rb","code.lineno":12}},
            {"ph":"M","pid":1,"tid":2,"name":"thread_name","args":{"name":"main"}}
        ]}).to_string();
        let report = parse(&super::super::tests::repo(), &source);
        assert_eq!(report.groups.len(), 2);
        assert_eq!(report.frames.len(), 2);
        assert_eq!(
            report.frames[0].span.as_ref().unwrap().duration_ms,
            Some(1.)
        );
        assert_eq!(
            report.frames[1].span.as_ref().unwrap().duration_ms,
            Some(0.25)
        );
    }

    #[test]
    fn malformed_unknown_or_cyclic_json_never_falls_back_to_text_location_scraping() {
        let repo = super::super::tests::repo();
        for source in ["{bad src/main.rs:2", r#"{"message":"src/main.rs:2"}"#] {
            let report = parse(&repo, source);
            assert!(report.frames.is_empty());
            assert!(!report.error.is_empty());
        }
        let source = json!({"resourceSpans":[{"scopeSpans":[{"spans":[span("a","b","src/main.rs",2,1,4), span("b","a","lib/demo.rb",12,2,3)]}]}]}).to_string();
        let report = parse(&repo, &source);
        assert!(report.frames.is_empty());
        assert!(report.error.contains("cycle"));
    }

    #[test]
    fn async_chrome_events_pair_across_threads_and_call_frames_use_zero_based_lines() {
        let source = json!([
            {"ph":"b","pid":1,"tid":2,"ts":100,"cat":"io","id":7,"name":"request","args":{"callFrame":{"url":"file:///project/src/app.js","lineNumber":0}}},
            {"ph":"e","pid":1,"tid":9,"ts":900,"cat":"io","id":7},
            {"ph":"E","pid":1,"tid":3,"ts":1000}
        ]).to_string();
        let report = parse(&super::super::tests::repo(), &source);
        assert_eq!(report.frames.len(), 1);
        assert_eq!(report.frames[0].line, 1);
        assert_eq!(
            report.frames[0].span.as_ref().unwrap().duration_ms,
            Some(0.8)
        );
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.contains("no matching begin"))
        );
    }

    #[test]
    fn selection_prevents_cross_trace_routes_and_edited_input_requires_rebuilding() {
        let repo = super::super::tests::repo();
        let mut other = span("other", "", "src/app.js", 9, 1, 4);
        other["traceId"] = json!("OTHER");
        let source = json!({"spans":[span("first", "", "src/main.rs", 2, 1, 5),other]}).to_string();
        let mut trace = Trace::default();
        trace.import(&repo, source.clone());
        assert!(!trace.follow(&repo));
        trace.select("otel:other");
        assert!(trace.follow(&repo));
        assert_eq!(
            trace
                .frames()
                .map(|frame| frame.path.as_str())
                .collect::<Vec<_>>(),
            ["src/app.js"]
        );
        trace.source.push(' ');
        assert!(!trace.can_follow());
        assert!(!trace.follow(&repo));
        trace.import(&repo, source);
        assert!(trace.selected.is_none());
    }

    #[test]
    fn duration_and_identity_errors_do_not_create_false_measurements_or_partial_routes() {
        let repo = super::super::tests::repo();
        let backwards = json!({"spans":[span("bad", "", "src/main.rs", 2, 10, 1)]}).to_string();
        assert!(parse(&repo, &backwards).frames.is_empty());
        let duplicate = json!({"spans":[span("same","","src/main.rs",2,1,3),span("same","","src/app.js",9,2,4)]}).to_string();
        let report = parse(&repo, &duplicate);
        assert!(report.error.contains("Duplicate"));
        assert!(report.frames.is_empty());
        let spans = (0..129)
            .map(|index| span(&index.to_string(), "", "src/main.rs", 2, 1, 3))
            .collect::<Vec<_>>();
        let report = parse(&repo, &json!({"spans":spans}).to_string());
        assert!(report.error.contains("128"));
        assert!(report.frames.is_empty());
    }
}
