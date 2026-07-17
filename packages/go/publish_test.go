package alexsdk_test

import (
	"bytes"
	"io"
	"net/http"
	"path/filepath"
	"strings"
	"testing"

	alexsdk "github.com/The10thHunter/alexandria-sdk/packages/go"
)

// mockDoer captures the request and returns a canned response — no live server.
type mockDoer struct {
	captured   *http.Request
	body       []byte
	statusCode int
	respBody   string
}

func (m *mockDoer) Do(req *http.Request) (*http.Response, error) {
	m.captured = req
	if req.Body != nil {
		m.body, _ = io.ReadAll(req.Body)
	}
	return &http.Response{
		StatusCode: m.statusCode,
		Body:       io.NopCloser(strings.NewReader(m.respBody)),
		Header:     make(http.Header),
	}, nil
}

func publishFixture(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	out := filepath.Join(dir, "doer-0.1.0.aagent")
	if _, err := alexsdk.NewAgent("essentials/doer", "0.1.0").
		Description("doer").
		SystemPrompt("You are the doer.").
		Model("claude-opus-4-7").
		Pack(out); err != nil {
		t.Fatalf("pack fixture: %v", err)
	}
	return out
}

func TestPublishDerivesArtifactTypeAndPostsTarball(t *testing.T) {
	out := publishFixture(t)
	doer := &mockDoer{statusCode: 202, respBody: `{"assessment_id":"abc"}`}

	r, err := alexsdk.Publish(out, "https://reg.example/", alexsdk.PublishOptions{
		Token:      "sekret",
		HTTPClient: doer,
	})
	if err != nil {
		t.Fatalf("Publish: %v", err)
	}
	if !r.OK || r.Status != 202 {
		t.Fatalf("expected ok 202, got ok=%v status=%d", r.OK, r.Status)
	}
	if r.ArtifactType != "aagent" {
		t.Fatalf("expected artifact_type=aagent, got %q", r.ArtifactType)
	}
	if r.Name != "essentials/doer" {
		t.Fatalf("expected name=essentials/doer, got %q", r.Name)
	}
	if doer.captured.URL.String() != "https://reg.example/v1/submit" {
		t.Fatalf("expected trailing slash trimmed, got %q", doer.captured.URL.String())
	}
	if got := doer.captured.Header.Get("Authorization"); got != "Bearer sekret" {
		t.Fatalf("expected bearer auth, got %q", got)
	}
	if !strings.HasPrefix(doer.captured.Header.Get("Content-Type"), "multipart/form-data") {
		t.Fatalf("expected multipart content type, got %q", doer.captured.Header.Get("Content-Type"))
	}
	if !bytes.Contains(doer.body, []byte(`name="artifact_type"`)) || !bytes.Contains(doer.body, []byte("aagent")) {
		t.Fatal("multipart body missing artifact_type=aagent")
	}
	if !bytes.Contains(doer.body, []byte(`name="tarball"`)) || !bytes.Contains(doer.body, []byte(`filename="doer-0.1.0.aagent"`)) {
		t.Fatal("multipart body missing tarball part")
	}
}

func TestPublishSurfacesNon2xxAsNotOK(t *testing.T) {
	out := publishFixture(t)
	doer := &mockDoer{statusCode: 400, respBody: `{"error":"stage1_kind_enum"}`}

	r, err := alexsdk.Publish(out, "https://reg.example", alexsdk.PublishOptions{HTTPClient: doer})
	if err != nil {
		t.Fatalf("Publish: %v", err)
	}
	if r.OK {
		t.Fatal("expected ok=false for 400")
	}
	if r.Status != 400 {
		t.Fatalf("expected status 400, got %d", r.Status)
	}
}

func TestPublishHonorsArtifactTypeOverride(t *testing.T) {
	out := publishFixture(t)
	doer := &mockDoer{statusCode: 202, respBody: "{}"}

	r, err := alexsdk.Publish(out, "https://reg.example", alexsdk.PublishOptions{
		ArtifactType: "amodel:llm-backend",
		HTTPClient:   doer,
	})
	if err != nil {
		t.Fatalf("Publish: %v", err)
	}
	if r.ArtifactType != "amodel:llm-backend" {
		t.Fatalf("expected override artifact_type, got %q", r.ArtifactType)
	}
	if !bytes.Contains(doer.body, []byte("amodel:llm-backend")) {
		t.Fatal("multipart body missing overridden artifact_type")
	}
	// no auth header when no token
	if got := doer.captured.Header.Get("Authorization"); got != "" {
		t.Fatalf("expected no auth header, got %q", got)
	}
}
