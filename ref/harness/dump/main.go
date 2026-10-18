// Command yutu-go-dump writes yutu's cobra command tree and the Go layout of
// every YouTube API type as JSON, so the Rust port derives from exact data.
package main

import (
	"encoding/json"
	"os"
	"reflect"
	"sort"

	"github.com/eat-pray-ai/yutu/cmd"
	_ "github.com/eat-pray-ai/yutu/cmd/agent"
	_ "github.com/eat-pray-ai/yutu/cmd/resources"
	"github.com/spf13/cobra"
	"github.com/spf13/pflag"
)

type flag struct {
	Name        string              `json:"name"`
	Shorthand   string              `json:"shorthand"`
	Type        string              `json:"type"`
	Default     string              `json:"default"`
	NoOptDefVal string              `json:"noOptDefVal"`
	Usage       string              `json:"usage"`
	Hidden      bool                `json:"hidden"`
	Persistent  bool                `json:"persistent"`
	Annotations map[string][]string `json:"annotations,omitempty"`
}

type command struct {
	Path     string     `json:"path"`
	Use      string     `json:"use"`
	Short    string     `json:"short"`
	Long     string     `json:"long"`
	Example  string     `json:"example"`
	Aliases  []string   `json:"aliases"`
	Hidden   bool       `json:"hidden"`
	Runnable bool       `json:"runnable"`
	Flags    []flag     `json:"flags"`
	Children []*command `json:"children"`
}

func flags(fs *pflag.FlagSet, persistent bool) []flag {
	var out []flag
	fs.VisitAll(func(f *pflag.Flag) {
		out = append(out, flag{f.Name, f.Shorthand, f.Value.Type(), f.DefValue, f.NoOptDefVal, f.Usage, f.Hidden, persistent, f.Annotations})
	})
	return out
}

func walk(c *cobra.Command) *command {
	out := &command{Path: c.CommandPath(), Use: c.Use, Short: c.Short, Long: c.Long, Example: c.Example, Aliases: c.Aliases, Hidden: c.Hidden, Runnable: c.Runnable()}
	out.Flags = append(flags(c.LocalNonPersistentFlags(), false), flags(c.PersistentFlags(), true)...)
	for _, s := range c.Commands() {
		out.Children = append(out.Children, walk(s))
	}
	return out
}

type field struct {
	Name string `json:"name"`
	Tag  string `json:"tag"`
	Type string `json:"type"`
}

func typeName(t reflect.Type, seen map[string][]field) string {
	switch t.Kind() {
	case reflect.Pointer:
		return "*" + typeName(t.Elem(), seen)
	case reflect.Slice:
		return "[]" + typeName(t.Elem(), seen)
	case reflect.Map:
		return "map[" + typeName(t.Key(), seen) + "]" + typeName(t.Elem(), seen)
	case reflect.Struct:
		name := t.PkgPath() + "." + t.Name()
		if _, ok := seen[name]; !ok {
			seen[name] = nil
			var fs []field
			for i := 0; i < t.NumField(); i++ {
				f := t.Field(i)
				if !f.IsExported() {
					continue
				}
				fs = append(fs, field{f.Name, string(f.Tag), typeName(f.Type, seen)})
			}
			seen[name] = fs
		}
		return name
	case reflect.Interface:
		return "interface"
	default:
		return t.Kind().String()
	}
}

func main() {
	cmd.RootCmd.InitDefaultHelpCmd()
	cmd.RootCmd.InitDefaultCompletionCmd()
	seen := map[string][]field{}
	for _, v := range apiTypes {
		typeName(reflect.TypeOf(v), seen)
	}
	names := make([]string, 0, len(seen))
	for n := range seen {
		names = append(names, n)
	}
	sort.Strings(names)
	types := make([]map[string]any, 0, len(names))
	for _, n := range names {
		types = append(types, map[string]any{"name": n, "fields": seen[n]})
	}
	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", " ")
	enc.SetEscapeHTML(false)
	_ = enc.Encode(map[string]any{"commands": walk(cmd.RootCmd), "types": types})
}
