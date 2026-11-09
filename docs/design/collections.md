# Collections

A collection is hosts, secrets and snippets an organization shares: "Acme ·
Production". Who may do what is in [teams.md](teams.md); this is how it works.
The records are encrypted as personal ones are ([vault.md](vault.md)): the server
never holds a key that opens them.

## Decided

- **A collection's name is in the clear** on the server, so that its admins can
  manage access on the web. What is in it is not.
- **Its hosts live in the vault only**, never in `hosts.toml`, which stays each
  person's own. They are edited in the app.
- **Only a shellwing server syncs collections.** A folder, S3 or Git cannot say
  who may read what.
- **Owners and admins make collections.** A setting letting members do it may
  come later.
- **The key is replaced by itself** when somebody loses access: the first app
  with Manage that runs does it, and says so.
- **Granting the key shows the new member's fingerprint** to compare before it
  is given, as Bitwarden's "Confirm member" does. Nothing forces comparing it
  some other way.

## A person's identity

Each person has two key pairs: X25519, which keys are wrapped for, and Ed25519,
which signs what they wrap.

- The first device that syncs with a shellwing server makes them, and keeps them
  as a record of the personal space, so that every device of the person has them
  through sync. They are one field: two devices making them at once end up with
  one pair, whichever wins the merge.
- The app publishes the public halves to the server, with the X25519 key signed
  by the Ed25519 one. The server checks the signature, and keeps one identity per
  person.
- **The fingerprint** is what people compare: the first 30 bytes of
  `SHA-256("shellwing identity v1" ‖ x25519 ‖ ed25519)`, in six groups of five
  digits, each the big-endian number of five bytes modulo 100000.

```
fingerprint  41520 07391 88214 30567 61209 94730
```

A new identity cannot open the keys wrapped for the one before. The server drops
them when another is published, and the person waits for each key again, as
somebody newly granted does.

## A collection's key

32 random bytes, with a version. Records are sealed with it exactly as personal
ones are with the space key, with the collection's id as their space and the
version as their key: the additional data binds a record to both.

**Wrapping it for a member:**

1. An ephemeral X25519 key pair, agreed with the member's public key.
2. HKDF-SHA256 over the shared secret, with both public keys as salt and
   `"shellwing collection key v1" ‖ collection id ‖ version` as info.
3. The collection key sealed with that (XChaCha20-Poly1305), with the same info
   as additional data.
4. The wrapper signs, with their Ed25519 key, the collection id, the version,
   the member's public key, the ephemeral public key, the sealed key and the
   commitment.

The sealed key is 72 bytes: the 24-byte nonce, then the key and its tag. What is
signed is, in this order:

```
"shellwing collection key wrap v1"
len(collection id), 2 bytes big-endian ‖ collection id
version, 4 bytes big-endian
member's X25519 key (32) ‖ ephemeral public key (32)
sealed key (72) ‖ commitment (32)
```

shellwing-server's `pkg/collectionkey` checks it, with a test vector the app
checks too.

The wrap also carries a commitment, `SHA-256("shellwing collection key
commitment v1" ‖ key)`. Neither Poly1305 nor GCM commits to its key; with it, a
key that opens a record is the key the commitment names.

**Why the signature.** Without it the server could hand a member a key it made
itself, and read whatever they write. The member's app checks the wrap is signed
by somebody in the collection, and shows who.

**Owners hold every key.** When a collection is made, or its key replaced, it is
wrapped for every active owner too. Otherwise a collection all of whose members
left would be lost, and nobody could grant it again.

## Granting access

A collection made on the web has no key, version 0: the first app with Manage
that opens it makes version 1, wrapped for itself and every owner.

1. An admin, or somebody with Manage, grants a person or a group Connect, Edit
   or Manage: on the web or in the app. An admin with no grant manages access,
   and cannot read the collection.
2. The server keeps the grant **waiting for the key**. The web never wraps a key:
   it has none.
3. The app of somebody holding the key lists who waits, each with their
   fingerprint. Once confirmed, it wraps the key for them, and the grant is
   active.

A member without the key sees the collection as waiting for somebody to share
it.

## Taking it away

- **Suspending** a member stops the server serving them; their wraps stay, and
  reactivating them gives it back.
- **Losing access** (a grant removed, leaving a group, being removed from the
  organization) marks the collection for a new key: the spaces the app syncs
  say `needsNewKey` to those with Manage. The first of their apps that syncs
  holding the current key makes version + 1, wrapped for everybody who still
  has access and an identity (those suspended too); the server takes one, and
  the other apps take its key as any other.
- Records are sealed again as they sync: a file of the store sealed with an
  older version is written again with the newest, by any app that may change
  the collection. The older keys stay held, so what is not sealed again yet
  still opens.
- The app that replaced it keeps, on that device, the names of the
  collection's secrets (its records other than hosts), which the person could
  read: "Secrets to change in …" in the palette lists them until marked done.
- What the person copied before stays theirs: that is the limit
  [teams.md](teams.md) states, and why SSH certificates matter.

## The server

- It serves a collection only to its active members with a grant, directly or
  through a group, and takes writes only from those with Edit or Manage.
- It keeps the grants, the wraps per member and version, and which version is
  current. It takes a wrap only signed by the caller's published identity and
  made for the recipient's, and a new version only from somebody holding the
  current one, wrapped for them and every owner with an identity.
- A collection needs a new key when somebody holding the current one has no
  access any more; being suspended keeps access.
- The audit log has each grant and its removal, each wrap, each new key, and
  each download of a collection.
- Its hosts count as hosts do in a personal space.

## The app

- The vault holds several spaces: the personal one as it is now, and each
  collection, with its own key and versions.
- Each collection is a section of the sidebar: its hosts once the key is held,
  else "waiting for its key"; "read only" with Connect. The vault keeps the
  collections the server served at the last sync, so they show offline too.
- A collection's hosts are read from the vault as hosts.toml's are, and found by
  their id, since names may repeat across collections. They sign in with the
  organization's certificates unless one says otherwise, and their passwords are
  looked for in the collection first.
- The host editor saves a new host in hosts.toml or a collection the person may
  change ("Saved in"); a host saved stays where it is. Moving one is duplicating
  it into another place and deleting the original, since a record lives in one
  space.
- With Manage, the palette's "Give the key of …" lists who waits, each with the
  fingerprint to compare with their Profile page; choosing one gives it.
- Granting and taking away access is on the web for now.

## Order

1. Identities: made, synced and published; the fingerprint on the web. Done.
2. The server: collections, grants, wraps, and access to their sync. Done:
   `/api/orgs/{org}/collections`, and `/api/sync/spaces` lists the collections
   after the personal space.
3. The app: spaces in the vault, syncing collections, wrapping on making one and
   on confirming who waits, and the sidebar. Done for the desktop.
4. Replacing the key when somebody loses access, with the checklist. Done for the
   desktop.
5. A format document with test vectors for the Rust app and the Go server, and
   an end-to-end test of two people sharing a collection.

The phone apps follow the desktop.
