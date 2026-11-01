# Teams and the self-hosted server

## Concepts

```
Organization (Acme)                  a tenant: one on a self-hosted server,
├─ Members        owner, admin, member
├─ Groups         "Backend", "Ops", "Intern": people bundled for granting access
└─ Collections    "Production", "Staging": shared hosts, secrets and snippets
     granted to a group or a person as:
       Connect   see the hosts and connect
       Edit      add and change hosts
       Manage    grant others access to the collection
```

- **Personal stays private.** Each person's Personal space belongs to them and
  to no organization, which never sees it.
- **One home per record.** A record lives in exactly one space: Personal or one
  collection.
- **Sidebar.** Each collection is its own section, for example
  "Acme · Production".

## Sharing with end-to-end encryption

The key hierarchy is in [vault.md](vault.md). In short, each person has a key
pair, and each collection key is wrapped once for each member's public key.

- **Adding a member.**
  1. The new member signs in and publishes a public key.
  2. An admin's app checks the key's fingerprint. The fingerprint is shown on
     both screens, as in Bitwarden's "Confirm member".
  3. The admin's app wraps the collection key for the new member.

  The server cannot do this step, because it never holds collection keys.
- **Removing a member.**
  - The server stops serving the collection to them.
  - An admin's app rotates the collection key and re-encrypts its records.
  - The app lists every secret the person could read, so they can be changed.
- **SSO.** SSO establishes identity only. The decryption key still comes from a
  trusted device or an admin approving the new one.

Limits to state plainly to customers:

- **"Can connect but cannot see the password" is a UI rule, not cryptography.**
  The app must decrypt a secret to use it.
- **Removing a member cannot undo what they already copied.** That is why
  removal comes with the list of secrets to change.

### SSH certificates

The real fix for both limits is an SSH certificate authority:

- The organization's server signs short-lived user certificates, for example
  for 8 hours.
- Target servers trust the CA with `TrustedUserCAKeys`.
- No private key is shared between people.
- Removing someone means no new certificates; existing ones simply expire.
- Each certificate issued is an audit record the server can vouch for.

Client side, russh 0.64 supports certificate login: `authenticate_openssh_cert`,
and `authenticate_certificate_with` for agent-held keys.

The trade-off is that the CA key lives on the server. That is acceptable when
self-hosted; in shellwing cloud it belongs in a KMS or HSM.

## Audit log

The server records reliably:

- Sign-ins.
- Record created, changed or deleted (by id).
- Membership and permission changes.
- Who downloaded which collection.
- Certificates issued.

"Who connected to which host, when" is reported by the client and cannot be
verified, except through certificates.

## Editions

Two products are sold through the portal shared by the owner's products: a
license for a server somebody hosts, and shellwing cloud, the service the owner
runs. **Every edition has every feature.** Only the number of users differs.

| | Self-hosted, free | Self-hosted, licensed | shellwing cloud |
| --- | --- | --- | --- |
| Run by | The customer | The customer | The owner |
| Users | Up to 2 | The license's tier, such as 10, 25, 50 (**Open**) | The plan's (**Open**) |
| Features | All | All | All |

**Open:** prices. The design is in the `hivepaas-portal` repository:
`docs/superpowers/specs/2026-10-30-multi-product-design.md`.

## shellwing-server

**One codebase.** One public repository. shellwing cloud is the same server,
which the owner runs, configured to sign in with the Wing account and read plans
from the portal. It holds no payment code: buying happens at the portal.

**Purpose.** It syncs people's encrypted data and issues SSH certificates.

**Licensing.** The whole server is AGPL-3.0; the app stays GPL-3.0. Public code
earns trust for a product that holds credentials, and anyone offering it as a
service must publish their changes. As with HivePaaS, the limit is a fair-use
gate, not DRM: the code is open, so anybody may remove it for their own use.

**License key.** Signed by the portal, with a key pair of this product's own,
and carrying the customer, the number of users and the expiry.

- The server verifies it offline, so it works without internet access. Fetching
  a renewed key from the portal is optional.
- The limit counts users across the whole server, not per organization. A
  self-hosted server has one organization, so that a second one cannot get round
  it. In shellwing cloud each organization has its plan's limit.
- A limit only stops creating. Over the limit, or when a license ends, nobody is
  removed or locked out of their own credentials, and every feature keeps
  working.

**Stack.**

- **Language.** Go: the server never decrypts anything.
  A format spec with test vectors keeps the Rust client and the Go server in
  agreement.
- **Database.** PostgreSQL, for users, settings and the metadata of synced files.
- **Redis.** Required: sessions, rate limits, background tasks, and telling other
  instances that something changed. Nothing in it is needed for good: a device's
  refresh token is in PostgreSQL, and a lost change notice only delays a sync.
- **Sync content.** A local folder or S3-compatible storage ([sync.md](sync.md)).
- **Distribution.** A Docker image, plus a one-click template on HivePaaS.

## Tenants

A tenant is an organization, in one shared database schema. A self-hosted server
has exactly one, made when it is installed and not shown as such. shellwing cloud
has many; turning that on comes after the features below, so the groundwork is
laid now:

- `users` are global, and a person may belong to several organizations through
  `org_members`. `users.role` is about the server, `org_members.role` about one
  organization.
- `spaces` hold synced records. A personal space names its user, a collection its
  organization, and never both.
- The API names the organization from the start (`/orgs/{org}/...`), repositories
  of organization-owned tables require its id, and tests use two organizations to
  check that neither sees the other's data.
- Settings are either the server's (storage, mail, license) or an organization's
  (SSO, certificate policy, CA).

An organization may use SSO only for email domains it has verified. Emails are
unique across the server, so otherwise its SSO could claim anybody's account.

## Order

1. Personal sync ([sync.md](sync.md)), on a local folder, then S3.
2. The 2-user limit and the license key.
3. SSH CA.
4. Collections, groups and permissions, plus the removal checklist.
5. Several organizations, for shellwing cloud.
6. Audit log views, SSO per organization and SCIM.
