# Accounts and sync

## Accounts (shellwing portal)

The portal is a web app where people sign in (Google, GitHub, …) and buy plans.
The desktop app works fully without it.

**Sign-in.** It follows RFC 8252:

1. The app opens the browser at the portal.
2. The portal does the Google/GitHub OAuth. The app never sees those tokens.
3. The portal redirects to `http://127.0.0.1:<port>/callback` with a code.
4. The app exchanges the code, with PKCE, for shellwing tokens.

The device-code flow is the fallback when no browser can open. The refresh
token is kept in the OS keychain.

**License.** The portal issues a token signed with Ed25519. It carries the
account, plan, `host_limit` and expiry.

- The app checks the signature offline and refreshes the token daily.
- After about 14 days without reaching the portal, cloud sync pauses. Nothing
  else stops working.
- The sync server enforces the limit on every write, so editing the app cannot
  raise it.

**Signing in never unlocks data.** Encryption keys come from the user's devices
or recovery key ([vault.md](vault.md)). A compromised portal exposes no hosts or
secrets.

## Host limits

Individual plans limit how many hosts sync through **shellwing cloud**. Draft
numbers:

| Plan | Hosts synced |
| --- | --- |
| Free | None |
| Pro 1 | 10 |
| Pro 2 | 20 |
| Pro 3 | Unlimited |

**Open:** plan names and prices.

What counts toward the limit:

- **Only hosts count.** Groups, snippets, themes and settings sync free. So do
  secrets used by synced hosts. A fair-use storage cap applies, tens of MB.
- **A jump host counts.** Syncing `api-1` through `bastion` uses two slots, and
  the app says so before enabling it.
- **Hosts that never count:**
  - `local_only` hosts.
  - Hosts from `~/.ssh/config`.
  - Hosts synced through any other backend (Git, S3, folder…), which is
    unlimited and free.
- **Team plans** are per seat with unlimited hosts ([teams.md](teams.md)).
- **When over the limit** (for example after a downgrade), nothing is deleted:
  1. The user picks which hosts keep syncing.
  2. The rest become local-only on the devices that have them.
  3. Their server copies stay read-only for a grace period, then are removed.
- **Counting without reading.** The server counts by the record's clear `kind`
  field. It learns how many hosts there are, not which.

The sidebar shows a cloud mark on synced hosts and a "Synced 8/10" counter.

## Engine

One engine does encryption, merging and conflict handling. A backend only
stores files:

```rust
trait Backend {
    async fn list(&self, prefix: &str) -> Result<Vec<Entry>>;   // name, etag/version
    async fn get(&self, name: &str) -> Result<Option<(Bytes, Etag)>>;
    async fn put(&self, name: &str, data: Bytes, cond: Cond) -> Result<PutResult>; // Cond: Any | IfMatch(etag) | IfAbsent
    async fn delete(&self, name: &str, cond: Cond) -> Result<()>;
    fn capabilities(&self) -> Caps;                                 // conditional writes? push notifications?
}
```

Layout on a backend:

```
shellwing/
  format.json            format version, key ids (never keys)
  records/<ulid>.sw      one encrypted record per host, group, snippet, secret
  devices/<id>.json      last sync per device, to know when tombstones can go
```

One file per record means two devices editing different hosts never touch the
same file.

### Merging

Each field carries a hybrid logical clock (HLC): physical milliseconds, a
counter and the device id.

- **Different fields.** Edits by two devices to different fields both survive.
  Example: A changes `port` while B, offline, changes `tags`.
- **The same field.** The later HLC wins, and the losing value goes to the sync
  log shown in Settings.
- **Deletes.** A delete is a tombstone with its own HLC. An edit made after the
  delete brings the record back. Tombstones are dropped once every device in
  `devices/` has synced past them.
- **Convergence.** The merge is commutative and idempotent, so every device ends
  with the same result whatever order changes arrive in. That holds even on
  stores without locking. If a device's write is overwritten, it still holds its
  own fields; at its next sync it merges them back and writes again.
