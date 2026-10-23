"""`/.well-known/security.txt` (RFC 9116): where to report a vulnerability, on every public server.

    from loamz.server.securitytxt import install
    install(app)

Operators point it at their own contact with `LOAMZ_SECURITY_CONTACT` (a `mailto:` or https URL) and
`LOAMZ_SECURITY_POLICY`. The defaults are this project's private vulnerability reporting and its security policy. The
`Expires` field is six months after the process started, so a long-running cell does not advertise a stale file for long.
"""

from __future__ import annotations

import os
from datetime import UTC, datetime, timedelta

from fastapi import FastAPI, Response

DEFAULT_CONTACT = "https://github.com/loamz/loamz-server/security/advisories/new"
DEFAULT_POLICY = "https://github.com/loamz/loamz-server/blob/main/SECURITY.md"


def render(contact: str, policy: str, expires: datetime) -> str:
    return (
        f"Contact: {contact}\n"
        f"Expires: {expires.strftime('%Y-%m-%dT%H:%M:%SZ')}\n"
        f"Policy: {policy}\n"
        "Preferred-Languages: en\n"
    )


def install(app: FastAPI) -> None:
    expires = datetime.now(UTC) + timedelta(days=182)

    @app.get("/.well-known/security.txt", include_in_schema=False)
    def security_txt() -> Response:
        body = render(
            os.environ.get("LOAMZ_SECURITY_CONTACT", DEFAULT_CONTACT),
            os.environ.get("LOAMZ_SECURITY_POLICY", DEFAULT_POLICY),
            expires,
        )
        return Response(body, media_type="text/plain; charset=utf-8")
