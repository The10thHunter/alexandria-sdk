// Command alex-sdk authors and inspects Alexandria .atool / .aagent packages.
package main

import (
	"encoding/json"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"

	alexsdk "github.com/The10thHunter/alexandria-sdk/packages/go"
)

const help = `alex-sdk — author .atool / .aagent packages

USAGE
  alex-sdk init <template> <dir>     Scaffold a new package source dir
  alex-sdk pack <src-dir> [-o out]   Pack into .atool or .aagent
  alex-sdk verify <pkg>              Re-hash files, validate manifest
  alex-sdk inspect <pkg>             Print manifest + file list
  alex-sdk migrate <src> [-o out]    Upgrade v1 atool.json to v2

TEMPLATES
  tool-node, tool-python, agent-basic, agent-collection

EXAMPLES
  alex-sdk init agent-basic ./my-agent
  alex-sdk pack ./my-agent -o my-agent-0.1.0.aagent
  alex-sdk verify my-agent-0.1.0.aagent
  alex-sdk migrate old-atool.json -o atool.json
`

func die(msg string, code int) {
	fmt.Fprintln(os.Stderr, msg)
	os.Exit(code)
}

func main() {
	if len(os.Args) < 2 {
		fmt.Print(help)
		return
	}
	cmd := os.Args[1]
	rest := os.Args[2:]
	switch cmd {
	case "-h", "--help":
		fmt.Print(help)
	case "init":
		cmdInit(rest)
	case "pack":
		cmdPack(rest)
	case "verify":
		cmdVerify(rest)
	case "inspect":
		cmdInspect(rest)
	case "migrate":
		cmdMigrate(rest)
	default:
		die(fmt.Sprintf("unknown command '%s'\n\n%s", cmd, help), 1)
	}
}

func cmdInit(args []string) {
	if len(args) < 2 {
		die("usage: alex-sdk init <template> <dir>", 1)
	}
	tpl, dir := args[0], args[1]
	root, err := templatesRoot()
	if err != nil {
		die(err.Error(), 1)
	}
	src := filepath.Join(root, tpl)
	if _, err := os.Stat(src); err != nil {
		entries, _ := os.ReadDir(root)
		names := make([]string, 0, len(entries))
		for _, e := range entries {
			names = append(names, e.Name())
		}
		die(fmt.Sprintf("unknown template '%s'. Available: %v", tpl, names), 1)
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		die(err.Error(), 1)
	}
	if err := copyTree(src, dir); err != nil {
		die(err.Error(), 1)
	}
	fmt.Printf("Scaffolded %s into %s\nEdit atool.json, then: alex-sdk pack %s\n", tpl, dir, dir)
}

func cmdPack(args []string) {
	if len(args) < 1 {
		die("usage: alex-sdk pack <src-dir> [-o out]", 1)
	}
	srcDir := args[0]
	out := ""
	for i := 1; i < len(args); i++ {
		if args[i] == "-o" && i+1 < len(args) {
			out = args[i+1]
			i++
		}
	}
	manifestPath := filepath.Join(srcDir, "atool.json")
	raw, err := os.ReadFile(manifestPath)
	if err != nil {
		die(err.Error(), 1)
	}
	var m alexsdk.Manifest
	if err := json.Unmarshal(raw, &m); err != nil {
		die(fmt.Sprintf("parse %s: %v", manifestPath, err), 1)
	}
	if out == "" {
		out = defaultOutPath(&m)
	}
	pm, err := alexsdk.Pack(srcDir, out)
	if err != nil {
		die(err.Error(), 1)
	}
	fmt.Printf("Packed %s@%s -> %s\n", pm.Name, pm.Version, out)
}

func cmdVerify(args []string) {
	if len(args) < 1 {
		die("usage: alex-sdk verify <pkg>", 1)
	}
	m, err := alexsdk.Verify(args[0])
	if err != nil {
		die(err.Error(), 1)
	}
	fmt.Printf("OK %s@%s (kind=%s)\n", m.Name, m.Version, m.Kind)
}

func cmdInspect(args []string) {
	if len(args) < 1 {
		die("usage: alex-sdk inspect <pkg>", 1)
	}
	r, err := alexsdk.Inspect(args[0])
	if err != nil {
		die(err.Error(), 1)
	}
	b, _ := json.MarshalIndent(r, "", "  ")
	fmt.Println(string(b))
}

