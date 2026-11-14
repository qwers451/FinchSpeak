#!/usr/bin/env bash
set -Eeuo pipefail
umask 077

workdir=/root/finchspeak
compose="$workdir/docker-compose.yml"
settings="$workdir/finch-server.toml"
service=finch-server
old=finchspeak-server:1.7.0
new=finchspeak-server:1.7.1
new_id="$1"
volume_name=finchspeak_finch-data
volume=/var/lib/docker/volumes/finchspeak_finch-data/_data

cd "$workdir"
exec 9> "$workdir/.finch-upgrade.lock"
flock -n 9
dc() { docker compose -p finchspeak -f "$compose" "$@"; }
fp() { sha256sum "$1" | cut -d ' ' -f 1; }
serve_fp() {
    timeout 6 openssl s_client -connect 127.0.0.1:8443 </dev/null 2>/dev/null |
      openssl x509 -outform DER | sha256sum | cut -d ' ' -f 1
}
health() {
    local version="$1" response
    for attempt in {1..35}; do
        if response=$(curl -kfsS --max-time 2 https://127.0.0.1:8443/health 2>/dev/null) &&
            python3 -c 'import json,sys; h=json.loads(sys.argv[2]); assert h["status"]=="ok" and h["version"]==sys.argv[1]' "$version" "$response" 2>/dev/null; then
            printf '%s\n' "$response"
            return
        fi
        sleep 2
    done
    return 1
}
db_summary() {
    python3 - "$volume/finchspeak.db" <<'PY'
import json, pathlib, sqlite3, sys
db = sqlite3.connect(pathlib.Path(sys.argv[1]).as_uri() + "?mode=ro", uri=True, timeout=5)
assert [row[0] for row in db.execute("PRAGMA quick_check")] == ["ok"], "SQLite quick_check failed"
counts = {table: db.execute(f"SELECT count(*) FROM {table}").fetchone()[0] for table in ("users", "channels", "messages")}
print(json.dumps(counts, sort_keys=True))
PY
}
devcontest_state() {
    docker ps -aq --filter label=com.docker.compose.project=devcontest |
        xargs -r docker inspect --format '{{.Id}} {{.State.Status}} {{.State.StartedAt}} {{.RestartCount}}' |
        sort
}
mounts() {
    docker inspect "$service" --format '{{range .Mounts}}{{println .Name .Source .Destination .RW}}{{end}}' | sort
}
ports() { docker inspect "$service" --format '{{json .HostConfig.PortBindings}}'; }

test "$(dc config --services)" = "$service"
test "$(dc config --images)" = "$old"
test "$(docker inspect --format '{{.Config.Image}} {{.State.Status}}' "$service")" = "$old running"
test "$(docker volume inspect --format '{{.Mountpoint}}' "$volume_name")" = "$volume"
test "$(docker image inspect --format '{{.Id}}' "$new")" = "$new_id"
test "$(docker image inspect --format '{{.Os}}/{{.Architecture}} {{.Config.User}}' "$new")" = 'linux/amd64 finch:finch'
test "$(docker inspect --format '{{.HostConfig.Memory}}' "$service")" = 268435456
docker image inspect "$old" --format '{{.Id}}' >/dev/null
test -s "$volume/finchspeak.db"
test -s "$volume/tls/cert.der"
cert=$(fp "$volume/tls/cert.der")
test "$(serve_fp)" = "$cert"
settings_hash=$(fp "$settings")
compose_hash=$(fp "$compose")
mount_signature=$(mounts)
port_signature=$(ports)

# Existing attachments count toward the new per-user quota. Refuse a rollout that would
# unexpectedly prevent current users from uploading; adjust the setting explicitly and rerun.
python3 - "$volume/finchspeak.db" "$settings" <<'PY'
import pathlib, sqlite3, sys, tomllib
settings = tomllib.loads(pathlib.Path(sys.argv[2]).read_text())
quota = max(1, int(settings.get("storage", {}).get("user_quota_mb", 1024))) * 1024 * 1024
db = sqlite3.connect(pathlib.Path(sys.argv[1]).as_uri() + "?mode=ro", uri=True, timeout=5)
rows = list(db.execute("SELECT uploader_id, SUM(size_bytes) FROM attachments GROUP BY uploader_id HAVING SUM(size_bytes) > ?", (quota,)))
if rows:
    for user, used in rows:
        print(f"User {user}: {used} bytes exceeds quota {quota}", file=sys.stderr)
    raise SystemExit("Raise [storage] user_quota_mb before deployment")
print(f"PASS existing attachment usage fits per-user quota {quota} bytes")
PY
# Rust now refuses public/private-key permissions on Unix. Check ownership and mode before stop.
for secret in "$volume/tls/key.der" "$volume/jwt_secret"; do
    if [ -e "$secret" ]; then
        test "$(stat -c '%u:%g:%a' "$secret")" = '10001:10001:600' || {
            echo "Fix ownership/mode of $secret (10001:10001, 0600) before deployment" >&2
            exit 1
        }
    fi
done

if [ -d "$volume/backups" ]; then
    test "$(stat -c '%u:%g' "$volume/backups")" = '10001:10001' || {
        echo 'Fix ownership of the backup directory before deployment' >&2; exit 1;
    }
    while IFS= read -r -d '' backup_file; do
        test "$(stat -c '%u:%g:%a' "$backup_file")" = '10001:10001:600' || {
            echo "Fix ownership/mode of $backup_file (10001:10001, 0600) before deployment" >&2; exit 1;
        }
    done < <(find "$volume/backups" -type f -print0)
fi

install -d -m 700 "$workdir/rollbacks"
backup=$(mktemp -d "$workdir/rollbacks/1.7.0-to-1.7.1-$(date -u +%Y%m%dT%H%M%SZ)-XXXXXX")
cp -a -- "$compose" "$settings" "$backup/"
devcontest_state > "$backup/devcontest.before"
test -s "$backup/devcontest.before"
candidate=$(mktemp "$workdir/.compose-1.7.1-XXXXXX")
rollback_needed=0
new_started=0
finish() {
    local rc=$?
    trap - EXIT HUP INT TERM
    if [ "$rollback_needed" -eq 1 ]; then
        set +e
        echo "Rolling back only $service to 1.7.0"
        restored=1
        if [ "$new_started" -eq 1 ]; then
            # 1.7.1 has no schema change. Keep messages written after the restart;
            # the cold backup remains available for manual recovery if the data itself is damaged.
            dc stop -t 15 "$service" || restored=0
        fi
        if [ "$restored" -eq 1 ] &&
            cp -a -- "$backup/finch-server.toml" "$settings" &&
            cp -a -- "$backup/docker-compose.yml" "$candidate" && mv -fT -- "$candidate" "$compose" &&
            [ "$(dc config --images)" = "$old" ] &&
            dc up -d --no-deps --no-build --pull never "$service" &&
            health 1.7.0; then
            echo "Rollback healthy; settings and compose restored from $backup; compatible live data retained"
        else
            echo "Rollback needs attention; backup is $backup" >&2
        fi
        rc=1
    fi
    rm -f -- "$candidate"
    exit "$rc"
}
trap finish EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

cp -a -- "$compose" "$candidate"
test "$(grep -c '^    image: finchspeak-server:1.7.0$' "$candidate")" = 1
sed -i 's/^    image: finchspeak-server:1.7.0$/    image: finchspeak-server:1.7.1/' "$candidate"
test "$(docker compose -p finchspeak -f "$candidate" config --services)" = "$service"
test "$(docker compose -p finchspeak -f "$candidate" config --images)" = "$new"

rollback_needed=1
dc stop -t 15 "$service"
test "$(docker inspect --format '{{.State.Running}}' "$service")" = false
db_summary > "$backup/database.before.json"
tar --numeric-owner -C "$volume" -cf - . | gzip -1 > "$backup/data.tar.gz.tmp"
gzip -t "$backup/data.tar.gz.tmp"
test "$(tar -xzOf "$backup/data.tar.gz.tmp" ./finchspeak.db | sha256sum | cut -d ' ' -f 1)" = "$(fp "$volume/finchspeak.db")"
mv -- "$backup/data.tar.gz.tmp" "$backup/data.tar.gz"
(cd "$backup"; sha256sum data.tar.gz docker-compose.yml finch-server.toml > SHA256SUMS.txt)
echo "PASS cold backup $backup"

test "$(fp "$compose")" = "$compose_hash"
test "$(fp "$settings")" = "$settings_hash"
mv -fT -- "$candidate" "$compose"
# Room music: limits sized for this VPS (shared with Dev Contest, about 7 GB free).
if ! grep -qx '\[music\]' "$settings"; then
    cat >> "$settings" <<'TOML'

[music]
enabled = true
max_track_mb = 30
library_quota_mb = 512
temp_quota_mb_total = 256
temp_quota_mb_per_user = 200
max_queue = 100
max_track_minutes = 180
TOML
fi
# Soundboard (1.7): short WAV clips stored in the database, at most 48 per server.
if ! grep -qx '\[soundboard\]' "$settings"; then
    cat >> "$settings" <<'TOML'

[soundboard]
enabled = true
max_sounds = 48
TOML
fi
settings_hash=$(fp "$settings")
new_started=1
dc up -d --no-deps --no-build --pull never "$service"
health 1.7.1
test "$(docker inspect --format '{{.Image}}' "$service")" = "$new_id"
test "$(docker inspect --format '{{.HostConfig.Memory}}' "$service")" = 268435456
test "$(mounts)" = "$mount_signature"
test "$(ports)" = "$port_signature"
test "$(fp "$settings")" = "$settings_hash"
test "$(fp "$volume/tls/cert.der")" = "$cert"
test "$(serve_fp)" = "$cert"
db_summary > "$backup/database.after.json"
python3 - "$backup/database.before.json" "$backup/database.after.json" <<'PY'
import json, sys
before, after = (json.load(open(path)) for path in sys.argv[1:])
assert all(after[table] >= count for table, count in before.items()), (before, after)
print(f"PASS SQLite integrity and counts {before} -> {after}")
PY

rollback_needed=0
echo "PASS 1.7.1 health, settings with [music] and [soundboard], ports, volume, TLS identity and memory limit"
docker stats --no-stream "$service" --format 'FinchSpeak CPU={{.CPUPerc}} memory={{.MemUsage}}'
devcontest_state > "$backup/devcontest.after"
if cmp -s "$backup/devcontest.before" "$backup/devcontest.after"; then
    echo "PASS Dev Contest container identities, start times and restart counts unchanged"
else
    echo "Dev Contest container state changed independently; this script targets only FinchSpeak" >&2
    exit 3
fi
