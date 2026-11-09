# Collections: the format

What an implementation needs to make and read [collections](collections.md)'
keys and records byte for byte: the identities people publish, a collection's key
wrapped for each of them, and the records sealed with it. Each part ends with a
test vector, which these tests check:

- the app: `crates/vault/src/identity.rs` and `crates/vault/src/collection.rs`,
  in Rust.
- shellwing-server: `pkg/identity` and `pkg/collectionkey`, in Go. The server
  only checks signatures, but its tests make every value below again with Go's
  own libraries, as a second implementation.

A change to any of this is a new version of its label (`… v2`), never an edit of
`v1`.

## Conventions

- Keys are 32 bytes, signatures 64.
- Numbers are big-endian.
- `‖` is concatenation.
- Labels are ASCII, with no terminator.
- On the wire and in files, bytes are standard base64 with padding (RFC 4648
  §4).
- In the vectors, `counting(n)` is the 32 bytes `n, n+1, n+2…`, and `[b; n]` is
  `n` bytes of `b`.

**`bound(id, version)`** is how the derivation and the signature both name a
collection's key: the id's length in 2 bytes, the id (its ULID, as ASCII), then
the version in 4 bytes.

## A person's identity

Two key pairs, made by the first device of the person that syncs with a
shellwing server and kept in their personal space:

- **X25519**, which collection keys are wrapped for.
- **Ed25519**, which signs the X25519 key and every wrap the person makes. Its
  secret is the 32-byte seed (RFC 8032).

Both secrets are in one field, `keys`, of the personal record
`1de0717e000000000000000000000000` (type `identity`): base64 of the X25519
secret ‖ the Ed25519 seed.

**Published** to the server (`PUT /api/users/current/identity`) as
`x25519Public`, `ed25519Public` and `signature`. The Ed25519 key signs:

```
"shellwing identity signature v1" ‖ x25519 public ‖ ed25519 public
```

**The fingerprint** people compare is the first 30 bytes of

```
SHA-256("shellwing identity v1" ‖ x25519 public ‖ ed25519 public)
```

read as six groups of 5 bytes. Each group is its big-endian number modulo
100000, written as five digits with leading zeros. The groups are separated by
spaces.

### Vector

| | |
| --- | --- |
| X25519 secret | `counting(1)` |
| Ed25519 seed | `counting(33)` |
| X25519 public | `B6N8vBQgk8i3VdwbEOhstCY3StFqqFPtC9/AsrhtHHw=` |
| Ed25519 public | `5/FioQvsVZr+oZXk3OhLaVaNXSywlj60RsBoXisX8vA=` |
| signature | `I+mHldAuIZ4mZvZQbMr+4cdL6LdTZ7qA1Xz0egaIf0MvLbChGT55sA1gZQTCnuJV+hW3ZzeKafF1lHrxk2uNCA==` |
| fingerprint | `42092 42879 96924 77795 39558 81840` |

## A collection's key, wrapped

A collection's key is 32 random bytes, with a version from 1. To wrap it for a
member whose identity's signature checks, and whose fingerprint was compared:

1. Make an ephemeral X25519 pair. The shared secret is X25519 of the ephemeral
   secret and the member's public key. Refuse an all-zero result: a low-order
   key gives everyone the same one.
2. Set `info = "shellwing collection key v1" ‖ bound(id, version)`.
3. The sealing key is 32 bytes of HKDF-SHA256 (RFC 5869):
   - input: the shared secret;
   - salt: ephemeral public ‖ member's public;
   - info: `info`.
4. Seal the collection key with XChaCha20-Poly1305, under the sealing key, with
   a random 24-byte nonce and `info` as additional data. The sealed key is the
   nonce ‖ the ciphertext and its tag: 72 bytes.
5. The commitment is `SHA-256("shellwing collection key commitment v1" ‖ key)`.
   Poly1305 does not commit to its key; the commitment says which key the wrap
   holds.
6. Sign with the wrapper's Ed25519 key:

   ```
   "shellwing collection key wrap v1" ‖ bound(id, version)
     ‖ member's X25519 public (32) ‖ ephemeral public (32)
     ‖ sealed key (72) ‖ commitment (32)
   ```

On the wire (`POST /api/orgs/{org}/collections/{id}/keys` for a new version,
`…/wraps` to give the current one), a request is `{"version": n, "wraps": [...]}`.
Each wrap is `userId`, `recipientX25519`, `ephemeralPublic`, `sealedKey`,
`commitment` and `signature`. The server takes a wrap only if two things hold:

- the caller's published Ed25519 key signed it;
- `recipientX25519` is the recipient's published key.

