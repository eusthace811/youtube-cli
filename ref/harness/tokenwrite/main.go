// Command yutu-go-tokenwrite writes a token file exactly as yutu's Go
// saveToken does, for the lanes that check Go and Rust share token files.
//
//	yutu-go-tokenwrite <file> <access-token> <refresh-token> <expiry RFC3339>
package main

import (
	"encoding/json/v2"
	"os"
	"time"

	"golang.org/x/oauth2"
)

func main() {
	expiry, err := time.Parse(time.RFC3339, os.Args[4])
	if err != nil {
		panic(err)
	}
	token := &oauth2.Token{AccessToken: os.Args[2], TokenType: "Bearer", RefreshToken: os.Args[3], Expiry: expiry, ExpiresIn: 3599}
	f, err := os.OpenFile(os.Args[1], os.O_RDWR|os.O_CREATE|os.O_TRUNC, 0o600)
	if err != nil {
		panic(err)
	}
	defer f.Close()
	if err := json.MarshalWrite(f, token); err != nil {
		panic(err)
	}
}
