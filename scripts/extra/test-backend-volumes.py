"""Real Docker storage lifecycle smoke, using only a unique disposable project.

Run: python3 scripts/extra/test-backend-volumes.py
Requires Docker Compose, alpine:3.20, python:3.12-alpine and postgres:18-alpine.
Missing images are pulled.
Uses production mount declarations and real backup/restore scripts, with small
shell services replacing business processes. This is NOT a full updater rollout.
"""
import copy
import json
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import tempfile

REPO = Path(__file__).resolve().parents[2]
PROJECT = "myriad-volumes-" + secrets.token_hex(6)
TEMP = tempfile.TemporaryDirectory(prefix=PROJECT + "-")
ROOT = Path(TEMP.name).resolve()
ENV = {k: v for k, v in os.environ.items() if k not in ("COMPOSE_FILE", "COMPOSE_PROJECT_NAME")}
ENV["COMPOSE_PROJECT_NAME"] = PROJECT
VOLUMES = [f"{PROJECT}_backend_{kind}" for kind in ("data", "cache")]


def run(args, check=True, data=None):
    result = subprocess.run(args, cwd=ROOT, env=ENV, input=data, capture_output=True, text=True)
    if check and result.returncode:
        raise RuntimeError(f"{args[:3]} failed:\n{result.stderr}")
    return result


def compose(*args, check=True):
    return run(["docker", "compose", "--env-file", str(ROOT / ".env"), "-p", PROJECT,
                "-f", str(ROOT / "docker-compose.yml"), *args], check=check)


def container(command, mounts=()):
    args = ["docker", "run", "--rm", "--network", "none"]
    for mount in mounts:
        args += ["--mount", mount]
    return run([*args, "alpine:3.20", "sh", "-eu", "-c", command])


def provision():
    run(["bash", "-eu", "-c", 'source "$1"; load_backend_storage docker "$COMPOSE_PROJECT_NAME" "$PWD" prepare',
         "provision", str(REPO / "scripts/extra/backend-storage.sh")])


def sql(query):
    return compose("exec", "-T", "postgres", "psql", "-U", "myriad", "-d", "myriad",
                   "-At", "-v", "ON_ERROR_STOP=1", "-c", query).stdout.strip()


def start():
    compose("up", "-d", "--force-recreate", "backend-volume-init")
    initializer = compose("ps", "-a", "-q", "backend-volume-init").stdout.strip()
    assert initializer, "initializer container missing"
    assert run(["docker", "wait", initializer]).stdout.strip() == "0"
    compose("up", "-d", "--wait", "--wait-timeout", "60", "postgres", "backend",
            "persona-worker", "federation-worker")


def save(model):
    (ROOT / "docker-compose.yml").write_text(json.dumps(model))


def archive(kind, folder, restore=False, bind=False):
    volume = f"{PROJECT}_backend_{kind}"
    target = (f"type=bind,src={ROOT / kind},dst=/data" if bind
              else f"type=volume,src={volume},dst=/data,volume-nocopy")
    command = (f"trap 'status=$?; chmod 700 /data || exit 1; exit \"$status\"' 0; tar xzpf /archive/{kind}.tar.gz -C /data" if restore
               else f"tar czf /archive/{kind}.tar.gz -C /data .")
    container(command, [target, f"type=bind,src={folder},dst=/archive"])
    if not restore:
        container(f"tar tzf /archive/{kind}.tar.gz >/dev/null",
                  [f"type=bind,src={folder},dst=/archive,readonly"])


