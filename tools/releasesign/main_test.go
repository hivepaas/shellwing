package main

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func run(t *testing.T, f func([]string) error, args ...string) error {
	t.Helper()
	return f(args)
}

// keys makes a key of each algorithm in a folder of its own.
func keys(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	for _, k := range [][2]string{{"ed25519", "t-ed"}, {"ml-dsa-65", "t-ml"}} {
		if err := run(t, keygen, "-alg", k[0], "-key-id", k[1], "-dir", dir); err != nil {
			t.Fatal(err)
		}
	}
	return dir
}

func TestASignedListVerifiesAndCarriesTheListWhole(t *testing.T) {
	dir := keys(t)
	list := filepath.Join(dir, "release.json")
	if err := os.WriteFile(list, []byte(`{"channels": {}, "releases": {}}`+"\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	out := filepath.Join(dir, "release.signed.json")
	if err := run(t, sign, "-key", filepath.Join(dir, "t-ed.key"), "-key", filepath.Join(dir, "t-ml.key"),
		"-in", list, "-out", out); err != nil {
		t.Fatal(err)
	}
	pubs := []string{"-pub", filepath.Join(dir, "t-ed.pub.pem"), "-pub", filepath.Join(dir, "t-ml.pub.pem")}
	if err := run(t, verify, append(pubs, "-in", out, "-expect", list)...); err != nil {
		t.Fatal(err)
	}
	// Each signature on a line of its own, for the script's openssl check.
	data, _ := os.ReadFile(out)
	if !strings.Contains(string(data), "\n    {\"keyId\":\"t-ed\",\"alg\":\"ed25519\",\"sig\":\"") {
		t.Fatalf("not one signature a line:\n%s", data)
	}
	// Another list than the one expected.
	other := filepath.Join(dir, "other.json")
	_ = os.WriteFile(other, []byte(`{}`), 0o644)
	if err := run(t, verify, append(pubs, "-in", out, "-expect", other)...); err == nil {
		t.Fatal("another list was taken for it")
	}
	// A payload changed after signing.
	changed := strings.Replace(string(data), "sha256\": \"", "sha256\": \"0", 1)
	_ = os.WriteFile(out, []byte(changed), 0o644)
	if err := run(t, verify, append(pubs, "-in", out)...); err == nil {
		t.Fatal("a changed envelope verified")
	}
}

func TestOneAlgorithmAloneIsRefused(t *testing.T) {
	dir := keys(t)
	list := filepath.Join(dir, "release.json")
	_ = os.WriteFile(list, []byte(`{}`), 0o644)
	err := run(t, sign, "-key", filepath.Join(dir, "t-ed.key"), "-in", list, "-out", filepath.Join(dir, "o.json"))
	if err == nil || !strings.Contains(err.Error(), "ml-dsa-65") {
		t.Fatalf("signed with one key: %v", err)
	}
}

func TestAKeyIsNeverWrittenOver(t *testing.T) {
	dir := keys(t)
	if err := run(t, keygen, "-alg", "ed25519", "-key-id", "t-ed", "-dir", dir); err == nil {
		t.Fatal("a key was written over")
	}
}
