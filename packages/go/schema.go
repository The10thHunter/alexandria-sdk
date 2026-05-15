package alexsdk

import (
	_ "embed"
	"encoding/json"
	"fmt"
	"strings"
	"sync"

	"github.com/santhosh-tekuri/jsonschema/v5"
)

//go:embed atool.schema.json
var embeddedSchema []byte

// ValidationError describes a single schema violation.
type ValidationError struct {
	Path    string
	Message string
}

// Error renders the violation as "<path>: <message>".
func (e ValidationError) Error() string { return e.Path + ": " + e.Message }

var (
	compiledOnce sync.Once
	compiled     *jsonschema.Schema
	compiledErr  error
)

func compiledSchema() (*jsonschema.Schema, error) {
	compiledOnce.Do(func() {
		c := jsonschema.NewCompiler()
		var raw any
		if err := json.Unmarshal(embeddedSchema, &raw); err != nil {
			compiledErr = fmt.Errorf("parse embedded schema: %w", err)
			return
		}
		if err := c.AddResource("atool.schema.json", strings.NewReader(string(embeddedSchema))); err != nil {
			compiledErr = fmt.Errorf("add schema resource: %w", err)
			return
		}
		s, err := c.Compile("atool.schema.json")
		if err != nil {
			compiledErr = fmt.Errorf("compile schema: %w", err)
			return
		}
		compiled = s
	})
	return compiled, compiledErr
}

// Validate runs the embedded JSON Schema against the given manifest.
//
// The argument may be either a *Manifest, a map[string]any, or anything that
// round-trips through encoding/json. The first return is the list of
// violations (empty == valid); the second is a hard error (e.g. compilation).
func Validate(manifest any) ([]ValidationError, error) {
	s, err := compiledSchema()
	if err != nil {
		return nil, err
	}
	// Re-encode through JSON so typed structs are turned into the same shape
	// the schema expects (map[string]any with snake_case keys).
	b, err := json.Marshal(manifest)
	if err != nil {
		return nil, fmt.Errorf("marshal manifest for validation: %w", err)
	}
	var generic any
	if err := json.Unmarshal(b, &generic); err != nil {
		return nil, fmt.Errorf("re-decode manifest: %w", err)
	}
	if err := s.Validate(generic); err != nil {
		var ve *jsonschema.ValidationError
		if asValidation(err, &ve) {
			return flattenValidation(ve), nil
		}
		return nil, fmt.Errorf("validate: %w", err)
	}
	return nil, nil
}

// AssertValid wraps Validate and returns a joined error if the manifest is
// invalid (matching the TS SDK contract).
func AssertValid(manifest any) error {
	errs, err := Validate(manifest)
	if err != nil {
		return err
	}
	if len(errs) == 0 {
		return nil
	}
	var b strings.Builder
	b.WriteString("Invalid atool manifest:\n")
	for _, e := range errs {
		fmt.Fprintf(&b, "  %s: %s\n", e.Path, e.Message)
	}
	return fmt.Errorf("%s", strings.TrimRight(b.String(), "\n"))
}

func asValidation(err error, out **jsonschema.ValidationError) bool {
	if ve, ok := err.(*jsonschema.ValidationError); ok {
		*out = ve
		return true
	}
	return false
}

func flattenValidation(ve *jsonschema.ValidationError) []ValidationError {
	var out []ValidationError
	var walk func(v *jsonschema.ValidationError)
	walk = func(v *jsonschema.ValidationError) {
		if len(v.Causes) == 0 {
			path := v.InstanceLocation
			if path == "" {
				path = "(root)"
			}
			out = append(out, ValidationError{Path: path, Message: v.Message})
			return
		}
		for _, c := range v.Causes {
			walk(c)
		}
	}
	walk(ve)
	if len(out) == 0 {
		path := ve.InstanceLocation
		if path == "" {
			path = "(root)"
		}
		out = append(out, ValidationError{Path: path, Message: ve.Message})
	}
	return out
}
