#!/usr/bin/env bash
# Backs up the catalog database (and any other Postgres database you name) to a folder, one file each, with a checksum. This is the
# backup of what Postgres holds: every grant, owner, table pointer, run and audit entry. The data files and table metadata are in the
# bucket, and secrets in OpenBao, which have their own backups (see docs/architecture/platform.md, "When the server is down").
#
#   scripts/backup.sh <out folder> <postgres url> [<postgres url> ...]
#   scripts/backup.sh /var/backups/loamz postgres://data:...@db/data postgres://kc:...@db/keycloak
#
# Run it from cron or a systemd timer; continuous archiving (WAL) is set up on the database host, and this is the base backup that
# restore-check.sh proves can be restored. Needs pg_dump (the same major version as the server, or newer).
set -euo pipefail
[ $# -ge 2 ] || { sed -n '2,11p' "$0" | sed 's/^# \?//'; exit 2; }
out=$1; shift
mkdir -p "$out"
stamp=$(date -u +%Y%m%d-%H%M%S)
for url in "$@"; do
  name=$(basename "${url%%\?*}")
  file="$out/$name-$stamp.dump"
  pg_dump --format=custom --no-owner --no-privileges --dbname "$url" --file "$file"
  (cd "$out" && sha256sum "$(basename "$file")" > "$(basename "$file").sha256")
  echo "backed up $name: $file ($(du -h "$file" | cut -f1))"
done
