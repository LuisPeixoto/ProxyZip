use std::collections::BTreeMap;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct AssertionResult {
    pub description: String,
    pub passed: bool,
    /// Why it failed.
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestResult {
    pub name: String,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub duration_ms: u64,
    pub passed: bool,
    /// The request could not be sent, or the suite stopped before it ran.
    pub error: Option<String>,
    pub assertions: Vec<AssertionResult>,
    pub extracted: BTreeMap<String, String>,
    pub skipped: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub suite: String,
    pub environment: Option<String>,
    pub duration_ms: u64,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub results: Vec<RequestResult>,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.failed == 0
    }

    /// A readable summary for a terminal.
    pub fn to_text(&self) -> String {
        let mut out =
            format!("{}{}\n", self.suite, self.environment.as_ref().map(|e| format!(" [{e}]")).unwrap_or_default());
        for r in &self.results {
            let mark = if r.skipped {
                "-"
            } else if r.passed {
                "✓"
            } else {
                "✗"
            };
            let status = r.status.map(|s| s.to_string()).unwrap_or_else(|| "---".into());
            out.push_str(&format!("{mark} {:<40} {status} {:>5} ms\n", r.name, r.duration_ms));
            if let Some(e) = &r.error {
                out.push_str(&format!("    {e}\n"));
            }
            for a in r.assertions.iter().filter(|a| !a.passed) {
                out.push_str(&format!(
                    "    ✗ {}{}\n",
                    a.description,
                    a.message.as_ref().map(|m| format!(": {m}")).unwrap_or_default()
                ));
            }
        }
        out.push_str(&format!(
            "\n{} passed, {} failed, {} skipped in {} ms\n",
            self.passed, self.failed, self.skipped, self.duration_ms
        ));
        out
    }
}

fn escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".to_string(),
            '<' => "&lt;".to_string(),
            '>' => "&gt;".to_string(),
            '"' => "&quot;".to_string(),
            '\'' => "&apos;".to_string(),
            c if (c as u32) < 0x20 && c != '\n' && c != '\t' => String::new(),
            c => c.to_string(),
        })
        .collect()
}

/// JUnit XML, understood by every CI system.
pub fn to_junit(report: &Report) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" time=\"{:.3}\">\n",
        escape(&report.suite),
        report.results.len(),
        report.failed,
        report.skipped,
        report.duration_ms as f64 / 1000.0
    ));
    for r in &report.results {
        out.push_str(&format!(
            "  <testcase classname=\"{}\" name=\"{}\" time=\"{:.3}\"",
            escape(&report.suite),
            escape(&r.name),
            r.duration_ms as f64 / 1000.0
        ));
        if r.skipped {
            out.push_str("><skipped/></testcase>\n");
        } else if r.passed {
            out.push_str("/>\n");
        } else {
            let mut details = Vec::new();
            if let Some(e) = &r.error {
                details.push(e.clone());
            }
            for a in r.assertions.iter().filter(|a| !a.passed) {
                details.push(format!(
                    "{}{}",
                    a.description,
                    a.message.as_ref().map(|m| format!(": {m}")).unwrap_or_default()
                ));
            }
            let message = details.first().cloned().unwrap_or_else(|| "failed".into());
            out.push_str(&format!(
                ">\n    <failure message=\"{}\">{}</failure>\n  </testcase>\n",
                escape(&message),
                escape(&details.join("\n"))
            ));
        }
    }
    out.push_str("</testsuite>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> Report {
        Report {
            suite: "api <v1>".into(),
            environment: Some("dev".into()),
            duration_ms: 1500,
            passed: 1,
            failed: 1,
            skipped: 1,
            results: vec![
                RequestResult {
                    name: "ok".into(),
                    method: "GET".into(),
                    url: "u".into(),
                    status: Some(200),
                    duration_ms: 10,
                    passed: true,
                    error: None,
                    assertions: vec![],
                    extracted: BTreeMap::new(),
                    skipped: false,
                },
                RequestResult {
                    name: "bad \"one\"".into(),
                    method: "GET".into(),
                    url: "u".into(),
                    status: Some(500),
                    duration_ms: 20,
                    passed: false,
                    error: None,
                    assertions: vec![AssertionResult {
                        description: "status is 200".into(),
                        passed: false,
                        message: Some("got 500".into()),
                    }],
                    extracted: BTreeMap::new(),
                    skipped: false,
                },
                RequestResult {
                    name: "later".into(),
                    method: "GET".into(),
                    url: "u".into(),
                    status: None,
                    duration_ms: 0,
                    passed: false,
                    error: Some("skipped".into()),
                    assertions: vec![],
                    extracted: BTreeMap::new(),
                    skipped: true,
                },
            ],
        }
    }

    #[test]
    fn junit_is_escaped_and_complete() {
        let xml = to_junit(&report());
        assert!(
            xml.contains("<testsuite name=\"api &lt;v1&gt;\" tests=\"3\" failures=\"1\" skipped=\"1\" time=\"1.500\">")
        );
        assert!(xml.contains("name=\"bad &quot;one&quot;\""));
        assert!(xml.contains("<failure message=\"status is 200: got 500\">"));
        assert!(xml.contains("<skipped/>"));
        assert_eq!(xml.matches("<testcase").count(), 3);
    }

    #[test]
    fn text_report_lists_failures() {
        let text = report().to_text();
        assert!(text.contains("✗ status is 200: got 500"));
        assert!(text.contains("1 passed, 1 failed, 1 skipped"));
        assert!(!report().ok());
    }
}
