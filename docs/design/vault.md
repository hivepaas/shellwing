# Vault

## What it holds

| Type | Contents | Syncs |
| --- | --- | --- |
| `password` | A host's login password | Yes |
| `ssh_key` | A private key imported or generated in the app (OpenSSH format), its public key and comment | Yes |
| `key_passphrase` | Passphrase for a key outside the vault (such as `~/.ssh/id_ed25519`). Matched by the key's fingerprint, since paths differ per device. | Yes |
| `totp` | One-time-code secret, to fill keyboard-interactive prompts | Yes, off by default. Keeping it next to the password weakens two-factor login, so the user turns it on. |
| `certificate` | Short-lived SSH certificate from the organization's CA ([teams.md](teams.md)) | No, it is requested again |

Not in the vault: credentials for the sync backends and the portal refresh
token. The vault cannot be fetched without them, so they live in the OS keychain
of each device.

## Keys

```
Recovery key (printed) ───────────┐
Passphrase (optional, Argon2id) ───┼── unlocks ──▶ Account Key (AK, 256-bit random)
OS keychain, per device ───────────┘                   │
                                                       ├─ wraps ─▶ user key pair (X25519 + Ed25519)
                                                       └─ wraps ─▶ Personal space key
Collection key ◀── wrapped for each member's X25519 public key
Space or collection key ── encrypts ──▶ each record (host, password, ssh_key, snippet…)
```

**Why a space key sits between the AK and the records:**

- Data keys can be rotated without touching the AK.
- Personal is a collection with one member. Moving a host into a team is
  re-encrypting one record under another key.

**Unlocking on a device you already use.**

- The AK sits in the OS keychain: macOS Keychain, Windows Credential Manager,
  Secret Service on Linux. The app opens without a prompt.
- The key can leave the keychain (`KeyStore::forget`), with a passphrase set:
  every start then asks for the passphrase or the recovery key.
- Then an idle lock (`[vault] lock_after`, per device) drops the open vault
  after that many minutes without a key or a click, and asks for the
  passphrase again; *Lock the vault* does it at once. With the key in the
  keychain there is nothing to lock: any program running as the user could
  read it there.
- **The user's presence** (built: `crates/presence`), with a passphrase set:
  the account key sealed with a secret the system gives only after it,
  `wrapped_by = 'presence'`, with the device's id for it in `meta`.
  - Windows Hello: a `KeyCredentialManager` key (in the TPM) signs one fixed
    challenge; the secret is SHA-256 of the signature (RSA PKCS#1 v1.5
    signatures are deterministic), as KeePass's Windows Hello plugins do.
  - Touch ID: a random secret in the data protection keychain, readable only
    with `kSecAccessControlBiometryCurrentSet`
    (`WhenPasscodeSetThisDeviceOnly`). Needs a Developer ID-signed app
    (`errSecMissingEntitlement` otherwise).
  - A gate only (a prompt before reading the keychain) would add nothing:
    any program running as the user could read the key without it.
- Linux without Secret Service falls back to the passphrase (built: the
  account key wrapped by Argon2id(passphrase), m = 64 MiB, t = 3, p = 1, with
  its salt and parameters in `meta`; `wrapped_by = 'passphrase'`).
- The account key is also wrapped by the recovery key (`wrapped_by =
  'recovery'`), and every vault has one, synced or not: it opens a vault whose
  keychain lost the key, or whose passphrase is forgotten.

**A new device gets the AK in one of three ways:**

1. **Pairing** (built for the folder backend; `crates/vault/src/pairing.rs`).
   The new device (asker) and one that syncs (answerer) exchange files in the
   store's `pairing/`, each written by one side:
   1. Asker: its name and a commitment, SHA-256 of a fresh X25519 public key.
   2. Answerer: its name and a fresh X25519 public key.
   3. Asker: its public key (checked against the commitment), and the answer
      it took (another device may have answered too).
   4. Both show a six-digit code, SHA-256 of the two public keys. The user
      approves on the answerer if the codes match.
   5. Answerer: the space key and the recovery key, sealed with a key from
      HKDF-SHA256 over the X25519 result (salt: both public keys; info: the
      request id), or a refusal.

   Whoever can write the store could put their own keys in between, but the
   codes would then differ: the commitment means neither side's key is chosen
   knowing the other's, so a forged pair matches by chance once in a million.
   Requests expire after 10 minutes; low-order keys are refused.
2. **The recovery key:** 32 random bytes shown as 13 groups of 4 Crockford
   base32 characters (no I, L, O, U; typing them loosely still works). The
   store's `format.json` holds the space key sealed with it; the device keeps it
   wrapped by the account key, so it can be shown again.
3. **A sync passphrase**, if the user enabled it (built for the folder
   backend). `format.json` gains `passphrase: {kdf, sealed}`: the space key
   and the recovery key, sealed with Argon2id(passphrase) under the same
   parameters as the device passphrase. It is its own passphrase, not the
   device's: one typed on a device that never syncs need not be strong.
   The store holds it, so a leaked store allows offline guessing. Two
   defences:
   - Argon2id, m = 64 MiB, t = 3: about half a second per guess.
   - A strength check: `zxcvbn` score 3 of 4 or more, and 10 characters.
   Turning it off removes the field.

