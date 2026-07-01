//! Publish tests using a mock [`Transport`] so no live registry is needed —
//! mirrors the TypeScript SDK's `test/publish.test.ts`.

use std::cell::RefCell;
use std::path::Path;

use alex_sdk::publish::{publish_with, PublishOptions, Transport};
use alex_sdk::{Agent, Result};

/// Build a real packed .aagent fixture so publish()'s verify() step passes.
fn fixture(dir: &Path) -> std::path::PathBuf {
    let out = dir.join("doer-0.1.0.aagent");
    Agent::new("essentials/doer", "0.1.0")
        .description("doer")
        .system_prompt("You are the doer.")
        .model("claude-opus-4-7")
        .pack(&out)
        .expect("pack fixture");
    out
}

#[derive(Default)]
struct MockTransport {
    status: u16,
    body: String,
    captured: RefCell<Option<Captured>>,
}

struct Captured {
    url: String,
    content_type: String,
    body: Vec<u8>,
    token: Option<String>,
}

impl Transport for MockTransport {
    fn send(
        &self,
        url: &str,
        content_type: &str,
        body: &[u8],
        token: Option<&str>,
    ) -> Result<(u16, String)> {
        *self.captured.borrow_mut() = Some(Captured {
            url: url.to_string(),
            content_type: content_type.to_string(),
            body: body.to_vec(),
            token: token.map(|t| t.to_string()),
        });
        Ok((self.status, self.body.clone()))
    }
}

#[test]
fn publish_derives_artifact_type_and_posts_tarball() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = fixture(dir.path());

    let mock = MockTransport {
        status: 202,
        body: r#"{"assessment_id":"abc"}"#.to_string(),
        ..Default::default()
    };
    let opts = PublishOptions {
        token: Some("sekret".to_string()),
        artifact_type: None,
    };

    let r = publish_with(&out, "https://reg.example/", &opts, &mock).expect("publish");

    assert!(r.ok);
    assert_eq!(r.status, 202);
    assert_eq!(r.artifact_type, "aagent"); // derived from manifest kind
    assert_eq!(r.name, "essentials/doer");

    let cap = mock.captured.borrow();
    let cap = cap.as_ref().expect("captured request");
    assert_eq!(cap.url, "https://reg.example/v1/submit"); // trailing slash trimmed
    assert_eq!(cap.token.as_deref(), Some("sekret"));
    assert!(cap
        .content_type
        .starts_with("multipart/form-data; boundary="));

    let body = String::from_utf8_lossy(&cap.body);
    assert!(body.contains("name=\"artifact_type\""));
    assert!(body.contains("aagent"));
    assert!(body.contains("name=\"tarball\""));
    assert!(body.contains("filename=\"doer-0.1.0.aagent\""));
}

#[test]
fn publish_surfaces_non_2xx_as_not_ok() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = fixture(dir.path());

    let mock = MockTransport {
        status: 400,
        body: r#"{"error":"stage1_kind_enum"}"#.to_string(),
        ..Default::default()
    };
    let r = publish_with(
        &out,
        "https://reg.example",
        &PublishOptions::default(),
        &mock,
    )
    .expect("publish");
    assert!(!r.ok);
    assert_eq!(r.status, 400);
}

#[test]
fn publish_honors_artifact_type_override() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = fixture(dir.path());

    let mock = MockTransport {
        status: 202,
        body: "{}".to_string(),
        ..Default::default()
    };
    let opts = PublishOptions {
        token: None,
        artifact_type: Some("amodel:llm-backend".to_string()),
    };
    let r = publish_with(&out, "https://reg.example", &opts, &mock).expect("publish");
    assert_eq!(r.artifact_type, "amodel:llm-backend");

    let cap = mock.captured.borrow();
    let body = String::from_utf8_lossy(&cap.as_ref().unwrap().body);
    assert!(body.contains("amodel:llm-backend"));
}