- **Conditional writes.** Where a backend has them, the engine uses them to avoid
  that extra round:
  - S3 `If-Match` / `If-None-Match`
  - WebDAV ETags
  - A rejected `git push`, which leads to fetch, merge and push

### When it runs

- **shellwing cloud and the self-hosted server** push changes over a WebSocket.
- **Other backends:**
  - Sync when the window gains focus.
  - Sync every few minutes.
  - Sync a few seconds after a local change (debounced).

## Backends

| Backend | For | Setup | Conflict guard | Build cost |
| --- | --- | --- | --- | --- |
| Local folder (iCloud Drive, Syncthing, Dropbox/OneDrive desktop apps, NAS) | Everyone | Pick a folder | None. The merge also reads "conflicted copy" files the sync tool leaves. | Very low |
| Git (GitHub, GitLab, Gitea, any host) | Developers | Private repo URL + token | Rejected push | Medium (`git2`) |
| S3-compatible (AWS, R2, B2, MinIO, Wasabi) | Self-hosters | Endpoint, bucket, keys | `If-Match` | Low ([OpenDAL](https://opendal.apache.org)) |
| WebDAV (Nextcloud, Synology, Koofr) | Self-hosters | URL, user, password | ETag | Low (OpenDAL) |
| Google Drive, OneDrive, Dropbox | Everyone | OAuth sign-in | Weak | Medium: shellwing must register an OAuth app with each |
| shellwing cloud | Paying users | Portal sign-in | Full | High (server + portal) |
| shellwing-server, self-hosted | Teams, self-hosters | URL + sign-in | Full | Shared with the cloud ([teams.md](teams.md)) |

Notes:

- **Drive scope.** Use the `drive.appdata` scope: a hidden per-app folder. Google
  classes it as non-sensitive, so only basic verification is needed, not the
  security assessment the full `drive` scope requires.
- **Backend credentials** (Git token, S3 secret) stay on each device: as built
  for S3 and WebDAV, a device secret in the local vault, never synced.
- **Git history.** Encrypted records do not delta-compress, so history grows by
  roughly a record per change. Records are small (under 2 KB), so this is fine
  for years.

Build order:

1. The engine plus the local folder backend (built). This exercises the
   engine, and gives iCloud (which has no API on Windows or Linux) and
   Syncthing for free.
2. S3 and WebDAV, through OpenDAL (built: `crates/remote`). Moved ahead of
   the cloud: no server of ours is needed.
3. shellwing cloud and the portal.
4. Git.
5. Drive, OneDrive and Dropbox.

**S3 and WebDAV as built.**

- `Place` names where the vault syncs (a folder, a WebDAV folder, a bucket
  and prefix); `Remote` implements the engine's `Backend` over OpenDAL's
  blocking operator, on a one-thread Tokio runtime of its own. TLS is rustls
  with ring, as russh's, so no C crypto library is built.
- **Fewer requests.** The engine reads every record each round. `Remote`
  keeps each file it read with the ETag its listing gave, and reads again
  only when a listing shows another. A round with nothing new costs a few
  small requests: the listings (`records/`, `pairing/`) and `format.json`.
- **Conditional writes.** A record is written only if it is still as this
  device last saw it: `If-Match` with the ETag of the listing (or of its own
  last write), `If-None-Match: *` for a new one. Refused (412), the device
  reads the record again, merges it in and writes once more, up to three
  times in a sync. S3 does it through OpenDAL; WebDAV through a `PUT` of
  our own, since OpenDAL writes WebDAV without conditions. Where it cannot
  work, the write goes as before and the next round merges:
  - a listing without ETags (some S3 look-alikes), or weak ones (`W/`);
  - a server that says it cannot (405, 501), or refuses six in a row
    though the file was looked at again;
  - a new file refused though it is still not there (a folder not made
    yet, on some WebDAV servers).
  A tag made of size and time (Apache's, dav-server's) misses another
  write of the same size within its time's precision.
- **Credentials** are a device secret in the vault (`keys` table, wrapped by
  the account key, never synced) rather than the keychain, so they work
  where there is no keychain too. The vault opens before syncing anyway.
- Calls that the app makes against the place (starting, joining) run off
  the UI thread: a server may take seconds to answer.
