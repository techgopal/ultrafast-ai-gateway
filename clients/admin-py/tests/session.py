"""Signs in once with a password (session cookie and CSRF token) to mint an
access token. Tests then use only the token, through the SDK."""

from __future__ import annotations

import secrets

import httpx

from .gateway import Account, Gateway


def mint_token(gateway: Gateway, account: Account, name: str = "sdk-test") -> str:
    with httpx.Client(base_url=gateway.origin) as http:
        login = http.post("/api/auth/login", json={"email": account.email, "password": account.password})
        if not login.is_success:
            raise RuntimeError(f"sign-in failed with {login.status_code}")
        csrf = login.json()["csrf_token"]
        created = http.post("/api/tokens", json={"name": name}, headers={"x-csrf-token": csrf})
        if created.status_code != 201:
            raise RuntimeError(f"minting a token failed with {created.status_code}")
        return str(created.json()["secret"])


def member_token(gateway: Gateway, admin_token: str, email: str) -> str:
    """Invites a member with the admin token, accepts the invite, returns a token of that member."""
    invited = httpx.post(
        f"{gateway.origin}/api/users",
        json={"email": email, "name": "Member", "role": "member"},
        headers={"authorization": f"Bearer {admin_token}"},
    )
    if invited.status_code != 201:
        raise RuntimeError(f"inviting failed with {invited.status_code}")
    # The link is `<origin>/accept-invite#token=<token>`.
    token = invited.json()["invite_link"].split("#token=")[1]
    password = f"pw-{secrets.token_hex(12)}"
    accepted = httpx.post(f"{gateway.origin}/api/auth/accept-invite", json={"token": token, "password": password})
    if accepted.status_code != 204:
        raise RuntimeError(f"accepting the invite failed with {accepted.status_code}")
    return mint_token(gateway, Account(email, password), "member-test")
