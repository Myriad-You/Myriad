"""Exercise bootstrap, release assembly and TAG-first Compose selection.

Run: python3 scripts/extra/test-deployment-compatibility.py
Requires bash and jq (available on the release runner). Compose selection tests
also use the Docker Compose CLI, but never connect to a daemon or pull images.
"""
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parents[2]
# v0.4.10 first shipped both split-worker routing and updater capability gates.
ROUTING_FLOOR = (0, 4, 10)
STORAGE_FLOOR = (0, 5, 8)


def version(value):
    match = re.fullmatch(r"v(\d+)\.(\d+)\.(\d+)", value)
    if not match:
        raise AssertionError(f"Expected a pinned release, got {value!r}")
    return tuple(map(int, match.groups()))


def env_values(path):
    return dict(line.split("=", 1) for line in path.read_text().splitlines()
                if line and not line.startswith("#") and "=" in line)


class DeploymentCompatibility(unittest.TestCase):
    def bootstrap(self, directory, initial="", template=None):
        root = Path(directory)
        script_path = root / "scripts/extra/deploy.sh"
        script_path.parent.mkdir(parents=True)
        script = (ROOT / "scripts/extra/deploy.sh").read_text()
        # Load real shell functions, stopping before dispatch (which requires Docker).
        library = script.split("COMPOSE_KIND=$(detect_compose)", 1)[0]
        script_path.write_text(library + "\nensure_current_layout\n")
        (root / ".env").write_text(initial)
        (root / ".env.production.example").write_text(
            template or (ROOT / ".env.production.example").read_text())
        subprocess.run(["bash", str(script_path)], check=True, capture_output=True, text=True)
        return env_values(root / ".env")

    def test_bootstrap_defaults_support_split_worker_routes(self):
        with tempfile.TemporaryDirectory() as directory:
            values = self.bootstrap(directory)
        self.assertGreaterEqual(version(values["UPDATER_TAG"]), STORAGE_FLOOR)
        for key in ["PROXY_TAG", "UPDATER_TAG"]:
            with self.subTest(key=key):
                self.assertGreaterEqual(version(values[key]), ROUTING_FLOOR)

    def test_bootstrap_reads_release_defaults_from_template(self):
        template = "MYRIAD_TAG=v1.2.3\nPROXY_TAG=v1.1.0\nUPDATER_TAG=v1.0.0\n"
        with tempfile.TemporaryDirectory() as directory:
            values = self.bootstrap(directory, template=template)
        self.assertEqual(values["MYRIAD_TAG"], "v1.2.3")
        self.assertEqual(values["PROXY_TAG"], "v1.1.0")
        self.assertEqual(values["UPDATER_TAG"], "v1.0.0")

    def test_bootstrap_preserves_operator_pins(self):
        with tempfile.TemporaryDirectory() as directory:
            values = self.bootstrap(directory, "MYRIAD_TAG=dev-abcdef1\nPROXY_TAG=v0.4.13\nUPDATER_TAG=v0.4.13\n")
        self.assertEqual(values["MYRIAD_TAG"], "dev-abcdef1")
        self.assertEqual(values["PROXY_TAG"], "v0.4.13")
        self.assertEqual(values["UPDATER_TAG"], "v0.4.13")

    def test_guard_policy_bootstrap_resolves_tag_instead_of_stale_pin(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script_path = root / "scripts/extra/deploy.sh"
            script_path.parent.mkdir(parents=True)
            (root / "guard-policy").mkdir()
            script = (ROOT / "scripts/extra/deploy.sh").read_text()
            library = script.split("COMPOSE_KIND=$(detect_compose)", 1)[0]
            digest = "docker.io/somekawahitomi/myriad-updater@sha256:" + "b" * 64
            mock = f'''
docker() {{
    printf '%s\\n' "$*" >> docker.calls
    case "$*" in
        "pull docker.io/somekawahitomi/myriad-updater:v0.4.14") return 0 ;;
        "image inspect --format "*) printf '%s\\n' '{digest}' ;;
        *) return 1 ;;
    esac
}}
seed_guard_policy_from_env
'''
            script_path.write_text(library + mock)
            (root / ".env").write_text(
                "UPDATER_TAG=v0.4.14\nDOCKER_GUARD_IMAGE=old-pin\n"
                "GUARD_SELF_UPDATE_TOKEN=test-only-bootstrap-token-00000000\n")
            subprocess.run(["bash", str(script_path)], check=True, capture_output=True, text=True)
            self.assertEqual(env_values(root / "guard-policy/docker-guard.env")["DOCKER_GUARD_IMAGE"], digest)
            calls = (root / "docker.calls").read_text()
            self.assertNotIn("old-pin", calls)
            self.assertIn("pull docker.io/somekawahitomi/myriad-updater:v0.4.14", calls)

    def test_release_requires_updater_with_edge_capability_checks(self):
        workflow = (ROOT / ".github/workflows/release.yml").read_text()
        step = workflow.split("      - name: Assemble release.json\n", 1)[1].split("\n      - name:", 1)[0]
        script = textwrap.dedent(step.split("        run: |\n", 1)[1])
        for expression, value in {
            "${{ env.IMAGE_NAMESPACE }}": "fixture",
            "${{ env.REGISTRY }}": "docker.io",
            "${{ github.repository }}": "fixture/myriad",
        }.items():
            script = script.replace(expression, value)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "_digests").mkdir()
            # Release assembly now also renders notes; supply its real helper.
            (root / "scripts/extra").mkdir(parents=True)
            (root / "scripts/extra/release-notes.py").write_text(
                (ROOT / "scripts/extra/release-notes.py").read_text())
            # No infra release: compatibility requirement must survive independent cadence.
            for component in ["backend", "frontend"]:
                (root / f"_digests/{component}.digest").write_text("sha256:" + "0" * 64)
            subprocess.run(["bash", "-c", script], cwd=root, check=True, capture_output=True,
                           env={**os.environ, "VERSION": "v0.4.99", "CHANNEL": "stable", "COMMIT_SHA": "a" * 40})
            manifest = json.loads((root / "release.json").read_text())
        self.assertGreaterEqual(version(manifest["updater"]["min_updater_version"]), STORAGE_FLOOR)
        self.assertNotIn("proxy", manifest["images"])
        self.assertNotIn("updater", manifest["images"])

    def test_doctor_verifies_actual_tag_image_instead_of_requiring_a_pin_selector(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script_path = root / "scripts/extra/deploy.sh"
            script_path.parent.mkdir(parents=True)
            library = (ROOT / "scripts/extra/deploy.sh").read_text().split("COMPOSE_KIND=$(detect_compose)", 1)[0]
            image_id = "sha256:" + "a" * 64
            digest = "docker.io/somekawahitomi/myriad-updater@sha256:" + "b" * 64
            checks = f'''
docker() {{
    case "$4" in
        "{{{{.Id}}}}") printf '%s\\n' '{image_id}' ;;
        *) printf '%s\\n' '{digest}' ;;
    esac
}}
guard_image_matches_selection 'docker.io/somekawahitomi/myriad-updater:v0.4.14' '{image_id}'
guard_image_matches_selection '{digest}' '{image_id}'
if guard_image_matches_selection 'docker.io/somekawahitomi/myriad-updater:v0.4.14' 'sha256:{"c" * 64}'; then exit 10; fi
if guard_image_matches_selection 'evil.example/updater:v0.4.14' '{image_id}'; then exit 11; fi
if guard_image_matches_selection 'docker.io/somekawahitomi/myriad-updater:latest' '{image_id}'; then exit 12; fi
'''
            script_path.write_text(library + checks)
            subprocess.run(["bash", str(script_path)], check=True, capture_output=True, text=True)


class ComposeTagSelection(unittest.TestCase):
    templates = ("docker-compose.yml", "docs/deployment/examples/docker-compose.external-db.example.yml")

    @classmethod
    def setUpClass(cls):
        try:
            subprocess.run(["docker", "compose", "version"], check=True, capture_output=True)
        except (OSError, subprocess.CalledProcessError):
            raise unittest.SkipTest("Docker Compose CLI unavailable; daemon not required")

    def resolve(self, directory, template, tag):
        root = Path(directory)
        values = {
            "COMPOSE_PROJECT_NAME": "custom-site",
            "MYRIAD_TAG": "v0.4.14", "PROXY_TAG": "v0.3.32", "UPDATER_TAG": tag,
            "DOCKER_GUARD_IMAGE": "docker.io/somekawahitomi/myriad-updater@sha256:" + "a" * 64,
            "UPDATER_IMAGE_REF": "docker.io/somekawahitomi/myriad-updater@sha256:" + "b" * 64,
            "UPDATER_GATEWAY_IMAGE_REF": "docker.io/somekawahitomi/myriad-updater@sha256:" + "c" * 64,
            "DATABASE_URL": "postgres://test:test@db:5432/test",
            "PERSONA_DATABASE_URL": "postgres://test:test@db:5432/test",
            "FEDERATION_DATABASE_URL": "postgres://test:test@db:5432/test",
        }
        for key in ("PERSONA_DB_PASSWORD", "FEDERATION_DB_PASSWORD", "POSTGRES_PASSWORD", "JWT_SECRET",
                    "MYRIAD_SETUP_SECRET", "GUARD_SELF_UPDATE_TOKEN", "UPDATE_TOKEN", "UPDATER_GATEWAY_SECRET"):
            values[key] = "test-only-no-real-credentials"
        (root / ".env").write_text("".join(f"{key}={value}\n" for key, value in values.items()))
        (root / "guard.env").write_text(f"DOCKER_GUARD_IMAGE={values['DOCKER_GUARD_IMAGE']}\n")
        env = {key: os.environ[key] for key in ("PATH", "HOME", "TMPDIR", "SYSTEMROOT") if key in os.environ}
        return subprocess.run([
            "docker", "compose", "--project-directory", str(root),
            "--env-file", str(root / ".env"), "--env-file", str(root / "guard.env"),
            "-f", str(ROOT / template), "config", "--no-env-resolution", "--format", "json",
        ], capture_output=True, text=True, env=env)

    def test_only_changing_tag_changes_all_tcb_images_despite_old_pins(self):
        for template in self.templates:
            with self.subTest(template=template), tempfile.TemporaryDirectory() as directory:
                for tag in ("v0.4.6", "v0.4.14", "dev-abcdef1"):
                    result = self.resolve(directory, template, tag)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    services = json.loads(result.stdout)["services"]
                    target = f"docker.io/somekawahitomi/myriad-updater:{tag}"
                    for service in ("docker-guard", "updater", "updater-gateway"):
                        self.assertEqual(services[service]["image"], target)
                        self.assertNotIn("MYRIAD_VERSION", services[service].get("environment", {}))
                    self.assertEqual(services["docker-guard"]["environment"]["DOCKER_GUARD_EXPECTED_IMAGE"], target)
                    self.assertEqual(services["backend"]["image"], "docker.io/somekawahitomi/myriad-backend:v0.4.14")
                    self.assertEqual(services["proxy"]["image"], "docker.io/somekawahitomi/myriad-proxy:v0.3.32")

    def test_missing_tag_does_not_silently_fall_back_to_old_pin(self):
        for template in self.templates:
            with self.subTest(template=template), tempfile.TemporaryDirectory() as directory:
                result = self.resolve(directory, template, "")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("UPDATER_TAG", result.stderr)

    def test_direct_binds_preserve_worker_mount_contract(self):
        for template in self.templates:
            with self.subTest(template=template), tempfile.TemporaryDirectory() as directory:
                result = self.resolve(directory, template, "v0.5.8")
                self.assertEqual(result.returncode, 0, result.stderr)
                model = json.loads(result.stdout)
                self.assertNotIn("backend_data", model.get("volumes", {}))
                for service in ("backend", "backend-volume-init", "persona-worker"):
                    mounts = model["services"][service]["volumes"]
                    for kind in ("data", "cache"):
                        mount = next(m for m in mounts if m["target"] == f"/app/{kind}")
                        self.assertEqual(mount["type"], "bind")
                        self.assertEqual(Path(mount["source"]).resolve(), Path(directory).resolve() / kind)
                        self.assertFalse(mount["bind"]["create_host_path"])
                mounts = model["services"]["federation-worker"]["volumes"]
                self.assertEqual(len(mounts), 5)
                for mount in mounts:
                    self.assertEqual(mount["type"], "bind")
                    self.assertFalse(mount["bind"]["create_host_path"])
                    self.assertEqual(mount.get("read_only", False), mount["target"] == "/app/data")


class BackendStorage(unittest.TestCase):
    def exercise(self, layout="bind", failure=None, caller="deploy", unsafe=None, old_volumes=False):
        with tempfile.TemporaryDirectory(prefix="myriad storage test ") as directory:
            root = Path(directory).resolve()
            scripts = root / "scripts/extra"
            scripts.mkdir(parents=True)
            for name in ("backend-storage.sh", "backup.sh"):
                (scripts / name).write_text((ROOT / "scripts/extra" / name).read_text())
            original_env = "COMPOSE_PROJECT_NAME=custom-site\nPOSTGRES_PASSWORD=test-only-password\nMARKER=original\n"
            (root / ".env").write_text(original_env)
            model = {"services": {"backend": {"volumes": []}}, "volumes": {}}
            for kind in ("data", "cache"):
                model["services"]["backend"]["volumes"].append({
                    "type": "volume" if layout.startswith("legacy") else "bind",
                    "source": f"backend_{kind}" if layout.startswith("legacy") else str(root / kind),
                    "target": f"/app/{kind}", "bind": {"create_host_path": False}})
                model["volumes"][f"backend_{kind}"] = {"name": f"custom-site_backend_{kind}"}
            if layout == "foreign":
                model["services"]["backend"]["volumes"][0]["source"] = "/other/data"
            if ((caller != "deploy" and not layout.startswith("legacy")) or layout == "legacy-bind") and unsafe != "missing":
                (root / "data").mkdir()
                (root / "cache").mkdir()
            if unsafe in ("symlink", "dangling", "file"):
                target = root / "data"
                if target.is_dir(): target.rmdir()
                if unsafe == "file":
                    target.write_text("keep")
                else:
                    outside = root / "outside"
                    if unsafe == "symlink": outside.mkdir()
                    target.symlink_to(outside, target_is_directory=True)
            for kind in ("data", "cache"):
                options = {"type":"none", "o":"bind", "device":str(root / kind)} if layout == "legacy-bind" else None
                (root / f"inspect-{kind}.json").write_text(json.dumps([{"Driver":"local", "Options":options}]))
            (root / "model.json").write_text(json.dumps(model))
            (root / "volumes.txt").write_text("custom-site_backend_data\ncustom-site_backend_cache\n"
                if layout.startswith("legacy") or old_volumes else "")
            mock = root / "docker"
            mock.write_text('''#!/usr/bin/env python3
import json, os, sys, subprocess, shlex
from pathlib import Path
root = Path(os.environ["STORAGE_TEST_ROOT"])
args = sys.argv[1:]
with (root / "calls").open("a") as f: f.write(json.dumps(args) + "\\n")
failure = os.environ.get("STORAGE_TEST_FAILURE")
if (args[0] == "compose" and failure == "config") or (args[0] == "volume" and args[1] == failure) or (args[0] == "run" and failure == "run"): sys.exit(1)
if failure == "probe" and args[0] == "run" and any(a.endswith(",dst=/data") for a in args): sys.exit(1)
if args[0] == "compose": print((root / "model.json").read_text())
elif args[:2] == ["volume", "ls"]: print((root / "volumes.txt").read_text())
elif args[:2] == ["volume", "inspect"]: print((root / ("inspect-" + args[2].rsplit("_", 1)[1] + ".json")).read_text())
elif args[0] == "run":
    if args[-2:] == ["storage", "prepare"]:
        program = args[args.index("-c") + 1]
        program = program.replace("/app/data", shlex.quote(str(root / "data"))).replace("/app/cache", shlex.quote(str(root / "cache")))
        # Plain named volumes live outside this host fixture; actual volume
        # preparation is exercised by the real Docker smoke.
        if (root / "data").is_dir():
            sys.exit(subprocess.run(["sh", "-eu", "-c", program, "storage", "prepare"]).returncode)
else: sys.exit(2)
''')
            mock.chmod(0o755)
            if caller == "deploy":
                script = (ROOT / "scripts/extra/deploy.sh").read_text().split("COMPOSE_KIND=$(detect_compose)", 1)[0]
                script += "\nensure_backend_volume_perms\n"
            else:
                archive = root / "archive"
                archive.mkdir()
                (archive / "env").write_text(original_env.replace("original", "restored"))
                for name in ("postgres.dump", "backend_data.tar.gz"): (archive / name).touch()
                script = '''source "$(dirname "$0")/backup.sh"
quiesce_writers() { printf '["stop"]\\n' >> "$ROOT/calls"; }
compose() { printf '["compose-action", "%s"]\\n' "$1" >> "$ROOT/calls"; }
wait_backend_ready() { :; }
do_restore --from "$ROOT/archive"
'''
            entry = scripts / "exercise.sh"
            entry.write_text(script)
            result = subprocess.run(["bash", str(entry)], capture_output=True, text=True,
                env={**os.environ, "PATH": str(root) + os.pathsep + os.environ["PATH"], "DOCKER": str(mock),
                     "COMPOSE_PROJECT_NAME": "", "STORAGE_TEST_ROOT": str(root), "STORAGE_TEST_FAILURE": failure or ""})
            calls = [json.loads(line) for line in (root / "calls").read_text().splitlines()] if (root / "calls").exists() else []
            if caller == "deploy" and result.returncode == 0 and layout == "bind":
                for relative in ("data/media", "data/federation", "data/federation_media", "cache/images"):
                    self.assertTrue((root / relative).is_dir(), f"worker bind missing before Compose create: {relative}")
            if caller == "restore" and result.returncode:
                self.assertEqual((root / ".env").read_text(), original_env)
                self.assertFalse((root / ".env.bak.restore").exists())
                self.assertFalse(any(c[0] in ("stop", "compose-action") for c in calls))
            return result, calls, str(root)

    def test_fresh_deploy_uses_bind_directories_without_creating_volumes(self):
        result, calls, root = self.exercise()
        self.assertEqual(result.returncode, 0, result.stderr)
        runs = [c for c in calls if c[0] == "run"]
        self.assertEqual(len(runs), 3)
        self.assertIn(f"type=bind,src={root}/data,dst=/app/data", runs[1])
        self.assertFalse(any(c[:2] == ["volume", "create"] for c in calls))

    def test_legacy_deploy_keeps_existing_named_volume_sources(self):
        result, calls, _ = self.exercise("legacy")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("type=volume,src=custom-site_backend_data,volume-nocopy,dst=/app/data",
                      next(c for c in calls if c[0] == "run"))

    def test_host_deploy_cannot_silently_switch_an_existing_install_to_empty_binds(self):
        result, calls, _ = self.exercise(old_volumes=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(c[0] == "run" for c in calls))

    def test_unsafe_paths_and_api_failures_fail_before_repairs(self):
        for layout, failure, unsafe in [("foreign", None, None), ("bind", "config", None),
                ("bind", "ls", None), ("legacy", "inspect", None),
                *[(layout, None, u) for layout in ("bind", "legacy-bind") for u in ("symlink", "dangling", "file")]]:
            with self.subTest(layout=layout, failure=failure, unsafe=unsafe):
                result, calls, _ = self.exercise(layout, failure, unsafe=unsafe)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(any(c[0] == "run" for c in calls))

    def test_restore_rejects_before_stopping_or_replacing_env_and_database(self):
        for layout, failure, unsafe in [("foreign", None, None), ("legacy", "inspect", None),
                ("bind", "run", None), ("bind", "probe", None), ("bind", None, "symlink"), ("bind", None, "missing")]:
            with self.subTest(layout=layout, failure=failure, unsafe=unsafe):
                result, _, _ = self.exercise(layout, failure, "restore", unsafe)
                self.assertNotEqual(result.returncode, 0)

    def test_restore_probe_precedes_stop_for_both_layouts(self):
        for layout in ("bind", "legacy", "legacy-bind"):
            result, calls, _ = self.exercise(layout, caller="restore")
            self.assertEqual(result.returncode, 0, result.stderr)
            probe = next(i for i, c in enumerate(calls)
                         if c[0] == "run" and any(a.endswith(",dst=/data") for a in c))
            self.assertLess(probe, calls.index(["stop"]))
            self.assertLess(probe, calls.index(["compose-action", "exec"]))

    def test_symlinked_parent_and_dot_segments_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / "real").mkdir()
            (root / "link").symlink_to(root / "real", target_is_directory=True)
            for path in (str(root / "link/data"), f"{root}/real/../data", f"{root}/real/.."):
                result = subprocess.run(["bash", "-c", 'source "$1"; validate_backend_directory "$2"',
                    "test", str(ROOT / "scripts/extra/backend-storage.sh"), path], capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)

    def test_restore_protects_root_permissions_on_success_and_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            old, dest = root / "old", root / "restored"
            old.mkdir(mode=0o755)
            old.chmod(0o755)
            (old / "media").write_text("keep")
            dest.mkdir(mode=0o700)
            archive = root / "data.tar.gz"
            subprocess.run(["tar", "czf", str(archive), "-C", str(old), "."], check=True)
            for valid in (True, False):
                if not valid:
                    archive.write_bytes(b"invalid archive")
                    dest.chmod(0o755)
                result = subprocess.run(["bash", "-c",
                    'source "$1" help; restore_data_tree "$2" "$3"', "restore",
                    str(ROOT / "scripts/extra/backup.sh"), str(dest), str(archive)],
                    capture_output=True, text=True)
                self.assertEqual(result.returncode == 0, valid, result.stderr)
                self.assertEqual(dest.stat().st_mode & 0o777, 0o700)
                if valid:
                    self.assertEqual((dest / "media").read_text(), "keep")


if __name__ == "__main__":
    unittest.main()