try:
    run(["docker", "info", "--format", "{{.ServerVersion}}"])
    for image in ("alpine:3.20", "python:3.12-alpine", "postgres:18-alpine"):
        if run(["docker", "image", "inspect", image], check=False).returncode:
            run(["docker", "pull", image])
    print("Checking Linux private storage with a non-owner operator", flush=True)
    checks = ROOT / "checks"
    checks.mkdir()
    validator = run(["bash", "-c", 'source "$1"; declare -f validate_backend_directory',
                     "check", str(REPO / "scripts/extra/backend-storage.sh")]).stdout
    (checks / "root-check.sh").write_text(validator + '\nvalidate_backend_directory /app/data\nvalidate_backend_directory /app/cache\n')
    payload = run(["bash", "-c", 'source "$1"; backend_storage_directories_posix',
                   "check", str(REPO / "scripts/extra/backend-storage.sh")]).stdout
    (checks / "directories.sh").write_text(payload)
    restore = run(["bash", "-c", 'source "$1" help; volume_restore_posix',
                   "check", str(REPO / "scripts/extra/backup.sh")]).stdout
    (checks / "restore.sh").write_text(restore)
    container('''mkdir -p /app/data /app/cache
chown 1000:1000 /app/data /app/cache
chmod 700 /app/data /app/cache
su nobody -s /bin/sh -c 'sh -eu /checks/root-check.sh; test ! -x /app/data; test ! -x /app/cache'
sh /checks/directories.sh prepare
test -d /app/data/federation_media && test -d /app/cache/images
test "$(stat -c %a /app/data)" = 700
rmdir /app/data/media; ln -s /tmp /app/data/media
if sh /checks/directories.sh prepare; then exit 1; fi
mkdir /old /restored
chmod 755 /old
echo content >/old/media
tar czf /tmp/archive.tar.gz -C /old .
DEST=/restored ARCHIVE=/tmp/archive.tar.gz RESTORE_OWNER=1000:1000 sh /checks/restore.sh
test "$(stat -c %a:%u:%g /restored)" = 700:1000:1000
test "$(cat /restored/media)" = content
chmod 755 /restored
echo invalid >/tmp/archive.tar.gz
if DEST=/restored ARCHIVE=/tmp/archive.tar.gz sh /checks/restore.sh; then exit 1; fi
test "$(stat -c %a /restored)" = 700
''', [f"type=bind,src={checks},dst=/checks,readonly"])
    values = {"COMPOSE_PROJECT_NAME": PROJECT, "MYRIAD_TAG": "fixture", "PROXY_TAG": "fixture",
              "UPDATER_TAG": "fixture", "CORS_ORIGINS": "http://localhost"}
    for key in ("POSTGRES_PASSWORD", "PERSONA_DB_PASSWORD", "FEDERATION_DB_PASSWORD", "JWT_SECRET",
                "MYRIAD_SETUP_SECRET", "GUARD_SELF_UPDATE_TOKEN", "UPDATE_TOKEN", "UPDATER_GATEWAY_SECRET"):
        values[key] = secrets.token_hex(24)
    (ROOT / ".env").write_text("".join(f"{k}={v}\n" for k, v in values.items()))
    production = json.loads(run(["docker", "compose", "--project-directory", str(ROOT),
        "--env-file", str(ROOT / ".env"), "-f", str(REPO / "docker-compose.yml"),
        "config", "--no-env-resolution", "--format", "json"]).stdout)
    model = {"services": {}, "volumes": {"pgdata": {}}}
    for service in ("backend", "backend-volume-init", "persona-worker", "federation-worker"):
        model["services"][service] = {
            "image": "alpine:3.20", "network_mode": "none", "user": "1000:1000",
            "read_only": True, "tmpfs": ["/tmp"], "command": ["sleep", "infinity"],
            "volumes": production["services"][service]["volumes"],
        }
    model["services"]["backend-volume-init"].update(user="0:0", command=["sh", "-eu", "-c",
        "mkdir -p /app/data/media /app/data/federation /app/data/federation_media /app/cache/images; "
        "chown -R 1000:1000 /app/data /app/cache; chmod -R u+rwX /app/data /app/cache"])
    model["services"]["backend"]["image"] = "python:3.12-alpine"
    model["services"]["backend"]["command"] = ["sh", "-eu", "-c",
        "mkdir /tmp/www; echo ready >/tmp/www/ready; exec python3 -m http.server 1103 --directory /tmp/www"]
    model["services"]["postgres"] = {"image": "postgres:18-alpine", "network_mode": "none",
        "environment": {"POSTGRES_USER": "myriad", "POSTGRES_DB": "myriad",
                        "POSTGRES_PASSWORD": values["POSTGRES_PASSWORD"]},
        "volumes": ["pgdata:/var/lib/postgresql"],
        "healthcheck": {"test": ["CMD-SHELL", "pg_isready -U myriad -d myriad"],
                        "interval": "1s", "timeout": "3s", "retries": 30}}
    direct = copy.deepcopy(model)
    legacy = copy.deepcopy(model)
    legacy["volumes"].update(backend_data={}, backend_cache={})
    for service in ("backend", "backend-volume-init", "persona-worker", "federation-worker"):
        mounts = legacy["services"][service]["volumes"]
        for mount in mounts:
            relative = Path(mount["source"]).relative_to(ROOT)
            mount.update(type="volume", source="backend_" + relative.parts[0])
            mount.pop("bind", None)
            if len(relative.parts) > 1:
                mount["volume"] = {"subpath": "/".join(relative.parts[1:]), "nocopy": True}
    save(direct)
    scripts = ROOT / "scripts/extra"
    scripts.mkdir(parents=True)
    for name in ("backup.sh", "backend-storage.sh"):
        shutil.copyfile(REPO / "scripts/extra" / name, scripts / name)

    print("Provisioning direct binds without Docker volume registrations", flush=True)
    provision()
    container("echo fresh >/data/fresh", [f"type=bind,src={ROOT / 'data'},dst=/data"])
    for volume in VOLUMES:
        assert run(["docker", "volume", "inspect", volume], check=False).returncode != 0
    container("test \"$(cat /fixture/data/fresh)\" = fresh; rm -rf /fixture/data /fixture/cache",
              [f"type=bind,src={ROOT},dst=/fixture"])
    print("Starting legacy named volumes and writing test data", flush=True)
    save(legacy)
    start()
    provision()
    compose("exec", "-T", "backend", "sh", "-c", "echo original >/app/data/media/sentinel; echo private >/app/data/private")
    sql("CREATE TABLE volume_test (value text); INSERT INTO volume_test VALUES ('original');")
    compose("down")
    old = ROOT / "old-archives"
    old.mkdir(mode=0o700)
    for kind in ("data", "cache"):
        archive(kind, old)
        (ROOT / kind).mkdir(mode=0o700)
        archive(kind, old, restore=True, bind=True)
    run(["docker", "volume", "rm", *VOLUMES])
    save(direct)
    provision()
    print("Starting direct binds with production worker boundaries", flush=True)
    start()
    assert compose("exec", "-T", "backend", "cat", "/app/data/media/sentinel").stdout.strip() == "original"
    for path in ("/app/data/media", "/app/data/federation", "/app/data/federation_media", "/tmp/cache/images"):
        compose("exec", "-T", "federation-worker", "sh", "-c", f"echo subpath >{path}/worker-check")
    assert compose("exec", "-T", "federation-worker", "sh", "-c", "echo forbidden >/app/data/private", check=False).returncode != 0
    compose("exec", "-T", "persona-worker", "sh", "-c", "echo persona >/app/data/persona-check")
    container("test -f /data/media/worker-check && test -f /data/persona-check",
              [f"type=bind,src={ROOT / 'data'},dst=/data,readonly"])
    # Recreate the completed initializer and both workers like an image rollout.
    before = compose("ps", "-q", "federation-worker").stdout.strip()
    start()
    compose("up", "-d", "--force-recreate", "backend", "persona-worker", "federation-worker")
    assert before != compose("ps", "-q", "federation-worker").stdout.strip()
    assert compose("exec", "-T", "backend", "cat", "/app/data/media/sentinel").stdout.strip() == "original"

    print("Running real PostgreSQL + media backup and restore", flush=True)
    backup = ROOT / "backup"
    # Old volume archives can carry a 0755 root; restoring one must not expose
    # the new host data directory with those historical permissions.
    container("chmod 755 /data; chown 0:0 /data", [f"type=bind,src={ROOT / 'data'},dst=/data"])
    run(["bash", str(scripts / "backup.sh"), "backup", "--out", str(backup)])
    sql("UPDATE volume_test SET value='changed';")
    compose("exec", "-T", "backend", "sh", "-c", "echo changed >/app/data/media/sentinel")
    with (ROOT / ".env").open("a") as env_file:
        env_file.write("RESTORE_MARKER=live\n")
    live_env = (ROOT / ".env").read_bytes()
    # A rejected restore must leave the actual DB, .env and writers untouched.
    parked = ROOT / "data-parked"
    (ROOT / "data").rename(parked)
    (ROOT / "data").symlink_to(parked, target_is_directory=True)
    try:
        rejected = run(["bash", str(scripts / "backup.sh"), "restore", "--from", str(backup)], check=False)
        assert rejected.returncode != 0
        assert (ROOT / ".env").read_bytes() == live_env
        assert sql("SELECT value FROM volume_test;") == "changed"
        for service in ("backend", "persona-worker", "federation-worker"):
            assert compose("ps", "--status", "running", "-q", service).stdout.strip()
    finally:
        (ROOT / "data").unlink()
        parked.rename(ROOT / "data")
    # Renaming an active directory can invalidate Docker Desktop's shared mount
    # even after the pathname is restored. Release it before the valid restore.
    compose("down")
    start()
    run(["bash", str(scripts / "backup.sh"), "restore", "--from", str(backup)])
    assert sql("SELECT value FROM volume_test;") == "original"
    assert compose("exec", "-T", "backend", "cat", "/app/data/media/sentinel").stdout.strip() == "original"
    assert container("stat -c %a /data", [f"type=bind,src={ROOT / 'data'},dst=/data,readonly"]).stdout.strip() == "700"
    # Docker Desktop remaps bind ownership; actual uid-1000 access is the
    # portable assertion. Exact Linux ownership is checked above on its filesystem.
    compose("exec", "-T", "backend", "sh", "-eu", "-c", "touch /app/data/restored-write-probe; rm /app/data/restored-write-probe")

    print("Rolling back to Docker-managed volumes, preserving post-migration writes", flush=True)
    compose("exec", "-T", "backend", "sh", "-c", "echo newest >/app/data/media/sentinel")
    compose("down")
    recent = ROOT / "recent-archives"
    recent.mkdir(mode=0o700)
    for kind in ("data", "cache"):
        archive(kind, recent, bind=True)
    save(legacy)
    for kind in ("data", "cache"):
        run(["docker", "volume", "create", f"{PROJECT}_backend_{kind}"])
        archive(kind, recent, restore=True)
    provision()  # Existing legacy volumes must be preserved.
    start()
    assert compose("exec", "-T", "backend", "cat", "/app/data/media/sentinel").stdout.strip() == "newest"
    print("PASS: fresh direct binds, migration, uid 1000 writes, read-only root, worker subpaths, recreation, rejected restore, database/media restore, rollback", flush=True)
finally:
    if (ROOT / "docker-compose.yml").exists():
        compose("down", "--volumes", "--remove-orphans", check=False)
    for volume in VOLUMES:
        run(["docker", "volume", "rm", volume], check=False)
    # Test directories can now belong to uid 1000; clean through the same daemon.
    if (ROOT / "data").exists():
        container("rm -rf /fixture/data /fixture/cache /fixture/backup /fixture/old-archives /fixture/recent-archives",
                  [f"type=bind,src={ROOT},dst=/fixture"])
    TEMP.cleanup()