**Rotation.**

- **Collection key.** Rotated whenever a member is removed.
- **Personal space key.** Rotated on request, for example after a lost device.
- **Rotating the AK** re-wraps everything under it. Records stay as they are.

## Record envelope

The same envelope is used on disk and on every backend:

```
id     01JAQ9…                  clear
kind   host | other             clear; the cloud only needs to count hosts
space  personal | col_01J…      clear
key    3                        version of the space key used
data   cipher id ‖ nonce ‖ AEAD(space key, nonce, payload,
                                aad = id ‖ kind ‖ space ‖ key)
sig    Ed25519 by the writer    collections only, to prove authorship (not yet)
```

**Ciphers.** The first byte of every sealed value (records and wrapped keys)
names the algorithm, so values sealed differently can sit side by side and a
new algorithm needs no guessing about old data. Ids are never reused.

| Id | Algorithm | Nonce | Use |
| --- | --- | --- | --- |
| 1 | XChaCha20-Poly1305 | 24 bytes, random | Default. Random nonces are safe however many devices write, and it is fast and constant-time without AES hardware. |
| 2 | AES-256-GCM | 12 bytes, random | Where FIPS-approved algorithms are required (Business). Random 96-bit nonces are fine for the few thousand values one key seals. |

New values are sealed with the vault's chosen cipher (a setting later); old
ones open with whatever they carry. Values from the first builds had no id
byte (`nonce ‖ ciphertext`, XChaCha20-Poly1305) and are rewritten on open.

**Later, for collections:** neither Poly1305 nor GCM commits to its key, so a
crafted ciphertext could decrypt under two keys. Shared collections should add
a key commitment (a hash of the key) to the envelope.

- **The AAD binds the ciphertext to its record.** A server cannot swap host A's
  data into host B.
- **`kind` is only host or other.** The server cannot tell passwords from keys
  from snippets.
- **Signatures matter only in collections.** There, every member holds the key
  and could otherwise write as someone else.

The payload is JSON. Each field carries its HLC for merging ([sync.md](sync.md)):

```json
{"v": 1, "type": "ssh_key", "name": "deploy key",
 "f": {"private_key": {"val": "-----BEGIN OPENSSH PRIVATE KEY-----…", "t": "1792312831000-0003-dev7"},
       "comment":     {"val": "deploy@ci", "t": "1792312831000-0003-dev7"}}}
```

Hosts from `hosts.toml` are turned into the same records when they sync.

## vault.db

SQLite, so a sync is one transaction and a crash mid-write loses nothing.

```sql
CREATE TABLE records (
  id       TEXT PRIMARY KEY,   -- ULID
  kind     TEXT NOT NULL,      -- 'host' | 'other'
  space    TEXT NOT NULL,
  key_ver  INTEGER NOT NULL,
  data     BLOB NOT NULL,      -- cipher id, nonce, ciphertext and tag
  sig      BLOB,               -- not yet: collections
  hlc      TEXT NOT NULL,      -- newest field clock, for quick change scans
  deleted  INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE keys (
  id          TEXT NOT NULL,   -- 'account', a space id or a collection id
  ver         INTEGER NOT NULL,
  wrapped     BLOB NOT NULL,
  wrapped_by  TEXT NOT NULL,   -- 'keychain' | 'recovery' | 'passphrase' | 'account' | a user key id
  PRIMARY KEY (id, ver, wrapped_by)
);
CREATE TABLE sync (            -- not yet: comes with sync
  backend     TEXT NOT NULL,
  record_id   TEXT NOT NULL,
  etag        TEXT,            -- remote version last seen
  pushed_hlc  TEXT,            -- local clock last written there
  PRIMARY KEY (backend, record_id)
);
```

`hosts.toml` stays plain text on disk, as `~/.ssh/config` is. Synced copies of
hosts are always encrypted.

## Implementation

- **Crates.** Audited, standard primitives; nothing home-made:
  - Encryption: `chacha20poly1305`, `argon2`, `hkdf`.
  - Key pairs: `x25519-dalek`, `ed25519-dalek`.
  - Secrets in memory: `zeroize`, `secrecy`, so secrets leave RAM after use.
  - Storage: `keyring` for the OS keychain, `rusqlite` with the bundled SQLite.
- **`$SHELLWING_PASSWORD`.** For development only: debug builds read it, release
  builds do not.
- **Export.** Encrypted by default. Plain-text export needs confirmation and
  shows a warning.
- **Later (Pro).** shellwing as an SSH agent, so `git`, `ssh` and other tools use
  vault keys, as 1Password and Bitwarden do.

## Threats

| If this happens | Then |
| --- | --- |
| Portal or sync server is breached | Only ciphertext and counts are exposed. |
| A backend is leaked (Git repo made public, S3 bucket opened) | Ciphertext is exposed. With a passphrase enabled, offline guessing is possible, which is why Argon2id and the strength check matter. |
| A locked laptop is stolen | The vault stays shut. `hosts.toml` (names and addresses) is readable, as `~/.ssh/config` would be. |
| An unlocked laptop is stolen | Everything that user can open is exposed. Mitigations are the idle lock and revoking the device from another one. |
| A removed team member | Keeps what they already copied. The answer is the removal checklist, and SSH certificates in Business. |