**Opening one**, from `GET …/keys`, whose wraps also carry the wrapper's keys
and fingerprint. A member refuses the wrap unless every check below passes, in
this order:

1. It is for their own X25519 key.
2. The wrapper's key signs it for this id and version: Ed25519 strict
   verification.
3. Its sealed key is 72 bytes.
4. It opens with the derivation above.
5. It holds 32 bytes.
6. Its commitment is that of the key it holds.

### Vector

The identity vector wraps the key `counting(201)` for itself:

| | |
| --- | --- |
| collection id | `01JC0LLECT10N0000000000000` |
| version | 1 |
| ephemeral secret | `[1; 32]` |
| nonce | `[9; 24]` |
| ephemeral public | `pOCSkrZRwni5dyxWn1+puxPZBrRqtoyd+dwrRAn4ogk=` |
| shared secret | `n7N3S7Rf6yLXUjr7Rik1/xE7woA8DndtxaUMe1DC7DQ=` |
| `info`, hex | `7368656c6c77696e6720636f6c6c656374696f6e206b6579207631` `001a30314a43304c4c45435431304e3030303030303030303030303000000001` |
| sealing key | `Rz1rtIlGoxq+04hvepUDwsOw4JxQCgXPn8thLKcpb0Y=` |
| sealed key | `CQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJD3nIsVTflOvUIw0qRWj6CBdhXmf5+zLW2+BQ21NL3D9QFo166bTPj7YqYRaS5Ymk` |
| commitment | `ruUTPgQFKROEd2yEBlBz55dKRs3Hj/1Lnb8OrpqrBE0=` |
| signature | `2VxS/LEyomoRmfqlvvyPEQi0jSFkkUZU63a8E+FKTL6OECH8jAx9UHezgF668S0ISYBwxRcFXOYLNgxDtHqWCw==` |

**For the signature alone**, the vector's Ed25519 key signs made-up parts:
- id `01JC0LLECT10N0000000000000`, version 1;
- recipient: the identity vector's X25519 public;
- ephemeral public: `counting(65)`;
- sealed key: the 72 bytes 97, 98, 99…;
- commitment: `counting(169)`.

The signature is
`hV8F1bdORRJKFUVYFOyYGOBIeiybJv+zC3lm3p8xFzyZGcJ7Hz889YqDOoBYHSwhqRqO8J8JH4ExwMrNxSjkAw==`.

## A collection's records

A collection's store holds what a personal space's does
([vault.md](vault.md#record-envelope)). Each record is in
`records/<id>.sw`, as a JSON envelope:

```json
{"id": "c0110000000000000000000000000001", "kind": "host",
 "space": "01JC0LLECT10N0000000000000", "key": 1,
 "hlc": "1792312831000-0000-dev1", "data": "AQcH…"}
```

- `space` is the collection's id.
- `key` is the version of its key that `data` is sealed with.
- `kind` is `host` or `other`, so the server can count hosts and nothing more.
- `hlc` is the record's newest field stamp.
- A deleted record has no `data`.

`data` is base64 of a cipher id ‖ the nonce ‖ the ciphertext and tag, sealing
the record's JSON payload ([vault.md](vault.md#record-envelope)) under the
collection's key of that version:

| Id | Cipher | Nonce |
| --- | --- | --- |
| 1 | XChaCha20-Poly1305 | 24 bytes |
| 2 | AES-256-GCM | 12 bytes |

The additional data binds the payload to the record:

```
id ‖ 0x00 ‖ kind ‖ 0x00 ‖ collection id ‖ 0x00 ‖ version, in decimal ASCII
```

A server cannot move one record's data into another, another collection, or
another version.

**When the key is replaced**, records stay readable: members keep the older
versions, and a record sealed with one opens with it. An app that may change the
collection writes a record sealed with an older version again, with the newest,
when it syncs.

### Vector

The key `counting(201)`, version 1, and the nonce `[7; 24]` seal this payload
(UTF-8, as is):

```
{"v":1,"type":"host","name":"db","f":{"address":{"val":"10.0.0.9","t":"1792312831000-0000-dev1"}}}
```

| | |
| --- | --- |
| record id | `c0110000000000000000000000000001` |
| kind | `host` |
| additional data, hex | `63303131303030303030303030303030303030303030303030303030303030310068` `6f73740030314a43304c4c45435431304e303030303030303030303030300031` |
| `data` | `AQcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBz3cVlKzDzIoGY6n90kcS7luilmfa80P8cBXS+cwiUEtSKAjE8UnX/sSTJphbM9dG4haPwAW3e9pGIPT2DJRRMXuW++aWOHj5AG4yIRBuaTGQO1cjEvbqk02T+9mtifTGSa+m8lsrCoNT3oJ+K8qo6Nrbw==` |
