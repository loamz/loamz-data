import os
import subprocess
import sys
from pathlib import Path
import pytest
import httpx

REPO_ROOT = Path(__file__).resolve().parents[2]
API_URL = os.environ.get("LOAMZ_URL", "http://127.0.0.1:8020")


def get_dev_token(who: str = "ana") -> str | None:
    token_script = REPO_ROOT / "deploy" / "dev" / "token.py"
    if not token_script.is_file():
        return None
    try:
        out = subprocess.check_output(
            [sys.executable, str(token_script), who],
            text=True,
            stderr=subprocess.DEVNULL,
            timeout=5,
        ).strip()
        return out.splitlines()[-1] if out else None
    except Exception:
        return None


def is_server_ready() -> bool:
    try:
        r = httpx.get(f"{API_URL}/healthz", timeout=2.0)
        return r.status_code == 200
    except Exception:
        return False


@pytest.fixture(scope="session")
def base_url():
    return API_URL


@pytest.fixture(scope="session")
def ana_token():
    if not is_server_ready():
        pytest.skip(f"Loamz Data server is not running at {API_URL} (start with scripts/dev.sh)")
    token = get_dev_token("ana")
    if not token:
        pytest.skip("Could not generate token for ana from deploy/dev/token.py")
    return token
