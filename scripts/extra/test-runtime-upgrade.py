"""Exercise an old-to-current business-image rollout with real Docker services.

Build the backend, frontend and proxy images first. Also build the MCP fixture
from docker/mcp-tool/tests. Defaults use the local myriad-closeout image names.
This test uses only a unique Compose project, random credentials and disposable
volumes. It does not call the updater or modify an installed Myriad deployment.
"""
import argparse
import base64
import hashlib
import hmac
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


parser = argparse.ArgumentParser(description=__doc__)
for name in ("backend", "frontend", "proxy"):
    parser.add_argument("--" + name, default=f"myriad-closeout/{name}:current")
parser.add_argument("--mcp", default="myriad-closeout/mcp-fixture:current")
parser.add_argument("--legacy", default="v0.4.8")
args = parser.parse_args()
repo = Path(__file__).resolve().parents[2]
root = Path(tempfile.mkdtemp(prefix="myriad-upgrade-")).resolve()
project = "myriad-upgrade-" + secrets.token_hex(4)
with socket.socket() as port:
    port.bind(("127.0.0.1", 0))
    http_port = port.getsockname()[1]
env = {**os.environ, "MYRIAD_TAG": args.legacy, "PROXY_TAG": "current", "UPDATER_TAG": args.legacy,
       "COMPOSE_PROJECT_NAME": project,
       "BACKEND_IMAGE": "docker.io/somekawahitomi/myriad-backend",
       "FRONTEND_IMAGE": "docker.io/somekawahitomi/myriad-frontend",
       "DOCKER_GUARD_IMAGE": "unused@sha256:" + "a" * 64,
       "MYRIAD_DOCKER_NETWORK": project + "-business",
       "MYRIAD_ADMIN_NETWORK": project + "-admin",
       "MYRIAD_DOCKER_GUARD_NETWORK": project + "-guard",
       "HTTP_PORT": f"127.0.0.1:{http_port}", "ENVIRONMENT": "production",
       "CORS_ORIGINS": f"http://localhost:{http_port}",
       "MCP_TOOL_IMAGE": args.mcp,
       "MCP_CATALOG_FILE": str(repo / "docker/mcp-tool/catalog.yaml"),
       "MCP_SECCOMP_PROFILE": str(repo / "docker/mcp-tool/namespace-seccomp.json")}
secret_keys = ("POSTGRES_PASSWORD", "PERSONA_DB_PASSWORD", "FEDERATION_DB_PASSWORD",
               "JWT_SECRET", "MYRIAD_SETUP_SECRET", "GUARD_SELF_UPDATE_TOKEN",
               "UPDATE_TOKEN", "UPDATER_GATEWAY_SECRET", "MCP_GATEWAY_AUTH_TOKEN")
env.update({key: secrets.token_hex(32) for key in secret_keys})


def run(command, check=True, data=None):
    result = subprocess.run(command, input=data, capture_output=True, text=True, env=env)
    if check and result.returncode:
        raise RuntimeError(redact(result.stderr))
    return result


def redact(text):
    for key in secret_keys:
        text = text.replace(env[key], "[REDACTED]")
    return text


def resolve(file):
    return json.loads(run(["docker", "compose", "--env-file", "/dev/null",
                           "--project-directory", str(root), "-f", str(repo / file),
                           "config", "--format", "json"]).stdout)


config = resolve("docker-compose.yml")
keep = ("postgres", "backend-volume-init", "backend", "frontend", "proxy",
        "persona-worker", "federation-worker")
config["services"] = {key: config["services"][key] for key in keep}
for service in config["services"].values():
    service.pop("container_name", None)
    if "healthcheck" in service:
        service["healthcheck"].update(interval="3s", start_interval="1s")
# Keep database writes in a disposable named volume, never a host installation.
config["services"]["postgres"]["volumes"] = ["test_pgdata:/var/lib/postgresql"]
config.setdefault("volumes", {})["test_pgdata"] = {}
# Use the production direct-bind layout in a disposable deployment directory.
for kind in ("data", "cache"):
    (root / kind).mkdir(mode=0o700)
