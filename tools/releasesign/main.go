// Command releasesign makes shellwing's release signing keys, and signs the list
// of releases (release.json) with them into release.signed.json, the file the
// app reads to tell its user that a newer version is out.
//
// The list is signed twice, with Ed25519 (Ed25519ctx) and with ML-DSA-65, both
// under the context "shellwing-release-v1"; the app (crates/update) requires
// both, by keys built into it. A forger has to break both: Ed25519 falls to a
// large enough quantum computer, ML-DSA is young.
//
// What is published is release.signed.json: release.json itself, base64, with
// its hash and its signatures in one file, so that a CDN caching files one by
// one can never serve a new list with an old signature.
//
// This is one file that imports the standard library only, on purpose, and must
// stay so: it holds the private keys, and scripts/release_sign.sh builds it from
// a reviewed commit with the network off, so an import from elsewhere fails the
// build rather than being fetched, and a change to it shows in review.
//
// Keys are PKCS#8 (private) and PKIX (public) PEM, which openssl 3.5 or later
// reads too; a key's id is its file name, <dir>/<id>.key and <dir>/<id>.pub.pem.
//
//	releasesign keygen -alg ed25519   -key-id 2026-ed -dir /offline
//	releasesign keygen -alg ml-dsa-65 -key-id 2026-ml -dir /offline
//	releasesign sign   -key /offline/2026-ed.key -key /offline/2026-ml.key \
//	                   [-in release.json] [-out release.signed.json]
//	releasesign verify -pub 2026-ed.pub.pem -pub 2026-ml.pub.pem \
//	                   [-in release.signed.json] [-expect release.json]
package main

import (
	"bytes"
	"crypto"
	"crypto/ed25519"
	"crypto/mldsa"
	"crypto/rand"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"encoding/pem"
	"errors"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
)

// context is what the list is signed for; crates/update verifies under it.
const context = "shellwing-release-v1"

const (
	algEd25519 = "ed25519"
	algMLDSA65 = "ml-dsa-65"
)

// required must match what crates/update requires.
var required = []string{algEd25519, algMLDSA65}

