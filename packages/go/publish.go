package alexsdk

import (
	"bytes"
	"fmt"
	"io"
	"mime/multipart"
	"net/http"
	"os"
	"path/filepath"
	"strings"
)

// PublishOptions controls a Publish call.
type PublishOptions struct {
	// Token is an optional bearer token (sent as Authorization: Bearer <token>).
	Token string
	// ArtifactType overrides the artifact_type; defaults to the manifest kind.
	ArtifactType string
	// HTTPClient lets callers (and tests) inject a custom transport. When nil,
	// http.DefaultClient is used.
	HTTPClient HTTPDoer
}

// HTTPDoer is the subset of *http.Client used by Publish. Tests inject a mock.
type HTTPDoer interface {
	Do(req *http.Request) (*http.Response, error)
}

// PublishResult is the outcome of a Publish attempt.
type PublishResult struct {
	Status       int
	OK           bool
	Body         string
	Name         string
	Version      string
	ArtifactType string
}

// Publish re-verifies the archive at pkgPath (hashes + schema) then POSTs a
// multipart body to {registry}/v1/submit — the missing consumer-side half of
// the registry loop (`alexandria install` pulls; nothing pushed until now).
//
// The multipart body mirrors the registry's handleSubmit contract:
//   - artifact_type — derived from the manifest kind (mcp|atool|aagent),
//     overridable for amodel sub-variants.
//   - tarball — the packed archive bytes.
func Publish(pkgPath, registry string, opts PublishOptions) (*PublishResult, error) {
	// Never publish an archive the local runtime would itself reject.
	manifest, err := Verify(pkgPath)
	if err != nil {
		return nil, fmt.Errorf("verify %s: %w", pkgPath, err)
	}
	artifactType := opts.ArtifactType
	if artifactType == "" {
		artifactType = string(manifest.Kind)
	}

	tarball, err := os.ReadFile(pkgPath)
	if err != nil {
		return nil, fmt.Errorf("read %s: %w", pkgPath, err)
	}

	var buf bytes.Buffer
	mw := multipart.NewWriter(&buf)
	if err := mw.WriteField("artifact_type", artifactType); err != nil {
		return nil, fmt.Errorf("write artifact_type field: %w", err)
	}
	part, err := mw.CreateFormFile("tarball", filepath.Base(pkgPath))
	if err != nil {
		return nil, fmt.Errorf("create tarball part: %w", err)
	}
	if _, err := part.Write(tarball); err != nil {
		return nil, fmt.Errorf("write tarball part: %w", err)
	}
	if err := mw.Close(); err != nil {
		return nil, fmt.Errorf("close multipart writer: %w", err)
	}

	base := strings.TrimRight(registry, "/")
	url := base + "/v1/submit"
	req, err := http.NewRequest(http.MethodPost, url, &buf)
	if err != nil {
		return nil, fmt.Errorf("build request: %w", err)
	}
	req.Header.Set("Content-Type", mw.FormDataContentType())
	if opts.Token != "" {
		req.Header.Set("Authorization", "Bearer "+opts.Token)
	}

	client := opts.HTTPClient
	if client == nil {
		client = http.DefaultClient
	}
	resp, err := client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("post %s: %w", url, err)
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)

	return &PublishResult{
		Status:       resp.StatusCode,
		OK:           resp.StatusCode >= 200 && resp.StatusCode < 300,
		Body:         string(body),
		Name:         manifest.Name,
		Version:      manifest.Version,
		ArtifactType: artifactType,
	}, nil
}
