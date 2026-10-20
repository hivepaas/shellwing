//go:build ignore

// Makes the test data of crates/update: a key pair of each algorithm, and a
// release list signed with them as tools/releasesign of HivePaaS signs one
// (Ed25519ctx and ML-DSA-65, both under the context shellwing-release-v1).
// Test keys only: no release is ever signed with them.
//
//	cd crates/update/testdata && go run gen.go
package main

import (
	"crypto/ed25519"
	"crypto/mldsa"
	"crypto/rand"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"os"
)

const context = "shellwing-release-v1"

type entry struct {
	KeyID string `json:"keyId"`
	Alg   string `json:"alg"`
	Sig   string `json:"sig"`
}

func must(err error) {
	if err != nil {
		panic(err)
	}
}

func writePub(name string, pub any) {
	der, err := x509.MarshalPKIXPublicKey(pub)
	must(err)
	must(os.WriteFile(name, pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: der}), 0o644))
}

func main() {
	payload, err := os.ReadFile("release.json")
	must(err)
	edPub, edKey, err := ed25519.GenerateKey(rand.Reader)
	must(err)
	mlKey, err := mldsa.GenerateKey(mldsa.MLDSA65())
	must(err)
	writePub("test_ed.pub.pem", edPub)
	writePub("test_ml.pub.pem", mlKey.PublicKey())
	edSig, err := edKey.Sign(nil, payload, &ed25519.Options{Context: context})
	must(err)
	mlSig, err := mlKey.Sign(rand.Reader, payload, &mldsa.Options{Context: context})
	must(err)
	sum := sha256.Sum256(payload)
	env := map[string]any{
		"payload": base64.StdEncoding.EncodeToString(payload),
		"sha256":  hex.EncodeToString(sum[:]),
		"signatures": []entry{
			{"test_ed", "ed25519", base64.StdEncoding.EncodeToString(edSig)},
			{"test_ml", "ml-dsa-65", base64.StdEncoding.EncodeToString(mlSig)},
		},
	}
	out, err := json.MarshalIndent(env, "", "  ")
	must(err)
	must(os.WriteFile("release.signed.json", append(out, '\n'), 0o644))
	// The same payload signed under another purpose's context: refused.
	edOther, err := edKey.Sign(nil, payload, &ed25519.Options{Context: "hivepaas-release-v1"})
	must(err)
	mlOther, err := mlKey.Sign(rand.Reader, payload, &mldsa.Options{Context: "hivepaas-release-v1"})
	must(err)
	env["signatures"] = []entry{
		{"test_ed", "ed25519", base64.StdEncoding.EncodeToString(edOther)},
		{"test_ml", "ml-dsa-65", base64.StdEncoding.EncodeToString(mlOther)},
	}
	out, err = json.MarshalIndent(env, "", "  ")
	must(err)
	must(os.WriteFile("other-context.signed.json", append(out, '\n'), 0o644))
}