(root / "state").mkdir()
config["services"]["proxy"]["image"] = args.proxy
config["services"]["backend-volume-init"]["image"] = args.backend
for domain in ("PERSONA", "FEDERATION"):
    config["services"]["proxy"]["environment"][f"PROXY_{domain}_UPSTREAM"] = "http://backend:1103"
gateway = resolve("docs/deployment/examples/docker-compose.mcp-gateway.example.yml")
gateway["services"]["gateway"].pop("ports", None)
gateway["services"]["gateway"]["networks"] = ["mcp", "myriad-net"]
config["services"].update(gateway["services"])
config["networks"]["mcp"] = {"internal": True}
config["services"]["persona-worker"]["environment"].update(
    MYRIAD_MCP_GATEWAY_URL="http://gateway:8811/mcp",
    MYRIAD_MCP_GATEWAY_TOKEN=env["MCP_GATEWAY_AUTH_TOKEN"])
compose_file = root / "compose.json"


def save():
    compose_file.write_text(json.dumps(config))
    compose_file.chmod(0o600)


def cp(*arguments, check=True):
    return run(["docker", "compose", "--env-file", "/dev/null", "-p", project,
                "-f", str(compose_file), *arguments], check=check)


def sql(query):
    return cp("exec", "-T", "postgres", "psql", "-U", "myriad", "-d", "myriad",
              "-At", "-v", "ON_ERROR_STOP=1", "-c", query).stdout.strip()


def http(path, body=None, token=None, method=None, grant=None):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = "Bearer " + token
    if grant:
        headers["X-Tapp-Runtime-Grant"] = grant
    request = urllib.request.Request(f"http://127.0.0.1:{http_port}" + path,
                                     data=json.dumps(body).encode() if body is not None else None,
                                     headers=headers, method=method)
    try:
        with urllib.request.urlopen(request, timeout=40) as response:
            return response.status, response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.read()


def healthy(service):
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        container = cp("ps", "-aq", service).stdout.strip()
        if container:
            state = json.loads(run(["docker", "inspect", "--format", "{{json .State}}", container]).stdout)
            if state.get("Health", {}).get("Status") == "healthy":
                return
            if state["Status"] == "exited":
                raise AssertionError(f"{service} exited: {state['ExitCode']}")
        time.sleep(1)
    raise AssertionError(service + " health timed out")


def jwt(user):
    def encode(value):
        return base64.urlsafe_b64encode(json.dumps(value).encode()).decode().rstrip("=")
    now = int(time.time())
    data = encode({"alg": "HS256", "typ": "JWT"}) + "." + encode(
        {"sub": user, "username": "upgrade-test", "is_admin": True, "is_owner": True,
         "exp": now + 3600, "iat": now, "tv": 0})
    signature = base64.urlsafe_b64encode(hmac.new(env["JWT_SECRET"].encode(), data.encode(), hashlib.sha256).digest())
    return data + "." + signature.decode().rstrip("=")


