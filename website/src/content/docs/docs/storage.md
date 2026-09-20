---
title: "Storage and upgrades"
description: "Choose SQLite or Postgres for metadata and disk or S3 for documents."
---

Draft HTML is stored as opaque objects, on local disk by default or in any
S3-compatible store: AWS, RustFS, MinIO, Ceph RGW, Cloudflare R2, Backblaze
B2. The flags below are ignored unless `--storage s3`.

| Flag / env | Default | Purpose |
| --- | --- | --- |
| `--s3-bucket` / `KERYX_S3_BUCKET` | unset | Required when storage is `s3` |
| `--s3-region` / `KERYX_S3_REGION` | `us-east-1` | Region, or the placeholder most S3-compatible endpoints accept |
| `--s3-endpoint` / `KERYX_S3_ENDPOINT` | AWS | Custom endpoint, addressed path-style. Falls back to `AWS_ENDPOINT_URL_S3` |
| `--s3-prefix` / `KERYX_S3_PREFIX` | empty | Key prefix inside the bucket. Never stored in the database, so it can change freely |
| `--s3-profile` / `KERYX_S3_PROFILE` | unset | Named AWS profile for credential lookup |

Credentials are never Keryx flags. They resolve through the standard AWS
chain: environment variables, the shared profile, SSO, `credential_process`,
web identity, ECS, then IMDS.

At startup Keryx writes and deletes one probe object and refuses to boot if
that fails, so a wrong bucket or a read-only credential never surfaces as a
500 on the first upload. The credential needs exactly four permissions on
the prefix: `s3:GetObject`, `s3:PutObject`, `s3:DeleteObject` on
`arn:aws:s3:::<bucket>/<prefix>/*`, and `s3:ListBucket` on the bucket.

**One server per database.** Moving blobs to S3 does not make Keryx
multi-node. With SQLite the index is still local and still the single source
of truth, so two servers pointed at one bucket would mint divergent
histories and purge each other's objects. Run exactly one. Pair S3 with
[Postgres](../storage/#postgres) and nothing durable is left on local disk except the
push identity, but it is still one replica.

### Moving between stores

Object keys are identical on every backend, so moving is a verified copy and
never rewrites a database row. Stop the server first.

```sh
keryx storage migrate --from disk --to s3 --dry-run
keryx storage migrate --from disk --to s3
keryx serve --storage s3            # check it, then, optionally:
keryx storage migrate --from disk --to s3 --remove-source
```

Each object is read back from the destination and checked against the sha256
recorded at upload. Objects already present with the recorded size are
skipped, so an interrupted run resumes by re-running it. Any failure exits
non-zero and leaves the source untouched. `--from s3 --to disk` works the
same way. `storage` commands take the same `--db`, `--database-url`,
`--data-dir` and `--s3-*` flags and environment variables as `serve`.

### Orphaned objects

Blob removal is best-effort, and an upload writes its object before its
database row, so a failed delete or a failed upload can leave an object no
version owns. `keryx storage gc [--storage s3]` lists them; `--delete`
removes them. Objects younger than one hour are always left alone, so gc can
never take a version that is about to commit.

## Postgres

SQLite on a local disk is the default and stays that way. Postgres is for
running Keryx with no persistent volume, for example as a small tenant on an
existing cluster that already has backups and point-in-time recovery.

```sh
keryx serve --database-url 'postgres://keryx:<password>@db-rw.internal:5432/keryx?sslmode=verify-full&sslrootcert=/etc/keryx/ca.crt'
```

Prefer `KERYX_DATABASE_URL` from a secret over the flag, which shows up in
the process list. Keryx never prints the URL's credentials or query string:
the banner and errors show only `postgres://host:port/database`.

- **TLS** is set in the URL. `sslmode=verify-full` checks the certificate and
  host name. `sslrootcert` adds a private CA, such as a CloudNativePG cluster
  CA, on top of the built-in roots.
- **Privileges.** The database user needs DDL rights on its own database,
  because Keryx creates and migrates its schema at startup. Two pods starting
  together are safe: the migrator runs under an advisory lock.
- **Connect to the read-write service directly**, not through a PgBouncer
  pooler in transaction mode, which breaks prepared statement caching.
- **Pool.** `--db-pool-size` defaults to 4. Keryx is a small tenant; leave
  the cluster's connection budget for everyone else.
- **Failover.** Pooled connections are checked before use, with connect and
  acquire timeouts, so Keryx heals after a failover without a restart.
- **Probes.** `/healthz` queries the database. Use it for readiness only. As
  a liveness probe it would restart a healthy pod on every Postgres failover.

The schema matches SQLite's: timestamps are `TEXT` in RFC 3339 with
milliseconds, which orders lexically, and only the two flag columns differ
(`BOOLEAN`). There is no tool to copy a SQLite database into Postgres; a
draft is a complete HTML document, so re-upload what you want to keep.

**Still one replica.** Dashboard live updates are a channel inside one
process, the push dispatcher sends without claiming rows, and authentication
is one shared key. Do not run two Keryx servers against one database.

**Disaster recovery is manual, by design.** If the database is lost and the
blobs survive, every object is a complete HTML document and re-uploading
with the client rebuilds the drafts. If the database is rewound past a
purge, the only rows that come back without objects belong to drafts someone
deliberately deleted: they serve a clean not-found, and a second purge
removes them. Afterwards, `keryx storage gc` lists any objects left without
a row.

## Upgrading

A database written by an older Keryx keeps working in place. The first time
a newer server opens it, Keryx takes a consistent snapshot next to it
(`keryx.db.backup-<timestamp>`, written with `VACUUM INTO`, so nothing still
in the write-ahead log is missed), brings the schema to the current shape,
and records that in a `seaql_migrations` table. The startup banner says what
it did. It happens once; `--no-backup` skips the snapshot.

Nothing is moved or rewritten, and `PRAGMA user_version` is left alone, so
the previous Keryx release can still open the same file if you need to go
back.