// keyIDPattern keeps a key id fit for a file name and for JSON.
var keyIDPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{0,31}$`)

// envelope is release.signed.json.
type envelope struct {
	Payload    string      `json:"payload"`
	SHA256     string      `json:"sha256"`
	Signatures []signature `json:"signatures"`
}

type signature struct {
	KeyID string `json:"keyId"`
	Alg   string `json:"alg"`
	Sig   string `json:"sig"`
}

type key struct {
	id, alg string
	signer  crypto.Signer
	public  crypto.PublicKey
}

func main() {
	if len(os.Args) < 2 {
		usage()
	}
	var err error
	switch os.Args[1] {
	case "keygen":
		err = keygen(os.Args[2:])
	case "sign":
		err = sign(os.Args[2:])
	case "verify":
		err = verify(os.Args[2:])
	default:
		usage()
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, "releasesign:", err)
		os.Exit(1)
	}
}

func usage() {
	fmt.Fprintln(os.Stderr, "usage: releasesign keygen|sign|verify [flags]  (-h on one for its flags)")
	os.Exit(2)
}

type files []string

func (f *files) String() string     { return strings.Join(*f, ",") }
func (f *files) Set(v string) error { *f = append(*f, v); return nil }

func keygen(args []string) error {
	fs := flag.NewFlagSet("keygen", flag.ExitOnError)
	alg := fs.String("alg", "", "ed25519 or ml-dsa-65")
	id := fs.String("key-id", "", "the new key's id, also its file name: 2026-ed")
	dir := fs.String("dir", ".", "where to write <key-id>.key and <key-id>.pub.pem")
	_ = fs.Parse(args)
	if !keyIDPattern.MatchString(*id) {
		return fmt.Errorf("-key-id %q must match %s", *id, keyIDPattern)
	}
	var signer crypto.Signer
	switch *alg {
	case algEd25519:
		_, k, err := ed25519.GenerateKey(rand.Reader)
		if err != nil {
			return err
		}
		signer = k
	case algMLDSA65:
		k, err := mldsa.GenerateKey(mldsa.MLDSA65())
		if err != nil {
			return err
		}
		signer = k
	default:
		return fmt.Errorf("-alg must be %s or %s", algEd25519, algMLDSA65)
	}
	private, err := x509.MarshalPKCS8PrivateKey(signer)
	if err != nil {
		return err
	}
	public, err := x509.MarshalPKIXPublicKey(signer.Public())
	if err != nil {
		return err
	}
	priv := filepath.Join(*dir, *id+".key")
	pub := filepath.Join(*dir, *id+".pub.pem")
	// A key is never written over: a lost private key the apps trust is
	// replaced only by shipping apps that trust another.
	if err = create(priv, pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: private}), 0o600); err != nil {
		return err
	}
	if err = create(pub, pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: public}), 0o644); err != nil {
		return err
	}
	fmt.Printf("wrote %s (keep it offline) and %s (copy it into crates/update/keys/)\n", priv, pub)
	return nil
}

func create(path string, data []byte, mode os.FileMode) error {
	f, err := os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, mode)
	if err != nil {
		return err
	}
	if _, err = f.Write(data); err != nil {
		_ = f.Close()
		return err
	}
	return f.Close()
}

// idOf is a key file's id: its name without .key or .pub.pem.
func idOf(path string, suffix string) (string, error) {
	name := filepath.Base(path)
	id, found := strings.CutSuffix(name, suffix)
	if !found || !keyIDPattern.MatchString(id) {
		return "", fmt.Errorf("%s: a key file is named <key-id>%s", path, suffix)
	}
	return id, nil
}

func readPEM(path, kind string) ([]byte, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	block, rest := pem.Decode(data)
	if block == nil || block.Type != kind || len(bytes.TrimSpace(rest)) != 0 {
		return nil, fmt.Errorf("%s: not one %s PEM block", path, kind)
	}
	return block.Bytes, nil
}

func algOf(k any) (string, error) {
	switch k := k.(type) {
	case ed25519.PrivateKey, ed25519.PublicKey:
		return algEd25519, nil
	case *mldsa.PrivateKey:
		if k.PublicKey().Parameters() == mldsa.MLDSA65() {
			return algMLDSA65, nil
		}
	case *mldsa.PublicKey:
		if k.Parameters() == mldsa.MLDSA65() {
			return algMLDSA65, nil
		}
	}
	return "", fmt.Errorf("neither an ed25519 nor an ML-DSA-65 key (%T)", k)
}

func privateKey(path string) (*key, error) {
	id, err := idOf(path, ".key")
	if err != nil {
		return nil, err
	}
	der, err := readPEM(path, "PRIVATE KEY")
	if err != nil {
		return nil, err
	}
	parsed, err := x509.ParsePKCS8PrivateKey(der)
	if err != nil {
		return nil, fmt.Errorf("%s: %w", path, err)
	}
	signer, ok := parsed.(crypto.Signer)
	if !ok {
		return nil, fmt.Errorf("%s: not a signing key", path)
	}
	alg, err := algOf(parsed)
	if err != nil {
		return nil, fmt.Errorf("%s: %w", path, err)
	}
	return &key{id: id, alg: alg, signer: signer, public: signer.Public()}, nil
}

func publicKey(path string) (*key, error) {
	id, err := idOf(path, ".pub.pem")
	if err != nil {
		return nil, err
	}
	der, err := readPEM(path, "PUBLIC KEY")
	if err != nil {
		return nil, err
	}
	parsed, err := x509.ParsePKIXPublicKey(der)
	if err != nil {
		return nil, fmt.Errorf("%s: %w", path, err)
	}
	alg, err := algOf(parsed)
	if err != nil {
		return nil, fmt.Errorf("%s: %w", path, err)
	}
	return &key{id: id, alg: alg, public: parsed}, nil
}

// signWith signs data with k under the context.
func signWith(k *key, data []byte) ([]byte, error) {
	switch k.alg {
	case algEd25519:
		return k.signer.Sign(nil, data, &ed25519.Options{Context: context})
	default:
		return k.signer.Sign(rand.Reader, data, &mldsa.Options{Context: context})
	}
}

// verifyWith reports whether sig is k's signature over data under the context.
func verifyWith(k *key, data, sig []byte) bool {
	switch k.alg {
	case algEd25519:
		return ed25519.VerifyWithOptions(k.public.(ed25519.PublicKey), data, sig,
			&ed25519.Options{Context: context}) == nil
	default:
		return mldsa.Verify(k.public.(*mldsa.PublicKey), data, sig, &mldsa.Options{Context: context}) == nil
	}
}

// covers is an error unless keys hold one of each required algorithm, and no
// two keys share an id.
func covers(keys []*key) error {
	ids := map[string]bool{}
	algs := map[string]bool{}
	for _, k := range keys {
		if ids[k.id] {
			return fmt.Errorf("two keys are %s", k.id)
		}
		ids[k.id] = true
		algs[k.alg] = true
	}
	for _, alg := range required {
		if !algs[alg] {
			return fmt.Errorf("no %s key: the app requires one of each of %s", alg, strings.Join(required, ", "))
		}
	}
	return nil
}

func sign(args []string) error {
	fs := flag.NewFlagSet("sign", flag.ExitOnError)
	var keyFiles files
	fs.Var(&keyFiles, "key", "a private key file, <key-id>.key; once for each algorithm")
	in := fs.String("in", "release.json", "the list to sign")
	out := fs.String("out", "release.signed.json", "the envelope to write")
	_ = fs.Parse(args)
	var keys []*key
	for _, path := range keyFiles {
		k, err := privateKey(path)
		if err != nil {
			return err
		}
		keys = append(keys, k)
	}
	if err := covers(keys); err != nil {
		return err
	}
	data, err := os.ReadFile(*in)
	if err != nil {
		return err
	}
	if !json.Valid(data) {
		return fmt.Errorf("%s is not JSON", *in)
	}
	sort.Slice(keys, func(i, j int) bool { return keys[i].id < keys[j].id })
	env := envelope{Payload: base64.StdEncoding.EncodeToString(data), SHA256: sha256Hex(data)}
	for _, k := range keys {
		sig, err := signWith(k, data)
		if err != nil {
			return fmt.Errorf("signing with %s: %w", k.id, err)
		}
		// Each signature checked before it is kept.
		if !verifyWith(k, data, sig) {
			return fmt.Errorf("the signature by %s does not verify", k.id)
		}
		env.Signatures = append(env.Signatures, signature{k.id, k.alg, base64.StdEncoding.EncodeToString(sig)})
	}
	if err = os.WriteFile(*out, render(env), 0o644); err != nil {
		return err
	}
	fmt.Printf("signed %s (sha256 %s) into %s with %s\n", *in, env.SHA256, *out, keyFiles.String())
	return nil
}

// render writes the envelope with the payload, and each signature, on a line
// of its own: scripts/release_sign.sh reads them back with sed to check them
// with openssl.
func render(env envelope) []byte {
	var b strings.Builder
	b.WriteString("{\n")
	fmt.Fprintf(&b, "  \"payload\": %q,\n", env.Payload)
	fmt.Fprintf(&b, "  \"sha256\": %q,\n", env.SHA256)
	b.WriteString("  \"signatures\": [\n")
	for i, s := range env.Signatures {
		line, _ := json.Marshal(s)
		b.WriteString("    ")
		b.Write(line)
		if i < len(env.Signatures)-1 {
			b.WriteString(",")
		}
		b.WriteString("\n")
	}
	b.WriteString("  ]\n}\n")
	return []byte(b.String())
}

func sha256Hex(data []byte) string {
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

// open is what the app does: the payload, if every required algorithm has a
// valid signature by one of keys; a known key's signature that does not
// verify refuses the whole envelope.
func open(keys []*key, data []byte) ([]byte, error) {
	var env envelope
	if err := json.Unmarshal(data, &env); err != nil {
		return nil, errors.New("not an envelope")
	}
	payload, err := base64.StdEncoding.DecodeString(env.Payload)
	if err != nil || len(payload) == 0 {
		return nil, errors.New("its payload does not read")
	}
	if sha256Hex(payload) != env.SHA256 {
		return nil, errors.New("its sha256 is not its payload's")
	}
	byID := map[string]*key{}
	for _, k := range keys {
		byID[k.id] = k
	}
	done := map[string]bool{}
	for _, s := range env.Signatures {
		k, found := byID[s.KeyID]
		if !found {
			continue
		}
		sig, err := base64.StdEncoding.DecodeString(s.Sig)
		if err != nil || s.Alg != k.alg || !verifyWith(k, payload, sig) {
			return nil, fmt.Errorf("the signature by %s does not verify", s.KeyID)
		}
		done[k.alg] = true
	}
	for _, alg := range required {
		if !done[alg] {
			return nil, fmt.Errorf("no valid %s signature by these keys", alg)
		}
	}
	return payload, nil
}

func verify(args []string) error {
	fs := flag.NewFlagSet("verify", flag.ExitOnError)
	var pubFiles files
	fs.Var(&pubFiles, "pub", "a public key file, <key-id>.pub.pem; once for each")
	in := fs.String("in", "release.signed.json", "the envelope to check")
	expect := fs.String("expect", "", "the list it must carry, byte for byte (optional)")
	_ = fs.Parse(args)
	var keys []*key
	for _, path := range pubFiles {
		k, err := publicKey(path)
		if err != nil {
			return err
		}
		keys = append(keys, k)
	}
	if err := covers(keys); err != nil {
		return err
	}
	data, err := os.ReadFile(*in)
	if err != nil {
		return err
	}
	payload, err := open(keys, data)
	if err != nil {
		return fmt.Errorf("%s: %w", *in, err)
	}
	if *expect != "" {
		want, err := os.ReadFile(*expect)
		if err != nil {
			return err
		}
		if !bytes.Equal(payload, want) {
			return fmt.Errorf("%s does not carry %s byte for byte", *in, *expect)
		}
	}
	fmt.Printf("%s verifies (sha256 %s)\n", *in, sha256Hex(payload))
	return nil
}
