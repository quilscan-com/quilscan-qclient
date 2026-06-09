package main

import (
	"crypto/ed25519"
	"encoding/base64"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

func main() {
	privateKeyRaw := strings.TrimSpace(os.Getenv("DEV_NODE_SIGNING_PRIVATE_KEY"))
	if privateKeyRaw == "" {
		privateKeyRaw = strings.TrimSpace(os.Getenv("AGENT_SIGNING_PRIVATE_KEY"))
	}
	if privateKeyRaw == "" {
		fatalf("DEV_NODE_SIGNING_PRIVATE_KEY or AGENT_SIGNING_PRIVATE_KEY is required")
	}
	privateKey, err := base64.StdEncoding.DecodeString(privateKeyRaw)
	if err != nil {
		fatalf("parse private key: %v", err)
	}
	if len(privateKey) != ed25519.PrivateKeySize {
		fatalf("private key has %d bytes, want %d", len(privateKey), ed25519.PrivateKeySize)
	}
	publicKey, ok := ed25519.PrivateKey(privateKey).Public().(ed25519.PublicKey)
	if !ok || len(publicKey) != ed25519.PublicKeySize {
		fatalf("derive public key failed")
	}

	if len(os.Args) < 2 {
		fatalf("usage: go run ./scripts/sign-qclient-artifacts.go <artifact> [artifact...]")
	}
	for _, path := range os.Args[1:] {
		binary, err := os.ReadFile(path)
		if err != nil {
			fatalf("read %s: %v", path, err)
		}
		signature := ed25519.Sign(ed25519.PrivateKey(privateKey), binary)
		if !ed25519.Verify(publicKey, binary, signature) {
			fatalf("signature verification failed for %s", path)
		}
		sigPath := path + ".sig"
		if err := os.WriteFile(sigPath, []byte(base64.StdEncoding.EncodeToString(signature)+"\n"), 0o644); err != nil {
			fatalf("write %s: %v", sigPath, err)
		}
		fmt.Printf("signed %s -> %s\n", filepath.Base(path), filepath.Base(sigPath))
	}
}

func fatalf(format string, args ...interface{}) {
	fmt.Fprintf(os.Stderr, format+"\n", args...)
	os.Exit(1)
}
