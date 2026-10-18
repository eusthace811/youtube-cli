// Package bench measures what `yutu video list -o json` does with a response:
// decode a VideoListResponse and re-encode its items with encoding/json/v2.
package bench

import (
	"encoding/json"
	jsonv2 "encoding/json/v2"
	"os"
	"testing"

	"google.golang.org/api/youtube/v3"
)

func BenchmarkDecodeEncode500(b *testing.B) {
	raw, err := os.ReadFile(os.Getenv("BIG_FIXTURE"))
	if err != nil {
		b.Fatal(err)
	}
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		var res youtube.VideoListResponse
		if err := json.Unmarshal(raw, &res); err != nil {
			b.Fatal(err)
		}
		if _, err := jsonv2.Marshal(res.Items); err != nil {
			b.Fatal(err)
		}
	}
}
