// Command yutu-go-encode decodes every parity fixture into its Go API type and
// writes the JSON and YAML that youtube would print, so the Rust codec can be
// checked against real Go output.
package main

import (
	"encoding/json"
	"encoding/json/jsontext"
	jsonv2 "encoding/json/v2"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"

	"gopkg.in/yaml.v3"
)

func main() {
	specPath, fixtures, out := os.Args[1], os.Args[2], os.Args[3]
	types := map[string]reflect.Type{}
	for _, v := range apiTypes {
		t := reflect.TypeOf(v)
		types[t.Name()] = t
	}
	var spec struct {
		Resources map[string]resource `json:"resources"`
	}
	raw, _ := os.ReadFile(specPath)
	if err := json.Unmarshal(raw, &spec); err != nil {
		panic(err)
	}
	_ = os.MkdirAll(out, 0o755)
	n := 0
	var walk func(map[string]resource)
	walk = func(rs map[string]resource) {
		for _, r := range rs {
			for _, m := range r.Methods {
				if m.Response.Ref == "" {
					continue
				}
				for _, p := range []string{m.FlatPath, m.MediaUpload.Protocols.Simple.Path} {
					if p == "" {
						continue
					}
					key := fixtureKey(m.HTTPMethod, p)
					n += encodeFixture(filepath.Join(fixtures, key+".json"), filepath.Join(out, key), types[m.Response.Ref])
				}
			}
			walk(r.Resources)
		}
	}
	walk(spec.Resources)
	fmt.Fprintf(os.Stderr, "encode: %d files written\n", n)
}

type resource struct {
	Methods map[string]struct {
		HTTPMethod  string `json:"httpMethod"`
		FlatPath    string `json:"flatPath"`
		Response    struct{ Ref string `json:"$ref"` } `json:"response"`
		MediaUpload struct {
			Protocols struct {
				Simple struct{ Path string `json:"path"` } `json:"simple"`
			} `json:"protocols"`
		} `json:"mediaUpload"`
	} `json:"methods"`
	Resources map[string]resource `json:"resources"`
}

func fixtureKey(method, path string) string {
	path = strings.TrimPrefix(path, "/")
	path = strings.Replace(path, "upload/youtube/v3/", "upload/", 1)
	path = strings.Replace(path, "youtube/v3/", "", 1)
	if strings.HasPrefix(path, "captions/") {
		path = "captions/_"
	}
	return method + "_" + strings.ReplaceAll(path, "/", ".")
}

func encodeFixture(path, out string, t reflect.Type) int {
	raw, err := os.ReadFile(path)
	if err != nil {
		return 0
	}
	var fx map[string]json.RawMessage
	_ = json.Unmarshal(raw, &fx)
	pages := []json.RawMessage{raw}
	if p, ok := fx["pages"]; ok {
		pages = nil
		_ = json.Unmarshal(p, &pages)
	}
	n := 0
	for i, page := range pages {
		v := reflect.New(t)
		if err := json.Unmarshal(page, v.Interface()); err != nil {
			panic(fmt.Sprintf("%s page %d: %v", path, i, err))
		}
		base := fmt.Sprintf("%s.p%d", out, i)
		n += write(base, v.Interface())
		if items := v.Elem().FieldByName("Items"); items.IsValid() && items.Kind() == reflect.Slice {
			n += write(base+".items", items.Interface())
		}
	}
	return n
}

func write(base string, v any) int {
	compact, err := jsonv2.Marshal(v)
	if err != nil {
		panic(err)
	}
	indented, _ := jsonv2.Marshal(v, jsontext.WithIndent("  "))
	y, err := yaml.Marshal(v)
	if err != nil {
		panic(err)
	}
	_ = os.WriteFile(base+".json", compact, 0o644)
	_ = os.WriteFile(base+".indent.json", indented, 0o644)
	_ = os.WriteFile(base+".yaml", y, 0o644)
	return 3
}
