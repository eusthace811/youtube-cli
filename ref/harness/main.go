// Command yutu-go-harness runs yutu's real command tree with every YouTube API
// call sent to the parity recorder named by YUTU_PARITY_ENDPOINT.
package main

import (
	"context"
	"net/http"
	"os"

	"github.com/eat-pray-ai/yutu/cmd"
	_ "github.com/eat-pray-ai/yutu/cmd/agent"
	_ "github.com/eat-pray-ai/yutu/cmd/resources"
	"github.com/eat-pray-ai/yutu/pkg/auth"
	"github.com/eat-pray-ai/yutu/pkg/common"
	"google.golang.org/api/option"
	"google.golang.org/api/youtube/v3"
)

func main() {
	endpoint := os.Getenv("YUTU_PARITY_ENDPOINT")
	if endpoint != "" {
		common.SetDefaultServiceProvider(func(ctx context.Context) (*youtube.Service, error) {
			return youtube.NewService(ctx, option.WithEndpoint(endpoint+"/"), option.WithHTTPClient(http.DefaultClient))
		})
	}
	if ti := os.Getenv("YUTU_PARITY_TOKENINFO"); ti != "" {
		auth.GoogleTokenVerifier = auth.NewGoogleTokenVerifier(ti)
	}
	cmd.Execute()
}