func cmdMigrate(args []string) {
	if len(args) < 1 {
		die("usage: alex-sdk migrate <src> [-o <out>]", 1)
	}
	srcArg := args[0]
	outPath := ""
	for i := 1; i < len(args); i++ {
		if args[i] == "-o" && i+1 < len(args) {
			outPath = args[i+1]
			i++
		}
	}

	// Resolve src: if directory, look for atool.json inside
	info, err := os.Stat(srcArg)
	if err != nil {
		die(fmt.Sprintf("cannot access %s: %v", srcArg, err), 1)
	}
	resolved := srcArg
	if info.IsDir() {
		resolved = filepath.Join(srcArg, "atool.json")
	}

	raw, err := os.ReadFile(resolved)
	if err != nil {
		die(fmt.Sprintf("cannot read %s: %v", resolved, err), 1)
	}

	var v1 map[string]interface{}
	if err := json.Unmarshal(raw, &v1); err != nil {
		die(fmt.Sprintf("invalid JSON in %s: %v", resolved, err), 1)
	}

	result, warnings, errors := alexsdk.MigrateManifest(v1)

	if len(errors) > 0 {
		fmt.Fprintln(os.Stderr, "Migration errors:")
		for _, e := range errors {
			fmt.Fprintf(os.Stderr, "  ERROR: %s\n", e)
		}
		os.Exit(1)
	}

	out, err := json.MarshalIndent(result, "", "  ")
	if err != nil {
		die(fmt.Sprintf("marshal: %v", err), 1)
	}
	out = append(out, '\n')

	dest := outPath
	if dest == "" {
		dest = resolved
	}
	if err := os.WriteFile(dest, out, 0o644); err != nil {
		die(fmt.Sprintf("write %s: %v", dest, err), 1)
	}

	if len(warnings) > 0 {
		fmt.Fprintln(os.Stderr, "Migration warnings:")
		for _, w := range warnings {
			fmt.Fprintf(os.Stderr, "  WARN: %s\n", w)
		}
	}
	fmt.Printf("Migrated to v2 -> %s\n", dest)
}

func defaultOutPath(m *alexsdk.Manifest) string {
	short := m.Name
	if i := lastSlash(short); i >= 0 {
		short = short[i+1:]
	}
	ext := "atool"
	if m.Kind == alexsdk.KindAgent {
		ext = "aagent"
	}
	return fmt.Sprintf("%s-%s.%s", short, m.Version, ext)
}

func lastSlash(s string) int {
	for i := len(s) - 1; i >= 0; i-- {
		if s[i] == '/' {
			return i
		}
	}
	return -1
}

// templatesRoot resolves the templates/ directory the same way the TS CLI
// does: alongside the executable first, then walking upward from cwd.
func templatesRoot() (string, error) {
	candidates := []string{}
	if exe, err := os.Executable(); err == nil {
		dir := filepath.Dir(exe)
		candidates = append(candidates,
			filepath.Join(dir, "templates"),
			filepath.Join(dir, "..", "templates"),
			filepath.Join(dir, "..", "..", "templates"),
			filepath.Join(dir, "..", "..", "..", "templates"),
		)
	}
	if cwd, err := os.Getwd(); err == nil {
		d := cwd
		for i := 0; i < 8; i++ {
			candidates = append(candidates, filepath.Join(d, "templates"))
			parent := filepath.Dir(d)
			if parent == d {
				break
			}
			d = parent
		}
	}
	for _, p := range candidates {
		if st, err := os.Stat(p); err == nil && st.IsDir() {
			return p, nil
		}
	}
	return "", fmt.Errorf("templates/ not found near executable or cwd")
}

func copyTree(src, dst string) error {
	return filepath.WalkDir(src, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(src, path)
		if err != nil {
			return err
		}
		target := filepath.Join(dst, rel)
		if d.IsDir() {
			return os.MkdirAll(target, 0o755)
		}
		return copyOne(path, target)
	})
}

func copyOne(src, dst string) error {
	in, err := os.Open(src)
	if err != nil {
		return err
	}
	defer in.Close()
	if err := os.MkdirAll(filepath.Dir(dst), 0o755); err != nil {
		return err
	}
	out, err := os.Create(dst)
	if err != nil {
		return err
	}
	defer out.Close()
	_, err = io.Copy(out, in)
	return err
}