save()
results = {}
try:
    cp("up", "-d", "postgres", "backend-volume-init", "backend", "frontend", "proxy")
    healthy("backend")
    healthy("frontend")
    assert http("/api/config/public")[0] == 200
    assert http("/")[0] == 200
    user = sql("INSERT INTO users(username,is_admin,is_owner) VALUES ('upgrade-test',true,true) RETURNING id").splitlines()[0]
    token = jwt(user)
    sql("CREATE TABLE upgrade_canary(value text); INSERT INTO upgrade_canary VALUES ('before-upgrade')")
    print("PASS legacy image serves homepage/API and seeded data", flush=True)
    cp("stop", "backend", "frontend")
    for service in ("backend", "persona-worker", "federation-worker"):
        config["services"][service]["image"] = args.backend
    config["services"]["frontend"]["image"] = args.frontend
    for domain in ("persona", "federation"):
        config["services"]["proxy"]["environment"][f"PROXY_{domain.upper()}_UPSTREAM"] = f"http://{domain}-worker:1103"
    save()
    cp("up", "-d")
    for service in ("backend", "frontend", "persona-worker", "federation-worker"):
        healthy(service)
    assert sql("SELECT value FROM upgrade_canary") == "before-upgrade"
    for domain in ("persona", "federation"):
        health = json.loads(cp("exec", "-T", domain + "-worker", "wget", "-qO-", "http://localhost:1103/health").stdout)
        assert health["ready"] and health["role"] == domain + "-worker", health
        assert int(sql(f"SELECT count(*) FROM pg_stat_activity WHERE usename='myriad_{domain}'")) > 0
    results["old_to_current_upgrade_and_worker_logins"] = True
    status, raw = http("/api/agent/mcp/config", token=token)
    assert status == 200, (status, raw)
    assert json.loads(raw)["runtime_policy"] == {"local_stdio_allowed": False, "gateway_configured": True}
    assert env["MCP_GATEWAY_AUTH_TOKEN"].encode() not in raw
    local = {"id": "forbidden", "transport": "stdio", "enabled": True, "command": "/bin/true"}
    assert http("/api/agent/mcp/config", {"servers": [local]}, token, "PUT")[0] == 400
    assert http("/api/agent/mcp/config", {"servers": [{"id": "gateway", "transport": "gateway", "enabled": True}]}, token, "PUT")[0] == 200
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        status, raw = http("/api/agent/mcp/status", token=token)
        snapshot = json.loads(raw)
        if status == 200 and snapshot.get("tool_count") == 7:
            break
        time.sleep(.5)
    else:
        raise AssertionError(snapshot)
    assert snapshot["servers"][0]["healthy"], snapshot
    results["production_mcp_discovery_and_stdio_denial"] = True
    print("PASS current web/workers and real sandboxed MCP gateway", flush=True)
    # Runtime grant and data operations must work while both background domains stop.
    cp("stop", "persona-worker", "federation-worker")
    manifest = {"id": "com.example.upgrade-probe", "name": "Upgrade probe", "version": "1.0.0",
                "category": "game", "core": {"entry": "core.js"}, "page": {"entry": "page.js"},
                "permissions": ["storage:read", "storage:write"]}
    status, raw = http("/api/tapps/install", {"source": "direct", "manifest": manifest,
                       "modules": {"core.js": "module.exports={};", "page.js": "module.exports={};"},
                       "permissions": manifest["permissions"]}, token)
    assert status == 200, (status, raw)
    base = "/api/tapps/com.example.upgrade-probe"
    status, raw = http(base + "/runtime-grants", {"instanceId": "upgrade-test", "kind": "page"}, token)
    assert status == 200, (status, raw)
    grant = json.loads(raw)["token"]
    assert http(base + "/storage/canary", {"survived": True}, token, grant=grant)[0] == 200
    status, raw = http(base + "/storage/canary", token=token, grant=grant)
    assert status == 200 and b'"survived":true' in raw, (status, raw)
    assert http("/")[0] == 200 and http("/api/config/public")[0] == 200
    results["homepage_tapp_grant_and_storage_with_workers_stopped"] = True
    cp("start", "persona-worker", "federation-worker")
    healthy("persona-worker")
    healthy("federation-worker")
    # Recreating the worker must reconnect with its own bounded role and load MCP config.
    cp("up", "-d", "--force-recreate", "persona-worker")
    healthy("persona-worker")
    status, raw = http("/api/agent/mcp/status", token=token)
    assert status == 200 and json.loads(raw)["tool_count"] == 7, (status, raw)
    assert http("/api/agent/mcp/config", {"servers": []}, token, "PUT")[0] == 200
    status, raw = http("/api/agent/mcp/status", token=token)
    assert status == 200 and json.loads(raw)["tool_count"] == 0, (status, raw)
    results["worker_recreation_and_mcp_revocation"] = True
    (root / "results.json").write_text(json.dumps(results, indent=2) + "\n")
    print("PASS", json.dumps(results), flush=True)
finally:
    (root / "containers.log").write_text(redact(cp("logs", "--no-color", check=False).stdout))
    cp("down", "-v", "--remove-orphans")
    run(["docker", "run", "--rm", "--network", "none",
         "--mount", f"type=bind,src={root},dst=/fixture", "alpine:3.20",
         "sh", "-c", "rm -rf /fixture/data /fixture/cache"])
    compose_file.unlink(missing_ok=True)
    print("Redacted test evidence:", root, flush=True)
