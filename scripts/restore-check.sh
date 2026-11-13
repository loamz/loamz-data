#!/usr/bin/env bash
# Proves a backup can be restored: restores a dump from backup.sh into a throwaway database, checks its checksum, that the tables
# are there, and that every organization's audit chain is complete (no gap in its sequence numbers), then drops the database.
# A backup nobody has restored is a hope, not a backup: run this on a schedule too.
#
#   scripts/restore-check.sh <dump file> <admin postgres url>      (the admin url names a database the scratch one is made beside)
set -euo pipefail
[ $# -eq 2 ] || { sed -n '2,8p' "$0" | sed 's/^# \?//'; exit 2; }
dump=$1; admin=$2
(cd "$(dirname "$dump")" && sha256sum --check --quiet "$(basename "$dump").sha256")
scratch="restore_check_$(date +%s)"
base="${admin%/*}"
psql "$admin" -qc "CREATE DATABASE $scratch"
trap 'psql "$admin" -qc "DROP DATABASE IF EXISTS $scratch" >/dev/null' EXIT
pg_restore --no-owner --no-privileges --dbname "$base/$scratch" "$dump"
q() { psql "$base/$scratch" -Atc "$1"; }
tables=$(q "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'public'")
[ "$tables" -ge 10 ] || { echo "only $tables tables came back" >&2; exit 1; }
# An audit chain with a gap has lost entries: per organization, the highest sequence number equals the count.
gaps=$(q "SELECT count(*) FROM (SELECT org_id FROM audit GROUP BY org_id HAVING max(seq) <> count(*) - 1 AND max(seq) <> count(*)) g")
[ "$gaps" -eq 0 ] || { echo "$gaps organizations have a gap in their audit chain" >&2; exit 1; }
echo "restore ok: $tables tables, $(q "SELECT count(*) FROM audit") audit entries in $(q "SELECT count(DISTINCT org_id) FROM audit") organizations, $(q "SELECT count(*) FROM securables") securables"
