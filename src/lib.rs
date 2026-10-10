//! GitHub-specific behavior lives entirely in this unprivileged component.
#![allow(clippy::too_many_arguments)] // Generated canonical ABI flattens WIT records.
wit_bindgen::generate!({ path: "cardbe-plugin-api/wit", world: "importer" });

use cardbe::plugin::types::{ExternalTask, HttpResponse};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

struct Github;
export!(Github);

#[derive(Deserialize)]
struct Config {
    repository: String,
    open_column_id: i64,
    closed_column_id: i64,
    #[serde(default = "yes")]
    include_labels: bool,
}
fn yes() -> bool {
    true
}
#[derive(Deserialize)]
struct Issue {
    number: u64,
    node_id: String,
    title: String,
    body: Option<String>,
    html_url: String,
    state: String,
    labels: Vec<Label>,
    updated_at: String,
    #[serde(default)]
    pull_request: Option<serde_json::Value>,
}
#[derive(Deserialize)]
struct Label {
    name: String,
}

impl Guest for Github {
    fn run(context: RunContext) -> Result<Changeset, PluginError> {
        let result = synchronize(&context, |url| {
            cardbe::plugin::host::http_get(url).map_err(|_| error("HTTP_FAILED", None))
        });
        if let Ok(changeset) = &result {
            cardbe::plugin::host::log("ISSUES_SYNCED", changeset.tasks.len() as u32);
        }
        result
    }
}
fn error(code: &str, retry_after_seconds: Option<u32>) -> PluginError {
    PluginError {
        code: code.into(),
        retry_after_seconds,
    }
}
fn repository(value: &str) -> bool {
    let parts: Vec<_> = value.split('/').collect();
    parts.len() == 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 100
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
                && !matches!(*part, "." | "..")
        })
}
fn previous_minute(value: &str) -> Option<String> {
    if value.len() != 20 || !value.is_ascii() {
        return None;
    }
    let bytes = value.as_bytes();
    if [4, 7, 10, 13, 16, 19].into_iter().any(|i| {
        bytes[i]
            != match i {
                4 | 7 => b'-',
                10 => b'T',
                13 | 16 => b':',
                _ => b'Z',
            }
    }) || bytes
        .iter()
        .enumerate()
        .any(|(i, b)| ![4, 7, 10, 13, 16, 19].contains(&i) && !b.is_ascii_digit())
    {
        return None;
    }
    let mut year: u32 = value[..4].parse().ok()?;
    let mut month: u32 = value[5..7].parse().ok()?;
    let mut day: u32 = value[8..10].parse().ok()?;
    let mut hour: u32 = value[11..13].parse().ok()?;
    let mut minute: u32 = value[14..16].parse().ok()?;
    let second: u32 = value[17..19].parse().ok()?;
    fn days(year: u32, month: u32) -> u32 {
        match month {
            2 => {
                if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
                {
                    29
                } else {
                    28
                }
            }
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        }
    }
    if year < 1
        || !(1..=12).contains(&month)
        || day < 1
        || day > days(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    if minute > 0 {
        minute -= 1;
    } else {
        minute = 59;
        if hour > 0 {
            hour -= 1;
        } else {
            hour = 23;
            if day > 1 {
                day -= 1;
            } else {
                if month > 1 {
                    month -= 1;
                } else {
                    year = year.checked_sub(1).filter(|year| *year >= 1)?;
                    month = 12;
                }
                day = days(year, month);
            }
        }
    }
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:00Z"
    ))
}
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}
fn header<'a>(response: &'a HttpResponse, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}
fn next_page(response: &HttpResponse) -> bool {
    header(response, "link").is_some_and(|link| {
        link.split(',').any(|part| {
            part.split(';')
                .skip(1)
                .any(|attribute| attribute.trim() == "rel=\"next\"")
        })
    })
}
fn synchronize(
    context: &RunContext,
    mut get: impl FnMut(&str) -> Result<HttpResponse, PluginError>,
) -> Result<Changeset, PluginError> {
    let config: Config =
        serde_json::from_str(&context.config_json).map_err(|_| error("INVALID_CONFIG", None))?;
    if !repository(&config.repository) || config.open_column_id < 1 || config.closed_column_id < 1 {
        return Err(error("INVALID_CONFIG", None));
    }
    // Overlap the previous minute, including updates sharing the last cursor second.
    // GitHub timestamps use canonical UTC seconds. Invalid cursors safely force a complete sync.
    let since = context.cursor.as_deref().and_then(previous_minute);
    let mut tasks: BTreeMap<String, ExternalTask> = BTreeMap::new();
    let mut cursor = context
        .cursor
        .as_ref()
        .filter(|value| previous_minute(value).is_some())
        .cloned();
    for page in 1..=100 {
        let mut url = format!("https://api.github.com/repos/{}/issues?state=all&sort=updated&direction=asc&per_page=100&page={page}", config.repository);
        // GitHub's `since` filter includes updates after this time; overlap by querying the
        // previous minute rather than skipping issues sharing the last second.
        if let Some(since) = &since {
            url.push_str("&since=");
            url.push_str(&encode(since));
        }
        let response = get(&url)?;
        if response.status == 429
            || (response.status == 403
                && (header(&response, "x-ratelimit-remaining") == Some("0")
                    || header(&response, "retry-after").is_some()))
        {
            return Err(error(
                "RATE_LIMITED",
                header(&response, "retry-after")
                    .and_then(|value| value.parse::<u32>().ok())
                    .map(|seconds| seconds.clamp(60, 86400))
                    .or(Some(3600)),
            ));
        }
        if response.status == 401 {
            return Err(error("AUTHENTICATION_FAILED", None));
        }
        if response.status == 404 {
            return Err(error("REPOSITORY_NOT_FOUND", None));
        }
        if response.status != 200 {
            return Err(error("API_FAILED", Some(60)));
        }
        let issues: Vec<Issue> =
            serde_json::from_slice(&response.body).map_err(|_| error("INVALID_RESPONSE", None))?;
        for issue in issues {
            if previous_minute(&issue.updated_at).is_none() {
                return Err(error("INVALID_RESPONSE", None));
            }
            if cursor
                .as_ref()
                .is_none_or(|previous| &issue.updated_at > previous)
            {
                cursor = Some(issue.updated_at.clone());
            }
            if issue.pull_request.is_some() {
                continue;
            }
            if issue.node_id.is_empty() || !matches!(issue.state.as_str(), "open" | "closed") {
                return Err(error("INVALID_RESPONSE", None));
            }
            let labels: BTreeSet<_> = if config.include_labels {
                issue.labels.into_iter().map(|label| label.name).collect()
            } else {
                BTreeSet::new()
            };
            let task = ExternalTask {
                external_key: format!("github:{}", issue.node_id),
                title: format!("#{} {}", issue.number, issue.title),
                description: format!("{}\n\n{}", issue.body.unwrap_or_default(), issue.html_url),
                url: issue.html_url,
                state: issue.state.clone(),
                labels: labels.into_iter().collect(),
                updated_at: issue.updated_at,
                column_id: if issue.state == "open" {
                    config.open_column_id
                } else {
                    config.closed_column_id
                },
            };
            // Pages can repeat an issue; never replace a newer update with an older one.
            if tasks
                .get(&task.external_key)
                .is_none_or(|previous| task.updated_at >= previous.updated_at)
            {
                tasks.insert(task.external_key.clone(), task);
            }
            if tasks.len() > 2000 {
                return Err(error("CHANGESET_TOO_LARGE", None));
            }
        }
        if !next_page(&response) {
            return Ok(Changeset {
                tasks: tasks.into_values().collect(),
                cursor,
            });
        }
    }
    Err(error("PAGE_LIMIT_EXCEEDED", None))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context(cursor: Option<&str>) -> RunContext {
        RunContext {
            instance_id: "instance".into(),
            run_id: "run".into(),
            config_json: r#"{"repository":"owner/repo","open_column_id":1,"closed_column_id":2}"#
                .into(),
            board_json: "{}".into(),
            cursor: cursor.map(str::to_owned),
        }
    }
    fn issue(node: &str, state: &str, pull: bool) -> serde_json::Value {
        serde_json::json!({"number":7,"node_id":node,"title":"Broken window","body":"Details","html_url":"https://github.com/owner/repo/issues/7","state":state,"labels":[{"name":"bug"}],"updated_at":"2026-10-09T12:34:56Z","pull_request":if pull { Some(serde_json::json!({})) } else {None}})
    }
    fn response(body: serde_json::Value, next: bool) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: if next {
                vec![(
                    "link".into(),
                    "<https://api.github.com/ignored>; rel=\"next\"".into(),
                )]
            } else {
                vec![]
            },
            body: serde_json::to_vec(&body).unwrap(),
        }
    }
    #[test]
    fn mapping_pagination_overlap_dedup_and_pull_requests() {
        let mut urls = vec![];
        let changes = synchronize(&context(Some("2026-10-09T12:34:56Z")), |url| {
            urls.push(url.to_string());
            Ok(response(
                serde_json::json!([issue("ID", "closed", false), issue("PR", "open", true)]),
                urls.len() == 1,
            ))
        })
        .unwrap();
        assert_eq!(urls.len(), 2);
        assert!(urls[1].contains("page=2"));
        assert!(urls[0].contains("since=2026-10-09T12%3A33%3A00Z"));
        assert_eq!(changes.tasks.len(), 1);
        let task = &changes.tasks[0];
        assert_eq!(task.external_key, "github:ID");
        assert_eq!(task.column_id, 2);
        assert_eq!(task.labels, vec!["bug"]);
        assert_eq!(
            task.description,
            "Details\n\nhttps://github.com/owner/repo/issues/7"
        );
        assert_eq!(changes.cursor.as_deref(), Some("2026-10-09T12:34:56Z"));
    }
    #[test]
    fn partial_failure_never_returns_a_changeset_or_cursor() {
        let mut calls = 0;
        let failure = synchronize(&context(None), |_| {
            calls += 1;
            if calls == 1 {
                Ok(response(
                    serde_json::json!([issue("ID", "open", false)]),
                    true,
                ))
            } else {
                Err(error("HTTP_FAILED", None))
            }
        })
        .unwrap_err();
        assert_eq!(failure.code, "HTTP_FAILED");
    }
    #[test]
    fn duplicate_issues_keep_the_latest_update_in_either_page_order() {
        for newest_first in [true, false] {
            let mut calls = 0;
            let changes = synchronize(&context(None), |_| {
                calls += 1;
                let mut item = issue("ID", "closed", false);
                if (calls == 1) == newest_first {
                    item["updated_at"] = "2026-10-09T12:35:56Z".into();
                } else {
                    item["state"] = "open".into();
                }
                Ok(response(serde_json::json!([item]), calls == 1))
            })
            .unwrap();
            assert_eq!(changes.tasks.len(), 1);
            assert_eq!(changes.tasks[0].state, "closed");
            assert_eq!(changes.tasks[0].column_id, 2);
            assert_eq!(changes.tasks[0].updated_at, "2026-10-09T12:35:56Z");
            assert_eq!(changes.cursor.as_deref(), Some("2026-10-09T12:35:56Z"));
        }
    }
    #[test]
    fn sync_limits_fail_without_returning_partial_changes() {
        let mut calls = 0;
        let failure = synchronize(&context(None), |_| {
            calls += 1;
            Ok(response(serde_json::json!([]), true))
        })
        .unwrap_err();
        assert_eq!(calls, 100);
        assert_eq!(failure.code, "PAGE_LIMIT_EXCEEDED");

        for count in [2000, 2001] {
            let result = synchronize(&context(None), |_| {
                let items: Vec<_> = (0..count)
                    .map(|i| issue(&i.to_string(), "open", false))
                    .collect();
                Ok(response(serde_json::json!(items), false))
            });
            if count == 2000 {
                assert_eq!(result.unwrap().tasks.len(), count);
            } else {
                assert_eq!(result.unwrap_err().code, "CHANGESET_TOO_LARGE");
            }
        }
    }
    #[test]
    fn cursor_validation_and_overlap_cross_calendar_boundaries() {
        assert!(previous_minute("0001-01-01T00:00:00Z").is_none());
        assert_eq!(
            previous_minute("2026-01-01T00:00:00Z").as_deref(),
            Some("2025-12-31T23:59:00Z")
        );
        assert_eq!(
            previous_minute("2024-03-01T00:00:00Z").as_deref(),
            Some("2024-02-29T23:59:00Z")
        );
        for cursor in [
            "\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}AZ",
            "2026-02-30T00:00:00Z",
            "2026-10-09T25:00:00Z",
            "2026-10-09T12:34:56Q",
        ] {
            assert!(previous_minute(cursor).is_none());
        }
    }
    #[test]
    fn rate_limits_and_invalid_config_are_structured() {
        let failure = synchronize(&context(None), |_| {
            Ok(HttpResponse {
                status: 429,
                headers: vec![("retry-after".into(), "120".into())],
                body: vec![],
            })
        })
        .unwrap_err();
        assert_eq!(failure.code, "RATE_LIMITED");
        assert_eq!(failure.retry_after_seconds, Some(120));
        assert!(!repository("../repo"));
        assert!(!repository("owner/repo?token=x"));
    }
}
