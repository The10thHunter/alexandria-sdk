package alexsdk

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
)

// InspectFile is one entry in the tar listing surfaced by Inspect.
type InspectFile struct {
	Name string `json:"name"`
	Size int64  `json:"size"`
}

// InspectResult is the parsed output of Inspect.
type InspectResult struct {
	Manifest   *Manifest     `json:"manifest"`
	Files      []InspectFile `json:"files"`
	TotalBytes int64         `json:"totalBytes"`
}

// Pack reads <srcDir>/atool.json, hashes every declared file, then writes a
// gzipped tar archive to outPath. atool.json is the first tar entry; each
// files[] entry follows in declaration order. Returns the populated manifest.
func Pack(srcDir, outPath string) (*Manifest, error) {
	manifestPath := filepath.Join(srcDir, "atool.json")
	raw, err := os.ReadFile(manifestPath)
	if err != nil {
		return nil, fmt.Errorf("read manifest %s: %w", manifestPath, err)
	}
	var manifest Manifest
	if err := json.Unmarshal(raw, &manifest); err != nil {
		return nil, fmt.Errorf("parse manifest %s: %w", manifestPath, err)
	}

	// Hash each declared file and write the digest back into the manifest.
	for i := range manifest.Files {
		abs := filepath.Join(srcDir, manifest.Files[i].ArchivePath)
		sum, err := sha256File(abs)
		if err != nil {
			return nil, fmt.Errorf("hash %s: %w", abs, err)
		}
		manifest.Files[i].SHA256 = sum
	}

	if err := AssertValid(&manifest); err != nil {
		return nil, err
	}

	manifestBytes, err := json.MarshalIndent(&manifest, "", "  ")
	if err != nil {
		return nil, fmt.Errorf("encode manifest: %w", err)
	}

	out, err := os.Create(outPath)
	if err != nil {
		return nil, fmt.Errorf("create %s: %w", outPath, err)
	}
	defer out.Close()

	gz := gzip.NewWriter(out)
	tw := tar.NewWriter(gz)

	if err := writeTarFile(tw, "atool.json", manifestBytes, 0o644); err != nil {
		return nil, fmt.Errorf("write atool.json entry: %w", err)
	}

	for _, f := range manifest.Files {
		abs := filepath.Join(srcDir, f.ArchivePath)
		st, err := os.Stat(abs)
		if err != nil {
			return nil, fmt.Errorf("stat %s: %w", abs, err)
		}
		mode := int64(0o644)
		if f.Executable {
			mode = 0o755
		}
		hdr := &tar.Header{
			Name:    f.ArchivePath,
			Mode:    mode,
			Size:    st.Size(),
			ModTime: st.ModTime(),
		}
		if err := tw.WriteHeader(hdr); err != nil {
			return nil, fmt.Errorf("write header %s: %w", f.ArchivePath, err)
		}
		src, err := os.Open(abs)
		if err != nil {
			return nil, fmt.Errorf("open %s: %w", abs, err)
		}
		if _, err := io.Copy(tw, src); err != nil {
			src.Close()
			return nil, fmt.Errorf("copy %s: %w", abs, err)
		}
		src.Close()
	}

	if err := tw.Close(); err != nil {
		return nil, fmt.Errorf("close tar: %w", err)
	}
	if err := gz.Close(); err != nil {
		return nil, fmt.Errorf("close gzip: %w", err)
	}
	return &manifest, nil
}

// Verify extracts pkgPath in memory, validates the embedded manifest, and
// re-hashes every declared file with a non-empty sha256.
func Verify(pkgPath string) (*Manifest, error) {
	manifest, bytesMap, _, err := readArchive(pkgPath, true)
	if err != nil {
		return nil, err
	}
	if err := AssertValid(manifest); err != nil {
		return nil, err
	}
	for _, f := range manifest.Files {
		if f.SHA256 == "" {
			continue
		}
		buf, ok := bytesMap[f.ArchivePath]
		if !ok {
			return nil, fmt.Errorf("declared file missing from archive: %s", f.ArchivePath)
		}
		sum := sha256.Sum256(buf)
		got := hex.EncodeToString(sum[:])
		if got != f.SHA256 {
			return nil, fmt.Errorf("sha256 mismatch for %s: want %s, got %s", f.ArchivePath, f.SHA256, got)
		}
	}
	return manifest, nil
}

// Inspect lists the contents of a package without verifying file digests.
func Inspect(pkgPath string) (*InspectResult, error) {
	manifest, _, sizes, err := readArchive(pkgPath, false)
	if err != nil {
		return nil, err
	}
	files := make([]InspectFile, 0, len(sizes))
	var total int64
	for _, s := range sizes {
		files = append(files, s)
		total += s.Size
	}
	return &InspectResult{Manifest: manifest, Files: files, TotalBytes: total}, nil
}

func readArchive(pkgPath string, keepBytes bool) (*Manifest, map[string][]byte, []InspectFile, error) {
	f, err := os.Open(pkgPath)
	if err != nil {
		return nil, nil, nil, fmt.Errorf("open %s: %w", pkgPath, err)
	}
	defer f.Close()

	gz, err := gzip.NewReader(f)
	if err != nil {
		return nil, nil, nil, fmt.Errorf("gunzip %s: %w", pkgPath, err)
	}
	defer gz.Close()

	tr := tar.NewReader(gz)
	var manifest *Manifest
	bytesMap := map[string][]byte{}
	var sizes []InspectFile

	for {
		hdr, err := tr.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil, nil, nil, fmt.Errorf("read tar entry: %w", err)
		}
		var buf bytes.Buffer
		if _, err := io.Copy(&buf, tr); err != nil {
			return nil, nil, nil, fmt.Errorf("read entry %s: %w", hdr.Name, err)
		}
		sizes = append(sizes, InspectFile{Name: hdr.Name, Size: int64(buf.Len())})
		if hdr.Name == "atool.json" {
			var m Manifest
			if err := json.Unmarshal(buf.Bytes(), &m); err != nil {
				return nil, nil, nil, fmt.Errorf("parse atool.json: %w", err)
			}
			manifest = &m
			continue
		}
		if keepBytes {
			bytesMap[hdr.Name] = append([]byte(nil), buf.Bytes()...)
		}
	}

	if manifest == nil {
		return nil, nil, nil, fmt.Errorf("atool.json not found in archive %s", pkgPath)
	}
	return manifest, bytesMap, sizes, nil
}

func writeTarFile(tw *tar.Writer, name string, data []byte, mode int64) error {
	hdr := &tar.Header{Name: name, Mode: mode, Size: int64(len(data))}
	if err := tw.WriteHeader(hdr); err != nil {
		return err
	}
	_, err := tw.Write(data)
	return err
}

func sha256File(p string) (string, error) {
	f, err := os.Open(p)
	if err != nil {
		return "", err
	}
	defer f.Close()
	h := sha256.New()
	if _, err := io.Copy(h, f); err != nil {
		return "", err
	}
	return hex.EncodeToString(h.Sum(nil)), nil
}
