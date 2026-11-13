from __future__ import annotations

import httpx


class LoamzError(Exception):
    """RFC 9457 Problem Details error returned by Loamz Data API."""

    def __init__(self, status: int, title: str, detail: str) -> None:
        super().__init__(f"[{status}] {title}: {detail}" if detail else f"[{status}] {title}")
        self.status = status
        self.title = title
        self.detail = detail

    @classmethod
    def from_response(cls, response: httpx.Response) -> LoamzError:
        status = response.status_code
        try:
            body = response.json()
            if isinstance(body, dict):
                title = body.get("title") or response.reason_phrase or "Error"
                detail = body.get("detail") or ""
                return cls(status=int(body.get("status", status)), title=title, detail=detail)
        except Exception:
            pass
        title = response.reason_phrase or "Error"
        detail = response.text.strip()
        return cls(status=status, title=title, detail=detail)
