# Teams and the self-hosted server

## Concepts

```
Organization (Acme)
├─ Members        owner, admin, member
├─ Groups         "Backend", "Ops", "Intern": people bundled for granting access
└─ Collections    "Production", "Staging": shared hosts, secrets and snippets
     granted to a group or a person as:
       Connect   see the hosts and connect
       Edit      add and change hosts
       Manage    grant others access to the collection
```

- **Personal stays private.** Each person's Personal space is never visible to
  the organization.
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

### SSH certificates (Business)

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

| | Community (self-hosted, free) | Team | Business |
| --- | --- | --- | --- |
| Users | Up to 3 (**Open**) | Per seat | Per seat |
| Personal sync, unlimited hosts | Yes | Yes | Yes |
| Collections | 1 | Unlimited | Unlimited |
| Groups, Connect/Edit/Manage | | Yes | Yes |
| Removal checklist, key rotation | | Yes | Yes |
| Audit log | | 30 days | Unlimited, export |
| SSO (OIDC, SAML), SCIM | | | Yes |
| Policies (no export, vault lock required…) | | | Yes |
| SSH CA, short-lived certificates | | | Yes |

Team and Business are sold both in shellwing cloud and as self-hosted licenses.
**Open:** prices.

## shellwing-server

**One codebase.** shellwing cloud runs the same server, with billing turned on.

**Licensing.** This is open core in one repository:

- **The core is AGPL-3.0.** Public code earns trust for a product that holds
  credentials. Anyone offering it as a service must publish their changes.
- **Team features live in `ee/` under a commercial license**, as with GitLab EE or
  Bitwarden. The source is visible, but use needs a valid license.
- **Not FSL or BSL for the whole server.** Those allow modifying the code for
  internal use, which includes removing the license check. That suits Sentry,
  which sells its cloud, not self-hosted licenses.

**License key.** A file signed with Ed25519 by the portal, carrying the
organization, seats, features and expiry.

- The server verifies it offline, so it works without internet access.
- When the license expires, team features become read-only. People are never
  locked out of their own credentials.

**Stack.**

- **Language.** Go: the server never decrypts anything.
  A format spec with test vectors keeps the Rust client and the Go server in
  agreement.
- **Database.** SQLite by default, Postgres optional.
- **Distribution.** A single binary or Docker image, plus a one-click template
  on HivePaaS.

## Order

1. Personal sync, working well ([sync.md](sync.md)).
2. Collections, groups and permissions, plus the removal checklist.
3. Audit log.
4. SSO and SCIM.
5. SSH CA.
