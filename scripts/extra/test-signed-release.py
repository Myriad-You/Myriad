"""Real signed-release rollout, explicit rollback and injected-failure recovery.

Uses published business images and real GitHub/Fulcio/Rekor verification with
COSIGN_VERIFY=strict. Build the current updater/Guard in debug mode first:
  docker build --build-arg CARGO_PROFILE=dev --build-arg MYRIAD_VERSION=v0.5.8 \
    -t myriad-updater-dev:storage-rehearsal -f updater/Dockerfile .
  python3 scripts/extra/test-signed-release.py --out /tmp/myriad-release-evidence

Requires local Docker/Compose, network access, and no installed Myriad containers
on this daemon (production uses fixed container names). Only the Guard identity
bootstrap uses its existing debug mode; business signatures/digests and Guard
mount policy stay enabled. A test-only forwarding proxy can reject one Docker
request to exercise automatic recovery. No release/tag is published.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import secrets
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.request

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--from-version', default='v0.5.6')
parser.add_argument('--to-version', default='v0.5.7')
parser.add_argument('--updater-image', default='myriad-updater-dev:storage-rehearsal')
parser.add_argument('--out', required=True, type=Path)
args = parser.parse_args()
REPO = Path(__file__).resolve().parents[2]
OUT = args.out.resolve()
OUT.mkdir(mode=0o700, parents=True, exist_ok=False)
ROOT = Path(tempfile.mkdtemp(prefix='myriad-signed-release-')).resolve()
PROJECT = 'myriad-release-' + secrets.token_hex(5)
ENV = {key: value for key, value in os.environ.items()
       if key not in ('COMPOSE_FILE', 'COMPOSE_PROJECT_NAME')}
SECRET_KEYS = ('POSTGRES_PASSWORD', 'PERSONA_DB_PASSWORD', 'FEDERATION_DB_PASSWORD',
               'JWT_SECRET', 'MYRIAD_SETUP_SECRET', 'GUARD_SELF_UPDATE_TOKEN',
               'UPDATE_TOKEN', 'UPDATER_GATEWAY_SECRET')
VALUES = {key: secrets.token_hex(32) for key in SECRET_KEYS}
RESULTS = {'from': args.from_version, 'to': args.to_version, 'checks': {},
           'scope': 'real signed business release; locally built debug updater/Guard bootstrap'}
CREATED = False


def redact(text):
    for key in SECRET_KEYS:
        text = text.replace(VALUES[key], '[REDACTED]')
    return text


def run(command, check=True):
    result = subprocess.run(command, cwd=ROOT, env=ENV, capture_output=True, text=True, timeout=900)
    if check and result.returncode:
        raise RuntimeError(redact(result.stderr))
    return result


def cp(*command, file='docker-compose.yml', check=True):
    return run(['docker', 'compose', '--env-file', str(ROOT / '.env'), '-p', PROJECT,
                '-f', str(ROOT / file), *command], check)


def free_port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


def api(path, body=None):
    headers = {'X-Update-Token': VALUES['UPDATE_TOKEN']}
    data = None if body is None else json.dumps(body).encode()
    if data is not None:
        headers['Content-Type'] = 'application/json'
    request = urllib.request.Request(f'http://127.0.0.1:{UPDATER_PORT}{path}', data=data, headers=headers)
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.load(response)


def wait_job(job_id, expected):
    deadline = time.monotonic() + 600
    previous = None
    while time.monotonic() < deadline:
        job = api('/jobs/' + job_id)
        phases = [(step['phase'], step.get('ok')) for step in job.get('steps', [])]
        progress = (job['status'], phases)
        if progress != previous:
            print('Job:', progress, flush=True)
            previous = progress
        if job['status'] in ('succeeded', 'failed', 'needs_manual'):
            (OUT / f'{job_id}.json').write_text(redact(json.dumps(job, indent=2)))
            assert job['status'] == expected, redact(json.dumps(job))
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                status = api('/status')
                if not status['maintenance_active'] and status.get('job_in_flight') is None:
                    return job
                time.sleep(.5)
            raise AssertionError('job terminated but maintenance/active state did not clear')
        time.sleep(1)
    raise TimeoutError('update job exceeded ten minutes')


def update(expected='succeeded'):
    return wait_job(api('/update', {'target_version': args.to_version, 'mode': 'release',
        'allow_compose_override': True, 'confirm_risk': True})['job_id'], expected)


def sql(query):
    return cp('exec', '-T', 'postgres', 'psql', '-U', 'myriad', '-d', 'myriad', '-At',
              '-v', 'ON_ERROR_STOP=1', '-c', query).stdout.strip()


def media():
    return cp('exec', '-T', 'backend', 'cat', '/app/data/media/release-canary').stdout.strip()


def assert_layout():
    for service in ('backend', 'persona-worker', 'federation-worker'):
        inspection = json.loads(run(['docker', 'inspect', 'myriad-' + service]).stdout)[0]
        mounts = inspection['Mounts']
        data = next(m for m in mounts if m['Destination'] == '/app/data')
        assert data['Type'] == 'bind' and data['Source'] == str(ROOT / 'data'), data
        assert data['RW'] == (service != 'federation-worker')
    names = run(['docker', 'volume', 'ls', '--format', '{{.Name}}']).stdout.splitlines()
    assert not any(f'{PROJECT}_backend_{kind}' in names for kind in ('data', 'cache'))


def assert_healthy(version):
    deadline = time.monotonic() + 120
    while True:
        pending = {}
        for service in ('backend', 'frontend', 'persona-worker', 'federation-worker'):
            inspection = json.loads(run(['docker', 'inspect', 'myriad-' + service]).stdout)[0]
            state = inspection['State']
            if service == 'federation-worker' and not state['Running'] and state['ExitCode'] == 0:
                continue  # host-location gate can deliberately stop this worker
            if not state['Running'] or state.get('Health', {}).get('Status') != 'healthy':
                pending[service] = {key: state.get(key) for key in ('Status', 'ExitCode', 'Health')}
        if not pending:
            break
        assert time.monotonic() < deadline, redact(json.dumps(pending))
        time.sleep(1)
    body = cp('exec', '-T', 'backend', 'wget', '-qO-', 'http://localhost:1103/health').stdout
    health = json.loads(body)
    assert health['version'] == version, health
    assert all(health[key] for key in ('db_connected', 'migrations_applied', 'routes_full', 'storage_writable'))
    assert_layout()


try:
    assert args.updater_image.startswith('myriad-updater-dev:'), 'use a locally built debug Guard image'
    names = run(['docker', 'ps', '-a', '--format', '{{.Names}}']).stdout.splitlines()
    reserved = ('myriad-backend', 'myriad-frontend', 'myriad-postgres', 'myriad-persona-worker',
                'myriad-federation-worker', 'myriad-proxy', 'myriad-updater', 'myriad-updater-gateway',
                'myriad-docker-guard', 'myriad-backend-volume-init')
    assert not set(names).intersection(reserved) and not any(n.startswith('myriad-tcb-') for n in names), \
        'an installed Myriad uses this daemon; run the rehearsal on an isolated daemon'
    RESULTS['updater_image_id'] = json.loads(run(['docker', 'image', 'inspect', args.updater_image]).stdout)[0]['Id']
    RESULTS['engine'] = run(['docker', 'info', '--format', '{{.ServerVersion}}']).stdout.strip()
    RESULTS['compose'] = run(['docker', 'compose', 'version', '--short']).stdout.strip()
    HTTP_PORT, UPDATER_PORT = free_port(), free_port()
    VALUES.update(COMPOSE_PROJECT_NAME=PROJECT, MYRIAD_TAG=args.from_version,
                  PROXY_TAG=args.to_version, UPDATER_TAG='storage-rehearsal',
                  BACKEND_IMAGE='docker.io/somekawahitomi/myriad-backend',
                  FRONTEND_IMAGE='docker.io/somekawahitomi/myriad-frontend',
                  UPDATER_IMAGE='myriad-updater-dev', MYRIAD_COMPOSE_HOST_ROOT=str(ROOT),
                  MYRIAD_DOCKER_NETWORK=PROJECT + '-business', MYRIAD_ADMIN_NETWORK=PROJECT + '-admin',
                  MYRIAD_DOCKER_GUARD_NETWORK=PROJECT + '-guard',
                  GUARD_COMPOSE_PROJECT_NAME=PROJECT,
                  GUARD_MYRIAD_DOCKER_NETWORK=PROJECT + '-business',
                  GUARD_MYRIAD_ADMIN_NETWORK=PROJECT + '-admin',
                  GUARD_MYRIAD_DOCKER_GUARD_NETWORK=PROJECT + '-guard',
                  HTTP_PORT=f'127.0.0.1:{HTTP_PORT}', CORS_ORIGINS=f'http://localhost:{HTTP_PORT}',
                  CHECK_INTERVAL_SECS='0', COSIGN_VERIFY='strict')
    for key in VALUES:
        ENV.pop(key, None)  # the fixture's .env, including later tag swaps, is authoritative
    (ROOT / '.env').write_text(''.join(f'{key}={value}\n' for key, value in VALUES.items()))
    (ROOT / '.env').chmod(0o600)
    for directory in ('state', 'pgdata', 'data', 'cache', 'guard-policy', 'fault'):
        (ROOT / directory).mkdir(mode=0o700)
    shutil.copyfile(REPO / 'scripts/extra/fixtures/guard-fault-proxy.py', ROOT / 'fault/proxy.py')
    model = json.loads(run(['docker', 'compose', '--env-file', str(ROOT / '.env'),
        '--project-directory', str(ROOT), '-p', PROJECT, '-f', str(REPO / 'docker-compose.yml'),
        'config', '--no-env-resolution', '--format', 'json']).stdout)
    for service in model['services'].values():
        service['restart'] = 'no'
    for service in ('updater', 'docker-guard', 'updater-gateway'):
        model['services'][service]['image'] = args.updater_image
    guard = model['services']['docker-guard']['environment']
    guard.update(DOCKER_GUARD_ALLOW_UNPINNED_DEV='true', DOCKER_GUARD_EXPECTED_IMAGE=args.updater_image)
    updater = model['services']['updater']
    updater['ports'] = [{'target': 1101, 'published': str(UPDATER_PORT), 'host_ip': '127.0.0.1'}]
    updater['environment'].update(DOCKER_HOST='tcp://fault-proxy:2375',
        UPDATER_DEBUG_GUARDED_DOCKER_HOST='tcp://fault-proxy:2375')
    model['services']['fault-proxy'] = {'image': 'python:3.12-alpine',
        'command': ['python3', '/fault/proxy.py'], 'volumes': [f'{ROOT / "fault"}:/fault'],
        'networks': ['myriad-docker-guard-net'], 'restart': 'no'}
    # Guard bootstrap writes the host policy before updater begins.
    for filename in ('docker-compose.yml', 'bootstrap.json'):
        (ROOT / filename).write_text(json.dumps(model))
        (ROOT / filename).chmod(0o600)
    run(['bash', '-eu', '-c', 'source "$1"; load_backend_storage docker "$2" "$PWD" prepare',
         'prepare', str(REPO / 'scripts/extra/backend-storage.sh'), PROJECT])
    CREATED = True
    print('Starting isolated real business stack:', PROJECT, flush=True)
    cp('up', '-d', '--wait', '--wait-timeout', '240')
    assert_healthy(args.from_version)
    sql("CREATE TABLE release_canary(value text); INSERT INTO release_canary VALUES ('before');")
    cp('exec', '-T', 'backend', 'sh', '-c', 'echo before >/app/data/media/release-canary')
    print('Installing signed release with strict verification', flush=True)
    job = update()
    assert_healthy(args.to_version)
    assert sql('SELECT value FROM release_canary') == 'before'
    assert media() == 'before'
    # Prove the actual updater fetched and verified a published manifest.
    logs = cp('logs', '--no-color', 'updater').stdout
    assert 'cosign verify-blob OK' in logs
    manifest_path = ROOT / f'state/cache/release-{args.to_version}.json'
    manifest = json.loads(manifest_path.read_text())
    for component in ('backend', 'frontend', 'proxy'):
        selected = manifest['images'][component]
        inspection = json.loads(run(['docker', 'image', 'inspect', selected['ref']]).stdout)[0]
        assert any(d.endswith('@' + selected['digest']) for d in inspection['RepoDigests'])
        container = json.loads(run(['docker', 'inspect', 'myriad-' + component]).stdout)[0]
        assert container['Image'] == inspection['Id']
    RESULTS['manifest_sha256'] = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
    RESULTS['checks']['signed_upgrade_and_data_preservation'] = True
    print('Rolling back through the real updater API', flush=True)
    sql("UPDATE release_canary SET value='after';")
    cp('exec', '-T', 'backend', 'sh', '-c', 'echo newest >/app/data/media/release-canary')
    wait_job(api('/rollback', {'snapshot_id': job['snapshot_id']})['job_id'], 'succeeded')
    assert_healthy(args.from_version)
    assert sql('SELECT value FROM release_canary') == 'before'
    assert media() == 'newest'  # rollback snapshots cover pgdata, not media
    RESULTS['checks']['explicit_rollback_database_and_latest_media'] = True
    print('Injecting one initializer-create failure after the tag/Compose swap', flush=True)
    (ROOT / 'fault/armed').touch()
    failed = update('failed')
    assert (ROOT / 'fault/injected').exists()
    assert any(step['phase'] == 'swap_tag' and step.get('ok') for step in failed['steps'])
    assert_healthy(args.from_version)
    assert sql('SELECT value FROM release_canary') == 'before'
    assert media() == 'newest'
    RESULTS['checks']['automatic_recovery_after_swap'] = True
    print('PASS:', json.dumps(RESULTS['checks']), flush=True)
except BaseException as error:
    RESULTS['error'] = redact(f'{type(error).__name__}: {error}')
    raise
finally:
    if CREATED:
        (OUT / 'containers.log').write_text(redact(cp('logs', '--no-color', file='bootstrap.json', check=False).stdout))
        cleanup = cp('down', '--volumes', '--remove-orphans', file='bootstrap.json', check=False)
        RESULTS['cleanup_ok'] = cleanup.returncode == 0
        if cleanup.returncode == 0:
            run(['docker', 'run', '--rm', '--network', 'none', '--mount', f'type=bind,src={ROOT},dst=/fixture',
                 'alpine:3.20', 'sh', '-c', 'rm -rf /fixture/* /fixture/.env'])
            shutil.rmtree(ROOT)
    else:
        shutil.rmtree(ROOT)
    (OUT / 'results.json').write_text(redact(json.dumps(RESULTS, indent=2)))
    print('Evidence:', OUT, flush=True)
