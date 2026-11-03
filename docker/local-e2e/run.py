"""Run the real embedded panel against isolated Docker OAuth/model fixtures."""

import argparse
import json
import os
import re
import secrets
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.request
from urllib.parse import urlsplit
from pathlib import Path


def port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def ready(url, seconds=90):
    deadline = time.monotonic() + seconds
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    while time.monotonic() < deadline:
        try:
            with opener.open(url, timeout=2) as response:
                if response.status == 200:
                    return
        except (OSError, ValueError):
            pass
        time.sleep(0.25)
    raise RuntimeError("Local test service did not become ready")


def in_repository(path):
    return any((parent / ".git").exists() for parent in [path, *path.parents])


def require_local_docker():
    selected_context = os.environ.get("DOCKER_CONTEXT")
    endpoint = None if selected_context else os.environ.get("DOCKER_HOST")
    if not endpoint:
        result = subprocess.run(
            ["docker", "context", "inspect", *([selected_context] if selected_context else [])],
            capture_output=True, text=True, check=True,
        )
        endpoint = json.loads(result.stdout)[0]["Endpoints"]["docker"]["Host"]
    if not endpoint.startswith(("unix://", "npipe://")):
        raise RuntimeError("This runner only supports a local Docker daemon")


def cleanup(source, artifacts):
    marker = artifacts / "run.json"
    state = json.loads(marker.read_text())
    if not re.fullmatch(r"albert-local-[0-9a-f]{10}", state["project"]):
        raise RuntimeError("Not a local fixture project")
    if state.get("cleaned"):
        return
    env = os.environ.copy()
    env["ALBERT_E2E_CONTEXT"] = str(artifacts / "context")
    for field, variable in (("url", "ALBERT_E2E_PORT"), ("fixture", "ALBERT_E2E_AUTH_PORT")):
        endpoint = urlsplit(state[field])
        if endpoint.scheme != "http" or endpoint.hostname != "127.0.0.1" or not endpoint.port:
            raise RuntimeError("Invalid local fixture endpoint")
        env[variable] = str(endpoint.port)
    subprocess.run([
        "docker", "compose", "--project-directory", str(artifacts),
        "--env-file", str(artifacts / "compose.env"), "-p", state["project"],
        "-f", str(source / "compose.yaml"), "down", "--volumes", "--remove-orphans",
    ], env=env, check=True, timeout=60)
    state["cleaned"] = True
    marker.write_text(json.dumps(state, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, help="Binary built with --features local-e2e")
    parser.add_argument("--cleanup", type=Path, help="Stop only a retained fixture run and remove its volume")
    parser.add_argument("--artifacts", type=Path, help="New directory outside every Git working tree")
    parser.add_argument("--frontend", type=Path, help="Prepared frontend dependency copy outside Git")
    parser.add_argument("--keep", action="store_true", help="Keep this isolated stack running for manual testing")
    parser.add_argument("--skip-browser", action="store_true", help="Start for manual exploration; does not claim tests passed")
    args = parser.parse_args()
    source = Path(__file__).resolve().parent
    require_local_docker()
    if args.cleanup:
        if in_repository(args.cleanup.resolve()):
            parser.error("Run artifacts must be outside repositories")
        cleanup(source, args.cleanup.resolve())
        return
    if not args.binary:
        parser.error("--binary is required when starting a fixture run")
    if args.skip_browser and not args.keep:
        parser.error("Manual preview requires --skip-browser --keep")
    os.umask(0o077)
    root = source.parents[1]
    artifacts = args.artifacts.resolve() if args.artifacts else Path(tempfile.mkdtemp(prefix="albert-local-e2e-"))
    if in_repository(artifacts):
        parser.error("Artifacts must be outside the repository")
    artifacts.mkdir(parents=True, exist_ok=True)
    if (artifacts / "run.json").exists():
        parser.error("Existing run found; inspect or clean it before starting another")
    binary = args.binary.resolve(strict=True)
    context = artifacts / "context"
    context.mkdir()
    shutil.copy2(binary, context / "albert")
    if shutil.which("strip"):
        # Keep Docker layers small; leave the source executable untouched.
        subprocess.run(["strip", "--strip-debug", str(context / "albert")], check=True)
    for name in ("Runtime.Dockerfile", "start.py", "mock_provider.py", "gateway.conf"):
        shutil.copy2(source / name, context / name)
    env = os.environ.copy()
    env.update(
        ALBERT_E2E_CONTEXT=str(context),
        ALBERT_E2E_PORT=str(port()),
        ALBERT_E2E_AUTH_PORT=str(port()),
    )
    project = "albert-local-" + secrets.token_hex(5)
    environment_file = artifacts / "compose.env"
    environment_file.write_text("")
    compose = ["docker", "compose", "--project-directory", str(artifacts), "--env-file", str(environment_file), "-p", project, "-f", str(source / "compose.yaml")]
    url = "http://127.0.0.1:" + env["ALBERT_E2E_PORT"]
    fixture = "http://127.0.0.1:" + env["ALBERT_E2E_AUTH_PORT"]
    state = {"project": project, "url": url, "fixture": fixture, "phase": "starting", "browser_verified": False}
    marker = artifacts / "run.json"
    marker.write_text(json.dumps(state, indent=2))
    try:
        with (artifacts / "compose.log").open("w") as log:
            subprocess.run([*compose, "up", "-d", "--build", "--wait", "--wait-timeout", "120"], env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
        ready(url + "/api/v1/context")
        state["phase"] = "ready"
        if not args.skip_browser:
            frontend = args.frontend.resolve() if args.frontend else artifacts / "frontend"
            if in_repository(frontend):
                parser.error("Frontend validation copy must be outside the repository")
            if not args.frontend:
                shutil.copytree(root / "frontend", frontend, ignore=shutil.ignore_patterns("dist", "test-results", "playwright-report"))
                shutil.copytree(root / "contracts", artifacts / "contracts")
            if not (frontend / "node_modules").exists():
                raise RuntimeError("Install locked frontend dependencies before running the browser suite")
            browser_env = {
                **env,
                "ALBERT_E2E_URL": url,
                "ALBERT_E2E_AUTH_URL": fixture,
                "ALBERT_E2E_COMPOSE_PROJECT": project,
                "ALBERT_E2E_COMPOSE_FILE": str(source / "compose.yaml"),
                "ALBERT_E2E_COMPOSE_ENV": str(environment_file),
                "ALBERT_E2E_ARTIFACTS": str(artifacts),
            }
            with (artifacts / "browser.log").open("w") as log:
                subprocess.run(["npm", "run", "test:backend", "--", "tests/integration/openai-login.spec.ts"], cwd=frontend, env=browser_env, stdout=log, stderr=subprocess.STDOUT, check=True)
            state["browser_verified"] = True
        state["phase"] = "complete"
    except BaseException:
        state["phase"] = "failed"
        raise
    finally:
        with (artifacts / "runtime.log").open("w") as log:
            subprocess.run([*compose, "logs", "--no-color"], env=env, stdout=log, stderr=subprocess.STDOUT, timeout=30)
        marker.write_text(json.dumps(state, indent=2))
        if not args.keep:
            subprocess.run([*compose, "down", "--volumes", "--remove-orphans"], env=env, check=True, timeout=60)
            state["cleaned"] = True
            marker.write_text(json.dumps(state, indent=2))
    print(json.dumps(state))
    print("Artifacts:", artifacts)


if __name__ == "__main__":
    main()
